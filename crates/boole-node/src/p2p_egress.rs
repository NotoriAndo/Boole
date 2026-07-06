//! N3.2 — share-gossip egress: fan every locally-admitted (and dedup-
//! cleared) share out to the static peer set as a `ShareAnnounce` frame.
//!
//! Best-effort by design: gossip must never change the local submit
//! outcome, so failures are counted and dropped, never retried or
//! surfaced to the submitter. Each announce is one short-lived
//! connection (`Hello` → validate reply → `ShareAnnounce` → close) —
//! stateless and self-healing for 2–3 static peers (S7 scope).

use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use boole_p2p::{Frame, FrameError, HeadSummary, TcpTransport, Transport};
use serde_json::Value;

use crate::p2p_ingress::{P2pIdentity, P2pMetrics};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const EGRESS_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Poll interval for the shutdown flag while the announce queue is idle.
const QUEUE_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// One admitted share headed to the peer set. `submission` is the same
/// `/submit` envelope shape (`{body, canonTag, ts}`) the local admission
/// consumed, verbatim (ADR-0009 (b)); `head` fills the outbound `Hello`.
pub(crate) struct ShareAnnouncement {
    pub(crate) submission: Value,
    pub(crate) head: HeadSummary,
}

pub(crate) fn spawn_egress_thread(
    rx: Receiver<ShareAnnouncement>,
    peers: Vec<SocketAddr>,
    identity: P2pIdentity,
    stop: Arc<AtomicBool>,
    metrics: Arc<P2pMetrics>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("boole-p2p-egress".to_string())
        .spawn(move || egress_loop(rx, peers, identity, stop, metrics))
        .expect("spawn boole-p2p-egress thread")
}

fn egress_loop(
    rx: Receiver<ShareAnnouncement>,
    peers: Vec<SocketAddr>,
    identity: P2pIdentity,
    stop: Arc<AtomicBool>,
    metrics: Arc<P2pMetrics>,
) {
    while !stop.load(Ordering::Relaxed) {
        let announcement = match rx.recv_timeout(QUEUE_POLL_INTERVAL) {
            Ok(announcement) => announcement,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        for peer in &peers {
            match announce_to_peer(peer, &identity, &announcement) {
                Ok(()) => {
                    metrics.egress_announces.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    metrics.egress_failures.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

fn announce_to_peer(
    peer: &SocketAddr,
    identity: &P2pIdentity,
    announcement: &ShareAnnouncement,
) -> Result<(), FrameError> {
    let stream = TcpStream::connect_timeout(peer, CONNECT_TIMEOUT)?;
    stream.set_read_timeout(Some(EGRESS_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(EGRESS_IO_TIMEOUT))?;
    let transport = TcpTransport::new();
    let mut conn = TcpTransport::conn_from_stream(stream)?;
    transport.send_frame(&mut conn, &identity.hello(announcement.head.clone()))?;
    // The dialer validates the peer's Hello symmetrically to ingress
    // (ADR-0009 (e)): never hand a share to a wrong-network/wrong-genesis
    // listener.
    let reply = transport.recv_frame(&mut conn)?;
    if !identity.matches(&reply) {
        return Err(FrameError::Malformed {
            detail: "peer hello mismatch (protocol_version/network_id/genesis_hash)".to_string(),
        });
    }
    transport.send_frame(
        &mut conn,
        &Frame::ShareAnnounce {
            submission: announcement.submission.clone(),
        },
    )
}
