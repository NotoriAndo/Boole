use crate::block_store::FileBlockStore;
use crate::bounty_catalog_store::load_bounties_from_path;
use crate::bounty_event_store::FileBountyEventLedger;
use crate::family_manifest_store::load_family_manifest_registry_from_dir;
use crate::http_error::HttpError;
use crate::nonce_ledger::FileNonceLedger;
use crate::receipt_store::FileReceiptStore;
use crate::runtime::{RuntimeAdmissionState, RuntimeConfig};
use crate::session_store::FileSessionStore;
use crate::state_dir::{self, StateDirGuard, StateManifest};
use crate::work_manifest_store::load_work_manifests_from_path;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, Path as AxumPath, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::{from_fn, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use boole_core::{
    agent_passport_events_for_receipt, canonical_payload_hash_hex, compute_block_reward_credits,
    replay_blocks, ticket, verify_signature, AdmissionDecision, BountyProofVerifier,
    BountyRegistry, BountyShare, BountySidePool, BuildSelectionResult, CalibrationReport,
    CreateBountyInput, DifficultyRetargetPolicy, FamilyManifestRegistry, Hex32, Hex64,
    PersistedBlock, ReceiptCommitment, ReceiptCommitmentInput, SessionState, SubmitProofInput,
    UpdateStatusInput, WorkManifest, SIGNED_ENVELOPE_SCHEMA,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::convert::Infallible;
use std::future::Future;
use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::{Notify, RwLock};
use tower::Service;
use tower_http::timeout::TimeoutLayer;

const MAX_HTTP_BODY_BYTES: usize = 1_048_576;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const VERIFY_ANSWER_SCHEME: &str = "boole-native-test";
const VERIFY_ANSWER_AMOUNT: &str = "1";
const VERIFY_ANSWER_PAYMENT_SIGNATURE: &str = "boole-native-test:paid";
const DEFAULT_X402_VERSION: &str = "x402.draft-2";
const X402_VERSIONS_FIXTURE: &str = include_str!("../../../fixtures/protocol/x402/versions.json");
/// Default network id stamped into `state.manifest.json` when the caller
/// did not pin one via `LocalNodeConfig::network_id`. P2.10 will graduate
/// `--network testnet` presets to a richer surface; for P1.1b the
/// default keeps legacy embeddings on a single named network so the
/// manifest's network-id verification has something to check against.
const DEFAULT_NETWORK_ID: &str = "boole-mvp";
/// Coarse binary identifier persisted into `state.manifest.json`. Pinned
/// at build time so a re-boot can detect that the running binary's
/// version differs from the one that created the directory. A finer
/// SHA-256 over `current_exe()` is the eventual goal but `CARGO_PKG_VERSION`
/// is the lowest-cost identifier that survives a release rebuild.
const BINARY_SHA: &str = env!("CARGO_PKG_VERSION");

pub struct LocalNodeConfig {
    pub scenario_path: PathBuf,
    pub block_path: PathBuf,
    /// When `Some`, the runtime persists `PersistedRewardEvent` rows here
    /// on every commit and exposes balances via `/account/{pk}/balance`.
    /// When `None`, reward bookkeeping is disabled (legacy embeddings).
    pub reward_ledger_path: Option<PathBuf>,
    /// Optional path to a `WorkManifestList` JSON file. When `Some`, the node
    /// loads the catalog at boot and serves it via `GET /work` and
    /// `GET /work/:id`. When `None`, the routes still serve but the list is
    /// empty and every id returns `work_not_found`. Catalog is static for
    /// the process lifetime — pof has no live mutation surface either.
    pub work_manifests_path: Option<PathBuf>,
    /// Optional path to a `BountyList` JSON file. When `Some`, the node
    /// loads the catalog at boot and serves it via `GET /bounties` and
    /// `GET /bounties/:id`. When `None`, the routes still serve but the
    /// list is empty and every id returns `bounty_not_found`.
    pub bounties_path: Option<PathBuf>,
    /// Optional NDJSON audit log for `POST /bounties/{id}/proof` events.
    /// When `Some`, every accepted/rejected proof envelope is appended
    /// here and replayed on the next boot to restore "solved" status.
    /// When `None`, the route still serves but events are not durable.
    pub bounty_event_ledger_path: Option<PathBuf>,
    /// Pluggable verifier registry keyed by `bounty.verifier.kind`. When
    /// `None`, every proof submission against a known bounty falls
    /// through to `501 no_verifier`. Built-in `lean` verifier is wired
    /// here by `main.rs` via `LeanBountyVerifier`.
    pub bounty_verifiers: Option<HashMap<String, Arc<dyn BountyProofVerifier>>>,
    /// Optional directory of `*.json` `FamilyManifest` files. Loaded once
    /// at boot and held in `FamilyManifestRegistry` keyed by `family_id`.
    /// S21 ships the loader + side-pool only — `activation_height` is
    /// not yet evaluated. None means no families are registered, so every
    /// bounty proof routes through the side-pool with no promotion path.
    pub family_manifests_dir: Option<PathBuf>,
    /// Operator signing pks (hex32) trusted to sign `FamilyManifest`s.
    /// `select_promoted_bounty_shares` only promotes a manifest into
    /// block selection if its embedded `signature` verifies against one
    /// of these pks AND `activation_height ≤ runtime height`. Empty list
    /// (the default) disables promotion entirely — manifests can be
    /// loaded for inspection but no side-pool share is ever forwarded
    /// to `build_block_selection`. This is the safe default; operators
    /// opt in with `--operator-signer-pks <hex,hex,…>`.
    pub operator_signer_pks: Vec<String>,
    /// Optional NDJSON ledger path for the agent-wallet session registry.
    /// When `Some`, the node mounts `POST /sessions`, `GET /sessions/{pk}`,
    /// and `POST /sessions/{pk}/revoke`, recovers the in-memory
    /// `FileSessionStore` at boot, and persists every register/revoke
    /// event on append. When `None`, the routes still resolve but every
    /// call returns `session_registry_disabled` — the agent-wallet stack
    /// is opt-in so legacy embeddings keep their pre-N1.2 behavior.
    pub session_registry_path: Option<PathBuf>,
    /// Optional NDJSON ledger path for the session-bound `/submit` nonce
    /// dedup set. When `Some`, `submit` envelopes that carry a `session`
    /// block burn `(submittedBy, nonce)` into the ledger before reaching
    /// the admission path; a replayed pair is rejected with
    /// `nonce_replayed` (HTTP 409). Recovery rehydrates the dedup set so
    /// the rejection survives process restarts. When `None`, the
    /// session-gated path returns `session_registry_disabled` — the
    /// ledger and the session registry are opted in together.
    pub submit_nonce_ledger_path: Option<PathBuf>,
    /// Optional NDJSON receipt ledger for accepted session-bound `/submit`
    /// work. When configured, accepted session submits append the exact
    /// receipt returned in the HTTP response so agents can later prove the
    /// requestHash/nonce/sessionPk/block/reward credit tuple.
    pub submit_receipt_ledger_path: Option<PathBuf>,
    /// Optional NDJSON ledger path for verified-answer `ReceiptCommitment` rows.
    /// When `Some`, the node serves `GET /receipts/{receiptId}` and local
    /// MVP `POST /receipts`; when `None`, these routes return
    /// `receipt_store_disabled`.
    pub receipt_commitment_ledger_path: Option<PathBuf>,
    pub max_requests: Option<usize>,
    /// When `Some`, replaces the scenario's `genesis_c`. Surfaced so the
    /// CLI wrapper (`boole node start --genesis HEX32`) can override the
    /// canned scenario without rewriting the fixture file. The override
    /// is applied during `LocalNodeState::from_config`, before the runtime
    /// adopts the head, so `replay_matches_runtime_at_boot` still matches.
    pub genesis_override: Option<String>,
    /// Optional L7 state directory (P1.1). When `Some`, the runtime
    /// acquires an exclusive `flock` on `<dir>/state.lock` before opening
    /// any ledger and writes/verifies `<dir>/state.manifest.json`. A
    /// second `boole-node` pointed at the same directory is rejected with
    /// `state-dir-locked` before it touches any payload file. When
    /// `None`, the legacy embedding semantics are preserved (per-store
    /// paths only, no cross-process lock).
    pub state_dir: Option<PathBuf>,
    /// Network identifier persisted into `state.manifest.json`. Pinned at
    /// first boot and verified on every subsequent boot so a directory
    /// built for one network cannot be silently re-used on another. When
    /// `None`, the runtime defaults to `"boole-mvp"`. Ignored unless
    /// `state_dir` is set.
    pub network_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalNodeScenarioConfig {
    cfg: CalibrationReport,
    difficulty_retarget: Option<DifficultyRetargetPolicy>,
    genesis_c: String,
}

struct LocalNodeState {
    runtime: RuntimeAdmissionState,
    genesis_c: String,
    block_path: PathBuf,
    report: CalibrationReport,
    /// Set at boot from the disk replay. The block_cache mirror is updated in
    /// lockstep with FileBlockStore::append (see runtime::commit_using_cache),
    /// so once boot agrees, `cached_block_count() / current_c` and
    /// `replay_blocks(disk).{height, latest_c}` cannot diverge during this
    /// process's lifetime. Surfaced through /status as `replayMatchesRuntime`
    /// so operators see the real boot-time invariant rather than a hardcoded
    /// constant.
    replay_matches_runtime_at_boot: bool,
    /// Static catalog of `WorkManifest`s loaded once at boot from
    /// `LocalNodeConfig.work_manifests_path` (empty when unconfigured).
    /// Served read-only via `GET /work` and `GET /work/:id`.
    work_manifests: Vec<WorkManifest>,
    /// Mutable bounty registry. Seeded at boot from
    /// `LocalNodeConfig.bounties_path` (catalog of `create` events) and
    /// then replayed forward through the audit log so a restart shows the
    /// same status as the live process before shutdown.
    bounty_registry: BountyRegistry,
    /// Optional NDJSON audit log path for proof events. Mirrors the
    /// `FileRewardLedger` pattern: one validated JSON object per line,
    /// schema-checked on append, recovery is an idempotent replay.
    bounty_event_ledger_path: Option<PathBuf>,
    /// Verifier registry keyed by `bounty.verifier.kind`. Populated from
    /// `LocalNodeConfig.bounty_verifiers` (empty when unconfigured).
    bounty_verifiers: HashMap<String, Arc<dyn BountyProofVerifier>>,
    /// Per-family manifest registry. Used at proof-submit time to look
    /// up the manifest for `Bounty.domain` (which is a `family_id`) so
    /// the side-pool entry can record the correct `family_id` and so
    /// S22's activation gate has somewhere to read from.
    family_manifest_registry: FamilyManifestRegistry,
    /// Per-family side-pool of accepted bounty shares. Isolated from
    /// `SharePool` — `build_block_selection` does not consume from it.
    /// S22 gates from here into block selection via
    /// `select_promoted_bounty_shares` when a family manifest is signed
    /// by one of `operator_signer_pks` AND its `activation_height` is at
    /// or below the runtime height.
    bounty_side_pool: BountySidePool,
    operator_signer_pks: Vec<String>,
    /// On-disk NDJSON ledger for the session registry. `Some` whenever
    /// the agent-wallet stack is opted in via
    /// `LocalNodeConfig.session_registry_path`. Kept here so the three
    /// session handlers can persist register/revoke events with the same
    /// {recover → append-then-apply} flow the bounty audit log uses.
    session_registry_path: Option<PathBuf>,
    /// In-memory mirror of the session ledger. `Some` iff
    /// `session_registry_path` is `Some`; `None` keeps the agent-wallet
    /// routes disabled so legacy callers can't observe a partially-wired
    /// surface.
    session_store: Option<FileSessionStore>,
    /// On-disk NDJSON ledger for the session-bound `/submit` nonce dedup
    /// set. `Some` whenever the agent-wallet stack is opted in via
    /// `LocalNodeConfig.submit_nonce_ledger_path`. Kept here so the
    /// session gate can burn `(submittedBy, nonce)` events with the same
    /// {append-then-apply} flow the session store uses.
    submit_nonce_ledger_path: Option<PathBuf>,
    /// In-memory mirror of the submit-nonce ledger. `Some` iff
    /// `submit_nonce_ledger_path` is `Some`; required for the gate to
    /// answer dedup queries without re-reading the ledger on every call.
    nonce_ledger: Option<FileNonceLedger>,
    /// Optional append-only receipt ledger for accepted session-bound submit
    /// artifacts. The response receipt and ledger line intentionally match.
    submit_receipt_ledger_path: Option<PathBuf>,
    /// Optional append-only `ReceiptCommitment` ledger plus recovered index.
    receipt_commitment_ledger_path: Option<PathBuf>,
    receipt_store: Option<FileReceiptStore>,
    /// RAII guard for the L7 state-directory `flock`. `Some` whenever the
    /// caller passed a `state_dir` in `LocalNodeConfig`; held for the
    /// lifetime of the node so a second process at the same directory
    /// cannot race for the lock. Field is `_`-prefixed because it is
    /// never read directly — drop semantics are the entire contract.
    _state_dir_guard: Option<StateDirGuard>,
}

#[derive(Clone)]
struct AppState {
    inner: Arc<RwLock<LocalNodeState>>,
}

pub fn serve_local_node(listener: StdTcpListener, config: LocalNodeConfig) -> anyhow::Result<()> {
    let max_requests = config.max_requests;
    let state = LocalNodeState::from_config(config)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve_local_node_async(listener, state, max_requests))
}

async fn serve_local_node_async(
    listener: StdTcpListener,
    state: LocalNodeState,
    max_requests: Option<usize>,
) -> anyhow::Result<()> {
    listener.set_nonblocking(true)?;
    let tokio_listener = TcpListener::from_std(listener)?;
    let app_state = AppState {
        inner: Arc::new(RwLock::new(state)),
    };
    let shutdown_notify = Arc::new(Notify::new());
    let app = build_router(app_state);
    // Mirror the raw-TCP server's `--max-requests N` semantics: count an
    // event per *accepted-and-closed* TCP connection, regardless of whether
    // a complete HTTP request was ever sent. This matches the regression
    // suite (notably the TCP readiness probe in
    // `node_start_spawns_daemon_serving_health`). The counter increments
    // when the per-connection `tower::Service` is dropped, i.e. after hyper
    // is done with the connection — firing earlier (on accept) races
    // axum::serve's graceful_shutdown drop and produces RSTs on the last
    // request before exit.
    let make_service = ConnectionCountingMakeService {
        inner: app.into_make_service_with_connect_info::<SocketAddr>(),
        counter: Arc::new(ConnectionCounter {
            served: AtomicUsize::new(0),
            max_requests,
            shutdown: shutdown_notify.clone(),
        }),
    };
    axum::serve(tokio_listener, make_service)
        .with_graceful_shutdown(async move { shutdown_notify.notified().await })
        .await?;
    Ok(())
}

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status_handler))
        .route("/head", get(head_handler))
        .route("/config", get(config_handler))
        .route("/health", get(health_handler))
        .route("/block/latest", get(block_latest_handler))
        .route("/block/{height}", get(block_by_height_handler))
        .route("/account/{pk}/balance", get(account_balance_handler))
        .route("/work", get(work_list_handler))
        .route("/work/{id}", get(work_by_id_handler))
        .route(
            "/bounties",
            get(bounty_list_handler).post(bounty_announce_handler),
        )
        .route("/bounties/{id}", get(bounty_by_id_handler))
        .route("/bounties/{id}/proof", post(bounty_proof_handler))
        .route("/bounties/{id}/status", post(bounty_status_handler))
        .route("/ticket", post(ticket_handler))
        .route("/submit", post(submit_handler))
        .route("/verify-answer", post(verify_answer_handler))
        .route("/sessions", post(session_register_handler))
        .route("/sessions/{session_pk}", get(session_get_handler))
        .route("/receipts", post(receipt_post_handler))
        .route("/receipts/{receipt_id}", get(receipt_get_handler))
        .route(
            "/sessions/{session_pk}/revoke",
            post(session_revoke_handler),
        )
        .fallback(fallback_handler)
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .layer(from_fn(body_cap_middleware))
        .layer(from_fn(connection_close_middleware))
        .with_state(state)
}

