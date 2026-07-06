//! N3.2 — share-gossip ingress: accept allowlisted peers, validate `Hello`,
//! and re-admit every announced share through the exact local admission path
//! (`admit_parsed_submission_typed` via the runtime wrapper) — ADR-0009 (e):
//! peers are never trusted, and there is no second validation policy.
//!
//! The thread is a plain blocking `std::thread` (the transport is blocking
//! `std::net` by design, ADR-0009 (a)); it takes the SAME single
//! `tokio::sync::RwLock` write guard the HTTP submit path uses, so the share
//! pool and the N2.3 proof-dedup ledger can never diverge between the two
//! ingress surfaces.

use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use boole_p2p::{
    Frame, FrameError, HeadSummary, TcpConn, TcpTransport, Transport, PROTOCOL_VERSION,
};
use tokio::sync::RwLock;

use crate::local_node::{
    head_summary, ingest_announced_block, ingress_admit_share, HttpRateLimiter,
    IngressBlockOutcome, IngressShareOutcome, LocalNodeState,
};

/// How long an accepted connection may sit silent before it is dropped.
/// Bounds a slow/hung peer's hold on the (serial) ingress thread; the
/// egress side sends `Hello` + `ShareAnnounce` immediately after connect,
/// so an honest announce never comes near it.
const INGRESS_IO_TIMEOUT: Duration = Duration::from_secs(10);

/// Poll interval of the nonblocking accept loop (also the shutdown latency
/// bound for the ingress thread).
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// N3.3 — default per-peer ingress frame budget (frames per 60s window,
/// keyed by peer IP; ADR-0009 (c) makes the limit's PRESENCE part of the
/// wire contract). Honest S7 gossip is a handful of frames per announce
/// connection (Hello + one announce, plus GetBlocks/Blocks for a block),
/// and closed-testnet share/block cadence is well under one per second —
/// 600/min (10/s sustained) leaves an order of magnitude of headroom
/// while still bounding a misbehaving allowlisted peer. Tunable via
/// `--p2p-rate-limit-per-60s`; 0 disables (closed-harness escape hatch).
pub const DEFAULT_P2P_RATE_LIMIT_PER_60S: usize = 600;

/// N3.2 — static gossip surface for one node (ADR-0009 (d)).
pub struct P2pConfig {
    /// Pre-bound gossip listener. `None` = no ingress (egress-only node).
    pub listener: Option<TcpListener>,
    /// Static peer set. Egress announces to every entry; the entries'
    /// IPs double as the inbound allowlist (address-based only in S7 —
    /// inbound source ports are ephemeral, so matching is by IP).
    pub peers: Vec<SocketAddr>,
    /// N3.3 — per-peer ingress frame quota per 60s window (ADR-0009 (c)).
    /// 0 disables the limit.
    pub rate_limit_per_60s: usize,
}

/// The identity fields both `Hello` directions must agree on before any
/// other frame is processed (ADR-0009 (b)/(e)).
#[derive(Clone)]
pub(crate) struct P2pIdentity {
    pub(crate) network_id: String,
    pub(crate) genesis_c: String,
}

impl P2pIdentity {
    pub(crate) fn hello(&self, head: HeadSummary) -> Frame {
        Frame::Hello {
            protocol_version: PROTOCOL_VERSION,
            network_id: self.network_id.clone(),
            genesis_hash: self.genesis_c.clone(),
            head,
        }
    }

    /// A peer `Hello` matches iff protocol_version, network_id AND
    /// genesis_hash all agree (`Hello.genesis_hash` is load-bearing for
    /// N5.2's per-network genesis commitment; here both nodes must already
    /// carry the same configured genesis).
    pub(crate) fn matches(&self, frame: &Frame) -> bool {
        matches!(
            frame,
            Frame::Hello {
                protocol_version,
                network_id,
                genesis_hash,
                ..
            } if *protocol_version == PROTOCOL_VERSION
                && network_id == &self.network_id
                && genesis_hash == &self.genesis_c
        )
    }
}

