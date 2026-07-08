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
    Frame, FrameError, HeadSummary, TcpConn, TcpTransport, Transport, GET_BLOCKS_RANGE_CAP,
    PROTOCOL_VERSION,
};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::local_node::{
    blocks_range_values, head_summary, ingest_announced_block, ingest_candidate_chain,
    ingress_admit_share, CandidateChainOutcome, HttpRateLimiter, IngressBlockOutcome,
    IngressShareOutcome, LocalNodeState,
};
use crate::p2p_egress::open_validated_conn;

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
    pub(crate) ingress_get_blocks_served: AtomicU64,
    pub(crate) sync_blocks_applied: AtomicU64,
    pub(crate) sync_reorgs_applied: AtomicU64,
    pub(crate) sync_peer_failures: AtomicU64,
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
            Ok(Frame::GetBlocks { from, to }) => {
                // N3.4 — serve a sync pull from the local block cache.
                // Range shape (≤256, not inverted) was validated by the
                // codec on receive; heights past our head are simply not
                // included (the requester sees a shorter/empty batch).
                let blocks = blocks_range_values(&state.blocking_read(), from, to);
                if transport
                    .send_frame(&mut conn, &Frame::Blocks { blocks })
                    .is_err()
                {
                    return;
                }
                metrics
                    .ingress_get_blocks_served
                    .fetch_add(1, Ordering::Relaxed);
            }
            Ok(_) => {
                // Unsolicited Blocks / Hello re-sends are harmless. Count
                // and keep the connection so additive frame evolution
                // never wedges an older node (ADR-0009 (b)).
                metrics
                    .ingress_unsupported_frames
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(()) => return,
        }
    }
}

/// N3.4 — how often the sync loop re-checks every peer's head. Catch-up
/// during the closed testnet is announce-driven in the common case; this
/// poll is the gap-filler (missed announces, fresh boot, a peer that was
/// down). The value trades convergence latency against idle Hello traffic
/// (2 peers × 1 Hello per interval ≈ nothing against the 600/min budget).
const SYNC_POLL_INTERVAL: Duration = Duration::from_secs(5);

pub(crate) fn spawn_sync_thread(
    peers: Vec<SocketAddr>,
    identity: P2pIdentity,
    state: Arc<RwLock<LocalNodeState>>,
    stop: Arc<AtomicBool>,
    metrics: Arc<P2pMetrics>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("boole-p2p-sync".to_string())
        .spawn(move || sync_loop(peers, identity, state, stop, metrics))
        .expect("spawn boole-p2p-sync thread")
}

/// N3.4 — initial/catch-up sync (`GetBlocks`/`Blocks`): learn each peer's
/// head from the `Hello` exchange and pull the missing range in
/// `GET_BLOCKS_RANGE_CAP` pages, pushing every block through the exact
/// N3.3 verify-then-append path. First pass runs immediately (fresh-boot
/// catch-up — the N5.3 `node join` seam), then the loop re-checks every
/// `SYNC_POLL_INTERVAL`. Non-goals per spec: competing-chain selection
/// (N4), parallel/headers-first optimizations.
fn sync_loop(
    peers: Vec<SocketAddr>,
    identity: P2pIdentity,
    state: Arc<RwLock<LocalNodeState>>,
    stop: Arc<AtomicBool>,
    metrics: Arc<P2pMetrics>,
) {
    while !stop.load(Ordering::Relaxed) {
        for peer in &peers {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if sync_with_peer(peer, &identity, &state, &stop, &metrics).is_err() {
                metrics.sync_peer_failures.fetch_add(1, Ordering::Relaxed);
            }
        }
        // Sleep in short slices so shutdown stays bounded by the accept
        // poll, not by the sync interval.
        let deadline = std::time::Instant::now() + SYNC_POLL_INTERVAL;
        while std::time::Instant::now() < deadline {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            thread::sleep(ACCEPT_POLL_INTERVAL);
        }
    }
}