struct ConnectionCounter {
    served: AtomicUsize,
    max_requests: Option<usize>,
    shutdown: Arc<Notify>,
}

/// Drop-once token: hyper clones the per-connection `tower::Service` for
/// every HTTP request it processes on the connection. Counting on the
/// service's `Drop` would over-count. Instead we share a single
/// `Arc<ConnectionLifetime>` across all clones of one connection's service;
/// the lifetime token's `Drop` fires exactly once when the last clone is
/// released, i.e. when hyper has fully closed the connection.
struct ConnectionLifetime {
    counter: Arc<ConnectionCounter>,
}

impl Drop for ConnectionLifetime {
    fn drop(&mut self) {
        let served = self.counter.served.fetch_add(1, Ordering::AcqRel) + 1;
        if let Some(max) = self.counter.max_requests {
            if served >= max {
                self.counter.shutdown.notify_one();
            }
        }
    }
}

#[derive(Clone)]
struct ConnectionCountingMakeService<M> {
    inner: M,
    counter: Arc<ConnectionCounter>,
}

impl<M, T> Service<T> for ConnectionCountingMakeService<M>
where
    M: Service<T, Error = Infallible>,
{
    type Response = ConnectionCountedService<M::Response>;
    type Error = Infallible;
    type Future = ConnectionCountedFuture<M::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, target: T) -> Self::Future {
        ConnectionCountedFuture {
            inner: Box::pin(self.inner.call(target)),
            lifetime: Some(Arc::new(ConnectionLifetime {
                counter: self.counter.clone(),
            })),
        }
    }
}

struct ConnectionCountedFuture<F> {
    inner: Pin<Box<F>>,
    lifetime: Option<Arc<ConnectionLifetime>>,
}

impl<F, S> Future for ConnectionCountedFuture<F>
where
    F: Future<Output = Result<S, Infallible>>,
{
    type Output = Result<ConnectionCountedService<S>, Infallible>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.inner.as_mut().poll(cx) {
            Poll::Ready(Ok(svc)) => {
                let lifetime = self
                    .lifetime
                    .take()
                    .expect("ConnectionCountedFuture polled after completion");
                Poll::Ready(Ok(ConnectionCountedService {
                    inner: svc,
                    _lifetime: lifetime,
                }))
            }
            Poll::Ready(Err(_)) => unreachable!("Infallible"),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[derive(Clone)]
struct ConnectionCountedService<S> {
    inner: S,
    _lifetime: Arc<ConnectionLifetime>,
}

impl<S, R> Service<R> for ConnectionCountedService<S>
where
    S: Service<R>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: R) -> Self::Future {
        self.inner.call(req)
    }
}

