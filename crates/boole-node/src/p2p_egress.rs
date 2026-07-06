//! N3.2/N3.3 — gossip egress: fan locally-admitted shares and locally-
//! committed blocks out to the static peer set.
//!
//! Best-effort by design: gossip must never change the local submit
//! outcome, so failures are counted and dropped, never retried or
//! surfaced to the submitter. Each announce is one short-lived
//! connection — stateless and self-healing for 2–3 static peers (S7).
//!
//! Blocks follow the ADR-0009 (b) announce/pull shape: the announce
//! carries only `{height, c}`; the body moves only inside a `Blocks`
//! frame, which the receiving peer requests with `GetBlocks` on the same
//! connection. The egress can serve that request statelessly because it
//! holds the just-committed block it is announcing.

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

/// N3.3 — one committed block headed to the peer set. `block` is the full
/// `PersistedBlock` as its canonical serde JSON (the byte shape the peer's
/// strict replay validates); `height`/`c` fill the summary announce.
pub(crate) struct BlockAnnouncement {
    pub(crate) height: u64,
    pub(crate) c: String,
    pub(crate) block: Value,
    pub(crate) head: HeadSummary,
}

pub(crate) enum EgressEvent {
    Share(ShareAnnouncement),
    Block(BlockAnnouncement),
}

pub(crate) fn spawn_egress_thread(
    rx: Receiver<EgressEvent>,
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
    rx: Receiver<EgressEvent>,
    peers: Vec<SocketAddr>,
    identity: P2pIdentity,
    stop: Arc<AtomicBool>,
    metrics: Arc<P2pMetrics>,
) {
    while !stop.load(Ordering::Relaxed) {
        let event = match rx.recv_timeout(QUEUE_POLL_INTERVAL) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        for peer in &peers {
            match &event {
                EgressEvent::Share(announcement) => {
                    match announce_share_to_peer(peer, &identity, announcement) {
                        Ok(()) => {
                            metrics.egress_announces.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(_) => {
                            metrics.egress_failures.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                EgressEvent::Block(announcement) => {
                    match announce_block_to_peer(peer, &identity, announcement) {
                        Ok(()) => {
                            metrics
                                .egress_block_announces
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        Err(_) => {
                            metrics
                                .egress_block_failures
                                .fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    }
}

/// Dial `peer`, exchange `Hello`s, and validate the reply symmetrically to
/// ingress (ADR-0009 (e)): never hand gossip to a wrong-network or
/// wrong-genesis listener. Returns the peer's own head summary from its
/// `Hello` reply — the N3.4 sync loop uses it to size the catch-up pull;
/// announce paths ignore it.
pub(crate) fn open_validated_conn(
    peer: &SocketAddr,
    identity: &P2pIdentity,
    head: HeadSummary,
) -> Result<(TcpTransport, boole_p2p::TcpConn, HeadSummary), FrameError> {
    let stream = TcpStream::connect_timeout(peer, CONNECT_TIMEOUT)?;
    stream.set_read_timeout(Some(EGRESS_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(EGRESS_IO_TIMEOUT))?;
    let transport = TcpTransport::new();
    let mut conn = TcpTransport::conn_from_stream(stream)?;
    transport.send_frame(&mut conn, &identity.hello(head))?;
    let reply = transport.recv_frame(&mut conn)?;
    if !identity.matches(&reply) {
        return Err(FrameError::Malformed {
            detail: "peer hello mismatch (protocol_version/network_id/genesis_hash)".to_string(),
        });
    }
    let peer_head = match reply {
        Frame::Hello { head, .. } => head,
        // Unreachable: `matches` only accepts a Hello.
        _ => {
            return Err(FrameError::Malformed {
                detail: "peer reply was not a Hello".to_string(),
            })
        }
    };
    Ok((transport, conn, peer_head))
}

fn announce_share_to_peer(
    peer: &SocketAddr,
    identity: &P2pIdentity,
    announcement: &ShareAnnouncement,
) -> Result<(), FrameError> {
    let (transport, mut conn, _peer_head) =
        open_validated_conn(peer, identity, announcement.head.clone())?;
    transport.send_frame(
        &mut conn,
        &Frame::ShareAnnounce {
            submission: announcement.submission.clone(),
        },
    )
}

fn announce_block_to_peer(
    peer: &SocketAddr,
    identity: &P2pIdentity,
    announcement: &BlockAnnouncement,
) -> Result<(), FrameError> {
    let (transport, mut conn, _peer_head) =
        open_validated_conn(peer, identity, announcement.head.clone())?;
    transport.send_frame(
        &mut conn,
        &Frame::BlockAnnounce {
            height: announcement.height,
            c: announcement.c.clone(),
        },
    )?;
    // The peer either pulls the body with GetBlocks or closes the
    // connection (it already has the block, or the announce doesn't extend
    // its head). A close/timeout after the announce is a normal outcome,
    // not a delivery failure.
    match transport.recv_frame(&mut conn) {
        Ok(Frame::GetBlocks { from, to }) => {
            if from <= announcement.height && announcement.height <= to {
                transport.send_frame(
                    &mut conn,
                    &Frame::Blocks {
                        blocks: vec![announcement.block.clone()],
                    },
                )?;
            }
            Ok(())
        }
        Ok(_) => Err(FrameError::Malformed {
            detail: "expected GetBlocks after BlockAnnounce".to_string(),
        }),
        Err(FrameError::ConnectionClosed) | Err(FrameError::Io(_)) => Ok(()),
        Err(err) => Err(err),
    }
}
