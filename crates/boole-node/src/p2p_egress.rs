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
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use boole_p2p::{Frame, FrameError, HeadSummary, TcpTransport, Transport};
use serde_json::Value;

use crate::p2p_ingress::{P2pIdentity, P2pMetrics};
use crate::p2p_lifecycle::{P2pLifecycle, SocketLease};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const EGRESS_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Per-peer pending-event cap. With the wire-level 16 MiB frame cap this
/// keeps one slow peer's retained payloads bounded, while separate workers
/// prevent that peer from delaying the rest of the static set.
const EGRESS_QUEUE_CAPACITY: usize = 4;

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

#[derive(Clone)]
pub(crate) struct P2pEgressFanout {
    peers: Vec<SyncSender<Arc<EgressEvent>>>,
    metrics: Arc<P2pMetrics>,
}

impl P2pEgressFanout {
    /// Best-effort and nonblocking: a full queue sheds work for that peer
    /// only, preserving both the local submit result and healthy peers.
    pub(crate) fn try_send(&self, event: EgressEvent) {
        let is_block = matches!(event, EgressEvent::Block(_));
        let event = Arc::new(event);
        for peer in &self.peers {
            match peer.try_send(event.clone()) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    self.metrics
                        .egress_queue_full_drops
                        .fetch_add(1, Ordering::Relaxed);
                    // A queue shed is still an announcement that did not
                    // reach this peer. Keep the event-specific legacy
                    // counters truthful for existing dashboards while the
                    // queue counter explains why it was lost.
                    if is_block {
                        self.metrics
                            .egress_block_failures
                            .fetch_add(1, Ordering::Relaxed);
                    } else {
                        self.metrics.egress_failures.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(TrySendError::Disconnected(_)) => {
                    if is_block {
                        self.metrics
                            .egress_block_failures
                            .fetch_add(1, Ordering::Relaxed);
                    } else {
                        self.metrics.egress_failures.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }
}

pub(crate) fn spawn_egress_workers(
    peers: Vec<SocketAddr>,
    identity: P2pIdentity,
    lifecycle: Arc<P2pLifecycle>,
    metrics: Arc<P2pMetrics>,
) -> (P2pEgressFanout, Vec<thread::JoinHandle<()>>) {
    let mut senders = Vec::with_capacity(peers.len());
    let mut workers = Vec::with_capacity(peers.len());
    for (index, peer) in peers.into_iter().enumerate() {
        let (tx, rx) = std::sync::mpsc::sync_channel(EGRESS_QUEUE_CAPACITY);
        senders.push(tx);
        let worker_identity = identity.clone();
        let worker_lifecycle = lifecycle.clone();
        let worker_metrics = metrics.clone();
        workers.push(
            thread::Builder::new()
                .name(format!("boole-p2p-egress-{index}"))
                .spawn(move || {
                    egress_peer_loop(rx, peer, worker_identity, worker_lifecycle, worker_metrics)
                })
                .expect("spawn boole-p2p peer egress thread"),
        );
    }
    (
        P2pEgressFanout {
            peers: senders,
            metrics,
        },
        workers,
    )
}

fn egress_peer_loop(
    rx: Receiver<Arc<EgressEvent>>,
    peer: SocketAddr,
    identity: P2pIdentity,
    lifecycle: Arc<P2pLifecycle>,
    metrics: Arc<P2pMetrics>,
) {
    while !lifecycle.is_stopped() {
        let event = match rx.recv_timeout(QUEUE_POLL_INTERVAL) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        match event.as_ref() {
            EgressEvent::Share(announcement) => {
                match announce_share_to_peer(&peer, &identity, announcement, &lifecycle) {
                    Ok(()) => {
                        metrics.egress_announces.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) if lifecycle.is_stopped() => return,
                    Err(_) => {
                        metrics.egress_failures.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            EgressEvent::Block(announcement) => {
                match announce_block_to_peer(&peer, &identity, announcement, &lifecycle) {
                    Ok(()) => {
                        metrics
                            .egress_block_announces
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) if lifecycle.is_stopped() => return,
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

pub(crate) struct ValidatedConn {
    pub(crate) transport: TcpTransport,
    pub(crate) conn: boole_p2p::TcpConn,
    pub(crate) peer_head: HeadSummary,
    pub(crate) peer_hello_wire_bytes: usize,
    _lease: SocketLease,
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
    lifecycle: &Arc<P2pLifecycle>,
) -> Result<ValidatedConn, FrameError> {
    open_validated_conn_with_limits(peer, identity, head, lifecycle, None)
}

/// Sync-only handshake variant: connect and Hello receive share the same
/// absolute peer-round deadline and cumulative wire ceiling as later range
/// responses. Gossip callers keep the ordinary per-I/O timeout above.
pub(crate) fn open_validated_conn_until(
    peer: &SocketAddr,
    identity: &P2pIdentity,
    head: HeadSummary,
    lifecycle: &Arc<P2pLifecycle>,
    remaining_wire_bytes: usize,
    deadline: Instant,
) -> Result<ValidatedConn, FrameError> {
    open_validated_conn_with_limits(
        peer,
        identity,
        head,
        lifecycle,
        Some((remaining_wire_bytes, deadline)),
    )
}

fn open_validated_conn_with_limits(
    peer: &SocketAddr,
    identity: &P2pIdentity,
    head: HeadSummary,
    lifecycle: &Arc<P2pLifecycle>,
    receive_limits: Option<(usize, Instant)>,
) -> Result<ValidatedConn, FrameError> {
    if lifecycle.is_stopped() {
        return Err(FrameError::Io(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "P2P lifecycle is stopped",
        )));
    }
    let connect_timeout = if let Some((_, deadline)) = receive_limits {
        deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(FrameError::ReceiveDeadlineExceeded)?
            .min(CONNECT_TIMEOUT)
    } else {
        CONNECT_TIMEOUT
    };
    let stream = TcpStream::connect_timeout(peer, connect_timeout)?;
    let io_timeout = if let Some((_, deadline)) = receive_limits {
        deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(FrameError::ReceiveDeadlineExceeded)?
            .min(EGRESS_IO_TIMEOUT)
    } else {
        EGRESS_IO_TIMEOUT
    };
    stream.set_read_timeout(Some(io_timeout))?;
    stream.set_write_timeout(Some(io_timeout))?;
    let lease = lifecycle.register(&stream)?;
    let transport = TcpTransport::new();
    let mut conn = TcpTransport::conn_from_stream(stream)?;
    transport.send_frame(&mut conn, &identity.hello(head))?;
    let (reply, peer_hello_wire_bytes) =
        if let Some((remaining_wire_bytes, deadline)) = receive_limits {
            transport.recv_frame_counted_until(&mut conn, remaining_wire_bytes, deadline)?
        } else {
            transport.recv_frame_counted(&mut conn)?
        };
    if !identity.matches(&reply) {
        return Err(FrameError::Malformed {
            detail: "peer hello mismatch \
                     (protocol_version/consensus_rule_version/network_id/genesis_hash)"
                .to_string(),
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
    Ok(ValidatedConn {
        transport,
        conn,
        peer_head,
        peer_hello_wire_bytes,
        _lease: lease,
    })
}

fn announce_share_to_peer(
    peer: &SocketAddr,
    identity: &P2pIdentity,
    announcement: &ShareAnnouncement,
    lifecycle: &Arc<P2pLifecycle>,
) -> Result<(), FrameError> {
    let mut validated = open_validated_conn(peer, identity, announcement.head.clone(), lifecycle)?;
    validated.transport.send_frame(
        &mut validated.conn,
        &Frame::ShareAnnounce {
            submission: announcement.submission.clone(),
        },
    )
}

fn announce_block_to_peer(
    peer: &SocketAddr,
    identity: &P2pIdentity,
    announcement: &BlockAnnouncement,
    lifecycle: &Arc<P2pLifecycle>,
) -> Result<(), FrameError> {
    let mut validated = open_validated_conn(peer, identity, announcement.head.clone(), lifecycle)?;
    validated.transport.send_frame(
        &mut validated.conn,
        &Frame::BlockAnnounce {
            height: announcement.height,
            c: announcement.c.clone(),
        },
    )?;
    // The peer either pulls the body with GetBlocks or closes the
    // connection (it already has the block, or the announce doesn't extend
    // its head). A close/timeout after the announce is a normal outcome,
    // not a delivery failure.
    match validated.transport.recv_frame(&mut validated.conn) {
        Ok(Frame::GetBlocks { from, to }) => {
            if from <= announcement.height && announcement.height <= to {
                validated.transport.send_frame(
                    &mut validated.conn,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::p2p_lifecycle::P2pLifecycle;
    use boole_core::CONSENSUS_RULE_VERSION;
    use boole_p2p::{Transport, PROTOCOL_VERSION};
    use serde_json::json;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::Instant;

    fn identity() -> P2pIdentity {
        P2pIdentity {
            network_id: "bounded-egress-test".to_string(),
            genesis_hash: "11".repeat(32),
        }
    }

    fn share_event() -> EgressEvent {
        EgressEvent::Share(ShareAnnouncement {
            submission: json!({"body": {"n": "1"}, "canonTag": 0, "ts": 1}),
            head: HeadSummary {
                height: 0,
                c: "22".repeat(32),
            },
        })
    }

    fn block_event() -> EgressEvent {
        EgressEvent::Block(BlockAnnouncement {
            height: 1,
            c: "33".repeat(32),
            block: json!({"height": 1}),
            head: HeadSummary {
                height: 1,
                c: "33".repeat(32),
            },
        })
    }

    #[test]
    fn disconnected_peer_accounts_for_the_event_kind_without_changing_submit_outcome() {
        let metrics = Arc::new(P2pMetrics::default());
        let (tx, rx) = std::sync::mpsc::sync_channel(EGRESS_QUEUE_CAPACITY);
        drop(rx);
        let fanout = P2pEgressFanout {
            peers: vec![tx],
            metrics: metrics.clone(),
        };

        fanout.try_send(block_event());
        assert_eq!(metrics.egress_block_failures.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.egress_failures.load(Ordering::Relaxed), 0);

        fanout.try_send(share_event());
        assert_eq!(metrics.egress_block_failures.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.egress_failures.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn full_peer_queue_counts_both_the_shed_reason_and_event_kind() {
        let metrics = Arc::new(P2pMetrics::default());
        let (tx, _rx) = std::sync::mpsc::sync_channel(EGRESS_QUEUE_CAPACITY);
        let fanout = P2pEgressFanout {
            peers: vec![tx],
            metrics: metrics.clone(),
        };
        for _ in 0..EGRESS_QUEUE_CAPACITY {
            fanout.try_send(share_event());
        }

        fanout.try_send(share_event());
        fanout.try_send(block_event());

        assert_eq!(metrics.egress_queue_full_drops.load(Ordering::Relaxed), 2);
        assert_eq!(metrics.egress_failures.load(Ordering::Relaxed), 1);
        assert_eq!(metrics.egress_block_failures.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn bounded_peer_queues_do_not_let_a_slow_peer_delay_a_healthy_peer() {
        let slow_listener = TcpListener::bind("127.0.0.1:0").expect("slow bind");
        let slow_addr = slow_listener.local_addr().expect("slow address");
        let healthy_listener = TcpListener::bind("127.0.0.1:0").expect("healthy bind");
        let healthy_addr = healthy_listener.local_addr().expect("healthy address");
        let (slow_started_tx, slow_started_rx) = mpsc::channel();
        let (release_slow_tx, release_slow_rx) = mpsc::channel();
        let slow_server = thread::spawn(move || {
            let (stream, _) = slow_listener.accept().expect("slow accept");
            let transport = TcpTransport::new();
            let mut conn = TcpTransport::conn_from_stream(stream).expect("slow conn");
            let _hello = transport.recv_frame(&mut conn).expect("slow hello");
            slow_started_tx.send(()).expect("slow started");
            let _ = release_slow_rx.recv();
        });

        let event_count = EGRESS_QUEUE_CAPACITY + 2;
        let healthy_identity = identity();
        let (healthy_tx, healthy_rx) = mpsc::channel();
        let healthy_server = thread::spawn(move || {
            let transport = TcpTransport::new();
            for _ in 0..event_count {
                let (stream, _) = healthy_listener.accept().expect("healthy accept");
                let mut conn = TcpTransport::conn_from_stream(stream).expect("healthy conn");
                let hello = transport.recv_frame(&mut conn).expect("healthy hello");
                assert!(healthy_identity.matches(&hello));
                transport
                    .send_frame(
                        &mut conn,
                        &Frame::Hello {
                            protocol_version: PROTOCOL_VERSION,
                            consensus_rule_version: CONSENSUS_RULE_VERSION,
                            network_id: healthy_identity.network_id.clone(),
                            genesis_hash: healthy_identity.genesis_hash.clone(),
                            head: HeadSummary {
                                height: 0,
                                c: "22".repeat(32),
                            },
                        },
                    )
                    .expect("healthy reply");
                assert!(matches!(
                    transport.recv_frame(&mut conn).expect("share announce"),
                    Frame::ShareAnnounce { .. }
                ));
                healthy_tx.send(()).expect("healthy delivered");
            }
        });

        let lifecycle = Arc::new(P2pLifecycle::new());
        let metrics = Arc::new(P2pMetrics::default());
        let (fanout, workers) = spawn_egress_workers(
            vec![slow_addr, healthy_addr],
            identity(),
            lifecycle.clone(),
            metrics.clone(),
        );

        fanout.try_send(share_event());
        slow_started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("slow peer is holding its worker");
        healthy_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("healthy peer receives before slow release");

        let started = Instant::now();
        for _ in 1..event_count {
            fanout.try_send(share_event());
            healthy_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("healthy peer keeps receiving");
        }
        assert!(started.elapsed() < EGRESS_IO_TIMEOUT);
        assert!(metrics.egress_queue_full_drops.load(Ordering::Relaxed) >= 1);

        lifecycle.stop();
        let _ = release_slow_tx.send(());
        drop(fanout);
        for worker in workers {
            worker.join().expect("egress worker joins");
        }
        slow_server.join().expect("slow server joins");
        healthy_server.join().expect("healthy server joins");
    }
}