impl LocalNodeState {
    fn from_config(config: LocalNodeConfig) -> anyhow::Result<Self> {
        // L7 state-dir lock + manifest must run first — every per-store
        // open below this line is guarded by the flock, so refusing the
        // lock guarantees a losing process never appends to a peer's
        // ledger and never half-writes its own.
        let state_dir_guard: Option<StateDirGuard> = if let Some(dir) = config.state_dir.as_ref() {
            let guard = state_dir::acquire(dir)?;
            let manifest = StateManifest::now(
                config.network_id.as_deref().unwrap_or(DEFAULT_NETWORK_ID),
                BINARY_SHA,
            );
            state_dir::ensure_manifest(dir, &manifest)?;
            Some(guard)
        } else {
            None
        };
        let raw = std::fs::read_to_string(&config.scenario_path)?;
        let mut scenario: LocalNodeScenarioConfig = serde_json::from_str(&raw)?;
        if let Some(genesis) = config.genesis_override.as_ref() {
            scenario.genesis_c = genesis.clone();
        }
        let mut runtime_config =
            RuntimeConfig::from_calibration_report(scenario.cfg.clone(), 60_000)
                .map_err(|err| anyhow::anyhow!(err))?;
        if let Some(policy) = scenario.difficulty_retarget.clone() {
            runtime_config = runtime_config
                .with_difficulty_retarget(policy)
                .map_err(|err| anyhow::anyhow!(err))?;
        }
        let recovered = FileBlockStore::recover(&config.block_path)?;
        // Always route through boot_from_store so the reward-ledger path is
        // initialized uniformly. For an empty chain, replay returns the all-
        // zero genesis hash; the scenario's `genesis_c` (possibly overridden
        // via --genesis) is restored below so the runtime head matches the
        // configured genesis instead of the replay default.
        let mut runtime = RuntimeAdmissionState::boot_from_store_with_bounty_ledger(
            runtime_config,
            &config.block_path,
            config.reward_ledger_path.clone(),
            config.bounty_event_ledger_path.clone(),
        )?;
        if recovered.size() == 0 {
            runtime.set_current_c(scenario.genesis_c.clone());
        }
        if runtime.current_c().is_none() {
            runtime.set_current_c(scenario.genesis_c.clone());
        }
        // Independently verify that the runtime mirrors what the disk replays
        // to. This is a paranoid second pass — boot_from_store already feeds
        // replay_blocks output into the runtime — but it lets /status report
        // a real boot-time observation rather than a hardcoded `true`. Once
        // commit_using_cache enforces {check, append, apply_unchecked}, the
        // boot value remains valid for the lifetime of the process.
        let replay_matches_runtime_at_boot = if recovered.size() == 0 {
            runtime.cached_block_count() == 0 && runtime.current_c().is_some()
        } else {
            let replay = replay_blocks(recovered.blocks())?;
            (replay.height as usize) == runtime.cached_block_count()
                && Some(replay.latest_c.as_str()) == runtime.current_c()
        };
        let work_manifests = match config.work_manifests_path.as_ref() {
            Some(path) => load_work_manifests_from_path(path)?,
            None => Vec::new(),
        };
        let bounties = match config.bounties_path.as_ref() {
            Some(path) => load_bounties_from_path(path)?,
            None => Vec::new(),
        };
        let mut bounty_registry = BountyRegistry::new();
        for bounty in bounties {
            // Seed via `create` event so the registry's `order` vector
            // mirrors the catalog order (matches the previous `Vec<Bounty>`
            // listing semantics).
            let create_event = json!({ "kind": "create", "bounty": bounty });
            bounty_registry
                .apply_event_fixture(create_event)
                .map_err(|err| anyhow::anyhow!("seed bounty registry: {err}"))?;
        }
        if let Some(path) = config.bounty_event_ledger_path.as_ref() {
            for event in FileBountyEventLedger::recover(path)? {
                replay_bounty_audit_event(&mut bounty_registry, &event)
                    .map_err(|err| anyhow::anyhow!("replay bounty audit log: {err}"))?;
            }
        }
        let family_manifest_registry = match config.family_manifests_dir.as_ref() {
            Some(dir) => load_family_manifest_registry_from_dir(dir).map_err(|err| {
                anyhow::anyhow!(
                    "load family manifests from {}: {err}",
                    dir.to_string_lossy()
                )
            })?,
            None => FamilyManifestRegistry::new(),
        };
        let session_store = match config.session_registry_path.as_ref() {
            Some(path) => Some(FileSessionStore::recover(path)?),
            None => None,
        };
        let nonce_ledger = match config.submit_nonce_ledger_path.as_ref() {
            Some(path) => Some(FileNonceLedger::recover(path)?),
            None => None,
        };
        let receipt_store = match config.receipt_commitment_ledger_path.as_ref() {
            Some(path) => Some(FileReceiptStore::recover(path)?),
            None => None,
        };
        Ok(Self {
            runtime,
            genesis_c: scenario.genesis_c,
            block_path: config.block_path,
            report: scenario.cfg,
            replay_matches_runtime_at_boot,
            work_manifests,
            bounty_registry,
            bounty_event_ledger_path: config.bounty_event_ledger_path,
            bounty_verifiers: config.bounty_verifiers.unwrap_or_default(),
            family_manifest_registry,
            bounty_side_pool: BountySidePool::new(),
            operator_signer_pks: config.operator_signer_pks,
            session_registry_path: config.session_registry_path,
            session_store,
            submit_nonce_ledger_path: config.submit_nonce_ledger_path,
            nonce_ledger,
            submit_receipt_ledger_path: config.submit_receipt_ledger_path,
            receipt_commitment_ledger_path: config.receipt_commitment_ledger_path,
            receipt_store,
            _state_dir_guard: state_dir_guard,
        })
    }
}

fn replay_bounty_audit_event(registry: &mut BountyRegistry, event: &Value) -> Result<(), String> {
    match event.get("kind").and_then(Value::as_str) {
        Some("proof") => replay_proof_event(registry, event),
        Some("create") => replay_create_event(registry, event),
        Some("status_change") => replay_status_change_event(registry, event),
        _ => Ok(()),
    }
}

fn replay_proof_event(registry: &mut BountyRegistry, event: &Value) -> Result<(), String> {
    let work_id = event
        .get("workId")
        .and_then(Value::as_str)
        .ok_or_else(|| "audit event missing workId".to_string())?;
    let proof_hash = event
        .get("proofHash")
        .and_then(Value::as_str)
        .ok_or_else(|| "audit event missing proofHash".to_string())?;
    let solver_pk = event
        .get("solverPk")
        .and_then(Value::as_str)
        .ok_or_else(|| "audit event missing solverPk".to_string())?;
    let accepted = event
        .get("accepted")
        .and_then(Value::as_bool)
        .ok_or_else(|| "audit event missing accepted".to_string())?;
    let ts = event
        .get("ts")
        .and_then(Value::as_u64)
        .ok_or_else(|| "audit event missing ts".to_string())?;
    registry
        .submit_proof(SubmitProofInput {
            bounty_id: work_id.to_string(),
            proof_hash: proof_hash.to_string(),
            prover: solver_pk.to_string(),
            accepted,
            ts,
        })
        .map(|_| ())
}

fn replay_status_change_event(registry: &mut BountyRegistry, event: &Value) -> Result<(), String> {
    let work_id = event
        .get("workId")
        .and_then(Value::as_str)
        .ok_or_else(|| "status_change audit event missing workId".to_string())?;
    let new_status = event
        .get("newStatus")
        .and_then(Value::as_str)
        .ok_or_else(|| "status_change audit event missing newStatus".to_string())?;
    let ts = event
        .get("ts")
        .and_then(Value::as_u64)
        .ok_or_else(|| "status_change audit event missing ts".to_string())?;
    match registry.update_status(UpdateStatusInput {
        id: work_id.to_string(),
        status: new_status.to_string(),
        ts,
    }) {
        Ok(_) => Ok(()),
        Err(err) => {
            // Static catalog may already have this bounty in a state that
            // refuses the replayed transition (e.g., bounty was promoted
            // to the static file with status=solved after the audit log
            // recorded an open→withdrawn). Log and continue — parallel to
            // the create-event overlap policy from S13b.
            eprintln!(
                "[boole-node] audit log status_change for {work_id} -> {new_status} skipped: {err}"
            );
            Ok(())
        }
    }
}

fn replay_create_event(registry: &mut BountyRegistry, event: &Value) -> Result<(), String> {
    let bounty = event
        .get("bounty")
        .cloned()
        .ok_or_else(|| "create audit event missing bounty".to_string())?;
    let bounty_id = bounty
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "create audit event bounty.id missing".to_string())?
        .to_string();
    let fixture = json!({"kind": "create", "bounty": bounty});
    match registry.apply_event_fixture(fixture) {
        Ok(()) => Ok(()),
        Err(err) if err.starts_with("duplicates id") => {
            // Static catalog already loaded this id. Per S13b decision §6,
            // static wins on overlap — emit a stderr warning and keep
            // booting so operators don't stall on a benign collision.
            eprintln!(
                "[boole-node] audit log create event for {bounty_id} skipped: static catalog already provides this id"
            );
            Ok(())
        }
        Err(err) => Err(err),
    }
}

// Force `Connection: close` on every response so the existing wire-level
// regression net (which uses `read_to_end()` after a single TCP write) sees
// EOF after each request. Hyper's HTTP/1.1 keep-alive is otherwise correct
// but would leave those test clients blocked indefinitely.
async fn connection_close_middleware(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        axum::http::header::CONNECTION,
        HeaderValue::from_static("close"),
    );
    response
}

async fn body_cap_middleware(headers: HeaderMap, request: Request, next: Next) -> Response {
    if let Some(value) = headers.get(axum::http::header::CONTENT_LENGTH) {
        if let Some(len) = value.to_str().ok().and_then(|s| s.parse::<usize>().ok()) {
            if len > MAX_HTTP_BODY_BYTES {
                return error_response(HttpError::body_too_large(MAX_HTTP_BODY_BYTES, len));
            }
        }
    }
    next.run(request).await
}

fn error_response(err: HttpError) -> Response {
    let status = StatusCode::from_u16(err.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = err.into_json();
    (status, Json(body)).into_response()
}

fn anyhow_to_internal(err: anyhow::Error) -> HttpError {
    HttpError::internal(err.to_string())
}

async fn status_handler(State(state): State<AppState>) -> Response {
    let guard = state.inner.read().await;
    match status_json(&guard) {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(err) => error_response(anyhow_to_internal(err)),
    }
}

async fn head_handler(State(state): State<AppState>) -> Response {
    let guard = state.inner.read().await;
    match head_json(&guard) {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(err) => error_response(anyhow_to_internal(err)),
    }
}

async fn config_handler(State(state): State<AppState>) -> Response {
    let guard = state.inner.read().await;
    (StatusCode::OK, Json(config_json(&guard))).into_response()
}

async fn health_handler(State(state): State<AppState>) -> Response {
    let guard = state.inner.read().await;
    (StatusCode::OK, Json(health_json(&guard))).into_response()
}

async fn block_latest_handler(State(state): State<AppState>) -> Response {
    let guard = state.inner.read().await;
    (StatusCode::OK, Json(block_latest_json(&guard))).into_response()
}

async fn block_by_height_handler(
    State(state): State<AppState>,
    AxumPath(height): AxumPath<String>,
) -> Response {
    let guard = state.inner.read().await;
    match block_by_height_json(&guard, &height) {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(err) => error_response(err),
    }
}

async fn account_balance_handler(
    State(state): State<AppState>,
    AxumPath(pk): AxumPath<String>,
) -> Response {
    let guard = state.inner.read().await;
    match account_balance_json(&guard, &pk) {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(err) => error_response(err),
    }
}

async fn work_list_handler(State(state): State<AppState>) -> Response {
    let guard = state.inner.read().await;
    (StatusCode::OK, Json(work_list_json(&guard))).into_response()
}

async fn work_by_id_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let guard = state.inner.read().await;
    match work_by_id_json(&guard, &id) {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(err) => error_response(err),
    }
}

async fn bounty_list_handler(State(state): State<AppState>) -> Response {
    let guard = state.inner.read().await;
    (StatusCode::OK, Json(bounty_list_json(&guard))).into_response()
}

async fn bounty_by_id_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let guard = state.inner.read().await;
    match bounty_by_id_json(&guard, &id) {
        Ok(body) => (StatusCode::OK, Json(body)).into_response(),
        Err(err) => error_response(err),
    }
}

async fn ticket_handler(State(state): State<AppState>, body: Bytes) -> Response {
    let mut guard = state.inner.write().await;
    match ticket_json(&mut guard, &body) {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) => error_response(err),
    }
}

async fn submit_handler(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Response {
    let mut guard = state.inner.write().await;
    let peer_ip = addr.ip().to_string();
    // N2.1 — agent-wallet session gate runs before the legacy admission
    // path so typed envelopes (`session_unknown` / `session_revoked` /
    // `reward_recipient_mismatch` / `nonce_replayed`) can surface with
    // the right HTTP status. Envelopes without a `session` block fall
    // through to the legacy path so pre-wallet callers stay unaffected.
    let checked_session = match submit_session_gate(&mut guard, &body) {
        Ok(session) => session,
        Err(err) => return error_response(err),
    };
    // P1.3a — burn moved INTO submit_json before block append. Do not
    // re-burn here on accepted=true; that would double-append the same
    // (pk, nonce) and surface a spurious nonce_replayed envelope.
    match submit_json(&mut guard, &body, &peer_ip, checked_session.as_ref()) {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) => error_response(anyhow_to_internal(err)),
    }
}

