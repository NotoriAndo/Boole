//! Bounded, closed-local native synchronization over mutually pinned TLS.
//! Wire version 1 is independent of the legacy Frame protocol version.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use boole_core::native_chain::{NativeBlock, NativeChain, NativeTransfer};
use boole_core::native_network::native_testnet;
use boole_core::Hex32;
use boole_p2p::{PeerId, TlsConn, TlsIdentity, TlsTransport};
use serde::{Deserialize, Serialize};

use crate::native_node::{MAX_NATIVE_HISTORY_BLOCKS, MAX_NATIVE_PENDING_TRANSFERS};
use crate::p2p_lifecycle::P2pLifecycle;
use crate::NativeNode;

pub const MAX_NATIVE_PEERS: usize = 8;
pub const MAX_NATIVE_PEER_WORKERS: usize = 4;
pub const MAX_NATIVE_PEER_MESSAGE_BYTES: usize = 1024 * 1024;
pub const MAX_NATIVE_PEER_ROUND_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_NATIVE_PEER_ROUND_BLOCKS: usize = 256;
pub const NATIVE_PEER_PROTOCOL_VERSION: u32 = 1;
const MAX_REQUESTS: usize = 64;
const BLOCK_PAGE: usize = 16;
const TRANSFER_PAGE: usize = 128;
const ROUND_TIME: Duration = Duration::from_secs(10);
const HANDSHAKE_TIME: Duration = Duration::from_secs(2);
const POLL_TIME: Duration = Duration::from_millis(500);
const PEER_COOLDOWN: Duration = Duration::from_millis(500);

#[derive(Clone)]
pub struct NativePeerConfig {
    pub identity: TlsIdentity,
    pub peers: Vec<(SocketAddr, PeerId)>,
}

impl NativePeerConfig {
    /// Validate before binding sockets or opening mutable node state.
    pub fn validate(&self) -> anyhow::Result<()> {
        self.transport().map(|_| ())
    }