/// Typed gossip counters (ADR-0009 (e): every dropped/rejected ingress
/// object is counted, never silently discarded). Rendered in `/metrics`.
#[derive(Default)]
pub(crate) struct P2pMetrics {
    pub(crate) ingress_not_allowlisted_drops: AtomicU64,
    pub(crate) ingress_hello_mismatch_drops: AtomicU64,
    pub(crate) ingress_malformed_frame_drops: AtomicU64,
    pub(crate) ingress_unsupported_frames: AtomicU64,
    pub(crate) ingress_shares_admitted: AtomicU64,
    pub(crate) ingress_shares_rejected: AtomicU64,
    pub(crate) ingress_blocks_ingested: AtomicU64,
    pub(crate) ingress_blocks_rejected: AtomicU64,
    pub(crate) ingress_block_announces_ignored: AtomicU64,
    pub(crate) ingress_rate_limited_drops: AtomicU64,
    pub(crate) egress_announces: AtomicU64,
    pub(crate) egress_failures: AtomicU64,
    pub(crate) egress_block_announces: AtomicU64,
    pub(crate) egress_block_failures: AtomicU64,
}

pub(crate) fn spawn_ingress_thread(
    listener: TcpListener,
    allowlist: Vec<IpAddr>,
    identity: P2pIdentity,
    state: Arc<RwLock<LocalNodeState>>,
    stop: Arc<AtomicBool>,
    metrics: Arc<P2pMetrics>,
    rate_limit_per_60s: usize,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("boole-p2p-ingress".to_string())
        .spawn(move || {
            ingress_loop(
                listener,
                allowlist,
                identity,
                state,
                stop,
                metrics,
                rate_limit_per_60s,
            )
        })
        .expect("spawn boole-p2p-ingress thread")
}

#[allow(clippy::too_many_arguments)]
fn ingress_loop(
    listener: TcpListener,
    allowlist: Vec<IpAddr>,
    identity: P2pIdentity,
    state: Arc<RwLock<LocalNodeState>>,
    stop: Arc<AtomicBool>,
    metrics: Arc<P2pMetrics>,
    rate_limit_per_60s: usize,
) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    // N3.3 — per-peer frame budget (ADR-0009 (c)). One limiter for the
    // whole ingress lifetime: the per-IP window must survive across the
    // short-lived announce connections, or a flooder could reset its
    // budget by reconnecting.
    let rate_limiter =
        (rate_limit_per_60s > 0).then(|| HttpRateLimiter::new(rate_limit_per_60s, 60_000));
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer)) => {
                if !allowlist.contains(&peer.ip()) {
                    // ADR-0009 (d)/(e): outside the static peer set → drop
                    // at accept, no response, counted.
                    metrics
                        .ingress_not_allowlisted_drops
                        .fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                // Connections are handled serially: S7 is 2–3 operator
                // peers and every announce is one short-lived connection,
                // so a queue depth of 1 with an IO timeout bounds a stuck
                // peer without a per-connection thread pool.
                handle_connection(
                    stream,
                    peer,
                    &identity,
                    &state,
                    &stop,
                    &metrics,
                    rate_limiter.as_ref(),
                );
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(_) => thread::sleep(ACCEPT_POLL_INTERVAL),
        }
    }
}