async fn session_register_handler(State(state): State<AppState>, body: Bytes) -> Response {
    let mut guard = state.inner.write().await;
    match session_register_json(&mut guard, &body) {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) => error_response(err),
    }
}

async fn session_get_handler(
    State(state): State<AppState>,
    AxumPath(session_pk): AxumPath<String>,
) -> Response {
    let guard = state.inner.read().await;
    match session_get_json(&guard, &session_pk) {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) => error_response(err),
    }
}

async fn session_revoke_handler(
    State(state): State<AppState>,
    AxumPath(session_pk): AxumPath<String>,
    body: Bytes,
) -> Response {
    let mut guard = state.inner.write().await;
    match session_revoke_json(&mut guard, &session_pk, &body) {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) => error_response(err),
    }
}

async fn receipt_get_handler(
    State(state): State<AppState>,
    AxumPath(receipt_id): AxumPath<String>,
) -> Response {
    let guard = state.inner.read().await;
    match receipt_get_json(&guard, &receipt_id) {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) => error_response(err),
    }
}

async fn receipt_post_handler(State(state): State<AppState>, body: Bytes) -> Response {
    let mut guard = state.inner.write().await;
    match receipt_post_json(&mut guard, &body) {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) => error_response(err),
    }
}

fn receipt_get_json(state: &LocalNodeState, receipt_id: &str) -> Result<Value, HttpError> {
    if !is_well_formed_hex32(receipt_id) {
        return Err(HttpError::bad_hex("receiptId"));
    }
    let store = state
        .receipt_store
        .as_ref()
        .ok_or_else(HttpError::receipt_store_disabled)?;
    let receipt = store
        .get(receipt_id)
        .ok_or_else(|| HttpError::receipt_not_found(receipt_id.to_string()))?;
    Ok(json!({"ok": true, "receiptCommitment": receipt}))
}

fn receipt_post_json(state: &mut LocalNodeState, body: &[u8]) -> Result<Value, HttpError> {
    let path = state
        .receipt_commitment_ledger_path
        .clone()
        .ok_or_else(HttpError::receipt_store_disabled)?;
    let store = state
        .receipt_store
        .as_mut()
        .ok_or_else(HttpError::receipt_store_disabled)?;
    let receipt: ReceiptCommitment = serde_json::from_slice(body)
        .map_err(|err| HttpError::bad_payload("receiptCommitment", err.to_string()))?;
    FileReceiptStore::append(&path, &receipt)
        .map_err(|err| HttpError::bad_request(err.to_string()))?;
    store
        .apply(receipt.clone())
        .map_err(|err| HttpError::bad_request(err.to_string()))?;
    Ok(json!({"ok": true, "receiptCommitment": receipt}))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct VerifyAnswerRequest {
    agent_pk: String,
    family_id: String,
    verifier_id: String,
    verifier_hash_version: String,
    answer: String,
    pay_to: String,
    #[allow(dead_code)]
    session_pk: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct X402VersionsFixture {
    accepted_versions: Vec<String>,
}

async fn verify_answer_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut guard = state.inner.write().await;
    match verify_answer_json(&mut guard, &headers, &body) {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) => error_response(err),
    }
}

fn verify_answer_json(
    state: &mut LocalNodeState,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Value, HttpError> {
    let body_value: Value = serde_json::from_slice(body)
        .map_err(|err| HttpError::bad_request(format!("body is not valid JSON: {err}")))?;
    let request_hash = canonical_payload_hash_hex(&body_value);
    let request: VerifyAnswerRequest = serde_json::from_value(body_value)
        .map_err(|err| HttpError::bad_payload("verifyAnswer", err.to_string()))?;
    let x402_version = header_str(headers, "X-Boole-X402-Version")
        .unwrap_or(DEFAULT_X402_VERSION)
        .to_string();
    let accepted_versions = accepted_x402_versions()?;
    if !accepted_versions.iter().any(|v| v == &x402_version) {
        return Err(HttpError::x402_version_unsupported(
            x402_version,
            accepted_versions,
        ));
    }
    let pay_to = request.pay_to.clone();
    match header_str(headers, "Payment-Signature") {
        None => {
            return Err(HttpError::payment_required(
                VERIFY_ANSWER_SCHEME,
                VERIFY_ANSWER_AMOUNT,
                request_hash,
                pay_to,
                x402_version,
            ));
        }
        Some(signature) if signature != VERIFY_ANSWER_PAYMENT_SIGNATURE => {
            return Err(HttpError::payment_invalid(
                VERIFY_ANSWER_SCHEME,
                x402_version,
            ));
        }
        Some(_) => {}
    }

    let result = if request.answer == "reject" {
        "rejected"
    } else {
        "accepted"
    };
    let artifact_hash = hex::encode(Sha256::digest(request.answer.as_bytes()));
    let mut receipt = ReceiptCommitment::new(ReceiptCommitmentInput {
        agent_pk: request.agent_pk,
        family_id: request.family_id.clone(),
        verifier_id: request.verifier_id,
        verifier_hash_version: request.verifier_hash_version,
        artifact_hash,
        request_hash: request_hash.clone(),
        result: result.to_string(),
        fee_charged: VERIFY_ANSWER_AMOUNT.to_string(),
        reward_recipient: pay_to,
    })
    .map_err(|err| HttpError::bad_payload("verifyAnswer", err.to_string()))?;
    receipt.x402_version = Some(x402_version.clone());
    receipt.receipt_id = receipt.compute_id();

    let path = state
        .receipt_commitment_ledger_path
        .clone()
        .ok_or_else(HttpError::receipt_store_disabled)?;
    let store = state
        .receipt_store
        .as_mut()
        .ok_or_else(HttpError::receipt_store_disabled)?;
    FileReceiptStore::append(&path, &receipt)
        .map_err(|err| HttpError::bad_request(err.to_string()))?;
    store
        .apply(receipt.clone())
        .map_err(|err| HttpError::bad_request(err.to_string()))?;
    let agent_events = agent_passport_events_for_receipt(&receipt);

    Ok(json!({
        "ok": true,
        "verified": result == "accepted",
        "scheme": VERIFY_ANSWER_SCHEME,
        "x402Version": x402_version,
        "familyId": request.family_id,
        "verifierScope": "declared_family_only",
        "requestHash": request_hash,
        "receiptId": receipt.receipt_id,
        "receiptCommitment": receipt,
        "agentEvents": agent_events,
    }))
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn accepted_x402_versions() -> Result<Vec<String>, HttpError> {
    let fixture: X402VersionsFixture = serde_json::from_str(X402_VERSIONS_FIXTURE)
        .map_err(|err| HttpError::internal(format!("x402 versions fixture invalid: {err}")))?;
    Ok(fixture.accepted_versions)
}

fn session_public_view(session: &SessionState) -> Value {
    serde_json::to_value(session).expect("SessionState serializes via serde")
}

fn session_register_json(state: &mut LocalNodeState, body: &[u8]) -> Result<Value, HttpError> {
    let path = state
        .session_registry_path
        .clone()
        .ok_or_else(HttpError::session_registry_disabled)?;
    let store = state
        .session_store
        .as_mut()
        .ok_or_else(HttpError::session_registry_disabled)?;
    let envelope: Value = serde_json::from_slice(body)
        .map_err(|err| HttpError::bad_request(format!("body is not valid JSON: {err}")))?;
    let envelope_obj = envelope
        .as_object()
        .ok_or_else(|| HttpError::bad_request("body must be a JSON object"))?;
    let session_value = envelope_obj
        .get("session")
        .ok_or_else(|| HttpError::missing_field("session"))?;
    let session: SessionState = serde_json::from_value(session_value.clone())
        .map_err(|err| HttpError::bad_payload("session", err.to_string()))?;
    let current_height = envelope_obj
        .get("currentHeight")
        .and_then(Value::as_u64)
        .ok_or_else(|| HttpError::missing_field("currentHeight"))?;
    store
        .append_register(&path, &session, current_height)
        .map_err(|err| HttpError::bad_request(err.to_string()))?;
    let stored = store
        .get(&session.session_pk)
        .ok_or_else(|| HttpError::internal("session vanished after register"))?;
    Ok(json!({"ok": true, "session": session_public_view(stored)}))
}

fn session_get_json(state: &LocalNodeState, session_pk: &str) -> Result<Value, HttpError> {
    if !is_well_formed_hex32(session_pk) {
        return Err(HttpError::malformed_pk());
    }
    let store = state
        .session_store
        .as_ref()
        .ok_or_else(HttpError::session_registry_disabled)?;
    let session = store
        .get(session_pk)
        .ok_or_else(|| HttpError::session_not_found(session_pk.to_string()))?;
    Ok(json!({"ok": true, "session": session_public_view(session)}))
}

fn session_revoke_json(
    state: &mut LocalNodeState,
    session_pk: &str,
    body: &[u8],
) -> Result<Value, HttpError> {
    if !is_well_formed_hex32(session_pk) {
        return Err(HttpError::malformed_pk());
    }
    let path = state
        .session_registry_path
        .clone()
        .ok_or_else(HttpError::session_registry_disabled)?;
    let store = state
        .session_store
        .as_mut()
        .ok_or_else(HttpError::session_registry_disabled)?;
    if !store.sessions().contains_key(session_pk) {
        return Err(HttpError::session_not_found(session_pk.to_string()));
    }
    let envelope: Value = serde_json::from_slice(body)
        .map_err(|err| HttpError::bad_request(format!("body is not valid JSON: {err}")))?;
    let height = envelope
        .as_object()
        .and_then(|m| m.get("height"))
        .and_then(Value::as_u64)
        .ok_or_else(|| HttpError::missing_field("height"))?;
    store
        .append_revoke(&path, session_pk, height)
        .map_err(|err| HttpError::bad_request(err.to_string()))?;
    let stored = store
        .get(session_pk)
        .ok_or_else(|| HttpError::internal("session vanished after revoke"))?;
    Ok(json!({"ok": true, "session": session_public_view(stored)}))
}

/// N2.1 — session-bound `/submit` gate. Runs before the legacy admission
/// path. If the envelope does not carry a `session` block the gate is a
/// no-op so pre-wallet callers keep their existing semantics. When the
/// block is present the gate enforces:
///
///   1. `submittedBy` is a well-formed lowercase hex32 (`malformed_pk`
///      otherwise).
///   2. The session registry is configured; otherwise
///      `session_registry_disabled` so the wallet stack is opted in or
///      out atomically.
///   3. The registry knows the key (`session_unknown` else).
///   4. The session is not revoked and is active at the current node
///      height (`session_revoked` / `session_denied` else).
///   5. `rewardRecipient` matches the registered
///      `fixedRewardRecipient` (`reward_recipient_mismatch` else).
///   6. `signedWork` is a valid `boole.signed.v1` envelope whose pk
///      equals `submittedBy`, whose payload schema is
///      `boole.signer.work.v1`, whose route is `/submit`, whose nonce
///      equals `session.nonce`, and whose requestHash matches the
///      canonical hash of the submitted work body.
///   7. The `(submittedBy, nonce)` pair has not been burned before;
///      after the underlying admission returns `accepted: true`, the
///      pair is appended to the persistent ledger so replay (in-process
///      or post-restart) is rejected with `nonce_replayed`.
#[derive(Debug, Clone)]
struct CheckedSubmitSession {
    submitted_by: String,
    nonce: String,
    reward_recipient: String,
    request_hash: String,
    route: String,
}

#[derive(Debug, Clone)]
struct VerifiedSubmitWork {
    request_hash: String,
    route: String,
}

fn submit_session_gate(
    state: &mut LocalNodeState,
    body: &[u8],
) -> Result<Option<CheckedSubmitSession>, HttpError> {
    let envelope: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        // Malformed JSON is reported by the legacy `submit_json` path so
        // the gate stays out of the way for non-wallet callers.
        Err(_) => return Ok(None),
    };
    let envelope_obj = match envelope.as_object() {
        Some(obj) => obj,
        None => return Ok(None),
    };
    let session_value = match envelope_obj.get("session") {
        Some(value) if value.is_object() => value,
        _ => return Ok(None),
    };
    let session_obj = session_value
        .as_object()
        .expect("session value checked to be object above");
    let submitted_by = session_obj
        .get("submittedBy")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::missing_field("session.submittedBy"))?;
    let reward_recipient = session_obj
        .get("rewardRecipient")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::missing_field("session.rewardRecipient"))?;
    let nonce = session_obj
        .get("nonce")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::missing_field("session.nonce"))?;
    if !is_well_formed_hex32(submitted_by) {
        return Err(HttpError::malformed_pk());
    }

    state
        .submit_nonce_ledger_path
        .as_ref()
        .ok_or_else(HttpError::session_registry_disabled)?;
    let session_store = state
        .session_store
        .as_ref()
        .ok_or_else(HttpError::session_registry_disabled)?;
    let session = session_store
        .get(submitted_by)
        .ok_or_else(|| HttpError::session_unknown(submitted_by.to_string()))?;
    if session.revoked {
        return Err(HttpError::session_revoked(submitted_by.to_string()));
    }
    let current_height = state.runtime.cached_block_count() as u64;
    if let Err(err) = session.validate_at_height(current_height) {
        return Err(HttpError::session_denied(
            submitted_by.to_string(),
            err.to_string(),
        ));
    }
    if session.fixed_reward_recipient != reward_recipient {
        return Err(HttpError::reward_recipient_mismatch(
            session.fixed_reward_recipient.clone(),
            reward_recipient.to_string(),
        ));
    }

    let verified_work = verify_signed_submit_work(
        envelope_obj.get("body"),
        session_obj,
        submitted_by,
        nonce,
        session,
    )?;

    let ledger = state
        .nonce_ledger
        .as_ref()
        .ok_or_else(HttpError::session_registry_disabled)?;
    if ledger.contains(submitted_by, nonce) {
        return Err(HttpError::nonce_replayed(
            submitted_by.to_string(),
            nonce.to_string(),
        ));
    }
    Ok(Some(CheckedSubmitSession {
        submitted_by: submitted_by.to_string(),
        nonce: nonce.to_string(),
        reward_recipient: reward_recipient.to_string(),
        request_hash: verified_work.request_hash,
        route: verified_work.route,
    }))
}

