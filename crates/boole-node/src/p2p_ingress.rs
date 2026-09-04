//! N3.2 — share-gossip ingress: accept allowlisted peers, validate `Hello`,
//! and re-admit every announced share through the exact local admission path
//! (`admit_parsed_submission_typed` via the runtime wrapper) — ADR-0009 (e):
//! peers are never trusted, and there is no second validation policy.
//!
//! The thread is a plain blocking `std::thread` (the transport is blocking
//! `std::net` by design, ADR-0009 (a)). Cheap admission and final commit each
//! take a short guard on the SAME `tokio::sync::RwLock` the HTTP path uses;
//! the pinned Lean subprocess runs between those guards so readiness/status
//! stay responsive. Finalization revalidates the current head/dedup state
//! before restoring a share or durably adopting a block/chain.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use boole_core::{LocalPackageStore, LocalPackageStoreError, PackageRoot, CONSENSUS_RULE_VERSION};
use boole_p2p::{
    Frame, FrameError, HeadSummary, TcpConn, TcpTransport, Transport, GET_BLOCKS_RANGE_CAP,
    MAX_FRAME_BYTES, PROTOCOL_VERSION,
};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::local_node::{
    blocks_range_values, head_summary, ingest_announced_block_shared,
    ingest_candidate_chain_shared, ingress_admit_share_shared, CandidateChainOutcome,
    HttpRateLimiter, IngressBlockOutcome, IngressShareOutcome, LocalNodeState,
};
use crate::p2p_egress::open_validated_conn_until;
use crate::p2p_lifecycle::P2pLifecycle;
use crate::p2p_package_fetch::PackageFetchingConfig;

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

/// Explicit least-authority grant for BF.6a package serving. A configured
/// static peer must still pass the existing IP allowlist, and only roots in
/// this frozen set may leave the local store. Constructing a store alone
/// never authorizes network disclosure.
pub struct PackageServingConfig {
    store: Arc<LocalPackageStore>,
    served_package_roots: BTreeSet<PackageRoot>,
}

impl PackageServingConfig {
    pub fn new(
        store: Arc<LocalPackageStore>,
        served_package_roots: impl IntoIterator<Item = PackageRoot>,
    ) -> Self {
        Self {
            store,
            served_package_roots: served_package_roots.into_iter().collect(),
        }
    }

    fn read_authorized(&self, root: PackageRoot) -> AuthorizedPackageRead {
        if !self.served_package_roots.contains(&root) {
            return AuthorizedPackageRead::Unavailable;
        }
        match self.store.read(root) {
            Ok(bytes) => AuthorizedPackageRead::Available(bytes),
            Err(
                LocalPackageStoreError::Disabled | LocalPackageStoreError::MissingObject { .. },
            ) => AuthorizedPackageRead::Unavailable,
            Err(_) => AuthorizedPackageRead::StoreError,
        }
    }
}

enum AuthorizedPackageRead {
    Available(Vec<u8>),
    Unavailable,
    StoreError,
}

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
    /// Exact-root network disclosure grant. `None` is the production default:
    /// GetPackage receives an explicit unavailable response with no disk read.
    pub package_serving: Option<PackageServingConfig>,
    /// Explicit node-owned package fetch intake. Supplied requests are merged
    /// into the durable unresolved-intent authority before any dial; restart
    /// recovers that authority even when this intake is empty. `None` is the
    /// production default: no package dial, receive, journal, or CAS write can
    /// occur.
    pub package_fetching: Option<PackageFetchingConfig>,
    /// N5.3 — keep `/ready` closed until every configured static bootstrap
    /// endpoint has completed an outbound, identity-checked sync round and
    /// advertised the node's current exact `(height, c)` head. Legacy
    /// embeddings leave this false and retain their existing readiness
    /// semantics.
    pub require_head_sync_for_readiness: bool,
}

/// N5.3 — process-local observations made only by the outbound sync path.
///
/// An endpoint is satisfied only after its authenticated-by-network-identity
/// `Hello` head and the live local head are exactly equal at the end of a sync
/// round. The HTTP readiness path compares these observations with the live
/// head again on every request, so a later local mutation cannot reuse stale
/// evidence. Socket endpoints are operational identity only; this remains a
/// controlled-loopback contract, not cryptographic peer authentication.
#[derive(Debug)]
pub(crate) struct P2pBootstrapReadiness {
    observed_heads: BTreeMap<SocketAddr, Option<HeadSummary>>,
}

impl P2pBootstrapReadiness {
    pub(crate) fn new(peers: &[SocketAddr]) -> Result<Self, String> {
        let mut observed_heads = BTreeMap::new();
        for peer in peers {
            if observed_heads.insert(*peer, None).is_some() {
                return Err(format!("duplicate bootstrap peer endpoint: {peer}"));
            }
        }
        if observed_heads.is_empty() {
            return Err("bootstrap head-sync readiness requires at least one peer".to_string());
        }
        Ok(Self { observed_heads })
    }

    pub(crate) fn observe_exact(&mut self, peer: SocketAddr, head: HeadSummary) {
        if let Some(slot) = self.observed_heads.get_mut(&peer) {
            *slot = Some(head);
        }
    }

