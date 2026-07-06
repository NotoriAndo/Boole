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

use boole_p2p::{Frame, FrameError, HeadSummary, TcpTransport, Transport, PROTOCOL_VERSION};
use tokio::sync::RwLock;

use crate::local_node::{head_summary, ingress_admit_share, IngressShareOutcome, LocalNodeState};

/// How long an accepted connection may sit silent before it is dropped.
/// Bounds a slow/hung peer's hold on the (serial) ingress thread; the
/// egress side sends `Hello` + `ShareAnnounce` immediately after connect,
/// so an honest announce never comes near it.
const INGRESS_IO_TIMEOUT: Duration = Duration::from_secs(10);

/// Poll interval of the nonblocking accept loop (also the shutdown latency
/// bound for the ingress thread).
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(25);

/// N3.2 — static gossip surface for one node (ADR-0009 (d)).
pub struct P2pConfig {
    /// Pre-bound gossip listener. `None` = no ingress (egress-only node).
    pub listener: Option<TcpListener>,
    /// Static peer set. Egress announces to every entry; the entries'
    /// IPs double as the inbound allowlist (address-based only in S7 —
    /// inbound source ports are ephemeral, so matching is by IP).
    pub peers: Vec<SocketAddr>,
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
    pub(crate) egress_announces: AtomicU64,
    pub(crate) egress_failures: AtomicU64,
}

pub(crate) fn spawn_ingress_thread(
    listener: TcpListener,
    allowlist: Vec<IpAddr>,
    identity: P2pIdentity,
    state: Arc<RwLock<LocalNodeState>>,
    stop: Arc<AtomicBool>,
    metrics: Arc<P2pMetrics>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("boole-p2p-ingress".to_string())
        .spawn(move || ingress_loop(listener, allowlist, identity, state, stop, metrics))
        .expect("spawn boole-p2p-ingress thread")
}

fn ingress_loop(
    listener: TcpListener,
    allowlist: Vec<IpAddr>,
    identity: P2pIdentity,
    state: Arc<RwLock<LocalNodeState>>,
    stop: Arc<AtomicBool>,
    metrics: Arc<P2pMetrics>,
) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }
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
                handle_connection(stream, peer, &identity, &state, &stop, &metrics);
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(_) => thread::sleep(ACCEPT_POLL_INTERVAL),
        }
    }
}

fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    identity: &P2pIdentity,
    state: &Arc<RwLock<LocalNodeState>>,
    stop: &Arc<AtomicBool>,
    metrics: &Arc<P2pMetrics>,
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
    match transport.recv_frame(&mut conn) {
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
        Err(FrameError::ConnectionClosed) | Err(FrameError::Io(_)) => return,
        Err(_) => {
            metrics
                .ingress_malformed_frame_drops
                .fetch_add(1, Ordering::Relaxed);
            return;
        }
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
        match transport.recv_frame(&mut conn) {
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
            Ok(_) => {
                // BlockAnnounce/GetBlocks/Blocks arrive with N3.3/N3.4;
                // Hello re-sends are harmless. Count and keep the
                // connection so additive frame evolution never wedges an
                // older node (ADR-0009 (b)).
                metrics
                    .ingress_unsupported_frames
                    .fetch_add(1, Ordering::Relaxed);
            }
            Err(FrameError::ConnectionClosed) | Err(FrameError::Io(_)) => return,
            Err(_) => {
                // Malformed / over-cap / invalid range → drop the
                // connection, counted (ADR-0009 (c)/(e)).
                metrics
                    .ingress_malformed_frame_drops
                    .fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    }
}