fn burn_submit_nonce(
    state: &mut LocalNodeState,
    session: &CheckedSubmitSession,
) -> Result<(), HttpError> {
    let nonce_path = state
        .submit_nonce_ledger_path
        .clone()
        .ok_or_else(HttpError::session_registry_disabled)?;
    let ledger = state
        .nonce_ledger
        .as_mut()
        .ok_or_else(HttpError::session_registry_disabled)?;
    let appended = ledger
        .append_burn(&nonce_path, &session.submitted_by, &session.nonce)
        .map_err(|err| HttpError::internal(err.to_string()))?;
    if !appended {
        return Err(HttpError::nonce_replayed(
            session.submitted_by.clone(),
            session.nonce.clone(),
        ));
    }
    Ok(())
}

fn verify_signed_submit_work(
    body_value: Option<&Value>,
    session_obj: &serde_json::Map<String, Value>,
    submitted_by: &str,
    nonce: &str,
    session: &SessionState,
) -> Result<VerifiedSubmitWork, HttpError> {
    let work_body = body_value.ok_or_else(|| HttpError::missing_field("body"))?;
    let signed_work = session_obj
        .get("signedWork")
        .ok_or_else(|| HttpError::missing_field("session.signedWork"))?;
    let signed_obj = signed_work
        .as_object()
        .ok_or_else(|| HttpError::bad_envelope("session.signedWork must be an object"))?;
    let schema = signed_obj
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::missing_field("session.signedWork.schema"))?;
    if schema != SIGNED_ENVELOPE_SCHEMA {
        return Err(HttpError::bad_envelope(format!(
            "expected schema {SIGNED_ENVELOPE_SCHEMA}, got {schema}"
        )));
    }
    let payload = signed_obj
        .get("payload")
        .ok_or_else(|| HttpError::missing_field("session.signedWork.payload"))?;
    let pk = signed_obj
        .get("pk")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::missing_field("session.signedWork.pk"))?;
    let signature = signed_obj
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::missing_field("session.signedWork.signature"))?;
    if pk != submitted_by {
        return Err(HttpError::bad_payload(
            "session.signedWork.pk",
            "signed envelope pk must equal session.submittedBy",
        ));
    }
    match verify_signature(pk, signature, payload) {
        Ok(true) => {}
        Ok(false) => return Err(HttpError::signature_invalid()),
        Err(err) => return Err(HttpError::bad_envelope(err)),
    }

    let payload_obj = payload.as_object().ok_or_else(|| {
        HttpError::bad_payload("session.signedWork.payload", "payload must be an object")
    })?;
    let payload_schema = payload_obj
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::missing_field("session.signedWork.payload.schema"))?;
    if payload_schema != "boole.signer.work.v1" {
        return Err(HttpError::bad_payload(
            "session.signedWork.payload.schema",
            "expected boole.signer.work.v1",
        ));
    }
    let route = payload_obj
        .get("route")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::missing_field("session.signedWork.payload.route"))?;
    if route != "/submit" {
        return Err(HttpError::bad_payload(
            "session.signedWork.payload.route",
            "route must be /submit",
        ));
    }
    let signed_nonce = payload_obj
        .get("nonce")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::missing_field("session.signedWork.payload.nonce"))?;
    if signed_nonce != nonce {
        return Err(HttpError::bad_payload(
            "session.signedWork.payload.nonce",
            "signed nonce must equal session.nonce",
        ));
    }
    let work_payload = payload_obj
        .get("workPayload")
        .ok_or_else(|| HttpError::missing_field("session.signedWork.payload.workPayload"))?;
    if work_payload != work_body {
        return Err(HttpError::bad_payload(
            "session.signedWork.payload.workPayload",
            "signed workPayload must equal submitted body",
        ));
    }
    let request_hash = payload_obj
        .get("requestHash")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::missing_field("session.signedWork.payload.requestHash"))?;
    let computed_hash = canonical_payload_hash_hex(work_payload);
    if request_hash != computed_hash {
        return Err(HttpError::bad_payload(
            "session.signedWork.payload.requestHash",
            "requestHash must equal canonical hash of workPayload",
        ));
    }
    let fee = payload_obj
        .get("fee")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::missing_field("session.signedWork.payload.fee"))?;
    let fee = fee
        .parse::<u128>()
        .map_err(|err| HttpError::bad_payload("session.signedWork.payload.fee", err.to_string()))?;
    let max_fee = session
        .max_fee_per_request
        .parse::<u128>()
        .map_err(|err| HttpError::bad_payload("session.maxFeePerRequest", err.to_string()))?;
    if fee > max_fee {
        return Err(HttpError::bad_payload(
            "session.signedWork.payload.fee",
            "fee exceeds session.maxFeePerRequest",
        ));
    }
    Ok(VerifiedSubmitWork {
        request_hash: request_hash.to_string(),
        route: route.to_string(),
    })
}

async fn fallback_handler(method: Method, uri: Uri) -> Response {
    error_response(HttpError::not_found(format!(
        "no route for {} {}",
        method,
        uri.path()
    )))
}

fn status_json(state: &LocalNodeState) -> anyhow::Result<Value> {
    // Serve from the in-memory block cache. After boot the cache is
    // authoritative; commits update it synchronously via {check, append,
    // apply_unchecked}, and replay invariants (chain linkage, latest_c) are
    // checked at boot via replay_blocks. The boot-time match is captured in
    // replay_matches_runtime_at_boot rather than asserted here as a constant.
    let height = state.runtime.cached_block_count();
    let head = current_head(state);
    let promoted = boole_core::select_promoted_bounty_shares(
        &state.bounty_side_pool,
        &state.family_manifest_registry,
        height as u64,
        &state.operator_signer_pks,
    );
    Ok(json!({
        "ok": true,
        "mode": "local",
        "height": height,
        "c": head.clone(),
        "genesisC": state.genesis_c,
        "replayHeight": height,
        "replayLatestC": head,
        "replayMatchesRuntime": state.replay_matches_runtime_at_boot,
        "blockStorePath": state.block_path.to_string_lossy(),
        "sharePoolSize": state.runtime.pool_size(),
        "familyManifestCount": state.family_manifest_registry.len(),
        "bountySidePoolTotal": state.bounty_side_pool.total_share_count(),
        "promotedBountySharesCount": promoted.len(),
    }))
}

fn head_json(state: &LocalNodeState) -> anyhow::Result<Value> {
    let height = state.runtime.cached_block_count();
    let report = &state.report;
    Ok(json!({
        "ok": true,
        "height": height,
        "c": current_head(state),
        "T_ticket": report.T_ticket,
        "T_share": report.T_share,
        "T_block": report.T_block,
        "T_submit": report.T_submit,
        "MinShareScoreMultiplier": report.MinShareScoreMultiplier,
        "M": report.M,
        "K_max": report.K_max,
        "L": report.L,
        "D_max": report.D_max,
        "provenance": report.provenance,
    }))
}

fn health_json(state: &LocalNodeState) -> Value {
    json!({
        "ok": true,
        "status": "ok",
        "sharePoolSize": state.runtime.pool_size(),
        "provenance": state.report.provenance,
    })
}

fn block_latest_json(state: &LocalNodeState) -> Value {
    let blocks = state.runtime.cached_blocks();
    if let Some(block) = blocks.last() {
        let height = blocks.len() - 1;
        json!({
            "ok": true,
            "block": block_json(block),
            "height": height,
            "c": block.c,
        })
    } else {
        json!({
            "ok": true,
            "block": Value::Null,
            "height": Value::Null,
            "c": state.genesis_c,
        })
    }
}