    pub(crate) fn clear(&mut self, peer: SocketAddr) {
        if let Some(slot) = self.observed_heads.get_mut(&peer) {
            *slot = None;
        }
    }

    pub(crate) fn matches_live_head(&self, live_head: &HeadSummary) -> bool {
        !self.observed_heads.is_empty()
            && self
                .observed_heads
                .values()
                .all(|head| head.as_ref() == Some(live_head))
    }
}

pub(crate) struct P2pIngressRuntimeConfig {
    pub(crate) listener: TcpListener,
    pub(crate) allowlist: Vec<IpAddr>,
    pub(crate) identity: P2pIdentity,
    pub(crate) rate_limit_per_60s: usize,
    pub(crate) package_serving: Option<PackageServingConfig>,
}

/// The identity fields both `Hello` directions must agree on before any
/// other frame is processed (ADR-0009 (b)/(e)).
#[derive(Clone)]
pub(crate) struct P2pIdentity {
    pub(crate) network_id: String,
    /// N5.2 — the content-addressed genesis identity (`GenesisSpec.hash()`,
    /// N5.1), NOT the raw chain anchor: peers that agree on the anchor but
    /// differ on any committed consensus parameter must refuse to gossip.
    pub(crate) genesis_hash: String,
}

impl P2pIdentity {
    pub(crate) fn hello(&self, head: HeadSummary) -> Frame {
        Frame::Hello {
            protocol_version: PROTOCOL_VERSION,
            consensus_rule_version: CONSENSUS_RULE_VERSION,
            network_id: self.network_id.clone(),
            genesis_hash: self.genesis_hash.clone(),
            head,
        }
    }