fn sync_with_peer(
    peer: &SocketAddr,
    identity: &P2pIdentity,
    state: &Arc<RwLock<LocalNodeState>>,
    stop: &Arc<AtomicBool>,
    metrics: &Arc<P2pMetrics>,
) -> Result<(), FrameError> {
    let my_head = head_summary(&state.blocking_read());
    let (transport, mut conn, peer_head) = open_validated_conn(peer, identity, my_head.clone())?;
    let mut my_height = my_head.height;
    while my_height < peer_head.height && !stop.load(Ordering::Relaxed) {
        let to = (peer_head.height - 1).min(my_height + GET_BLOCKS_RANGE_CAP - 1);
        transport.send_frame(
            &mut conn,
            &Frame::GetBlocks {
                from: my_height,
                to,
            },
        )?;
        let blocks = match transport.recv_frame(&mut conn)? {
            Frame::Blocks { blocks } => blocks,
            _ => {
                return Err(FrameError::Malformed {
                    detail: "expected Blocks in reply to GetBlocks".to_string(),
                })
            }
        };
        if blocks.is_empty() {
            // The peer served nothing for a range its Hello claimed to
            // have — stop rather than spin; the next poll retries.
            return Ok(());
        }
        for block_value in &blocks {
            // Scope the write guard to the ingest call so it is dropped before
            // any reorg path below re-acquires it (same-thread write-write
            // would deadlock).
            let outcome = {
                let mut guard = state.blocking_write();
                ingest_announced_block(&mut guard, block_value)
            };
            match outcome {
                IngressBlockOutcome::Ingested => {
                    metrics.sync_blocks_applied.fetch_add(1, Ordering::Relaxed);
                }
                IngressBlockOutcome::Ignored => {
                    // The peer's block does not extend our head by one: either
                    // the chain moved under us (raced with a local commit /
                    // announce) or the peer is on a COMPETING fork that
                    // diverges below our head. The extend-by-one path can make
                    // no progress here, so pull the peer's full chain from
                    // genesis and let fork-choice decide whether it is heavy
                    // enough to reorg onto (N4.2/N4.3).
                    return reorg_from_peer(&transport, &mut conn, &peer_head, state, metrics);
                }
                IngressBlockOutcome::Rejected => {
                    // Strict replay refused the peer's block: tampered or
                    // divergent chain. Abort this peer's sync (counted by
                    // the caller as a peer failure) — never adopt.
                    metrics
                        .ingress_blocks_rejected
                        .fetch_add(1, Ordering::Relaxed);
                    return Err(FrameError::Malformed {
                        detail: "peer served a block that failed strict validation".to_string(),
                    });
                }
            }
        }
        my_height = head_summary(&state.blocking_read()).height;
    }
    Ok(())
}

/// N4 — a peer advertised a head we cannot reach by extending our own chain
/// block-by-block (it diverges below our head, so `ingest_announced_block`
/// can only return `Ignored`). Download the peer's FULL chain from genesis
/// and hand it to fork-choice: adopt it iff it is strictly heavier (N4.2),
/// rewriting local consensus state from genesis (N4.3). A tie or lighter
/// chain is kept; a tampered/evidence-less chain is refused by the strict
/// replay inside the reorg primitive and counted as a rejected block.
fn reorg_from_peer(
    transport: &TcpTransport,
    conn: &mut TcpConn,
    peer_head: &HeadSummary,
    state: &Arc<RwLock<LocalNodeState>>,
    metrics: &Arc<P2pMetrics>,
) -> Result<(), FrameError> {
    let candidate = fetch_block_range(transport, conn, 0, peer_head.height)?;
    if candidate.is_empty() {
        // The peer advertised a head but served nothing for its own range —
        // stop rather than spin; the next poll retries.
        return Ok(());
    }
    // Network I/O is done; take the write guard only for the state mutation,
    // matching the single-writer discipline the HTTP submit path holds.
    let outcome = {
        let mut guard = state.blocking_write();
        ingest_candidate_chain(&mut guard, &candidate)
    };
    match outcome {
        CandidateChainOutcome::Reorged { new_head_height } => {
            metrics.sync_reorgs_applied.fetch_add(1, Ordering::Relaxed);
            eprintln!(
                "boole-node: sync adopted a heavier competing peer chain via reorg \
                 (new head height {new_head_height})"
            );
            Ok(())
        }
        // The competing chain lost fork-choice (an equal tie our tip already
        // holds, or a lighter chain). Benign: keep our chain and let the next
        // poll re-check.
        CandidateChainOutcome::KeptCurrent => Ok(()),
        CandidateChainOutcome::Rejected => {
            metrics
                .ingress_blocks_rejected
                .fetch_add(1, Ordering::Relaxed);
            Err(FrameError::Malformed {
                detail: "peer served a competing chain that failed strict validation".to_string(),
            })
        }
    }
}

/// Pull blocks `[from, upto)` from an open, validated peer connection,
/// paginated by the wire contract's range cap, in height order. `GetBlocks`
/// is inclusive on both bounds (matching the serving side), so each page for
/// heights `[next, to]` uses `to = min(upto - 1, next + cap - 1)`.
fn fetch_block_range(
    transport: &TcpTransport,
    conn: &mut TcpConn,
    from: u64,
    upto: u64,
) -> Result<Vec<Value>, FrameError> {
    let mut collected = Vec::new();
    let mut next = from;
    while next < upto {
        let to = (upto - 1).min(next + GET_BLOCKS_RANGE_CAP - 1);
        transport.send_frame(conn, &Frame::GetBlocks { from: next, to })?;
        let blocks = match transport.recv_frame(conn)? {
            Frame::Blocks { blocks } => blocks,
            _ => {
                return Err(FrameError::Malformed {
                    detail: "expected Blocks in reply to GetBlocks".to_string(),
                })
            }
        };
        if blocks.is_empty() {
            // The peer served nothing for a range its Hello claimed — stop
            // rather than spin.
            break;
        }
        next += blocks.len() as u64;
        collected.extend(blocks);
    }
    Ok(collected)
}