fn block_by_height_json(state: &LocalNodeState, raw: &str) -> Result<Value, HttpError> {
    let height: usize = raw
        .parse()
        .map_err(|_| HttpError::bad_request("height must be a non-negative integer"))?;
    let blocks = state.runtime.cached_blocks();
    let block = blocks
        .get(height)
        .ok_or_else(|| HttpError::not_found(format!("no block at height {height}")))?;
    Ok(json!({
        "ok": true,
        "block": block_json(block),
        "height": height,
        "c": block.c,
    }))
}

fn account_balance_json(state: &LocalNodeState, pk: &str) -> Result<Value, HttpError> {
    if !is_well_formed_hex32(pk) {
        return Err(HttpError::malformed_pk());
    }
    let balance = state.runtime.balance_for(pk);
    let (as_of_height, as_of_c) = match state.runtime.ledger_head() {
        Some((height, c)) => (Value::from(height), Value::from(c)),
        None => (Value::from(0u64), Value::from(state.genesis_c.clone())),
    };
    Ok(json!({
        "ok": true,
        "pk": pk,
        "balance": balance.to_string(),
        "asOfHeight": as_of_height,
        "asOfC": as_of_c,
    }))
}

fn is_well_formed_hex32(s: &str) -> bool {
    Hex32::from_hex(s).is_ok()
}

fn work_list_json(state: &LocalNodeState) -> Value {
    let work = serde_json::to_value(&state.work_manifests)
        .expect("WorkManifest serializes to JSON via serde");
    json!({
        "ok": true,
        "work": work,
    })
}

fn work_by_id_json(state: &LocalNodeState, id: &str) -> Result<Value, HttpError> {
    let manifest = state
        .work_manifests
        .iter()
        .find(|m| m.work_id == id)
        .ok_or_else(|| HttpError::work_not_found(id))?;
    let manifest_json =
        serde_json::to_value(manifest).expect("WorkManifest serializes to JSON via serde");
    Ok(json!({
        "ok": true,
        "work": manifest_json,
    }))
}

fn bounty_list_json(state: &LocalNodeState) -> Value {
    let listing = state.bounty_registry.list();
    let bounties = serde_json::to_value(&listing).expect("Bounty serializes to JSON via serde");
    json!({
        "ok": true,
        "bounties": bounties,
    })
}

fn bounty_by_id_json(state: &LocalNodeState, id: &str) -> Result<Value, HttpError> {
    let bounty = state
        .bounty_registry
        .get(id)
        .ok_or_else(|| HttpError::bounty_not_found(id))?;
    let bounty_json = serde_json::to_value(&bounty).expect("Bounty serializes to JSON via serde");
    Ok(json!({
        "ok": true,
        "bounty": bounty_json,
    }))
}

async fn bounty_proof_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> Response {
    let mut guard = state.inner.write().await;
    match bounty_proof_json(&mut guard, &id, &body) {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) => error_response(err),
    }
}

async fn bounty_announce_handler(State(state): State<AppState>, body: Bytes) -> Response {
    let mut guard = state.inner.write().await;
    match bounty_announce_json(&mut guard, &body) {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) => error_response(err),
    }
}

const ANNOUNCE_PAYLOAD_SCHEMA: &str = "boole.bounty.announce.v1";

fn bounty_announce_json(state: &mut LocalNodeState, body: &[u8]) -> Result<Value, HttpError> {
    // 1) Parse outer envelope: must be a JSON object with the four
    //    `boole.signed.v1` fields. Anything else is `bad_envelope`.
    let envelope: Value = serde_json::from_slice(body)
        .map_err(|err| HttpError::bad_envelope(format!("body is not valid JSON: {err}")))?;
    let envelope_obj = envelope
        .as_object()
        .ok_or_else(|| HttpError::bad_envelope("envelope must be a JSON object"))?;
    let schema = envelope_obj
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_envelope("envelope missing schema"))?;
    if schema != SIGNED_ENVELOPE_SCHEMA {
        return Err(HttpError::bad_envelope(format!(
            "expected schema {SIGNED_ENVELOPE_SCHEMA}, got {schema}"
        )));
    }
    let pk = envelope_obj
        .get("pk")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_envelope("envelope missing pk"))?;
    let signature = envelope_obj
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_envelope("envelope missing signature"))?;
    let payload = envelope_obj
        .get("payload")
        .ok_or_else(|| HttpError::bad_envelope("envelope missing payload"))?;

    // 2) Local hex-shape checks. Match the wire-malformed split used by
    //    `keys verify` so error vocabularies are consistent across the CLI
    //    and HTTP surface.
    if !is_well_formed_hex32(pk) {
        return Err(HttpError::bad_envelope("pk must be 64 lowercase hex chars"));
    }
    if Hex64::from_hex(signature).is_err() {
        return Err(HttpError::bad_envelope(
            "signature must be 128 lowercase hex chars",
        ));
    }

    // 3) Crypto: structurally valid envelope but wrong sig is 401, not 400.
    match verify_signature(pk, signature, payload) {
        Ok(true) => {}
        Ok(false) => return Err(HttpError::signature_invalid()),
        Err(detail) => return Err(HttpError::bad_envelope(detail)),
    }

    // 4) Inner payload validation. The CLI builds this; the wire format
    //    matches `CreateBountyInput` field-for-field with camelCase.
    let payload_obj = payload
        .as_object()
        .ok_or_else(|| HttpError::bad_payload("payload", "payload must be a JSON object"))?;
    let payload_schema = payload_obj
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_payload("schema", "payload missing schema"))?;
    if payload_schema != ANNOUNCE_PAYLOAD_SCHEMA {
        return Err(HttpError::bad_payload(
            "schema",
            format!("expected {ANNOUNCE_PAYLOAD_SCHEMA}, got {payload_schema}"),
        ));
    }
    let id = required_payload_string(payload_obj, "id")?.to_string();
    let domain = required_payload_string(payload_obj, "domain")?.to_string();
    let problem_hash = required_payload_string(payload_obj, "problemHash")?.to_string();
    let verifier = payload_obj
        .get("verifier")
        .and_then(Value::as_object)
        .ok_or_else(|| HttpError::bad_payload("verifier", "verifier must be a JSON object"))?;
    let verifier_kind = verifier
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_payload("verifier.kind", "verifier.kind missing"))?
        .to_string();
    let verifier_metadata = verifier
        .get("metadata")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let reward_str = required_payload_string(payload_obj, "reward")?;
    let reward: u128 = reward_str
        .parse()
        .map_err(|err| HttpError::bad_payload("reward", format!("reward must be u128: {err}")))?;
    let deadline = payload_obj
        .get("deadline")
        .and_then(Value::as_u64)
        .ok_or_else(|| HttpError::bad_payload("deadline", "deadline must be u64 unix ms"))?;
    let ts = payload_obj
        .get("ts")
        .and_then(Value::as_u64)
        .ok_or_else(|| HttpError::bad_payload("ts", "ts must be u64 unix ms"))?;

    // 5) Acquire registry mutation. validate_create surfaces field-level
    //    rejections; map duplicates to 409 so operators can distinguish
    //    "wire bad" (400) from "logically already there" (409).
    let bounty = match state.bounty_registry.create(CreateBountyInput {
        id: id.clone(),
        domain: domain.clone(),
        problem_hash: problem_hash.clone(),
        verifier_kind: verifier_kind.clone(),
        verifier_metadata,
        reward,
        deadline,
        ts,
    }) {
        Ok(b) => b,
        Err(err) if err.starts_with("bounty id already exists") => {
            return Err(HttpError::bounty_already_exists(id));
        }
        Err(err) => return Err(HttpError::bad_payload("create", err)),
    };

    // 6) Audit-log append. Same fatal-on-failure stance as the proof
    //    handler — once the registry mutated, dropping the durability
    //    promise silently is worse than a 500 the operator can retry.
    let bounty_value = serde_json::to_value(&bounty).expect("Bounty serializes to JSON via serde");
    let event = json!({
        "schemaVersion": 1,
        "kind": "create",
        "workId": id,
        "problemHash": problem_hash,
        "verifierKind": verifier_kind,
        "ts": ts,
        "announcerPk": pk,
        "bounty": bounty_value.clone(),
    });
    if let Some(path) = state.bounty_event_ledger_path.as_ref() {
        FileBountyEventLedger::append(path, &event)
            .map_err(|err| HttpError::internal(format!("bounty audit append: {err}")))?;
    }

    Ok(json!({
        "ok": true,
        "bounty": bounty_value,
    }))
}

async fn bounty_status_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> Response {
    let mut guard = state.inner.write().await;
    match bounty_status_json(&mut guard, &id, &body) {
        Ok(value) => (StatusCode::OK, Json(value)).into_response(),
        Err(err) => error_response(err),
    }
}

const STATUS_PAYLOAD_SCHEMA: &str = "boole.bounty.status.v1";