/// Receive one frame, charging it against the peer's rate budget. `Err(())`
/// means the connection must be dropped (typed counters already updated);
/// io/close errors are silent drops like before.
fn recv_frame_limited(
    transport: &TcpTransport,
    conn: &mut TcpConn,
    peer: &SocketAddr,
    rate_limiter: Option<&HttpRateLimiter>,
    metrics: &Arc<P2pMetrics>,
) -> Result<Frame, ()> {
    let frame = match transport.recv_frame(conn) {
        Ok(frame) => frame,
        Err(FrameError::ConnectionClosed) | Err(FrameError::Io(_)) => return Err(()),
        Err(_) => {
            metrics
                .ingress_malformed_frame_drops
                .fetch_add(1, Ordering::Relaxed);
            return Err(());
        }
    };
    if let Some(limiter) = rate_limiter {
        if !limiter.admit(peer.ip(), now_ms()) {
            // ADR-0009 (c): over-budget peer → drop the connection,
            // counted. The window state persists, so reconnecting does
            // not refill the budget.
            metrics
                .ingress_rate_limited_drops
                .fetch_add(1, Ordering::Relaxed);
            return Err(());
        }
    }
    Ok(frame)
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    identity: &P2pIdentity,
    state: &Arc<RwLock<LocalNodeState>>,
    stop: &Arc<AtomicBool>,
    metrics: &Arc<P2pMetrics>,
    rate_limiter: Option<&HttpRateLimiter>,
) {
    // The accepted socket may inherit O_NONBLOCK from the listener on some
    // platforms (macOS); force blocking + bounded IO explicitly.
    if stream.set_nonblocking(false).is_err()
        || stream.set_read_timeout(Some(INGRESS_IO_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(INGRESS_IO_TIMEOUT)).is_err()
    {
        return;
    }
    let transport = TcpTransport::new();
    let mut conn = match TcpTransport::conn_from_stream(stream) {
        Ok(conn) => conn,
        Err(_) => return,
    };
    // First frame MUST be a matching Hello; a mismatch is a typed
    // disconnect with no reply (ADR-0009 (e)).
    match recv_frame_limited(&transport, &mut conn, &peer, rate_limiter, metrics) {
        Ok(frame @ Frame::Hello { .. }) => {
            if !identity.matches(&frame) {
                metrics
                    .ingress_hello_mismatch_drops
                    .fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
        Ok(_) => {
            // A non-Hello opener violates the handshake contract.
            metrics
                .ingress_malformed_frame_drops
                .fetch_add(1, Ordering::Relaxed);
            return;
        }
        Err(()) => return,
    }
    // Reply with our own Hello so the dialer can validate symmetrically.
    let our_hello = {
        let guard = state.blocking_read();
        identity.hello(head_summary(&guard))
    };
    if transport.send_frame(&mut conn, &our_hello).is_err() {
        return;
    }
    let peer_ip = peer.ip().to_string();
    while !stop.load(Ordering::Relaxed) {
        match recv_frame_limited(&transport, &mut conn, &peer, rate_limiter, metrics) {
            Ok(Frame::ShareAnnounce { submission }) => {
                // The single write guard covers admit + dedup peek exactly
                // like the HTTP path (`submit_json`) — see
                // `ingress_admit_share` for the policy notes.
                let mut guard = state.blocking_write();
                match ingress_admit_share(&mut guard, &submission, &peer_ip) {
                    IngressShareOutcome::Admitted => {
                        metrics
                            .ingress_shares_admitted
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    IngressShareOutcome::Rejected { .. } => {
                        // A reject here is normal gossip weather (e.g. a
                        // stale `c` after this node advanced) — counted,
                        // connection stays up.
                        metrics
                            .ingress_shares_rejected
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            Ok(Frame::BlockAnnounce { height, c }) => {
                // N3.3 — announce/pull: the summary tells us whether the
                // block extends our head by exactly one; only then do we
                // pull the body on the same connection. Anything else
                // (stale re-announce, a gap needing sync) is counted and
                // skipped — fork-choice/reorg are N4, initial sync N3.4.
                let my_height = head_summary(&state.blocking_read()).height;
                if height != my_height {
                    metrics
                        .ingress_block_announces_ignored
                        .fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                if transport
                    .send_frame(
                        &mut conn,
                        &Frame::GetBlocks {
                            from: height,
                            to: height,
                        },
                    )
                    .is_err()
                {
                    return;
                }
                let block_value =
                    match recv_frame_limited(&transport, &mut conn, &peer, rate_limiter, metrics) {
                        Ok(Frame::Blocks { blocks }) => {
                            // Exactly the requested block, and the body must
                            // match the announced hash — a peer must not be
                            // able to bait with one hash and switch the body.
                            let Some(block_value) = blocks.into_iter().next() else {
                                metrics
                                    .ingress_malformed_frame_drops
                                    .fetch_add(1, Ordering::Relaxed);
                                return;
                            };
                            if block_value.get("c").and_then(serde_json::Value::as_str)
                                != Some(c.as_str())
                            {
                                metrics
                                    .ingress_malformed_frame_drops
                                    .fetch_add(1, Ordering::Relaxed);
                                return;
                            }
                            block_value
                        }
                        Ok(_) => {
                            metrics
                                .ingress_malformed_frame_drops
                                .fetch_add(1, Ordering::Relaxed);
                            return;
                        }
                        Err(()) => return,
                    };
                let mut guard = state.blocking_write();
                match ingest_announced_block(&mut guard, &block_value) {
                    IngressBlockOutcome::Ingested => {
                        metrics
                            .ingress_blocks_ingested
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    IngressBlockOutcome::Ignored => {
                        // The head moved between the read above and the
                        // write guard (e.g. we self-produced) — normal.
                        metrics
                            .ingress_block_announces_ignored
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    IngressBlockOutcome::Rejected => {
                        metrics
                            .ingress_blocks_rejected
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            Ok(_) => {
                // GetBlocks/Blocks outside an announce exchange arrive
                // with N3.4; Hello re-sends are harmless. Count and keep
                // the connection so additive frame evolution never wedges
                // an older node (ADR-0009 (b)).
                metrics
                    .ingress_unsupported_frames
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(()) => return,
        }
    }
}