    /// A peer `Hello` matches iff protocol_version, consensus_rule_version,
    /// network_id AND genesis_hash all agree. `genesis_hash` carries the
    /// N5.2 per-network genesis commitment (the spec hash);
    /// `consensus_rule_version` (ADR-0014 (b)) keeps a peer enforcing a
    /// different block-validity rule set from gossiping with us — same
    /// shares, different chosen blocks is a silent fork.
    pub(crate) fn matches(&self, frame: &Frame) -> bool {
        matches!(
            frame,
            Frame::Hello {
                protocol_version,
                consensus_rule_version,
                network_id,
                genesis_hash,
                ..
            } if *protocol_version == PROTOCOL_VERSION
                && *consensus_rule_version == CONSENSUS_RULE_VERSION
                && network_id == &self.network_id
                && genesis_hash == &self.genesis_hash
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
    /// Share admission whose semantic verifier was busy or unavailable.
    /// Deferred is distinct from invalid and leaves no eligible candidate.
    pub(crate) ingress_shares_deferred: AtomicU64,
    pub(crate) ingress_blocks_ingested: AtomicU64,
    pub(crate) ingress_blocks_rejected: AtomicU64,
    /// SC.10-ii-b — peer blocks whose pinned-checker re-verify could not
    /// reach a verdict (containment / availability); deferred, not adopted
    /// and not rejected (ADR-0016 (a-3)).
    pub(crate) ingress_blocks_deferred: AtomicU64,
    /// SC.10-iii-c-2 — peer blocks adopted WITHOUT running the pinned checker
    /// because they fall within this node's verified-prefix checkpoint
    /// (`assumevalid`); structural replay still ran. Node-local perf state.
    pub(crate) ingress_blocks_reverify_skipped_via_checkpoint: AtomicU64,
    pub(crate) ingress_block_announces_ignored: AtomicU64,
    pub(crate) ingress_rate_limited_drops: AtomicU64,
    pub(crate) ingress_get_blocks_served: AtomicU64,
    pub(crate) ingress_get_packages_served: AtomicU64,
    pub(crate) ingress_get_packages_unavailable: AtomicU64,
    pub(crate) ingress_get_packages_store_errors: AtomicU64,
    pub(crate) package_fetch_staged: AtomicU64,
    pub(crate) package_fetch_recovered: AtomicU64,
    pub(crate) package_fetch_unavailable: AtomicU64,
    pub(crate) package_fetch_invalid: AtomicU64,
    pub(crate) package_fetch_peer_failures: AtomicU64,
    pub(crate) package_fetch_store_errors: AtomicU64,
    pub(crate) sync_blocks_applied: AtomicU64,
    pub(crate) sync_reorgs_applied: AtomicU64,
    /// SC.10-ii-c — competing peer chains whose pinned-checker re-verify could
    /// not reach a verdict (containment / availability); deferred, not adopted
    /// and not rejected (ADR-0016 (a-3)).
    pub(crate) sync_reorgs_deferred: AtomicU64,
    pub(crate) sync_peer_failures: AtomicU64,
    pub(crate) sync_budget_drops: AtomicU64,
    pub(crate) sync_over_return_drops: AtomicU64,
    pub(crate) egress_announces: AtomicU64,
    pub(crate) egress_failures: AtomicU64,
    pub(crate) egress_queue_full_drops: AtomicU64,
    pub(crate) egress_block_announces: AtomicU64,
    pub(crate) egress_block_failures: AtomicU64,
}

pub(crate) fn spawn_ingress_thread(
    config: P2pIngressRuntimeConfig,
    state: Arc<RwLock<LocalNodeState>>,
    lifecycle: Arc<P2pLifecycle>,
    metrics: Arc<P2pMetrics>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("boole-p2p-ingress".to_string())
        .spawn(move || ingress_loop(config, state, lifecycle, metrics))
        .expect("spawn boole-p2p-ingress thread")
}

fn ingress_loop(
    config: P2pIngressRuntimeConfig,
    state: Arc<RwLock<LocalNodeState>>,
    lifecycle: Arc<P2pLifecycle>,
    metrics: Arc<P2pMetrics>,
) {
    let P2pIngressRuntimeConfig {
        listener,
        allowlist,
        identity,
        rate_limit_per_60s,
        package_serving,
    } = config;
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    // N3.3 — per-peer frame budget (ADR-0009 (c)). One limiter for the
    // whole ingress lifetime: the per-IP window must survive across the
    // short-lived announce connections, or a flooder could reset its
    // budget by reconnecting.
    let rate_limiter =
        (rate_limit_per_60s > 0).then(|| HttpRateLimiter::new(rate_limit_per_60s, 60_000));
    while !lifecycle.is_stopped() {
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
                let context = IngressConnectionContext {
                    identity: &identity,
                    state: &state,
                    lifecycle: &lifecycle,
                    metrics: &metrics,
                    rate_limiter: rate_limiter.as_ref(),
                    package_serving: package_serving.as_ref(),
                };
                handle_connection(stream, peer, &context);
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                lifecycle.wait_or_stop(ACCEPT_POLL_INTERVAL);
            }
            Err(_) => {
                lifecycle.wait_or_stop(ACCEPT_POLL_INTERVAL);
            }
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

struct IngressConnectionContext<'a> {
    identity: &'a P2pIdentity,
    state: &'a Arc<RwLock<LocalNodeState>>,
    lifecycle: &'a Arc<P2pLifecycle>,
    metrics: &'a Arc<P2pMetrics>,
    rate_limiter: Option<&'a HttpRateLimiter>,
    package_serving: Option<&'a PackageServingConfig>,
}

fn handle_connection(stream: TcpStream, peer: SocketAddr, context: &IngressConnectionContext<'_>) {
    let identity = context.identity;
    let state = context.state;
    let lifecycle = context.lifecycle;
    let metrics = context.metrics;
    let rate_limiter = context.rate_limiter;
    let package_serving = context.package_serving;
    // The accepted socket may inherit O_NONBLOCK from the listener on some
    // platforms (macOS); force blocking + bounded IO explicitly.
    if stream.set_nonblocking(false).is_err()
        || stream.set_read_timeout(Some(INGRESS_IO_TIMEOUT)).is_err()
        || stream.set_write_timeout(Some(INGRESS_IO_TIMEOUT)).is_err()
    {
        return;
    }
    let _lease = match lifecycle.register(&stream) {
        Ok(lease) => lease,
        Err(_) => return,
    };
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
    while !lifecycle.is_stopped() {
        match recv_frame_limited(&transport, &mut conn, &peer, rate_limiter, metrics) {
            Ok(Frame::ShareAnnounce { submission }) => {
                // Structural admission and final revalidation each use the
                // shared writer, while the pinned Lean subprocess runs with
                // no node-state guard held. This keeps readiness/status live
                // and shares the same bounded verifier permit as HTTP.
                let Some(_mutation) = lifecycle.begin_mutation() else {
                    return;
                };
                match ingress_admit_share_shared(state, &submission, &peer_ip) {
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
                    IngressShareOutcome::Deferred { .. } => {
                        metrics
                            .ingress_shares_deferred
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
                let Some(_mutation) = lifecycle.begin_mutation() else {
                    return;
                };
                match ingest_announced_block_shared(state, &block_value) {
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
                    IngressBlockOutcome::Deferred => {
                        // The pinned checker was unavailable or canonical
                        // publication is fail-closed; hold at the current head.
                        metrics
                            .ingress_blocks_deferred
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
            Ok(Frame::GetPackage { root }) => {
                // BF.6a — read-only serving under two independent grants:
                // the existing static peer-IP allowlist plus this exact root
                // allowlist. The store verifies object size and root again.
                // No node write lock is taken, so chain/admission state cannot
                // change on any availability or store-error outcome.
                let package_root =
                    PackageRoot::from_hex(&root).expect("wire-validated lowercase package root");
                let canonical_bytes = match package_serving
                    .map(|serving| serving.read_authorized(package_root))
                    .unwrap_or(AuthorizedPackageRead::Unavailable)
                {
                    AuthorizedPackageRead::Available(bytes) => {
                        metrics
                            .ingress_get_packages_served
                            .fetch_add(1, Ordering::Relaxed);
                        Some(bytes)
                    }
                    AuthorizedPackageRead::Unavailable => {
                        metrics
                            .ingress_get_packages_unavailable
                            .fetch_add(1, Ordering::Relaxed);
                        None
                    }
                    AuthorizedPackageRead::StoreError => {
                        metrics
                            .ingress_get_packages_store_errors
                            .fetch_add(1, Ordering::Relaxed);
                        None
                    }
                };
                if transport
                    .send_frame(
                        &mut conn,
                        &Frame::Package {
                            root,
                            canonical_bytes,
                        },
                    )
                    .is_err()
                {
                    return;
                }
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

/// One peer may advance a bounded amount per poll. This prevents an advertised
/// giant head or competing fork from growing one in-memory `Vec` without bound.
const MAX_SYNC_BLOCKS_PER_PEER_ROUND: u64 = 4_096;
const MAX_SYNC_WIRE_BYTES_PER_PEER_ROUND: usize = MAX_FRAME_BYTES * 4;
const MAX_SYNC_RESPONSES_PER_PEER_ROUND: u64 = 64;
const MAX_SYNC_PEER_ROUND_DURATION: Duration = Duration::from_secs(30);

#[derive(Debug, PartialEq, Eq)]
enum SyncBudgetError {
    ResponseOverReturn { requested: u64, returned: u64 },
    BlockLimit { attempted: u64, limit: u64 },
    WireByteLimit { attempted: usize, limit: usize },
    ResponseLimit { attempted: u64, limit: u64 },
    DeadlineExceeded,
}

struct SyncBudget {
    block_limit: u64,
    wire_byte_limit: usize,
    response_limit: u64,
    deadline: Instant,
    used_blocks: u64,
    used_wire_bytes: usize,
    used_responses: u64,
}

impl SyncBudget {
    fn new(
        block_limit: u64,
        wire_byte_limit: usize,
        response_limit: u64,
        deadline: Instant,
    ) -> Self {
        Self {
            block_limit,
            wire_byte_limit,
            response_limit,
            deadline,
            used_blocks: 0,
            used_wire_bytes: 0,
            used_responses: 0,
        }
    }

    fn for_peer_round() -> Self {
        Self::new(
            MAX_SYNC_BLOCKS_PER_PEER_ROUND,
            MAX_SYNC_WIRE_BYTES_PER_PEER_ROUND,
            MAX_SYNC_RESPONSES_PER_PEER_ROUND,
            Instant::now() + MAX_SYNC_PEER_ROUND_DURATION,
        )
    }

    fn charge_response(
        &mut self,
        requested: u64,
        returned: usize,
        wire_bytes: usize,
    ) -> Result<(), SyncBudgetError> {
        let returned = u64::try_from(returned).unwrap_or(u64::MAX);
        if returned > requested {
            return Err(SyncBudgetError::ResponseOverReturn {
                requested,
                returned,
            });
        }
        let attempted_blocks = self.used_blocks.saturating_add(returned);
        if attempted_blocks > self.block_limit {
            return Err(SyncBudgetError::BlockLimit {
                attempted: attempted_blocks,
                limit: self.block_limit,
            });
        }
        let attempted_wire_bytes = self.used_wire_bytes.saturating_add(wire_bytes);
        if attempted_wire_bytes > self.wire_byte_limit {
            return Err(SyncBudgetError::WireByteLimit {
                attempted: attempted_wire_bytes,
                limit: self.wire_byte_limit,
            });
        }
        let attempted_responses = self.used_responses.saturating_add(1);
        if attempted_responses > self.response_limit {
            return Err(SyncBudgetError::ResponseLimit {
                attempted: attempted_responses,
                limit: self.response_limit,
            });
        }
        self.used_blocks = attempted_blocks;
        self.used_wire_bytes = attempted_wire_bytes;
        self.used_responses = attempted_responses;
        Ok(())
    }

    fn ensure_before_receive(&self, now: Instant) -> Result<(), SyncBudgetError> {
        if now >= self.deadline {
            return Err(SyncBudgetError::DeadlineExceeded);
        }
        if self.used_blocks >= self.block_limit {
            return Err(SyncBudgetError::BlockLimit {
                attempted: self.used_blocks.saturating_add(1),
                limit: self.block_limit,
            });
        }
        if self.used_responses >= self.response_limit {
            return Err(SyncBudgetError::ResponseLimit {
                attempted: self.used_responses.saturating_add(1),
                limit: self.response_limit,
            });
        }
        if self.used_wire_bytes >= self.wire_byte_limit {
            return Err(SyncBudgetError::WireByteLimit {
                attempted: self.used_wire_bytes.saturating_add(1),
                limit: self.wire_byte_limit,
            });
        }
        Ok(())
    }

    fn ensure_before_mutation(&self, now: Instant) -> Result<(), SyncBudgetError> {
        if now >= self.deadline {
            Err(SyncBudgetError::DeadlineExceeded)
        } else {
            Ok(())
        }
    }

    fn remaining_blocks(&self) -> u64 {
        self.block_limit.saturating_sub(self.used_blocks)
    }

    fn used_blocks(&self) -> u64 {
        self.used_blocks
    }

    fn remaining_wire_bytes(&self) -> usize {
        self.wire_byte_limit.saturating_sub(self.used_wire_bytes)
    }

    fn deadline(&self) -> Instant {
        self.deadline
    }

    #[cfg(test)]
    fn used_wire_bytes(&self) -> usize {
        self.used_wire_bytes
    }

    #[cfg(test)]
    fn used_responses(&self) -> u64 {
        self.used_responses
    }
}

#[derive(Debug)]
enum SyncRoundError {
    Peer(FrameError),
    Budget(SyncBudgetError),
}

impl From<FrameError> for SyncRoundError {
    fn from(error: FrameError) -> Self {
        Self::Peer(error)
    }
}

impl From<SyncBudgetError> for SyncRoundError {
    fn from(error: SyncBudgetError) -> Self {
        Self::Budget(error)
    }
}

fn sync_budget_frame_error(error: SyncBudgetError) -> FrameError {
    FrameError::Malformed {
        detail: format!("peer sync budget violation: {error:?}"),
    }
}

fn charge_sync_response(
    budget: &mut SyncBudget,
    requested: u64,
    returned: usize,
    wire_bytes: usize,
    metrics: &P2pMetrics,
) -> Result<(), SyncRoundError> {
    match budget.charge_response(requested, returned, wire_bytes) {
        Ok(()) => Ok(()),
        Err(error @ SyncBudgetError::ResponseOverReturn { .. }) => {
            metrics
                .sync_over_return_drops
                .fetch_add(1, Ordering::Relaxed);
            Err(SyncRoundError::Peer(sync_budget_frame_error(error)))
        }
        Err(error) => Err(SyncRoundError::Budget(error)),
    }
}

fn recv_sync_frame(
    transport: &TcpTransport,
    conn: &mut TcpConn,
    budget: &SyncBudget,
) -> Result<(Frame, usize), SyncRoundError> {
    budget.ensure_before_receive(Instant::now())?;
    match transport.recv_frame_counted_until(conn, budget.remaining_wire_bytes(), budget.deadline())
    {
        Ok(frame) => Ok(frame),
        Err(FrameError::FrameBudgetExceeded { seen, .. }) => {
            Err(SyncRoundError::Budget(SyncBudgetError::WireByteLimit {
                attempted: budget.used_wire_bytes.saturating_add(seen),
                limit: budget.wire_byte_limit,
            }))
        }
        Err(FrameError::ReceiveDeadlineExceeded) => {
            Err(SyncRoundError::Budget(SyncBudgetError::DeadlineExceeded))
        }
        Err(error) => Err(SyncRoundError::Peer(error)),
    }
}

pub(crate) fn spawn_sync_thread(
    peers: Vec<SocketAddr>,
    identity: P2pIdentity,
    state: Arc<RwLock<LocalNodeState>>,
    lifecycle: Arc<P2pLifecycle>,
    metrics: Arc<P2pMetrics>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("boole-p2p-sync".to_string())
        .spawn(move || sync_loop(peers, identity, state, lifecycle, metrics))
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
    lifecycle: Arc<P2pLifecycle>,
    metrics: Arc<P2pMetrics>,
) {
    while !lifecycle.is_stopped() {
        for peer in &peers {
            if lifecycle.is_stopped() {
                return;
            }
            match sync_with_peer(peer, &identity, &state, &lifecycle, &metrics) {
                Ok(peer_head) => record_bootstrap_observation(&state, *peer, Some(peer_head)),
                Err(SyncRoundError::Budget(error)) => {
                    record_bootstrap_observation(&state, *peer, None);
                    metrics.sync_budget_drops.fetch_add(1, Ordering::Relaxed);
                    eprintln!("boole-node: peer sync deferred by local round budget: {error:?}");
                }
                Err(SyncRoundError::Peer(error)) => {
                    record_bootstrap_observation(&state, *peer, None);
                    if lifecycle.is_stopped() {
                        return;
                    }
                    metrics.sync_peer_failures.fetch_add(1, Ordering::Relaxed);
                    eprintln!("boole-node: peer sync failed: {error}");
                }
            }
        }
        // Sleep in short slices so shutdown stays bounded by the accept
        // poll, not by the sync interval.
        if lifecycle.wait_or_stop(SYNC_POLL_INTERVAL) {
            return;
        }
    }
}

fn sync_with_peer(
    peer: &SocketAddr,
    identity: &P2pIdentity,
    state: &Arc<RwLock<LocalNodeState>>,
    lifecycle: &Arc<P2pLifecycle>,
    metrics: &Arc<P2pMetrics>,
) -> Result<HeadSummary, SyncRoundError> {
    let my_head = head_summary(&state.blocking_read());
    let mut budget = SyncBudget::for_peer_round();
    budget.ensure_before_receive(Instant::now())?;
    let mut validated = match open_validated_conn_until(
        peer,
        identity,
        my_head.clone(),
        lifecycle,
        budget.remaining_wire_bytes(),
        budget.deadline(),
    ) {
        Ok(validated) => validated,
        Err(FrameError::FrameBudgetExceeded { seen, .. }) => {
            return Err(SyncRoundError::Budget(SyncBudgetError::WireByteLimit {
                attempted: seen,
                limit: budget.wire_byte_limit,
            }));
        }
        Err(FrameError::ReceiveDeadlineExceeded) => {
            return Err(SyncRoundError::Budget(SyncBudgetError::DeadlineExceeded));
        }
        Err(error) => return Err(SyncRoundError::Peer(error)),
    };
    let peer_head = validated.peer_head.clone();
    charge_sync_response(&mut budget, 0, 0, validated.peer_hello_wire_bytes, metrics)?;
    // A peer can advance after a previously exact observation. Invalidate
    // that old readiness evidence as soon as the new Hello is validated,
    // before waiting for or verifying any advertised blocks. The caller
    // records the final exact match again when this round completes.
    clear_bootstrap_observation_if_mismatched(state, *peer, &peer_head);
    let mut my_height = my_head.height;
    let mut first_probe = true;
    while my_height < peer_head.height && !lifecycle.is_stopped() {
        budget.ensure_before_receive(Instant::now())?;
        let requested = (peer_head.height - my_height)
            .min(GET_BLOCKS_RANGE_CAP)
            .min(budget.remaining_blocks())
            .min(if first_probe { 1 } else { u64::MAX });
        first_probe = false;
        if requested == 0 {
            return Ok(peer_head);
        }
        let to = my_height + requested - 1;
        budget.ensure_before_receive(Instant::now())?;
        validated.transport.send_frame(
            &mut validated.conn,
            &Frame::GetBlocks {
                from: my_height,
                to,
            },
        )?;
        let (reply, wire_bytes) =
            recv_sync_frame(&validated.transport, &mut validated.conn, &budget)?;
        let blocks = match reply {
            Frame::Blocks { blocks } => blocks,
            _ => {
                return Err(FrameError::Malformed {
                    detail: "expected Blocks in reply to GetBlocks".to_string(),
                }
                .into())
            }
        };
        charge_sync_response(&mut budget, requested, blocks.len(), wire_bytes, metrics)?;
        if blocks.is_empty() {
            // The peer served nothing for a range its Hello claimed to
            // have — stop rather than spin; the next poll retries.
            return Ok(peer_head);
        }
        for block_value in &blocks {
            // Scope the write guard to the ingest call so it is dropped before
            // any reorg path below re-acquires both the lifecycle permit and
            // the state lock. Keeping either guard alive across that call can
            // deadlock when shutdown is concurrently waiting at the mutation
            // barrier.
            let outcome = {
                budget.ensure_before_mutation(Instant::now())?;
                let Some(_mutation) = lifecycle.begin_mutation() else {
                    return Ok(peer_head);
                };
                budget.ensure_before_mutation(Instant::now())?;
                ingest_announced_block_shared(state, block_value)
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
                    reorg_from_peer(
                        &validated.transport,
                        &mut validated.conn,
                        &peer_head,
                        state,
                        lifecycle,
                        &mut budget,
                        metrics,
                    )?;
                    return Ok(peer_head);
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
                    }
                    .into());
                }
                IngressBlockOutcome::Deferred => {
                    // The pinned checker was unavailable or canonical
                    // publication is fail-closed. Neither is a peer fault:
                    // stop this sync without adopting or rejecting the block.
                    metrics
                        .ingress_blocks_deferred
                        .fetch_add(1, Ordering::Relaxed);
                    return Ok(peer_head);
                }
            }
        }
        my_height = head_summary(&state.blocking_read()).height;
    }
    Ok(peer_head)
}

/// Store a bootstrap observation under the same lock that owns the live
/// canonical head. A normal sync return is not itself success: empty,
/// deferred, peer-behind and incomplete rounds can all end without a transport
/// error. Only an exact final `(height, c)` match satisfies this endpoint.
fn record_bootstrap_observation(
    state: &Arc<RwLock<LocalNodeState>>,
    peer: SocketAddr,
    advertised_head: Option<HeadSummary>,
) {
    let mut guard = state.blocking_write();
    let live_head = head_summary(&guard);
    let Some(readiness) = guard.p2p_bootstrap_readiness.as_mut() else {
        return;
    };
    match advertised_head {
        Some(head) if head == live_head => readiness.observe_exact(peer, head),
        _ => readiness.clear(peer),
    }
}

/// Invalidate an old exact observation as soon as a validated Hello proves it
/// stale. A matching Hello does not create new readiness evidence here: the
/// caller records that only after the whole outbound round returns cleanly.
fn clear_bootstrap_observation_if_mismatched(
    state: &Arc<RwLock<LocalNodeState>>,
    peer: SocketAddr,
    advertised_head: &HeadSummary,
) {
    let mut guard = state.blocking_write();
    let live_head = head_summary(&guard);
    if advertised_head == &live_head {
        return;
    }
    if let Some(readiness) = guard.p2p_bootstrap_readiness.as_mut() {
        readiness.clear(peer);
    }
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
    lifecycle: &Arc<P2pLifecycle>,
    budget: &mut SyncBudget,
    metrics: &Arc<P2pMetrics>,
) -> Result<(), SyncRoundError> {
    // A full competing chain is deliberately evaluated as one strict replay.
    // If it cannot fit this bounded round, defer before downloading it. Deep
    // resumable reorg sync belongs to the later snapshot/checkpoint track; it
    // must not be faked with a partial candidate or repeatedly charged as a
    // peer protocol failure.
    if peer_head.height > budget.remaining_blocks() {
        return Err(SyncRoundError::Budget(SyncBudgetError::BlockLimit {
            attempted: budget.used_blocks().saturating_add(peer_head.height),
            limit: budget.block_limit,
        }));
    }
    let candidate = fetch_block_range(
        transport,
        conn,
        0,
        peer_head.height,
        lifecycle,
        budget,
        metrics,
    )?;
    if candidate.is_empty() {
        // The peer advertised a head but served nothing for its own range —
        // stop rather than spin; the next poll retries.
        return Ok(());
    }
    validate_complete_candidate(&candidate, peer_head)?;
    // Network I/O is done; take the write guard only for the state mutation,
    // matching the single-writer discipline the HTTP submit path holds.
    budget.ensure_before_mutation(Instant::now())?;
    let Some(_mutation) = lifecycle.begin_mutation() else {
        return Ok(());
    };
    budget.ensure_before_mutation(Instant::now())?;
    let outcome = ingest_candidate_chain_shared(state, &candidate);
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
        // The pinned checker was unavailable or canonical publication is
        // fail-closed. This is not a peer fault, so it does not fail the sync
        // round or mutate the current process further.
        CandidateChainOutcome::Deferred => {
            metrics.sync_reorgs_deferred.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        CandidateChainOutcome::Rejected => {
            metrics
                .ingress_blocks_rejected
                .fetch_add(1, Ordering::Relaxed);
            Err(FrameError::Malformed {
                detail: "peer served a competing chain that failed strict validation".to_string(),
            }
            .into())
        }
    }
}

fn validate_complete_candidate(
    candidate: &[Value],
    peer_head: &HeadSummary,
) -> Result<(), FrameError> {
    let candidate_len = u64::try_from(candidate.len()).unwrap_or(u64::MAX);
    let candidate_tip = candidate
        .last()
        .and_then(|block| block.get("c"))
        .and_then(Value::as_str);
    if candidate_len == peer_head.height && candidate_tip == Some(peer_head.c.as_str()) {
        Ok(())
    } else {
        Err(FrameError::Malformed {
            detail: "peer served an incomplete competing chain".to_string(),
        })
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
    lifecycle: &Arc<P2pLifecycle>,
    budget: &mut SyncBudget,
    metrics: &P2pMetrics,
) -> Result<Vec<Value>, SyncRoundError> {
    let mut collected = Vec::new();
    let mut next = from;
    while next < upto {
        if lifecycle.is_stopped() {
            // A partial competing chain is not a candidate. Returning it as
            // success would let shutdown race with fork-choice and mutate
            // canonical state after the lifecycle boundary was closed.
            return Err(SyncRoundError::Peer(FrameError::Io(io::Error::new(
                io::ErrorKind::Interrupted,
                "P2P lifecycle stopped during block-range fetch",
            ))));
        }
        budget.ensure_before_receive(Instant::now())?;
        let requested = (upto - next)
            .min(GET_BLOCKS_RANGE_CAP)
            .min(budget.remaining_blocks());
        let to = next + requested - 1;
        budget.ensure_before_receive(Instant::now())?;
        transport.send_frame(conn, &Frame::GetBlocks { from: next, to })?;
        let (reply, wire_bytes) = recv_sync_frame(transport, conn, budget)?;
        let blocks = match reply {
            Frame::Blocks { blocks } => blocks,
            _ => {
                return Err(FrameError::Malformed {
                    detail: "expected Blocks in reply to GetBlocks".to_string(),
                }
                .into())
            }
        };
        charge_sync_response(budget, requested, blocks.len(), wire_bytes, metrics)?;
        if blocks.is_empty() {
            // The peer served nothing for a range its Hello claimed — stop
            // rather than spin.
            break;
        }
        next += u64::try_from(blocks.len()).expect("bounded block response length fits u64");
        collected.extend(blocks);
    }
    Ok(collected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};

    #[test]
    fn bootstrap_readiness_rejects_empty_and_duplicate_peer_sets() {
        let peer: SocketAddr = "127.0.0.1:30101".parse().expect("peer");
        assert_eq!(
            P2pBootstrapReadiness::new(&[]).expect_err("empty peer set"),
            "bootstrap head-sync readiness requires at least one peer"
        );
        assert_eq!(
            P2pBootstrapReadiness::new(&[peer, peer]).expect_err("duplicate peer set"),
            "duplicate bootstrap peer endpoint: 127.0.0.1:30101"
        );
    }

    #[test]
    fn bootstrap_readiness_requires_every_unique_peer_to_match_the_live_head() {
        let peer_a: SocketAddr = "127.0.0.1:30101".parse().expect("peer a");
        let peer_b: SocketAddr = "127.0.0.1:30102".parse().expect("peer b");
        let mut readiness =
            P2pBootstrapReadiness::new(&[peer_a, peer_b]).expect("two distinct bootstrap peers");
        let genesis = HeadSummary {
            height: 0,
            c: "00".repeat(32),
        };
        let next = HeadSummary {
            height: 1,
            c: "11".repeat(32),
        };

        assert!(!readiness.matches_live_head(&genesis));
        readiness.observe_exact(peer_a, genesis.clone());
        assert!(
            !readiness.matches_live_head(&genesis),
            "one observed peer must not satisfy a two-peer bootstrap set"
        );
        readiness.observe_exact(peer_b, genesis.clone());
        assert!(readiness.matches_live_head(&genesis));

        assert!(
            !readiness.matches_live_head(&next),
            "a later local head must invalidate stale peer observations"
        );
        readiness.observe_exact(peer_a, next.clone());
        assert!(!readiness.matches_live_head(&next));
        readiness.observe_exact(peer_b, next.clone());
        assert!(readiness.matches_live_head(&next));

        readiness.clear(peer_a);
        assert!(!readiness.matches_live_head(&next));
    }

    #[test]
    fn sync_budget_charges_blocks_and_actual_wire_bytes_atomically() {
        let mut budget = SyncBudget::new(3, 12, 10, Instant::now() + Duration::from_secs(1));
        budget
            .charge_response(2, 2, 5)
            .expect("first response fits");
        assert_eq!(budget.used_blocks(), 2);
        assert_eq!(budget.used_wire_bytes(), 5);

        assert_eq!(
            budget.charge_response(1, 2, 1),
            Err(SyncBudgetError::ResponseOverReturn {
                requested: 1,
                returned: 2,
            })
        );
        assert_eq!((budget.used_blocks(), budget.used_wire_bytes()), (2, 5));

        assert_eq!(
            budget.charge_response(1, 1, 8),
            Err(SyncBudgetError::WireByteLimit {
                attempted: 13,
                limit: 12,
            })
        );
        assert_eq!((budget.used_blocks(), budget.used_wire_bytes()), (2, 5));

        budget
            .charge_response(1, 1, 7)
            .expect("exact remaining budget fits");
        assert!(budget.ensure_before_receive(Instant::now()).is_err());
    }

    #[test]
    fn sync_budget_rejects_block_overflow_before_changing_its_counters() {
        let mut budget = SyncBudget::new(2, 100, 10, Instant::now() + Duration::from_secs(1));
        assert_eq!(
            budget.charge_response(2, 3, 30),
            Err(SyncBudgetError::ResponseOverReturn {
                requested: 2,
                returned: 3,
            })
        );
        assert_eq!((budget.used_blocks(), budget.used_wire_bytes()), (0, 0));

        budget.charge_response(2, 2, 30).expect("at limit");
        assert_eq!(budget.remaining_blocks(), 0);
        assert_eq!(
            budget.charge_response(1, 1, 1),
            Err(SyncBudgetError::BlockLimit {
                attempted: 3,
                limit: 2,
            })
        );
        assert_eq!((budget.used_blocks(), budget.used_wire_bytes()), (2, 30));
    }

    #[test]
    fn sync_budget_response_limit_is_atomic_and_deadline_blocks_late_mutation() {
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut budget = SyncBudget::new(10, 100, 2, deadline);
        budget.charge_response(0, 0, 10).expect("hello fits");
        budget
            .charge_response(1, 1, 10)
            .expect("one block response fits");
        assert_eq!((budget.used_blocks(), budget.used_wire_bytes()), (1, 20));
        assert_eq!(budget.used_responses(), 2);

        assert_eq!(
            budget.charge_response(1, 1, 10),
            Err(SyncBudgetError::ResponseLimit {
                attempted: 3,
                limit: 2,
            })
        );
        assert_eq!(
            (
                budget.used_blocks(),
                budget.used_wire_bytes(),
                budget.used_responses()
            ),
            (1, 20, 2)
        );
        assert_eq!(
            budget.ensure_before_receive(Instant::now()),
            Err(SyncBudgetError::ResponseLimit {
                attempted: 3,
                limit: 2,
            })
        );
        assert_eq!(
            budget.ensure_before_mutation(deadline),
            Err(SyncBudgetError::DeadlineExceeded)
        );
    }

    #[test]
    fn sync_receive_maps_remaining_wire_cap_to_local_budget_deferral() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let mut sender =
            TcpStream::connect(listener.local_addr().expect("address")).expect("connect sender");
        let (receiver, _) = listener.accept().expect("accept receiver");
        let mut conn = TcpTransport::conn_from_stream(receiver).expect("wrap receiver");
        let transport = TcpTransport::new();
        sender
            .write_all(b"{\"type\":\"blocks\",\"blocks\":[]}\n")
            .expect("write oversized-for-round frame");
        let budget = SyncBudget::new(10, 12, 10, Instant::now() + Duration::from_secs(1));

        let error = recv_sync_frame(&transport, &mut conn, &budget)
            .expect_err("remaining wire budget must stop the frame before parse");
        assert!(matches!(
            error,
            SyncRoundError::Budget(SyncBudgetError::WireByteLimit { limit: 12, .. })
        ));
        assert_eq!(budget.used_blocks(), 0);
        assert_eq!(budget.used_wire_bytes(), 0);
        assert_eq!(budget.used_responses(), 0);
    }

    #[test]
    fn competing_candidate_must_reach_the_advertised_height_and_tip() {
        let block_a = serde_json::json!({"c": "aa"});
        let block_b = serde_json::json!({"c": "bb"});
        let head = HeadSummary {
            height: 2,
            c: "bb".to_string(),
        };

        assert!(validate_complete_candidate(std::slice::from_ref(&block_a), &head).is_err());
        assert!(validate_complete_candidate(&[block_a.clone(), block_a], &head).is_err());
        validate_complete_candidate(&[serde_json::json!({"c": "aa"}), block_b], &head)
            .expect("complete candidate matches advertised head");
    }
}