fn bounty_status_json(
    state: &mut LocalNodeState,
    url_id: &str,
    body: &[u8],
) -> Result<Value, HttpError> {
    // 1) Outer envelope. Same shape as the announce handler.
    let envelope: Value = serde_json::from_slice(body)
        .map_err(|err| HttpError::bad_envelope(format!("body is not valid JSON: {err}")))?;
    let envelope_obj = envelope
        .as_object()
        .ok_or_else(|| HttpError::bad_envelope("envelope must be a JSON object"))?;
    let schema = envelope_obj
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_envelope("envelope missing schema"))?;
    if schema != SIGNED_ENVELOPE_SCHEMA {
        return Err(HttpError::bad_envelope(format!(
            "expected schema {SIGNED_ENVELOPE_SCHEMA}, got {schema}"
        )));
    }
    let pk = envelope_obj
        .get("pk")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_envelope("envelope missing pk"))?;
    let signature = envelope_obj
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_envelope("envelope missing signature"))?;
    let payload = envelope_obj
        .get("payload")
        .ok_or_else(|| HttpError::bad_envelope("envelope missing payload"))?;
    if !is_well_formed_hex32(pk) {
        return Err(HttpError::bad_envelope("pk must be 64 lowercase hex chars"));
    }
    if Hex64::from_hex(signature).is_err() {
        return Err(HttpError::bad_envelope(
            "signature must be 128 lowercase hex chars",
        ));
    }
    match verify_signature(pk, signature, payload) {
        Ok(true) => {}
        Ok(false) => return Err(HttpError::signature_invalid()),
        Err(detail) => return Err(HttpError::bad_envelope(detail)),
    }

    // 2) Inner payload.
    let payload_obj = payload
        .as_object()
        .ok_or_else(|| HttpError::bad_payload("payload", "payload must be a JSON object"))?;
    let payload_schema = payload_obj
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_payload("schema", "payload missing schema"))?;
    if payload_schema != STATUS_PAYLOAD_SCHEMA {
        return Err(HttpError::bad_payload(
            "schema",
            format!("expected {STATUS_PAYLOAD_SCHEMA}, got {payload_schema}"),
        ));
    }
    let payload_id = required_payload_string(payload_obj, "id")?.to_string();
    if payload_id != url_id {
        return Err(HttpError::bounty_id_mismatch(url_id, payload_id));
    }
    let new_status = required_payload_string(payload_obj, "newStatus")?.to_string();
    if !is_known_status(&new_status) {
        return Err(HttpError::bad_status_value(new_status));
    }
    let ts = payload_obj
        .get("ts")
        .and_then(Value::as_u64)
        .ok_or_else(|| HttpError::bad_payload("ts", "ts must be u64 unix ms"))?;
    let reason = payload_obj
        .get("reason")
        .and_then(Value::as_str)
        .map(|s| s.to_string());

    // 3) Look up the existing bounty so we can stamp prevStatus into the
    //    audit event AND populate the flat index fields the ledger validator
    //    requires (workId/problemHash/verifierKind). 404 if unknown.
    let existing = state
        .bounty_registry
        .get(url_id)
        .ok_or_else(|| HttpError::bounty_not_found(url_id))?;
    let prev_status = existing.status.clone();
    let problem_hash = existing.problem_hash.clone();
    let verifier_kind = existing.verifier.kind.clone();

    // 4) Apply the transition. The registry enforces transition rules; map
    //    terminal-state errors to 409 and any other rule failure to 400 so
    //    a future stricter rule set doesn't need a wire-contract bump.
    let updated = match state.bounty_registry.update_status(UpdateStatusInput {
        id: url_id.to_string(),
        status: new_status.clone(),
        ts,
    }) {
        Ok(b) => b,
        Err(err) if err.starts_with("cannot transition from terminal status") => {
            return Err(HttpError::bounty_terminal(prev_status));
        }
        Err(err) if err.starts_with("unknown bounty id") => {
            return Err(HttpError::bounty_not_found(url_id));
        }
        Err(err) => return Err(HttpError::invalid_status_transition(err)),
    };

    // 5) Audit-log append. status_change events carry prevStatus + newStatus
    //    so a recovering node can rebuild the transition history.
    let mut event = json!({
        "schemaVersion": 1,
        "kind": "status_change",
        "workId": url_id,
        "problemHash": problem_hash,
        "verifierKind": verifier_kind,
        "ts": ts,
        "prevStatus": prev_status,
        "newStatus": new_status,
        "announcerPk": pk,
    });
    if let Some(reason_text) = reason {
        event["reason"] = Value::String(reason_text);
    }
    if let Some(path) = state.bounty_event_ledger_path.as_ref() {
        FileBountyEventLedger::append(path, &event)
            .map_err(|err| HttpError::internal(format!("bounty audit append: {err}")))?;
    }

    let bounty_value = serde_json::to_value(&updated).expect("Bounty serializes to JSON via serde");
    Ok(json!({
        "ok": true,
        "bounty": bounty_value,
    }))
}

fn is_known_status(s: &str) -> bool {
    matches!(s, "open" | "solved" | "expired" | "withdrawn")
}

fn required_payload_string<'a>(
    payload: &'a serde_json::Map<String, Value>,
    field: &'static str,
) -> Result<&'a str, HttpError> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_payload(field, format!("payload missing string {field}")))
}

fn bounty_proof_json(
    state: &mut LocalNodeState,
    id: &str,
    body: &[u8],
) -> Result<Value, HttpError> {
    // 1) 404 — bounty must exist (catalog or registry-replayed).
    let bounty = state
        .bounty_registry
        .get(id)
        .ok_or_else(|| HttpError::bounty_not_found(id))?;

    // 2) 400 — body must be JSON object with proofHash/prover/envelope.
    let body_value: Value = serde_json::from_slice(body)
        .map_err(|err| HttpError::bad_request(format!("body is not valid JSON: {err}")))?;
    let body_obj = body_value
        .as_object()
        .ok_or_else(|| HttpError::bad_request("proof body must be a JSON object"))?;
    let proof_hash = body_obj
        .get("proofHash")
        .and_then(Value::as_str)
        .ok_or_else(HttpError::bad_proof_hash)?
        .to_string();
    if Hex32::from_hex(&proof_hash).is_err() {
        return Err(HttpError::bad_proof_hash());
    }
    let prover = body_obj
        .get("prover")
        .and_then(Value::as_str)
        .ok_or_else(HttpError::bad_prover)?
        .to_string();
    if Hex32::from_hex(&prover).is_err() {
        return Err(HttpError::bad_prover());
    }
    let envelope = body_obj.get("envelope").cloned().unwrap_or(Value::Null);

    // 3) Dedup peek — wins over terminal status and verifier dispatch so
    //    a re-post is idempotent and does not pay for `lake exec`.
    if let Some(accepted) = state.bounty_registry.has_proof(id, &proof_hash) {
        return Ok(json!({
            "ok": true,
            "accepted": accepted,
            "duplicate": true,
            "bounty": serde_json::to_value(&bounty)
                .expect("Bounty serializes to JSON via serde"),
        }));
    }

    // 4) 501 — unknown verifier kind. Caller knows to retry with a node
    //    that has the verifier wired in.
    let verifier = state
        .bounty_verifiers
        .get(&bounty.verifier.kind)
        .cloned()
        .ok_or_else(|| HttpError::no_verifier(&bounty.verifier.kind))?;

    // 5) 409 — terminal bounty. Comes after dedup so a duplicate post on
    //    a now-solved bounty short-circuits with `duplicate=true`.
    if bounty.status != "open" {
        return Err(HttpError::bounty_terminal(&bounty.status));
    }

    // 6) Run verifier. `Err` → 502 verifier_error; `Ok(false)` is a
    //    valid reject signal that we still record so downstream tooling
    //    can audit and dedup against rejected hashes.
    let accepted = verifier
        .verify(&bounty, &envelope)
        .map_err(HttpError::verifier_error)?;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // 7) Mutate registry. submit_proof both records the dedup entry and
    //    flips status to "solved" on accepted=true.
    let outcome = state
        .bounty_registry
        .submit_proof(SubmitProofInput {
            bounty_id: id.to_string(),
            proof_hash: proof_hash.clone(),
            prover: prover.clone(),
            accepted,
            ts: now_ms,
        })
        .map_err(|err| HttpError::internal(format!("bounty registry: {err}")))?;

    // 7b) On accept, route the share into the per-family side-pool. The
    //     Hard Guard holds because (a) this writes to `bounty_side_pool`,
    //     never to `runtime` or `share_pool`, and (b) `build_block_selection`
    //     does not consume from the side-pool. S22 adds the gated read path.
    //     `family_id == bounty.domain` per the bounty/manifest fixture
    //     convention; if the domain has no registered manifest we still
    //     record the share so S22 can audit "would have promoted but no
    //     manifest" cases.
    if accepted {
        // S23a — stamp the matching bounty's reward onto the share so the
        // promotion gate can compute capped credit without a second
        // registry lookup. Malformed reward strings (which the registry
        // already validates as `u128` decimal) collapse to 0 here so the
        // share is still tracked but no credit ever issues.
        let reward: u128 = bounty.reward.parse().unwrap_or(0);
        state.bounty_side_pool.insert(BountyShare {
            bounty_id: id.to_string(),
            proof_hash: proof_hash.clone(),
            prover: prover.clone(),
            family_id: bounty.domain.clone(),
            ts: now_ms,
            reward,
        });
    }

    // 8) Audit-log append. Failure here is fatal — the in-memory state
    //    has already mutated; surfacing a 500 at this point is preferable
    //    to silently dropping the durability promise.
    let credit = if accepted {
        bounty.reward.clone()
    } else {
        "0".to_string()
    };
    let event = json!({
        "schemaVersion": 1,
        "kind": "proof",
        "workId": id,
        "problemHash": bounty.problem_hash,
        "verifierKind": bounty.verifier.kind,
        "ts": now_ms,
        "proofHash": proof_hash,
        "solverPk": prover,
        "accepted": accepted,
        "reward": bounty.reward,
        "credit": credit,
    });
    if let Some(path) = state.bounty_event_ledger_path.as_ref() {
        FileBountyEventLedger::append(path, &event)
            .map_err(|err| HttpError::internal(format!("bounty audit append: {err}")))?;
    }

    Ok(json!({
        "ok": true,
        "accepted": outcome.accepted,
        "duplicate": outcome.duplicate,
        "bounty": serde_json::to_value(&outcome.bounty)
            .expect("Bounty serializes to JSON via serde"),
    }))
}

fn config_json(state: &LocalNodeState) -> Value {
    let report = &state.report;
    json!({
        "ok": true,
        "T_submit": report.T_submit,
        "T_share": report.T_share,
        "T_block": report.T_block,
        "T_ticket": report.T_ticket,
        "MinShareScoreMultiplier": report.MinShareScoreMultiplier,
        "M": report.M,
        "K_max": report.K_max,
        "ShareCapPerPK_Block": report.ShareCapPerPK_Block,
        "L": report.L,
        "D_max": report.D_max,
        "EMAWindow": report.EMAWindow,
        "perIpRateLimitPer60s": report.perIpRateLimitPer60s,
        "provenance": report.provenance,
    })
}

/// pof TicketBody contract: `{c, pk, n}` only.
///
/// Order matters for diagnostics: the first field encountered outside the
/// allowed set is reported, and fields are required in {c, pk, n} order so
/// callers see `missing_field: c` before `missing_field: pk`.
const TICKET_BODY_FIELDS: &[&str] = &["c", "pk", "n"];

fn ticket_json(state: &mut LocalNodeState, body: &[u8]) -> Result<Value, HttpError> {
    let body_value: Value = serde_json::from_slice(body)
        .map_err(|err| HttpError::bad_request(format!("body is not valid JSON: {err}")))?;
    let ticket_body = body_value
        .as_object()
        .ok_or_else(|| HttpError::bad_request("ticket body must be a JSON object"))?;

    for key in ticket_body.keys() {
        if !TICKET_BODY_FIELDS.contains(&key.as_str()) {
            return Err(HttpError::unexpected_field(key.clone()));
        }
    }

    let c_str = required_string(ticket_body, "c")?;
    let pk_str = required_string(ticket_body, "pk")?;
    let n_str = required_string(ticket_body, "n")?;

    let c = Hex32::from_hex(c_str).map_err(|_| HttpError::bad_hex("c"))?;
    let pk = Hex32::from_hex(pk_str).map_err(|_| HttpError::bad_hex("pk"))?;
    let n = Hex32::from_hex(n_str).map_err(|_| HttpError::bad_hex("n"))?;

    state
        .runtime
        .observe_ticket_from_body(ticket_body)
        .map_err(|err| HttpError::bad_request(err.to_string()))?;
    let result = ticket(
        &c,
        &pk,
        &n,
        &state.runtime.config.policy.thresholds.t_ticket,
    );
    Ok(json!({
        "ok": true,
        "hashHex": result.hash_bytes.to_hex(),
        "valid": result.valid,
    }))
}