    fn transport(&self) -> anyhow::Result<TlsTransport> {
        anyhow::ensure!(
            self.peers.len() <= MAX_NATIVE_PEERS,
            "native peer count limit"
        );
        anyhow::ensure!(
            self.peers
                .iter()
                .all(|(address, _)| address.ip().is_loopback()),
            "native peers are closed-local only"
        );
        Ok(TlsTransport::new(
            self.identity.clone(),
            self.peers.clone(),
        )?)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePeerStatus {
    pub peer_id: String,
    pub address: String,
    pub successful_rounds: u64,
    pub failed_rounds: u64,
    pub consecutive_failures: u32,
    /// The last selected delay, not a continuously updated countdown.
    pub retry_delay_ms: u64,
    /// A snapshot match, never a finality or public-connectivity claim.
    pub state: &'static str,
    /// Fixed local phase of the last failed round, cleared on success. This is
    /// not a root-cause verdict and never contains a remote/error string.
    pub last_failure_stage: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePeerLimits {
    pub max_peers: usize,
    pub max_outbound_workers: usize,
    pub max_inbound_workers: usize,
    pub max_message_bytes: usize,
    pub max_round_bytes: usize,
    pub max_round_blocks: usize,
    pub max_round_requests: usize,
    pub handshake_timeout_ms: u128,
    pub round_timeout_ms: u128,
    pub inbound_key_cooldown_ms: u128,
    pub max_handshakes_per_second: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePeerSnapshot {
    pub enabled: bool,
    pub running: bool,
    pub local_peer_id: String,
    pub listen_address: String,
    pub limits: NativePeerLimits,
    pub peers: Vec<NativePeerStatus>,
    pub accepted_connections: u64,
    pub rejected_connections: u64,
    pub authenticated_connections: u64,
    pub authentication_failures: u64,
    pub rejected_peer_rounds: u64,
    pub completed_inbound_rounds: u64,
    pub failed_inbound_rounds: u64,
    pub active_inbound_workers: usize,
    pub peak_inbound_workers: usize,
    /// Executing a connect/handshake/round, excluding independent backoff.
    pub active_outbound_rounds: usize,
    pub peak_outbound_rounds: usize,
}

/// A read-only observation handle. It does not keep the durable node open.
#[derive(Clone)]
pub struct NativePeerMonitor {
    snapshot: Arc<Mutex<NativePeerSnapshot>>,
    lifecycle: Arc<P2pLifecycle>,
}

impl NativePeerMonitor {
    pub fn snapshot(&self) -> NativePeerSnapshot {
        let mut snapshot = self
            .snapshot
            .lock()
            .expect("native peer status lock")
            .clone();
        snapshot.running = !self.lifecycle.is_stopped();
        snapshot
    }

    fn update(&self, action: impl FnOnce(&mut NativePeerSnapshot)) {
        action(&mut self.snapshot.lock().expect("native peer status lock"));
    }
}

struct Shared {
    node: Arc<Mutex<NativeNode>>,
    transport: TlsTransport,
    lifecycle: Arc<P2pLifecycle>,
    active: Mutex<BTreeSet<PeerId>>,
    next_inbound: Mutex<BTreeMap<PeerId, Instant>>,
    monitor: NativePeerMonitor,
}

pub struct NativePeerService {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

impl NativePeerService {
    pub fn start(
        listener: TcpListener,
        node: Arc<Mutex<NativeNode>>,
        config: NativePeerConfig,
    ) -> anyhow::Result<Self> {
        Self::start_with_lifecycle(listener, node, config, Arc::new(P2pLifecycle::new()))
    }

    pub(crate) fn start_with_lifecycle(
        listener: TcpListener,
        node: Arc<Mutex<NativeNode>>,
        config: NativePeerConfig,
        lifecycle: Arc<P2pLifecycle>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            listener.local_addr()?.ip().is_loopback(),
            "native peers are closed-local only"
        );
        let transport = config.transport()?;
        lock_node(&node)?.ensure_ready()?;
        listener.set_nonblocking(true)?;
        let monitor = NativePeerMonitor {
            lifecycle: lifecycle.clone(),
            snapshot: Arc::new(Mutex::new(NativePeerSnapshot {
                enabled: true,
                running: true,
                local_peer_id: config.identity.peer_id().to_hex(),
                listen_address: listener.local_addr()?.to_string(),
                limits: NativePeerLimits {
                    max_peers: MAX_NATIVE_PEERS,
                    max_outbound_workers: MAX_NATIVE_PEERS,
                    max_inbound_workers: MAX_NATIVE_PEER_WORKERS,
                    max_message_bytes: MAX_NATIVE_PEER_MESSAGE_BYTES,
                    max_round_bytes: MAX_NATIVE_PEER_ROUND_BYTES,
                    max_round_blocks: MAX_NATIVE_PEER_ROUND_BLOCKS,
                    max_round_requests: MAX_REQUESTS,
                    handshake_timeout_ms: HANDSHAKE_TIME.as_millis(),
                    round_timeout_ms: ROUND_TIME.as_millis(),
                    inbound_key_cooldown_ms: PEER_COOLDOWN.as_millis(),
                    max_handshakes_per_second: 8,
                },
                peers: config
                    .peers
                    .iter()
                    .map(|(address, key)| NativePeerStatus {
                        peer_id: key.to_hex(),
                        address: address.to_string(),
                        successful_rounds: 0,
                        failed_rounds: 0,
                        consecutive_failures: 0,
                        retry_delay_ms: 0,
                        state: "not_connected",
                        last_failure_stage: None,
                    })
                    .collect(),
                accepted_connections: 0,
                rejected_connections: 0,
                authenticated_connections: 0,
                authentication_failures: 0,
                rejected_peer_rounds: 0,
                completed_inbound_rounds: 0,
                failed_inbound_rounds: 0,
                active_inbound_workers: 0,
                peak_inbound_workers: 0,
                active_outbound_rounds: 0,
                peak_outbound_rounds: 0,
            })),
        };
        let shared = Arc::new(Shared {
            node,
            transport,
            lifecycle,
            active: Mutex::new(BTreeSet::new()),
            // Fixed configured keys only: no attacker-selected label growth.
            next_inbound: Mutex::new(
                config
                    .peers
                    .iter()
                    .map(|(_, key)| (*key, Instant::now()))
                    .collect(),
            ),
            monitor,
        });
        let incoming = shared.clone();
        let acceptor = thread::Builder::new()
            .name("native-peer-accept".into())
            .spawn(move || accept_loop(listener, incoming))?;
        let mut threads = vec![acceptor];
        // Only the bounded, statically configured peer set creates workers.
        // A slow peer cannot serialize every other peer's network I/O.
        for (index, (address, _)) in config.peers.into_iter().enumerate() {
            let outgoing = shared.clone();
            match thread::Builder::new()
                .name(format!("native-peer-sync-{index}"))
                .spawn(move || outbound_loop(outgoing, index, address))
            {
                Ok(worker) => threads.push(worker),
                Err(error) => {
                    shared.lifecycle.stop();
                    for worker in threads {
                        let _ = worker.join();
                    }
                    return Err(error.into());
                }
            }
        }
        Ok(Self { shared, threads })
    }

    pub fn status(&self) -> Vec<NativePeerStatus> {
        self.monitor().snapshot().peers
    }

    pub fn monitor(&self) -> NativePeerMonitor {
        self.shared.monitor.clone()
    }

    pub fn request_stop(&self) {
        self.shared.lifecycle.request_stop();
    }

    pub fn stop(&mut self) {
        self.shared.lifecycle.stop();
        for worker in self.threads.drain(..) {
            let _ = worker.join();
        }
    }
}

impl Drop for NativePeerService {
    fn drop(&mut self) {
        self.stop();
    }
}

fn lock_node(node: &Mutex<NativeNode>) -> anyhow::Result<MutexGuard<'_, NativeNode>> {
    node.lock()
        .map_err(|_| anyhow::anyhow!("native state lock poisoned"))
}

fn outbound_loop(shared: Arc<Shared>, index: usize, address: SocketAddr) {
    // One ephemeral entry per fixed peer, never an attacker-sized hash cache.
    let mut declined = None;
    while !shared.lifecycle.is_stopped() {
        let delay = {
            let _round = OutboundRound::new(shared.monitor.clone());
            let mut stage = "connect";
            let outcome = synchronize(&shared, address, &mut declined, &mut stage);
            let mut snapshot = shared
                .monitor
                .snapshot
                .lock()
                .expect("native peer status lock");
            let status = &mut snapshot.peers[index];
            match outcome {
                Ok(state) => {
                    status.successful_rounds = status.successful_rounds.saturating_add(1);
                    status.state = state;
                    status.last_failure_stage = None;
                    status.consecutive_failures = 0;
                    status.retry_delay_ms = POLL_TIME.as_millis() as u64;
                }
                Err(_) => {
                    status.failed_rounds = status.failed_rounds.saturating_add(1);
                    status.state = "retrying";
                    status.last_failure_stage = Some(stage);
                    status.consecutive_failures = status.consecutive_failures.saturating_add(1);
                    status.retry_delay_ms =
                        (500 * (1u64 << (status.consecutive_failures - 1).min(6))).min(30_000);
                }
            }
            Duration::from_millis(status.retry_delay_ms)
        };
        shared.lifecycle.wait_or_stop(delay);
    }
}

struct OutboundRound(NativePeerMonitor);
impl OutboundRound {
    fn new(monitor: NativePeerMonitor) -> Self {
        monitor.update(|snapshot| {
            snapshot.active_outbound_rounds += 1;
            snapshot.peak_outbound_rounds = snapshot
                .peak_outbound_rounds
                .max(snapshot.active_outbound_rounds);
        });
        Self(monitor)
    }
}
impl Drop for OutboundRound {
    fn drop(&mut self) {
        self.0
            .update(|snapshot| snapshot.active_outbound_rounds -= 1);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Head {
    height: u64,
    #[serde(with = "wire_hash")]
    hash: Hex32,
}

/// Remembers only a fully verified candidate that lost normal fork choice.
/// This can avoid work, never authorize adoption or trust a claimed balance.
struct DeclinedFork {
    local: Head,
    remote: Head,
}

mod wire_hash {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        value: &Hex32,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_hex())
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Hex32, D::Error> {
        let value = String::deserialize(deserializer)?;
        let hash = Hex32::from_hex(&value).map_err(serde::de::Error::custom)?;
        if hash.to_hex() != value {
            return Err(serde::de::Error::custom("noncanonical native peer hash"));
        }
        Ok(hash)
    }
}

fn head(chain: &NativeChain) -> Head {
    Head {
        height: chain.ledger().height(),
        hash: chain.head_hash(),
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Message {
    Hello {
        protocol_version: u32,
        network_id: String,
        #[serde(with = "wire_hash")]
        genesis_hash: Hex32,
        head: Head,
    },
    GetHash {
        snapshot: Head,
        height: u64,
    },
    Hash {
        snapshot: Head,
        height: u64,
        #[serde(with = "wire_hash")]
        hash: Hex32,
    },
    GetBlocks {
        snapshot: Head,
        from: u64,
        limit: usize,
    },
    Blocks {
        snapshot: Head,
        from: u64,
        blocks: Vec<NativeBlock>,
    },
    GetPending {
        snapshot: Head,
        offset: usize,
        limit: usize,
    },
    Pending {
        snapshot: Head,
        offset: usize,
        total: usize,
        transfers: Vec<NativeTransfer>,
    },
    Done,
}

struct Round<'a> {
    shared: &'a Shared,
    connection: TlsConn,
    deadline: Instant,
    bytes: usize,
    requests: usize,
}

impl<'a> Round<'a> {
    fn send(&mut self, message: &Message) -> anyhow::Result<()> {
        let cap = MAX_NATIVE_PEER_MESSAGE_BYTES.min(MAX_NATIVE_PEER_ROUND_BYTES - self.bytes);
        self.bytes += self.shared.transport.send_json_counted_until(
            &mut self.connection,
            message,
            cap,
            self.deadline,
        )?;
        Ok(())
    }
    fn recv(&mut self) -> anyhow::Result<Message> {
        anyhow::ensure!(!self.shared.lifecycle.is_stopped(), "native peer stopped");
        let cap = MAX_NATIVE_PEER_MESSAGE_BYTES.min(MAX_NATIVE_PEER_ROUND_BYTES - self.bytes);
        let (message, bytes) = self.shared.transport.recv_json_counted_until(
            &mut self.connection,
            cap,
            self.deadline,
        )?;
        self.bytes += bytes;
        Ok(message)
    }
    fn request(&mut self, message: &Message) -> anyhow::Result<Message> {
        self.requests += 1;
        anyhow::ensure!(self.requests <= MAX_REQUESTS, "native peer request limit");
        self.send(message)?;
        self.recv()
    }
}

fn hello(node: &NativeNode) -> Message {
    Message::Hello {
        protocol_version: NATIVE_PEER_PROTOCOL_VERSION,
        network_id: native_testnet().network_id().to_owned(),
        genesis_hash: native_testnet().genesis_hash(),
        head: head(node.chain()),
    }
}

fn remote_head(message: Message) -> anyhow::Result<Head> {
    let Message::Hello {
        protocol_version,
        network_id,
        genesis_hash,
        head,
    } = message
    else {
        anyhow::bail!("native hello required");
    };
    anyhow::ensure!(
        protocol_version == NATIVE_PEER_PROTOCOL_VERSION,
        "native peer version mismatch"
    );
    anyhow::ensure!(
        network_id == native_testnet().network_id()
            && genesis_hash == native_testnet().genesis_hash(),
        "native peer network mismatch"
    );
    anyhow::ensure!(
        head.height <= MAX_NATIVE_HISTORY_BLOCKS as u64,
        "native peer history limit"
    );
    anyhow::ensure!(
        head.height != 0 || head.hash == genesis_hash,
        "native peer genesis mismatch"
    );
    Ok(head)
}

fn chain_hash(chain: &NativeChain, height: u64) -> anyhow::Result<Hex32> {
    if height == 0 {
        return Ok(native_testnet().genesis_hash());
    }
    chain
        .blocks()
        .get((height - 1) as usize)
        .ok_or_else(|| anyhow::anyhow!("native hash height unavailable"))?
        .hash()
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    let mut workers: Vec<JoinHandle<()>> = Vec::new();
    let mut window = Instant::now();
    let mut attempts = 0;
    while !shared.lifecycle.is_stopped() {
        let mut index = 0;
        while index < workers.len() {
            if workers[index].is_finished() {
                let _ = workers.swap_remove(index).join();
            } else {
                index += 1;
            }
        }
        if window.elapsed() >= Duration::from_secs(1) {
            window = Instant::now();
            attempts = 0;
        }
        match listener.accept() {
            Ok((socket, address)) => {
                if !address.ip().is_loopback()
                    || workers.len() >= MAX_NATIVE_PEER_WORKERS
                    || attempts >= 8
                {
                    shared.monitor.update(|snapshot| {
                        snapshot.rejected_connections =
                            snapshot.rejected_connections.saturating_add(1);
                    });
                    shared.lifecycle.wait_or_stop(Duration::from_millis(1));
                    continue;
                }
                attempts += 1;
                let owner = shared.clone();
                let lease = InboundWorker::new(shared.monitor.clone());
                if let Ok(worker) = thread::Builder::new()
                    .name("native-peer-inbound".into())
                    .spawn(move || {
                        let _lease = lease;
                        let outcome = serve_round(&owner, socket);
                        owner.monitor.update(|snapshot| {
                            if outcome.is_ok() {
                                snapshot.completed_inbound_rounds =
                                    snapshot.completed_inbound_rounds.saturating_add(1);
                            } else {
                                snapshot.failed_inbound_rounds =
                                    snapshot.failed_inbound_rounds.saturating_add(1);
                            }
                        });
                    })
                {
                    workers.push(worker);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                shared.lifecycle.wait_or_stop(Duration::from_millis(10));
            }
            Err(_) => {
                shared.lifecycle.request_stop();
                break;
            }
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
}

struct InboundWorker(NativePeerMonitor);
impl InboundWorker {
    fn new(monitor: NativePeerMonitor) -> Self {
        monitor.update(|snapshot| {
            snapshot.accepted_connections = snapshot.accepted_connections.saturating_add(1);
            snapshot.active_inbound_workers += 1;
            snapshot.peak_inbound_workers = snapshot
                .peak_inbound_workers
                .max(snapshot.active_inbound_workers);
        });
        Self(monitor)
    }
}
impl Drop for InboundWorker {
    fn drop(&mut self) {
        self.0
            .update(|snapshot| snapshot.active_inbound_workers -= 1);
    }
}

struct ActivePeer<'a> {
    shared: &'a Shared,
    key: PeerId,
}
impl Drop for ActivePeer<'_> {
    fn drop(&mut self) {
        self.shared
            .active
            .lock()
            .expect("native active peers")
            .remove(&self.key);
    }
}

fn serve_round(shared: &Shared, socket: TcpStream) -> anyhow::Result<()> {
    let _socket = shared.lifecycle.register(&socket)?;
    let connection = shared
        .transport
        .accept_stream_until(socket, Instant::now() + HANDSHAKE_TIME)
        .inspect_err(|_| {
            shared.monitor.update(|snapshot| {
                snapshot.authentication_failures =
                    snapshot.authentication_failures.saturating_add(1);
            })
        })?;
    shared.monitor.update(|snapshot| {
        snapshot.authenticated_connections = snapshot.authenticated_connections.saturating_add(1);
    });
    let key = connection.peer_id();
    {
        let mut limits = shared
            .next_inbound
            .lock()
            .expect("native peer cooldown lock");
        let next = limits
            .get_mut(&key)
            .ok_or_else(|| anyhow::anyhow!("unconfigured native peer"))?;
        if Instant::now() < *next {
            shared.monitor.update(|snapshot| {
                snapshot.rejected_peer_rounds = snapshot.rejected_peer_rounds.saturating_add(1);
            });
            anyhow::bail!("native peer round cooldown");
        }
        *next = Instant::now() + PEER_COOLDOWN;
    }
    anyhow::ensure!(
        shared
            .active
            .lock()
            .expect("native active peers")
            .insert(key),
        "native peer already active"
    );
    let _active = ActivePeer { shared, key };
    let mut round = Round {
        shared,
        connection,
        deadline: Instant::now() + ROUND_TIME,
        bytes: 0,
        requests: 0,
    };
    remote_head(round.recv()?)?;
    let (greeting, advertised) = {
        let node = lock_node(&shared.node)?;
        node.ensure_ready()?;
        (hello(&node), head(node.chain()))
    };
    round.send(&greeting)?;
    let mut pending = None;
    for _ in 0..MAX_REQUESTS {
        let request = round.recv()?;
        if matches!(request, Message::Done) {
            return Ok(());
        }
        let response = {
            let node = lock_node(&shared.node)?;
            node.ensure_ready()?;
            match request {
                Message::GetHash { snapshot, height } => {
                    anyhow::ensure!(
                        snapshot == advertised
                            && head(node.chain()) == snapshot
                            && height <= snapshot.height,
                        "native snapshot changed"
                    );
                    Message::Hash {
                        snapshot,
                        height,
                        hash: chain_hash(node.chain(), height)?,
                    }
                }
                Message::GetBlocks {
                    snapshot,
                    from,
                    limit,
                } => {
                    anyhow::ensure!(
                        snapshot == advertised && head(node.chain()) == snapshot,
                        "native snapshot changed"
                    );
                    anyhow::ensure!(
                        from > 0 && from <= snapshot.height && limit > 0 && limit <= BLOCK_PAGE,
                        "native block range limit"
                    );
                    let mut blocks = Vec::new();
                    let mut bytes = 512;
                    for block in node
                        .chain()
                        .blocks()
                        .iter()
                        .skip((from - 1) as usize)
                        .take(limit)
                    {
                        let size = serde_json::to_vec(block)?.len() + 1;
                        if bytes + size >= MAX_NATIVE_PEER_MESSAGE_BYTES {
                            break;
                        }
                        bytes += size;
                        blocks.push(block.clone());
                    }
                    anyhow::ensure!(!blocks.is_empty(), "native block cannot fit page");
                    Message::Blocks {
                        snapshot,
                        from,
                        blocks,
                    }
                }
                Message::GetPending {
                    snapshot,
                    offset,
                    limit,
                } => {
                    anyhow::ensure!(
                        snapshot == advertised && head(node.chain()) == snapshot,
                        "native snapshot changed"
                    );
                    let pending = pending.get_or_insert_with(|| node.pending().to_vec());
                    anyhow::ensure!(
                        offset <= pending.len() && limit > 0 && limit <= TRANSFER_PAGE,
                        "native pending page limit"
                    );
                    Message::Pending {
                        snapshot,
                        offset,
                        total: pending.len(),
                        transfers: pending.iter().skip(offset).take(limit).cloned().collect(),
                    }
                }
                _ => anyhow::bail!("unexpected native peer request"),
            }
        };
        round.send(&response)?;
    }
    anyhow::bail!("native peer request limit")
}

fn get_hash(round: &mut Round<'_>, snapshot: &Head, height: u64) -> anyhow::Result<Hex32> {
    match round.request(&Message::GetHash {
        snapshot: snapshot.clone(),
        height,
    })? {
        Message::Hash {
            snapshot: returned,
            height: actual,
            hash,
        } if returned == *snapshot && actual == height => Ok(hash),
        _ => anyhow::bail!("native hash response mismatch"),
    }
}

fn synchronize(
    shared: &Shared,
    address: SocketAddr,
    declined: &mut Option<DeclinedFork>,
    stage: &mut &'static str,
) -> anyhow::Result<&'static str> {
    let socket = TcpStream::connect_timeout(&address, Duration::from_millis(500))?;
    let _socket = shared.lifecycle.register(&socket)?;
    *stage = "tls_handshake";
    let connection = shared
        .transport
        .connect_stream_until(socket, Instant::now() + HANDSHAKE_TIME)?;
    let mut round = Round {
        shared,
        connection,
        deadline: Instant::now() + ROUND_TIME,
        bytes: 0,
        requests: 0,
    };
    *stage = "local_state";
    let (greeting, original, earliest_recent_fork) = {
        let node = lock_node(&shared.node)?;
        node.ensure_ready()?;
        (
            hello(&node),
            head(node.chain()),
            node.chain().earliest_recent_fork_height(),
        )
    };
    *stage = "hello";
    let remote = remote_head(round.request(&greeting)?)?;
    if declined
        .as_ref()
        .is_some_and(|known| known.local == original && known.remote == remote)
    {
        // The network exchange may have raced a local block or a storage fault.
        // A cached preference must not mask either change in readiness/state.
        {
            *stage = "local_state";
            let node = lock_node(&shared.node)?;
            node.ensure_ready()?;
            *stage = "local_snapshot";
            anyhow::ensure!(
                head(node.chain()) == original,
                "native local snapshot changed"
            );
        }
        *stage = "round_finish";
        round.send(&Message::Done)?;
        return Ok("local_chain_preferred");
    }
    *declined = None;
    if remote == original {
        synchronize_pending(&mut round, &remote, stage)?;
        *stage = "round_finish";
        round.send(&Message::Done)?;
        return Ok("snapshot_match");
    }
    // Search only hashes; no unvalidated peer work/height can become authority.
    let mut low = 0;
    let mut high = original.height.min(remote.height);
    while low < high {
        let height = low + (high - low).div_ceil(2);
        *stage = "hash_sync";
        let remote_hash = get_hash(&mut round, &remote, height)?;
        *stage = "local_state";
        let node = lock_node(&shared.node)?;
        *stage = "local_snapshot";
        anyhow::ensure!(
            head(node.chain()) == original,
            "native local snapshot changed"
        );
        if chain_hash(node.chain(), height)? == remote_hash {
            low = height;
        } else {
            high = height - 1;
        }
    }
    let common = low;
    let extension = common == original.height;
    if !extension
        && (remote.height - common > MAX_NATIVE_PEER_ROUND_BLOCKS as u64
            || common < earliest_recent_fork)
    {
        *stage = "round_finish";
        round.send(&Message::Done)?;
        return Ok("bounded_reorg_requires_recovery");
    }
    let target = remote
        .height
        .min(common + MAX_NATIVE_PEER_ROUND_BLOCKS as u64);
    let mut next = common + 1;
    let mut current = original.clone();
    let mut fork = Vec::new();
    while next <= target {
        let limit = BLOCK_PAGE.min((target - next + 1) as usize);
        *stage = "block_download";
        let Message::Blocks {
            snapshot,
            from,
            blocks,
        } = round.request(&Message::GetBlocks {
            snapshot: remote.clone(),
            from: next,
            limit,
        })?
        else {
            anyhow::bail!("native blocks response required");
        };
        anyhow::ensure!(
            snapshot == remote && from == next && !blocks.is_empty() && blocks.len() <= limit,
            "native block response range mismatch"
        );
        for block in blocks {
            *stage = "block_response";
            anyhow::ensure!(block.header.height == next, "native block height mismatch");
            if extension {
                *stage = "block_apply";
                let _permit = shared
                    .lifecycle
                    .begin_mutation()
                    .ok_or_else(|| anyhow::anyhow!("native peer stopped"))?;
                let mut node = lock_node(&shared.node)?;
                *stage = "local_snapshot";
                anyhow::ensure!(
                    head(node.chain()) == current,
                    "native local snapshot changed"
                );
                *stage = "block_apply";
                node.submit_block(block)?;
                current = head(node.chain());
            } else {
                fork.push(block);
            }
            next += 1;
        }
    }
    if !extension && !fork.is_empty() {
        *stage = "fork_response";
        anyhow::ensure!(
            fork.last().expect("fork nonempty").hash()? == remote.hash,
            "native advertised head mismatch"
        );
        *stage = "fork_apply";
        let _permit = shared
            .lifecycle
            .begin_mutation()
            .ok_or_else(|| anyhow::anyhow!("native peer stopped"))?;
        let mut node = lock_node(&shared.node)?;
        *stage = "local_snapshot";
        anyhow::ensure!(
            head(node.chain()) == original,
            "native local snapshot changed"
        );
        *stage = "fork_apply";
        if !node.adopt_recent_suffix(common, &fork)? {
            *declined = Some(DeclinedFork {
                local: original.clone(),
                remote: remote.clone(),
            });
        }
        current = head(node.chain());
    }
    if extension && target == remote.height {
        *stage = "block_response";
        anyhow::ensure!(current == remote, "native advertised head mismatch");
    }
    if current == remote {
        synchronize_pending(&mut round, &remote, stage)?;
    }
    *stage = "round_finish";
    round.send(&Message::Done)?;
    Ok(if current == remote {
        "snapshot_match"
    } else if extension {
        "catching_up"
    } else {
        "local_chain_preferred"
    })
}

fn synchronize_pending(
    round: &mut Round<'_>,
    snapshot: &Head,
    stage: &mut &'static str,
) -> anyhow::Result<()> {
    let mut offset = 0;
    let mut expected_total = None;
    loop {
        *stage = "pending_sync";
        let Message::Pending {
            snapshot: returned,
            offset: actual,
            total,
            transfers,
        } = round.request(&Message::GetPending {
            snapshot: snapshot.clone(),
            offset,
            limit: TRANSFER_PAGE,
        })?
        else {
            anyhow::bail!("native pending response required");
        };
        anyhow::ensure!(
            returned == *snapshot
                && actual == offset
                && total <= MAX_NATIVE_PENDING_TRANSFERS
                && offset <= total,
            "native pending response mismatch"
        );
        anyhow::ensure!(
            transfers.len() == TRANSFER_PAGE.min(total - offset),
            "native pending response count mismatch"
        );
        anyhow::ensure!(
            *expected_total.get_or_insert(total) == total,
            "native pending snapshot changed"
        );
        offset += transfers.len();
        for transfer in transfers {
            // Cryptographic/schema failures reject the peer round. Honest
            // nonce/funds/pool conflicts may differ between pending queues.
            *stage = "pending_signature";
            transfer.validated_fields()?;
            *stage = "pending_admission";
            let _permit = round
                .shared
                .lifecycle
                .begin_mutation()
                .ok_or_else(|| anyhow::anyhow!("native peer stopped"))?;
            let mut node = lock_node(&round.shared.node)?;
            *stage = "local_snapshot";
            anyhow::ensure!(
                head(node.chain()) == *snapshot,
                "native local snapshot changed"
            );
            *stage = "pending_admission";
            if node.submit_transfer(transfer).is_err() {
                // A failed publication poisons NativeNode. Never disguise a
                // durability/ownership fault as a benign admission conflict.
                *stage = "local_state";
                node.ensure_ready()?;
            }
        }
        if offset == total {
            return Ok(());
        }
    }
}