fn required_string<'a>(
    body: &'a serde_json::Map<String, Value>,
    field: &'static str,
) -> Result<&'a str, HttpError> {
    body.get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::missing_field(field))
}

fn normalize_pow_fields(body: &mut serde_json::Map<String, Value>) {
    for field in ["n", "j", "nonceS"] {
        if let Some(value) = body.get(field).and_then(Value::as_str) {
            if value.len() < 64
                && value.len() % 2 == 0
                && value.bytes().all(|b| b.is_ascii_hexdigit())
            {
                body.insert(field.to_string(), Value::String(format!("{value:0>64}")));
            }
        }
    }
}

fn submit_json(
    state: &mut LocalNodeState,
    body: &[u8],
    peer_ip: &str,
    checked_session: Option<&CheckedSubmitSession>,
) -> anyhow::Result<Value> {
    let body_value: Value = serde_json::from_slice(body)?;
    let submit_body = body_value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("submit body must be a JSON object"))?;
    let canon_tag_raw = submit_body
        .get("canonTag")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if canon_tag_raw > u8::MAX as u64 {
        return Ok(json!({
            "ok": false,
            "accepted": false,
            "error": "canon_tag_out_of_range",
            "canonTag": canon_tag_raw,
            "max": u8::MAX,
        }));
    }
    let canon_tag = canon_tag_raw as u8;
    let ts_raw = submit_body
        .get("ts")
        .and_then(Value::as_u64)
        .unwrap_or(1_800_000_000_000);
    if ts_raw > i64::MAX as u64 {
        return Ok(json!({
            "ok": false,
            "accepted": false,
            "error": "ts_out_of_range",
            "ts": ts_raw,
            "maxI64": i64::MAX,
        }));
    }
    let ts_i64 = ts_raw as i64;
    let mut body = submit_body
        .get("body")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(|| submit_body.clone());
    normalize_pow_fields(&mut body);

    state
        .runtime
        .observe_ticket_from_body(&body)
        .map_err(|err| anyhow::anyhow!(err))?;
    let decision = state.runtime.admit_body_with_canon_tag_and_reward_pk(
        ts_i64,
        peer_ip,
        &body,
        canon_tag,
        checked_session.map(|session| session.reward_recipient.as_str()),
    );
    let AdmissionDecision::Accepted { share_hash } = decision else {
        return Ok(json!({
            "ok": false,
            "accepted": false,
            "decision": format!("{decision:?}"),
            "c": current_head(state),
        }));
    };
    // P1.3a (L7) — burn (submittedBy, nonce) to disk BEFORE any block
    // disk write. A crash that lands the block but not the burn leaves
    // an irrecoverable replay window because recovery cannot tell that
    // the nonce was already consumed. The reverse — a burn with no
    // block — is recoverable: replay sees a burned nonce and rejects a
    // future submit with the same pair (correct behavior). The session
    // gate already serialized this admission under the write-lock so
    // `append_burn` cannot race with itself here.
    if let Some(session) = checked_session {
        burn_submit_nonce(state, session).map_err(|err| anyhow::anyhow!("{err:?}"))?;
    }
    let accepted_tags = BTreeSet::from([canon_tag]);
    match state
        .runtime
        .build_block_selection_for_current_c(&accepted_tags)?
    {
        BuildSelectionResult::Ok(_) => {}
        BuildSelectionResult::NoProposer { .. } => {
            return Ok(json!({
                "ok": true,
                "accepted": true,
                "shareAccepted": true,
                "blockProduced": false,
                "decision": "NoProposer",
                "shareHash": share_hash.to_hex(),
                "height": state.runtime.cached_block_count(),
                "c": current_head(state),
            }));
        }
        BuildSelectionResult::AmbiguousProposer { count, .. } => {
            return Ok(json!({
                "ok": true,
                "accepted": true,
                "shareAccepted": true,
                "blockProduced": false,
                "decision": "AmbiguousProposer",
                "proposerCount": count,
                "shareHash": share_hash.to_hex(),
                "height": state.runtime.cached_block_count(),
                "c": current_head(state),
            }));
        }
    }
    let block_path = state.block_path.clone();
    // S23c — compute the promoted bounty selection at the latest known
    // height (`block_cache.len()` is the about-to-be-committed block's
    // height). The selection feeds both the persisted block's
    // `promoted_bounty_credits` and the merged reward-ledger event.
    let promotion_height = state.runtime.cached_block_count() as u64;
    let selection = boole_core::select_promoted_bounty_selection(
        &state.bounty_side_pool,
        &state.family_manifest_registry,
        promotion_height,
        &state.operator_signer_pks,
    );
    let committed = state
        .runtime
        .commit_next_block_for_current_c_with_promoted(
            &block_path,
            ts_raw,
            &accepted_tags,
            &selection.shares,
            &selection.credits,
        )?;
    // S23c — mirror the credit rows into the bounty event ledger so the
    // divergence sweep (S23d) has a parallel source to compare against.
    if let Some(bounty_event_path) = state.bounty_event_ledger_path.as_ref() {
        for credit in &committed.block.promoted_bounty_credits {
            let event = json!({
                "schemaVersion": 1,
                "kind": "credit",
                "height": committed.block.height,
                "c": committed.block.c,
                "familyId": credit.family_id,
                "bountyId": credit.bounty_id,
                "prover": credit.prover,
                "amount": credit.amount,
            });
            FileBountyEventLedger::append(bounty_event_path, &event)?;
        }
    }
    // After commit_next_block: the runtime head is the new block's c, and the
    // store size is committed.block.height + 1 by construction. We do not need
    // to read the store again or re-replay the chain — apply_produced_block has
    // already verified linkage and updated runtime head.
    let new_height = committed.block.height + 1;
    let runtime_head = current_head(state);
    let block_value = block_json(&committed.block);
    let receipt = match checked_session {
        Some(session) => Some(submit_receipt_json(
            session,
            &committed.block,
            &share_hash.to_hex(),
        )?),
        None => None,
    };
    if let (Some(path), Some(receipt)) =
        (state.submit_receipt_ledger_path.as_ref(), receipt.as_ref())
    {
        append_submit_receipt(path, receipt)?;
    }
    let mut response = json!({
        "ok": true,
        "accepted": true,
        "shareHash": share_hash.to_hex(),
        "block": block_value,
        "height": new_height,
        "c": runtime_head,
        "replayHeight": new_height,
        "replayLatestC": runtime_head,
        "replayMatchesRuntime": state.replay_matches_runtime_at_boot,
        "droppedStaleShares": committed.dropped_stale_shares,
    });
    if let Some(receipt) = receipt {
        response["receipt"] = receipt;
    }
    Ok(response)
}

fn submit_receipt_json(
    session: &CheckedSubmitSession,
    block: &PersistedBlock,
    share_hash: &str,
) -> anyhow::Result<Value> {
    let reward_amount = compute_block_reward_credits(block)?
        .into_iter()
        .find(|credit| credit.pk == session.reward_recipient)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "rewardRecipient not credited by replay: {}",
                session.reward_recipient
            )
        })?
        .amount;
    Ok(json!({
        "schema": "boole.submit.receipt.v1",
        "accepted": true,
        "route": session.route,
        "sessionPk": session.submitted_by,
        "submittedBy": session.submitted_by,
        "nonce": session.nonce,
        "requestHash": session.request_hash,
        "blockHeight": block.height,
        "blockC": block.c,
        "shareHash": share_hash,
        "proposerPk": block.proposer_pk,
        "rewardRecipient": session.reward_recipient,
        "rewardAmount": reward_amount,
    }))
}

fn append_submit_receipt(path: &std::path::Path, receipt: &Value) -> anyhow::Result<()> {
    crate::durability::append_ndjson_line_durable(path, &serde_json::to_string(receipt)?)
}

fn current_head(state: &LocalNodeState) -> String {
    state
        .runtime
        .current_c()
        .unwrap_or(&state.genesis_c)
        .to_string()
}

fn block_json(block: &PersistedBlock) -> Value {
    let mut value = json!({
        "height": block.height,
        "prevC": block.prev_c,
        "c": block.c,
        "proposerPk": block.proposer_pk,
        "selectedShareHashes": block.selected_share_hashes,
        "selectedSharePks": block.selected_share_pks,
        "minShareScore": block.min_share_score,
        "kmaxApplied": block.kmax_applied,
        "difficultyEpoch": block.difficulty_epoch,
        "tBlock": block.t_block,
        "tShare": block.t_share,
        "difficultyWeight": block.difficulty_weight,
        "ts": block.ts,
    });
    if let Some(obj) = value.as_object_mut() {
        if !block.proposer_reward_pk.is_empty() {
            obj.insert(
                "proposerRewardPk".to_string(),
                Value::String(block.proposer_reward_pk.clone()),
            );
        }
        if !block.selected_share_reward_pks.is_empty() {
            obj.insert(
                "selectedShareRewardPks".to_string(),
                serde_json::to_value(&block.selected_share_reward_pks)
                    .expect("selected share reward pks serialize"),
            );
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    const PK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const HASH_0: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const HASH_1: &str = "1111111111111111111111111111111111111111111111111111111111111111";

    #[test]
    fn submit_receipt_json_fails_when_reward_recipient_is_not_replay_credited() {
        let session = CheckedSubmitSession {
            submitted_by: PK_A.to_string(),
            nonce: "n-missing-credit".to_string(),
            reward_recipient: PK_B.to_string(),
            request_hash: HASH_1.to_string(),
            route: "/submit".to_string(),
        };
        let block = PersistedBlock {
            height: 0,
            prev_c: HASH_0.to_string(),
            c: HASH_1.to_string(),
            proposer_pk: PK_A.to_string(),
            selected_share_hashes: vec![HASH_1.to_string()],
            selected_share_pks: vec![PK_A.to_string()],
            selected_share_reward_pks: vec![PK_A.to_string()],
            proposer_reward_pk: PK_A.to_string(),
            selected_share_evidence: Vec::new(),
            min_share_score: "0".to_string(),
            min_share_score_multiplier_nanos: 0,
            kmax_applied: 1,
            difficulty_epoch: 0,
            t_block: "ff".to_string(),
            t_share: "ff".to_string(),
            difficulty_weight: "1".to_string(),
            dropped_below_min_score: 0,
            dropped_kernel_reject: 0,
            truncated_by_kmax: 0,
            ts: 0,
            promoted_bounty_credits: Vec::new(),
        };

        let err = submit_receipt_json(&session, &block, HASH_1)
            .expect_err("missing replay credit must fail receipt creation");
        assert!(
            err.to_string().contains("rewardRecipient not credited"),
            "unexpected error: {err}"
        );
    }
}
