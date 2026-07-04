use crate::block_store::FileBlockStore;
use crate::bounty_catalog_store::load_bounties_from_path;
use crate::bounty_event_store::FileBountyEventLedger;
use crate::family_manifest_store::load_family_manifest_registry_from_dir;
use crate::http_error::HttpError;
use crate::nonce_ledger::FileNonceLedger;
use crate::proof_dedup_ledger::FileProofDedupLedger;
use crate::receipt_store::FileReceiptStore;
use crate::runtime::{RuntimeAdmissionState, RuntimeConfig};
use crate::session_store::FileSessionStore;
use crate::signed_nonce_ledger::FileSignedNonceLedger;
use crate::state_dir::{self, StateDirGuard, StateManifest};
use crate::work_manifest_store::load_work_manifests_from_path;
use axum::body::Bytes;
use axum::extract::DefaultBodyLimit;
use axum::extract::{ConnectInfo, Path as AxumPath, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::{from_fn, from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use boole_core::{
    agent_passport_events_for_receipt, canonical_payload_hash_hex, compute_block_reward_credits,
    replay_blocks, ticket, verify_signature_with_network, AdmissionDecision, BountyProofVerifier,
    BountyRegistry, BountyShare, BountySidePool, BuildSelectionResult, CalibrationReport,
    CreateBountyInput, DifficultyRetargetPolicy, FamilyManifestRegistry, Hex32, Hex64,
    PersistedBlock, ReceiptCommitment, ReceiptCommitmentInput, SessionState, SubmitProofInput,
    UpdateStatusInput, VerifyOutcome, WorkManifest, SIGNED_ENVELOPE_SCHEMA,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::convert::Infallible;
use std::future::Future;
use std::net::{IpAddr, SocketAddr, TcpListener as StdTcpListener};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::{Notify, RwLock};
use tower::limit::ConcurrencyLimitLayer;
use tower::Service;

/// P1.7 — default body cap for state-mutating and read routes (1 MiB),
/// stream-counted (not Content-Length-trusting). The bounty-proof route
/// carries Lean source + POFP envelope + signature and is raised to
/// [`PROOF_ROUTE_BODY_BYTES`].
pub const MAX_HTTP_BODY_BYTES: usize = 1_048_576;
/// P1.7 — `/bounties/{id}/proof` body cap (8 MiB): a real proof envelope
/// (Lean source + structural package + signature) can exceed the 1 MiB
/// default. Applied via a route-aware Content-Length check and a per-route
/// `DefaultBodyLimit` for the chunked path.
pub const PROOF_ROUTE_BODY_BYTES: usize = 8 * 1_048_576;
/// P1.7 — request timeout for every route except the bounty-proof route.
/// Replaces the former uniform 15 s. On expiry the request short-circuits
/// with a typed `request_timeout` (408) envelope.
pub const DEFAULT_ROUTE_TIMEOUT: Duration = Duration::from_secs(30);
/// P1.7 — request timeout for `/bounties/{id}/proof`, which runs the Lean
/// verifier (itself internally bounded). Larger than the default so a
/// legitimate verification is not cut off by the cheap-route limit.
pub const PROOF_ROUTE_TIMEOUT: Duration = Duration::from_secs(90);
/// P1.7 — workspace-wide cap on simultaneously in-flight HTTP requests.
/// `tower::limit::ConcurrencyLimitLayer` queues additional callers on a
/// semaphore so a flood of expensive routes (Lean verify, registry
/// scans) cannot exhaust file descriptors, threads, or `RwLock`
/// contention slots. Pinned at 256 by the production-readiness master
/// plan; raising it requires the same plan slice to be revised.
pub const MAX_CONCURRENT_REQUESTS: usize = 256;
const VERIFY_ANSWER_SCHEME: &str = "boole-native-test";
const VERIFY_ANSWER_AMOUNT: &str = "1";
// P1.8 — magic test-payment string accepted by `/verify-answer`. Hidden
// behind the `dev-mock-payment` feature so a release build with the
// feature off (`--no-default-features`) never compiles the constant
// into the binary; `enforce_verify_answer_payment` below uniformly
// returns `payment_invalid` instead of comparing against any
// allowlist.
#[cfg(feature = "dev-mock-payment")]
const VERIFY_ANSWER_PAYMENT_SIGNATURE: &str = "boole-native-test:paid";

/// P1.8 — split-bodies signature gate so the no-feature build does not
/// generate `unreachable_code` warnings inside the route handler. With
/// `dev-mock-payment` the helper compares against the magic string and
/// admits matches; without it every header is rejected with
/// `payment_invalid` and the constant is not referenced at all.
#[cfg(feature = "dev-mock-payment")]
fn enforce_verify_answer_payment(
    signature: Option<&str>,
    request_hash: String,
    pay_to: String,
    x402_version: String,
) -> Result<(), HttpError> {
    match signature {
        None => Err(HttpError::payment_required(
            VERIFY_ANSWER_SCHEME,
            VERIFY_ANSWER_AMOUNT,
            request_hash,
            pay_to,
            x402_version,
        )),
        Some(value) if value != VERIFY_ANSWER_PAYMENT_SIGNATURE => Err(HttpError::payment_invalid(
            VERIFY_ANSWER_SCHEME,
            x402_version,
        )),
        Some(_) => Ok(()),
    }
}

#[cfg(not(feature = "dev-mock-payment"))]
fn enforce_verify_answer_payment(
    signature: Option<&str>,
    request_hash: String,
    pay_to: String,
    x402_version: String,
) -> Result<(), HttpError> {
    match signature {
        None => Err(HttpError::payment_required(
            VERIFY_ANSWER_SCHEME,
            VERIFY_ANSWER_AMOUNT,
            request_hash,
            pay_to,
            x402_version,
        )),
        Some(_) => Err(HttpError::payment_invalid(
            VERIFY_ANSWER_SCHEME,
            x402_version,
        )),
    }
}
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
    /// P1.6b — Optional NDJSON ledger path for the per-signer signed
    /// envelope nonce dedup set covering the six non-session signed
    /// routes (`/sessions`, `/sessions/{pk}/revoke`, `/bounties`,
    /// `/bounties/{id}/status`, `/bounties/{id}/proof`, `/receipts`).
    /// When `Some`, each accepted envelope burns `(signerPk, nonce)`
    /// into the ledger; replays surface as `nonce_replayed` (HTTP 409).
    /// Sibling of `submit_nonce_ledger_path` (which keys on `sessionPk`
    /// for the session-bound `/submit` flow); the two stores live in
    /// separate files so a per-signer envelope replay cannot mask a
    /// session-bound replay or vice versa. When `None`, the freshness
    /// gate stops at `validBefore` and the routes accept previously-seen
    /// `(signerPk, nonce)` pairs — legacy embedding behavior.
    pub signed_nonce_ledger_path: Option<PathBuf>,
    /// N2.3 — Optional NDJSON ledger path for the proof-dedup set: the
    /// server-computed canonical proof hashes already credited on `/submit`.
    /// When `Some`, a second submit carrying the same proof bytes (under any
    /// prover pk) is rejected `duplicate_proof` before any block write, so one
    /// proof yields at most one credit (anti cross-pk farming). Recovery
    /// rehydrates the set so the rejection survives a restart. When `None`, no
    /// cross-pk proof dedup is enforced — legacy embedding behavior.
    pub proof_dedup_ledger_path: Option<PathBuf>,
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
    /// P2.6 b — Lean checker directory the operator selected at boot.
    /// `Some(path)` means the proof-verification path is wired up via
    /// `LeanBountyVerifier`; `None` means it was not configured at the
    /// CLI. `/ready` returns 503 when neither this nor
    /// `lean_checker_disabled` is set — the master plan refuses to
    /// silently accept a node that cannot verify proofs. N0-pre.7 —
    /// `/status` no longer echoes this absolute path (only the boolean
    /// `lean_checker_disabled`); path-level diagnostics are operator-tier.
    pub lean_checker_dir: Option<PathBuf>,
    /// P2.6 b — Explicit opt-out of the Lean checker requirement, set by
    /// the operator at boot via `--lean-checker-disabled` (testnet
    /// only). When `true`, `/ready` does not 503 on a missing
    /// `lean_checker_dir`; the operator is acknowledging that
    /// submissions arriving at this node will not be Lean-verified.
    /// When `false`, the boot-time choice must be `lean_checker_dir =
    /// Some(_)` instead.
    pub lean_checker_disabled: bool,
    /// P1.7 — per-source-IP HTTP rate limit applied to every route except
    /// `/live` and `/ready`. The limit is a fixed 60-second sliding
    /// window measured in HTTP requests per source IP, evaluated at
    /// middleware time before the handler observes the request. When
    /// `Some(n)`, requests beyond `n` within any 60s window are short-
    /// circuited with a typed `429 rate_limited` envelope. When `None`
    /// (the default for legacy embeddings and existing tests), the
    /// middleware is not installed and the routes behave as before.
    /// Readiness probes are intentionally excluded so an orchestrator
    /// flooding /ready or /live during incident response cannot self-
    /// blackhole the node.
    pub http_rate_limit_per_60s: Option<usize>,
    /// N2.1 — when `false` (the secure production default), a `/submit`
    /// envelope that carries no agent-wallet `session` block is rejected
    /// with `401 unauthenticated_submit` before admission: a bare prover pk
    /// cannot prove ownership of the reward it claims. When `true`, the
    /// legacy unauthenticated path is allowed (controlled local smoke,
    /// pre-wallet embeddings, existing tests). The production CLI defaults
    /// this to `false`; opt in with `--allow-anonymous-submit`.
    pub allow_anonymous_submit: bool,
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
    /// P2.6 — Unix epoch millis captured the moment the runtime hand-off
    /// completes (post-replay, pre-`axum::serve`). Surfaced through
    /// `/status` as `nodeStartedAt` so orchestrators and dashboards can
    /// compute `uptime = now - nodeStartedAt` without scraping process
    /// metrics. The value never mutates during the process lifetime.
    started_at_ms: u64,
    /// P2.6 e — runtime disk-full sentinel. `/ready` returns 503 with
    /// reason `disk_full_sentinel` when this is set, mirroring the master
    /// plan's "operator's disk fills up mid-mining → /ready 503" row.
    /// Defaults to `false`; the test seam
    /// `serve_local_node_with_disk_full_sentinel` injects it, and a future
    /// ENOSPC handler on the durable-append path is the production trigger.
    disk_full: Arc<AtomicBool>,
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
    /// P1.6b — on-disk path for the per-signer signed-envelope nonce
    /// ledger, mirroring `submit_nonce_ledger_path`. `Some` whenever the
    /// caller passed `LocalNodeConfig.signed_nonce_ledger_path = Some(_)`;
    /// kept here so the six signed-envelope handlers can burn
    /// `(signerPk, nonce)` events with the same {append-then-apply} flow.
    signed_nonce_ledger_path: Option<PathBuf>,
    /// P1.6b — in-memory mirror of the signed-envelope nonce ledger.
    /// `Some` iff `signed_nonce_ledger_path` is `Some`; absent when the
    /// operator has not opted in, in which case the six signed routes
    /// only enforce `validBefore` and accept previously-seen pairs.
    signed_nonce_ledger: Option<FileSignedNonceLedger>,
    /// N2.3 — on-disk path for the proof-dedup ledger, mirroring the nonce
    /// ledgers. `Some` iff `LocalNodeConfig.proof_dedup_ledger_path` is
    /// `Some`; the `/submit` admit guard records each credited proof's canon
    /// hash here and rejects a later submit carrying the same proof.
    proof_dedup_ledger_path: Option<PathBuf>,
    /// N2.3 — in-memory mirror of the proof-dedup ledger. `Some` iff
    /// `proof_dedup_ledger_path` is `Some`; absent when the operator has not
    /// opted in, in which case no cross-pk proof dedup is enforced.
    proof_dedup_ledger: Option<FileProofDedupLedger>,
    /// Optional append-only receipt ledger for accepted session-bound submit
    /// artifacts. The response receipt and ledger line intentionally match.
    submit_receipt_ledger_path: Option<PathBuf>,
    /// Optional append-only `ReceiptCommitment` ledger plus recovered index.
    receipt_commitment_ledger_path: Option<PathBuf>,
    receipt_store: Option<FileReceiptStore>,
    /// P2.6 b — Lean checker directory the operator passed at boot, or
    /// `None` if `--lean-checker-dir` was not supplied. Used by the
    /// `/ready` predicate. N0-pre.7 — no longer surfaced through `/status`
    /// (the absolute path is operator-tier, not anonymous-visible).
    lean_checker_dir: Option<PathBuf>,
    /// P2.6 b — Explicit operator opt-out of the Lean checker
    /// requirement (`--lean-checker-disabled`). When `true`, the
    /// readiness predicate accepts a missing `lean_checker_dir` as an
    /// acknowledged testnet configuration; when `false`, `/ready`
    /// returns 503 until the operator picks one of the two options.
    lean_checker_disabled: bool,
    /// P2.6 c — Directory path the operator passed via `--state-dir`,
    /// or `None` for the legacy single-store embedding. Production
    /// nodes opt into the state-dir layout to get an exclusive lock and
    /// a single root for every ledger; the `/ready` predicate treats a
    /// `Some(_)` value as a signal that the four agent-wallet ledger
    /// paths must also be set, so the node never silently loses
    /// session-bound submissions because one ledger was missing.
    state_dir: Option<PathBuf>,
    /// RAII guard for the L7 state-directory `flock`. `Some` whenever the
    /// caller passed a `state_dir` in `LocalNodeConfig`; held for the
    /// lifetime of the node so a second process at the same directory
    /// cannot race for the lock. Field is `_`-prefixed because it is
    /// never read directly — drop semantics are the entire contract.
    _state_dir_guard: Option<StateDirGuard>,
    /// P2.10 — network identifier this node is pinned to. Populated at
    /// boot from `LocalNodeConfig::network_id`, falling back to
    /// `DEFAULT_NETWORK_ID` when the operator did not set one. Every
    /// `boole.signed.v1` ingest route compares the outer envelope's
    /// optional `network_id` field against this value: a match (or an
    /// absent field, for backward compatibility) proceeds to ed25519
    /// verification; a mismatch returns `HttpError::cross_network_rejected`
    /// before any crypto runs so a cross-network replay attempt is
    /// rejected even if the signer's pk is on a session allow-list.
    network_id: String,
    /// N2.1 — mirrors `LocalNodeConfig.allow_anonymous_submit`. Read by
    /// `submit_handler` to reject session-less `/submit` envelopes with
    /// `401 unauthenticated_submit` unless the operator explicitly opted
    /// into the legacy unauthenticated path.
    allow_anonymous_submit: bool,
}

#[derive(Clone)]
struct AppState {
    inner: Arc<RwLock<LocalNodeState>>,
    /// P1.7 — per-source-IP HTTP rate limiter. `Some` whenever the
    /// caller passed `LocalNodeConfig.http_rate_limit_per_60s = Some(_)`;
    /// `None` keeps the middleware off so legacy embeddings and
    /// existing tests retain their pre-P1.7 wire behavior.
    rate_limiter: Option<Arc<HttpRateLimiter>>,
}

/// P1.7 — fixed-window per-IP HTTP rate limiter shared across the
/// router. The window is a sliding 60s bucket of monotonic timestamps;
/// admission is `count(ts in [now-window, now]) < quota`. The data
/// structure is small (one `VecDeque` per active source IP, each bounded
/// at `quota`), and the contention surface is a single `std::sync::Mutex`
/// — middleware critical sections are <10 µs even at high QPS, so a
/// tokio-aware lock is not warranted here.
struct HttpRateLimiter {
    quota: usize,
    window_ms: u128,
    state: StdMutex<HashMap<IpAddr, VecDeque<u128>>>,
}

impl HttpRateLimiter {
    fn new(quota: usize, window_ms: u128) -> Self {
        Self {
            quota,
            window_ms,
            state: StdMutex::new(HashMap::new()),
        }
    }

    fn admit(&self, ip: IpAddr, now_ms: u128) -> bool {
        let mut guard = self.state.lock().expect("rate-limit state mutex poisoned");
        let bucket = guard.entry(ip).or_default();
        let cutoff = now_ms.saturating_sub(self.window_ms);
        while bucket.front().is_some_and(|ts| *ts < cutoff) {
            bucket.pop_front();
        }
        if bucket.len() >= self.quota {
            return false;
        }
        bucket.push_back(now_ms);
        true
    }
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// N3-pre.3 (review #3) — wall-clock future-drift bound for the
/// self-produced block's `ts`. This is the ONLY place in the codebase that
/// checks a block's `ts` against real wall-clock time; the deterministic
/// median-time-past rule (`boole_core::MEDIAN_TIME_PAST_WINDOW`, enforced in
/// `replay_blocks`) is intentionally wall-clock-free so consensus replay
/// stays fully deterministic. `2h` mirrors the order of magnitude of
/// Bitcoin's own "block ts must not be more than 2 hours ahead of network
/// time" rule: generous enough to absorb real clock skew between operators,
/// tight enough that a self-reported `ts` cannot pre-stage a large forward
/// drift for a later median-time-past window. N3.3's p2p ingress is
/// expected to reuse this same boundary guard for peer-submitted blocks.
const BLOCK_TS_MAX_FUTURE_DRIFT_MS: u64 = 2 * 60 * 60 * 1000;

/// Rejects a block `ts` that lies more than `BLOCK_TS_MAX_FUTURE_DRIFT_MS`
/// ahead of `now_ms`. `now_ms` is threaded in explicitly (rather than read
/// internally via `now_unix_ms`) so this stays a pure, directly
/// unit-testable function; the real self-produce call site passes
/// `now_unix_ms()`.
fn check_block_ts_future_drift(ts_ms: u64, now_ms: u64) -> anyhow::Result<()> {
    let max_allowed_ms = now_ms.saturating_add(BLOCK_TS_MAX_FUTURE_DRIFT_MS);
    if ts_ms > max_allowed_ms {
        anyhow::bail!(
            "block ts {} exceeds the future-drift bound: now={} maxAllowedMs={} (driftBoundMs={})",
            ts_ms,
            now_ms,
            max_allowed_ms,
            BLOCK_TS_MAX_FUTURE_DRIFT_MS
        );
    }
    Ok(())
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// P1.6a — clock-skew leeway in seconds applied to `validBefore` so a
/// modest client/server skew does not bounce a legitimately-fresh
/// envelope. The window is short enough that a leaked signed envelope
/// has a bounded replay surface but long enough to absorb NTP jitter
/// and queue/transport delay.
const VALID_BEFORE_LEEWAY_SECS: u64 = 60;

/// D#3 — upper bound on how far into the future `validBefore` may point.
/// Matches the largest window the system's own producers stamp
/// (`SIGNED_PAYLOAD_VALID_BEFORE_WINDOW_SECS` /
/// `BOUNTY_PROOF_VALID_BEFORE_WINDOW_SECS`, both 300s); without this cap a
/// captured envelope stamped years ahead stays replayable until it
/// "expires", defeating the freshness gate. The skew leeway is added on
/// top so a producer clock modestly ahead of the server is not bounced.
const VALID_BEFORE_MAX_TTL_SECS: u64 = 300;

/// P1.6a — every signed inner payload must carry `validBefore`
/// (u64 Unix seconds). Returns `bad_payload` for missing/non-u64 values
/// so wallets see the same vocabulary as the rest of the inner-payload
/// gates, `envelope_expired` once the leeway window has elapsed, and
/// `bad_payload` again when `validBefore` exceeds the future cap (D#3).
fn check_payload_valid_before(payload: &serde_json::Map<String, Value>) -> Result<(), HttpError> {
    let valid_before = payload
        .get("validBefore")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            HttpError::bad_payload(
                "validBefore",
                "payload must include u64 unix-seconds validBefore",
            )
        })?;
    let now = now_unix_secs();
    if now > valid_before.saturating_add(VALID_BEFORE_LEEWAY_SECS) {
        return Err(HttpError::envelope_expired(valid_before, now));
    }
    let max_valid_before = now
        .saturating_add(VALID_BEFORE_MAX_TTL_SECS)
        .saturating_add(VALID_BEFORE_LEEWAY_SECS);
    if valid_before > max_valid_before {
        return Err(HttpError::bad_payload(
            "validBefore",
            "payload validBefore exceeds the maximum future window",
        ));
    }
    Ok(())
}

/// P1.6b — every signed inner payload on the six non-session routes
/// must carry a non-empty string `nonce`. Nonces are opaque to the
/// server; uniqueness is enforced against the per-signer ledger, not by
/// parsing the bytes. Missing or non-string → 400 `bad_payload` with
/// `field: "nonce"` so wallets see the same vocabulary as the other
/// inner-payload gates.
fn check_payload_nonce(payload: &serde_json::Map<String, Value>) -> Result<&str, HttpError> {
    let nonce = payload
        .get("nonce")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_payload("nonce", "payload must include string nonce"))?;
    if nonce.is_empty() {
        return Err(HttpError::bad_payload(
            "nonce",
            "payload nonce must be a non-empty string",
        ));
    }
    Ok(nonce)
}

/// P1.6b — soft per-signer dedup probe. When the operator opted into the
/// signed-envelope nonce ledger, reject any `(signer_pk, nonce)` already
/// burned with 409 `nonce_replayed` before the handler does any
/// state-mutating work. The atomic check-and-burn happens via
/// `burn_signed_envelope_nonce` once the handler is about to persist; this
/// probe is purely a fast-path so replays never reach the verifier or
/// ledger-mutation paths.
fn check_signed_envelope_nonce_not_replayed(
    state: &LocalNodeState,
    signer_pk: &str,
    nonce: &str,
) -> Result<(), HttpError> {
    let Some(ledger) = state.signed_nonce_ledger.as_ref() else {
        return Ok(());
    };
    if ledger.contains(signer_pk, nonce) {
        return Err(HttpError::signed_envelope_nonce_replayed(
            signer_pk.to_string(),
            nonce.to_string(),
        ));
    }
    Ok(())
}

/// P2.10 — parse the outer `boole.signed.v1` envelope's optional
/// `network_id` field and cross-check it against the node's pinned
/// `network_id`. Backward-compatible by design:
///
///   - `Ok(None)` when the wire envelope has no `network_id` field.
///     Pre-P2.10 clients keep working: callers pass `None` to
///     `verify_signature_with_network`, which recomputes the legacy
///     non-network-bound digest.
///   - `Ok(Some(nid))` when the wire `network_id` matches the node's
///     pinned id. Callers pass `Some(nid)` so the verifier folds the
///     same domain-separation tag the signer used.
///   - `Err(cross_network_rejected)` when the wire field is present
///     but does not match. 403, pre-crypto, so a cross-network replay
///     attempt is rejected even if the signer's pk is on a session
///     allow-list and the underlying digest would otherwise verify.
fn parse_envelope_network_id<'a>(
    envelope_obj: &'a serde_json::Map<String, Value>,
    node_network_id: &str,
) -> Result<Option<&'a str>, HttpError> {
    let Some(field) = envelope_obj.get("network_id") else {
        return Ok(None);
    };
    let Some(nid) = field.as_str() else {
        return Err(HttpError::bad_envelope(
            "envelope network_id must be a string",
        ));
    };
    if nid != node_network_id {
        return Err(HttpError::cross_network_rejected(
            node_network_id.to_string(),
            nid.to_string(),
        ));
    }
    Ok(Some(nid))
}

/// P1.6b — atomic burn of the `(signer_pk, nonce)` pair into the
/// per-signer signed-envelope nonce ledger. Returns 409 `nonce_replayed`
/// if the pair was already burned (covers the case where two concurrent
/// handlers raced past the soft probe). When the ledger is not
/// configured, this is a no-op so legacy embeddings retain their pre-
/// P1.6b semantics.
fn burn_signed_envelope_nonce(
    state: &mut LocalNodeState,
    signer_pk: &str,
    nonce: &str,
) -> Result<(), HttpError> {
    let Some(path) = state.signed_nonce_ledger_path.clone() else {
        return Ok(());
    };
    let ledger = state
        .signed_nonce_ledger
        .as_mut()
        .ok_or_else(|| HttpError::internal("signed_nonce_ledger unavailable"))?;
    let appended = ledger
        .append_burn(&path, signer_pk, nonce)
        .map_err(|err| HttpError::internal(err.to_string()))?;
    if !appended {
        return Err(HttpError::signed_envelope_nonce_replayed(
            signer_pk.to_string(),
            nonce.to_string(),
        ));
    }
    Ok(())
}

async fn rate_limit_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let Some(limiter) = state.rate_limiter.as_ref() else {
        return next.run(request).await;
    };
    let path = request.uri().path();
    if path == "/live" || path == "/ready" {
        return next.run(request).await;
    }
    let Some(ConnectInfo(addr)) = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .copied()
    else {
        return next.run(request).await;
    };
    if limiter.admit(addr.ip(), now_unix_ms()) {
        next.run(request).await
    } else {
        error_response(HttpError::rate_limited(limiter.quota, limiter.window_ms))
    }
}

/// P2.6 e — test seam: serve with an injected disk-full sentinel so the
/// `/ready` fault-injection matrix can assert the 503 + `disk_full_sentinel`
/// reason without an actual full filesystem. Production code never calls
/// this; the live trigger will be an ENOSPC handler on the durable-append
/// path storing into the same `Arc<AtomicBool>`.
#[doc(hidden)]
pub fn serve_local_node_with_disk_full_sentinel(
    listener: StdTcpListener,
    config: LocalNodeConfig,
    disk_full: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let max_requests = config.max_requests;
    let rate_limiter = build_rate_limiter(config.http_rate_limit_per_60s);
    let mut state = LocalNodeState::from_config(config)?;
    state.disk_full = disk_full;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve_local_node_async(
        listener,
        state,
        max_requests,
        rate_limiter,
        None,
    ))
}

pub fn serve_local_node(listener: StdTcpListener, config: LocalNodeConfig) -> anyhow::Result<()> {
    let max_requests = config.max_requests;
    let rate_limiter = build_rate_limiter(config.http_rate_limit_per_60s);
    let state = LocalNodeState::from_config(config)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve_local_node_async(
        listener,
        state,
        max_requests,
        rate_limiter,
        None,
    ))
}

fn build_rate_limiter(quota: Option<usize>) -> Option<Arc<HttpRateLimiter>> {
    quota
        .filter(|n| *n > 0)
        .map(|n| Arc::new(HttpRateLimiter::new(n, 60_000)))
}

/// P2.7 — Same as [`serve_local_node`] but with an externally-owned
/// shutdown trigger. Calling `external_shutdown.notify_one()` unblocks
/// `axum::serve`'s graceful-shutdown future, lets in-flight requests
/// drain, and returns `Ok(())`. Used by orchestrators that already own
/// a process-supervision channel and by tests that need deterministic
/// shutdown without raising real signals.
pub fn serve_local_node_with_shutdown(
    listener: StdTcpListener,
    config: LocalNodeConfig,
    external_shutdown: Arc<Notify>,
) -> anyhow::Result<()> {
    let max_requests = config.max_requests;
    let rate_limiter = build_rate_limiter(config.http_rate_limit_per_60s);
    let state = LocalNodeState::from_config(config)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve_local_node_async(
        listener,
        state,
        max_requests,
        rate_limiter,
        Some(external_shutdown),
    ))
}

/// P2.7 — production entry point: serve until a SIGTERM or SIGINT arrives,
/// then drain gracefully. On the trigger, axum's `with_graceful_shutdown`
/// stops accepting new connections and finishes in-flight requests; every
/// NDJSON ledger is already fsynced per-append; the Lean child is reaped via
/// `ChildKillOnDrop` when an interrupted proof future drops; and the
/// state-dir flock is released when `LocalNodeState` drops on return.
///
/// The `BountySidePool` is deliberately NOT snapshotted to a side file: it
/// is a pure projection of the durable bounty-event ledger and is rebuilt on
/// the next boot (P1.5b `rebuild_bounty_side_pool`), so there is one source
/// of truth that a separate snapshot could only diverge from.
pub fn serve_local_node_with_os_signals(
    listener: StdTcpListener,
    config: LocalNodeConfig,
) -> anyhow::Result<()> {
    let max_requests = config.max_requests;
    let rate_limiter = build_rate_limiter(config.http_rate_limit_per_60s);
    let state = LocalNodeState::from_config(config)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let shutdown = Arc::new(Notify::new());

    // P2.7 — register the OS signal handlers SYNCHRONOUSLY, in the runtime
    // context, BEFORE `block_on` yields to any worker thread. Two reasons:
    // (1) a SIGTERM/SIGINT that arrives the instant the server starts serving
    // cannot slip through to the kernel's default (terminate) action via a
    // registration race — by the time `/ready`/`/live` answer, the sigaction
    // is already installed; (2) a registration failure is propagated as a
    // boot error instead of being silently swallowed, so the bounded-drain
    // guarantee is never void without the operator knowing. The `Signal`
    // streams stay valid for the lifetime of the runtime that owns them.
    #[cfg(unix)]
    let signals = {
        use tokio::signal::unix::{signal, SignalKind};
        let _enter = runtime.enter();
        let term = signal(SignalKind::terminate())
            .map_err(|e| anyhow::anyhow!("failed to register SIGTERM handler: {e}"))?;
        let int = signal(SignalKind::interrupt())
            .map_err(|e| anyhow::anyhow!("failed to register SIGINT handler: {e}"))?;
        (term, int)
    };

    runtime.block_on(async move {
        #[cfg(unix)]
        {
            let (mut term, mut int) = signals;
            let term_notify = shutdown.clone();
            tokio::spawn(async move {
                term.recv().await;
                term_notify.notify_one();
            });
            let int_notify = shutdown.clone();
            tokio::spawn(async move {
                int.recv().await;
                int_notify.notify_one();
            });
        }
        #[cfg(not(unix))]
        {
            // Non-unix: Ctrl-C is the only portable stop signal.
            let ctrl_c_notify = shutdown.clone();
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    ctrl_c_notify.notify_one();
                }
            });
        }
        serve_local_node_async(listener, state, max_requests, rate_limiter, Some(shutdown)).await
    })
}

async fn serve_local_node_async(
    listener: StdTcpListener,
    state: LocalNodeState,
    max_requests: Option<usize>,
    rate_limiter: Option<Arc<HttpRateLimiter>>,
    external_shutdown: Option<Arc<Notify>>,
) -> anyhow::Result<()> {
    listener.set_nonblocking(true)?;
    let tokio_listener = TcpListener::from_std(listener)?;
    let app_state = AppState {
        inner: Arc::new(RwLock::new(state)),
        rate_limiter,
    };
    let shutdown_notify = Arc::new(Notify::new());
    // P2.7 — forward external trigger fires to the internal shutdown_notify
    // so the `max_requests` path and the external path use the same wake
    // signal. Spawn the forwarder only when the caller supplied a trigger;
    // otherwise the task is pure overhead.
    if let Some(external) = external_shutdown {
        let internal = shutdown_notify.clone();
        tokio::spawn(async move {
            external.notified().await;
            internal.notify_one();
        });
    }
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
        .route("/live", get(live_handler))
        .route("/ready", get(ready_handler))
        .route("/metrics", get(metrics_handler))
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
        .route(
            "/bounties/{id}/proof",
            // P1.7 — the proof route accepts a larger body (Lean source +
            // POFP envelope + signature); raise its streaming cap above the
            // 1 MiB default. The route-layer is inner-most, so this
            // `DefaultBodyLimit` overrides the global one for this route.
            post(bounty_proof_handler).layer(DefaultBodyLimit::max(PROOF_ROUTE_BODY_BYTES)),
        )
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
        // P1.7 — route-aware request timeout (default 30 s, bounty-proof
        // 90 s). Replaces the uniform `tower_http::TimeoutLayer` so a
        // timed-out request carries the typed `request_timeout` (408)
        // envelope instead of a bare empty body.
        .layer(from_fn(route_timeout_middleware))
        .layer(ConcurrencyLimitLayer::new(MAX_CONCURRENT_REQUESTS))
        // P1.7 — stream-counting body cap. `body_cap_middleware` below
        // catches honest Content-Length requests and returns the same
        // typed JSON envelope as other 4xx responses. `DefaultBodyLimit`
        // catches the chunked-transfer path, where there is no header
        // to inspect at middleware time; the extractor enforces the cap
        // as the body streams in and short-circuits with HTTP 413
        // before the handler observes the truncated bytes.
        .layer(DefaultBodyLimit::max(MAX_HTTP_BODY_BYTES))
        .layer(from_fn(body_cap_middleware))
        .layer(from_fn(connection_close_middleware))
        // P0.5 slice 66 — stamp x-request-id on every response and enter a
        // tracing span carrying it. Installed here so it wraps all handlers
        // and their responses uniformly.
        .layer(from_fn(request_id_middleware))
        // P1.7 — per-source-IP HTTP rate limiter. Installed last so the
        // typed `429 rate_limited` envelope short-circuits before
        // body-cap, timeout, and concurrency-limit layers wake the
        // handler; readiness probes (`/live`, `/ready`) are exempted
        // inside `rate_limit_middleware` itself so an orchestrator
        // flood cannot self-blackhole the node.
        .layer(from_fn_with_state(state.clone(), rate_limit_middleware))
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
        // P1.5b — `bounty_side_pool` is in-memory only. We rebuild it
        // here from the durable bounty audit log: each accepted `proof`
        // event maps to a side-pool insert; each `share_promoted` event
        // marks a (familyId, bountyId, proofHash) triple that has
        // already been committed into a block and must NOT be reinserted.
        // Without this, a node restart silently drops every accepted
        // bounty share that had not yet been promoted, deleting pending
        // credit the verifier already accepted.
        let mut bounty_side_pool = BountySidePool::new();
        if let Some(path) = config.bounty_event_ledger_path.as_ref() {
            let events = FileBountyEventLedger::recover(path)?;
            for event in &events {
                replay_bounty_audit_event(&mut bounty_registry, event)
                    .map_err(|err| anyhow::anyhow!("replay bounty audit log: {err}"))?;
            }
            rebuild_bounty_side_pool(&mut bounty_side_pool, &bounty_registry, &events)
                .map_err(|err| anyhow::anyhow!("rebuild bounty side-pool: {err}"))?;
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
        let signed_nonce_ledger = match config.signed_nonce_ledger_path.as_ref() {
            Some(path) => Some(FileSignedNonceLedger::recover(path)?),
            None => None,
        };
        let proof_dedup_ledger = match config.proof_dedup_ledger_path.as_ref() {
            Some(path) => Some(FileProofDedupLedger::recover(path)?),
            None => None,
        };
        let receipt_store = match config.receipt_commitment_ledger_path.as_ref() {
            Some(path) => Some(FileReceiptStore::recover(path)?),
            None => None,
        };
        Ok(Self {
            runtime,
            disk_full: Arc::new(AtomicBool::new(false)),
            genesis_c: scenario.genesis_c,
            block_path: config.block_path,
            report: scenario.cfg,
            started_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            work_manifests,
            bounty_registry,
            bounty_event_ledger_path: config.bounty_event_ledger_path,
            bounty_verifiers: config.bounty_verifiers.unwrap_or_default(),
            family_manifest_registry,
            bounty_side_pool,
            operator_signer_pks: config.operator_signer_pks,
            session_registry_path: config.session_registry_path,
            session_store,
            submit_nonce_ledger_path: config.submit_nonce_ledger_path,
            nonce_ledger,
            signed_nonce_ledger_path: config.signed_nonce_ledger_path,
            signed_nonce_ledger,
            proof_dedup_ledger_path: config.proof_dedup_ledger_path,
            proof_dedup_ledger,
            submit_receipt_ledger_path: config.submit_receipt_ledger_path,
            receipt_commitment_ledger_path: config.receipt_commitment_ledger_path,
            receipt_store,
            lean_checker_dir: config.lean_checker_dir,
            lean_checker_disabled: config.lean_checker_disabled,
            network_id: config
                .network_id
                .clone()
                .unwrap_or_else(|| DEFAULT_NETWORK_ID.to_string()),
            state_dir: config.state_dir,
            _state_dir_guard: state_dir_guard,
            allow_anonymous_submit: config.allow_anonymous_submit,
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

/// P1.5b — rebuild `BountySidePool` from the durable bounty audit log.
///
/// Walks the recovered events twice:
///   1. Collect the set of `(familyId, bountyId, proofHash)` triples
///      that appear in `share_promoted` events. Those shares have
///      already been folded into a committed block and must NOT
///      reappear in the live side-pool.
///   2. For each `proof` event with `accepted=true` that is not in
///      the promoted set, look up the bounty in the (now-replayed)
///      registry to recover `family_id` (= `bounty.domain`) and the
///      reward stamp, and re-insert the share.
///
/// Bounties missing from the registry (e.g., dropped from the static
/// catalog after the audit log was written) emit a stderr warning and
/// are skipped — the in-memory `BountyShare` cannot be reconstructed
/// without the registry record, and there is no consensus consequence
/// because the share also cannot be promoted into a future block.
fn rebuild_bounty_side_pool(
    pool: &mut BountySidePool,
    registry: &BountyRegistry,
    events: &[Value],
) -> Result<(), String> {
    use std::collections::HashSet;

    let mut promoted: HashSet<(String, String, String)> = HashSet::new();
    for event in events {
        if event.get("kind").and_then(Value::as_str) != Some("share_promoted") {
            continue;
        }
        let family_id = event
            .get("familyId")
            .and_then(Value::as_str)
            .ok_or_else(|| "share_promoted event missing familyId".to_string())?
            .to_string();
        let bounty_id = event
            .get("bountyId")
            .and_then(Value::as_str)
            .ok_or_else(|| "share_promoted event missing bountyId".to_string())?
            .to_string();
        let proof_hash = event
            .get("proofHash")
            .and_then(Value::as_str)
            .ok_or_else(|| "share_promoted event missing proofHash".to_string())?
            .to_string();
        promoted.insert((family_id, bounty_id, proof_hash));
    }

    for event in events {
        if event.get("kind").and_then(Value::as_str) != Some("proof") {
            continue;
        }
        if !event
            .get("accepted")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let bounty_id = event
            .get("workId")
            .and_then(Value::as_str)
            .ok_or_else(|| "proof event missing workId".to_string())?
            .to_string();
        let proof_hash = event
            .get("proofHash")
            .and_then(Value::as_str)
            .ok_or_else(|| "proof event missing proofHash".to_string())?
            .to_string();
        let prover = event
            .get("solverPk")
            .and_then(Value::as_str)
            .ok_or_else(|| "proof event missing solverPk".to_string())?
            .to_string();
        let ts = event
            .get("ts")
            .and_then(Value::as_u64)
            .ok_or_else(|| "proof event missing ts".to_string())?;

        let Some(bounty) = registry.get(&bounty_id) else {
            eprintln!(
                "[boole-node] audit log proof for unknown bounty {bounty_id} skipped during \
                 side-pool rebuild"
            );
            continue;
        };
        let key = (bounty.domain.clone(), bounty_id.clone(), proof_hash.clone());
        if promoted.contains(&key) {
            continue;
        }
        let reward: u128 = bounty.reward.parse().unwrap_or(0);
        pool.insert(BountyShare {
            bounty_id,
            proof_hash,
            prover,
            family_id: bounty.domain,
            ts,
            reward,
        });
    }
    Ok(())
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

/// P0.5 slice 66 — monotonic per-request id counter. Combined with the
/// boot-time millisecond stamp it yields an id that is unique within a
/// process (and overwhelmingly unique across restarts) without taking a
/// `uuid`/`rand` dependency. Hex-encoded `<boot_ms>-<seq>`.
static REQUEST_ID_SEQ: AtomicUsize = AtomicUsize::new(0);

/// Generate the next request id. `boot_ms` disambiguates restarts; the
/// atomic sequence disambiguates concurrent requests within one process.
fn next_request_id(boot_ms: u128) -> String {
    let seq = REQUEST_ID_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{boot_ms:x}-{seq:x}")
}

/// P0.5 slice 66 — stamp every response with a unique `x-request-id`
/// header and enter a tracing span carrying it, so a log line, a tracing
/// span, and an operator's `curl -i` all share one correlation id. The
/// header is the minimal propagation surface; echoing the id into every
/// response envelope and ledger line is a larger per-handler change
/// deferred to a follow-up slice. No consensus state is touched.
async fn request_id_middleware(request: Request, next: Next) -> Response {
    let request_id = next_request_id(now_unix_ms());
    let span = tracing::info_span!("http_request", request_id = %request_id);
    let _enter = span.enter();
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

/// P1.7 — true for the bounty-proof route (`/bounties/{id}/proof`), which
/// carries a larger body and runs the Lean verifier. Both the body cap and
/// the request timeout key off this single predicate so one source decides
/// the route class (no fragile per-route layer-composition ordering).
///
/// Matches ONLY the canonical three-segment form: a single non-empty `id`
/// segment with no extra depth. A loose `starts_with`/`ends_with` would also
/// match `/bounties/proof` (the `/bounties/{id}` GET with `id="proof"`) and
/// `/bounties/x/y/proof` (no registered route), handing them the wrong
/// 90 s timeout + 8 MiB body-cap class.
fn is_proof_route(path: &str) -> bool {
    if let Some(rest) = path.strip_prefix("/bounties/") {
        if let Some(id) = rest.strip_suffix("/proof") {
            return !id.is_empty() && !id.contains('/');
        }
    }
    false
}

/// P1.7 — per-route request timeout. The proof route gets
/// [`PROOF_ROUTE_TIMEOUT`] (Lean verify is heavier); everything else gets
/// [`DEFAULT_ROUTE_TIMEOUT`]. On expiry the request short-circuits with a
/// typed `request_timeout` (408) envelope instead of the bare empty body a
/// `tower_http::TimeoutLayer` would emit.
async fn route_timeout_middleware(request: Request, next: Next) -> Response {
    let timeout = if is_proof_route(request.uri().path()) {
        PROOF_ROUTE_TIMEOUT
    } else {
        DEFAULT_ROUTE_TIMEOUT
    };
    match tokio::time::timeout(timeout, next.run(request)).await {
        Ok(response) => response,
        Err(_) => error_response(HttpError::request_timeout()),
    }
}

async fn body_cap_middleware(headers: HeaderMap, request: Request, next: Next) -> Response {
    let cap = if is_proof_route(request.uri().path()) {
        PROOF_ROUTE_BODY_BYTES
    } else {
        MAX_HTTP_BODY_BYTES
    };
    if let Some(value) = headers.get(axum::http::header::CONTENT_LENGTH) {
        if let Some(len) = value.to_str().ok().and_then(|s| s.parse::<usize>().ok()) {
            if len > cap {
                return error_response(HttpError::body_too_large(cap, len));
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

/// P2.6 — `/live` is the orchestrator liveness probe. It must never
/// acquire the runtime `RwLock` or touch IO so a stuck verify path
/// cannot mask a healthy process as dead. A static `200 OK` envelope
/// is the right answer: if the route returns at all the process is
/// reachable and its async runtime is scheduling work.
async fn live_handler() -> Response {
    (StatusCode::OK, Json(json!({ "ok": true, "probe": "live" }))).into_response()
}

/// P2.6 — `/ready` is the orchestrator readiness probe. It must
/// evaluate the real readiness preconditions on every request and
/// surface a structured `503 Service Unavailable` envelope when any
/// precondition fails. A static `200 OK` would let orchestrators
/// route traffic to a divergent node — the 2026-05-18 design review
/// (concern #4) flagged exactly that gap (endpoint existence ≠
/// readiness correctness).
///
/// The body always carries a `checks` object naming each precondition
/// individually, so future slices can add more preconditions
/// (state-dir lock held, Lean checker configured-or-explicitly-disabled,
/// ledgers loaded) without breaking the response shape. The top-level
/// `reason` field names the first failing precondition so operators
/// can diagnose without scraping logs.
async fn ready_handler(State(state): State<AppState>) -> Response {
    let guard = state.inner.read().await;
    let replay_matches_runtime = compute_replay_matches_runtime(&guard);
    let state_dir_lock_held = compute_state_dir_lock_held(&guard);
    // P2.6 audit: "set" alone is not enough — a typoed --lean-checker-dir
    // would leave the path pointing nowhere and every proof would
    // silently fail verification. Require either an explicit disable or
    // a path that actually resolves to a directory on disk at probe
    // time. The check is per-probe (not boot-time only) so that an
    // operator who manually removes the dir mid-run also flips /ready
    // to 503 immediately.
    let lean_checker_configured = if guard.lean_checker_disabled {
        true
    } else {
        guard
            .lean_checker_dir
            .as_ref()
            .map(|p| p.is_dir())
            .unwrap_or(false)
    };
    let ledgers_loaded = compute_ledgers_loaded(&guard);
    // P2.6 e — disk-full sentinel. A positive boolean keeps the wire shape
    // consistent with the other `checks` keys (all are "ok"-form).
    let disk_space_ok = !guard.disk_full.load(Ordering::Acquire);
    drop(guard);

    let checks = json!({
        "replay_matches_runtime": replay_matches_runtime,
        "state_dir_lock_held": state_dir_lock_held,
        "lean_checker_configured": lean_checker_configured,
        "ledgers_loaded": ledgers_loaded,
        "disk_space_ok": disk_space_ok,
    });

    // First failing precondition names the reason. Boot-time invariants
    // (replay) take precedence over operator-config invariants (lean,
    // ledgers) so a divergent on-disk state is surfaced before the
    // operator sees a "fix your CLI" message. Live runtime drift on
    // the state-dir lock (the lock file disappearing while we still
    // hold the FD) is checked second — a second boole-node could now
    // attach to the same state, so traffic must stop before the
    // operator-config issues. Within operator-config invariants the
    // loud failure (missing lean checker -> proofs rejected) is named
    // before the quiet one (missing ledger -> rows silently dropped).
    // The `checks` object always reports every precondition's
    // individual status so operators get the full picture, not just
    // the first failure.
    if !replay_matches_runtime {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ok": false,
                "probe": "ready",
                "reason": "replay_runtime_mismatch",
                "checks": checks,
            })),
        )
            .into_response();
    }
    if !state_dir_lock_held {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ok": false,
                "probe": "ready",
                "reason": "state_dir_lock_lost",
                "checks": checks,
            })),
        )
            .into_response();
    }
    if !lean_checker_configured {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ok": false,
                "probe": "ready",
                "reason": "lean_checker_not_configured",
                "checks": checks,
            })),
        )
            .into_response();
    }
    if !ledgers_loaded {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ok": false,
                "probe": "ready",
                "reason": "ledgers_not_loaded",
                "checks": checks,
            })),
        )
            .into_response();
    }
    // P2.6 e — disk-full is checked last: it is the least actionable for an
    // operator (free disk, then it clears on its own) so a more specific
    // precondition failure is surfaced first when several coincide.
    if !disk_space_ok {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ok": false,
                "probe": "ready",
                "reason": "disk_full_sentinel",
                "checks": checks,
            })),
        )
            .into_response();
    }
    (
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "probe": "ready",
            "checks": checks,
        })),
    )
        .into_response()
}

/// P2.6 d — `state_dir_lock_held` predicate for `/ready`.
///
/// Legacy embedding (`state_dir: None`) trivially returns `true` —
/// the operator never asked for an advisory lock.
///
/// Production embedding (`state_dir: Some(dir)`) verifies that the
/// advisory `<dir>/state.lock` file still exists at the expected
/// path. We hold the file's flock through an open FD inside
/// `_state_dir_guard`, so the kernel will not release our exclusive
/// lock if the directory entry is removed, but a peer `boole-node`
/// could then create a fresh `state.lock` at the same path and
/// acquire its own exclusive lock — breaking the contract. The
/// existence check catches that drift without re-issuing flock(),
/// which would race with our own held lock on the underlying inode.
fn compute_state_dir_lock_held(state: &LocalNodeState) -> bool {
    match state.state_dir.as_ref() {
        None => true,
        Some(dir) => dir.join(state_dir::STATE_LOCK_FILE).is_file(),
    }
}

/// P2.6 c — `ledgers_loaded` predicate for `/ready`.
///
/// Legacy embedding (`state_dir: None`) is unaffected: the agent-wallet
/// ledgers remain individually opt-in, so the predicate returns `true`
/// regardless of which of them are configured.
///
/// Production embedding (`state_dir: Some(_)`) requires every audit-
/// critical agent-wallet ledger — `session_registry`,
/// `submit_nonce_ledger`, `signed_nonce_ledger`,
/// `submit_receipt_ledger`, `receipt_commitment_ledger` — to be loaded.
/// `signed_nonce_ledger` (P1.6b) tracks per-signer nonces for non-
/// session signed envelopes; a production node without it cannot
/// detect replay of direct-signed (wallet-agent) submissions across
/// restarts, so it joins the other four as a hard precondition.
///
/// The runtime holds an in-memory handle for each ledger when its path
/// is configured (`session_store`, `nonce_ledger`,
/// `signed_nonce_ledger`, `submit_receipt_ledger_path`,
/// `receipt_store`), so this predicate reads those handle fields rather
/// than reaching back into the `LocalNodeConfig` — a future post-boot
/// tear-down that nulls a handle flips this to `false` without further
/// plumbing.
fn compute_ledgers_loaded(state: &LocalNodeState) -> bool {
    if state.state_dir.is_none() {
        return true;
    }
    state.session_store.is_some()
        && state.nonce_ledger.is_some()
        && state.signed_nonce_ledger.is_some()
        && state.submit_receipt_ledger_path.is_some()
        && state.receipt_store.is_some()
}

/// P0.5 slice 67 — process-wide outcome counters surfaced on `/metrics`
/// as Prometheus counters. Global atomics (not `AppState` fields) so the
/// outcome sites can bump them without threading a handle through every
/// call path; a node runs one HTTP server per process, so process-global
/// is the correct cardinality. `boole_panic_total` is wired here and
/// incremented by the panic hook (slice 68); it reads 0 until a panic
/// fires.
static SUBMITS_ACCEPTED: AtomicUsize = AtomicUsize::new(0);
static SUBMITS_REJECTED: AtomicUsize = AtomicUsize::new(0);
static PROOFS_ACCEPTED: AtomicUsize = AtomicUsize::new(0);
static PROOFS_REJECTED: AtomicUsize = AtomicUsize::new(0);
// `boole_panic_total` is owned by `boole_core::telemetry` (P0.5 slice 68)
// so every binary's panic hook bumps one shared counter; the renderer
// reads it via `telemetry::panic_total()`.

/// Outcome label for the submit/proof counters. `accepted` = the handler
/// returned a 2xx envelope; `rejected` = any typed-error / verifier path.
fn record_submit_outcome(accepted: bool) {
    if accepted {
        SUBMITS_ACCEPTED.fetch_add(1, Ordering::Relaxed);
    } else {
        SUBMITS_REJECTED.fetch_add(1, Ordering::Relaxed);
    }
}

fn record_proof_outcome(accepted: bool) {
    if accepted {
        PROOFS_ACCEPTED.fetch_add(1, Ordering::Relaxed);
    } else {
        PROOFS_REJECTED.fetch_add(1, Ordering::Relaxed);
    }
}

/// P2.6 — `/metrics` exposes a Prometheus text-format scrape surface
/// (exposition v0.0.4). The body lists each gauge with a `# HELP` /
/// `# TYPE` header followed by `<name> <value>` samples. P0.5 slice 67
/// adds mutating counters (`boole_submits_total`, `boole_proofs_total`,
/// `boole_panic_total`) alongside the boot-time state gauges.
async fn metrics_handler(State(state): State<AppState>) -> Response {
    let guard = state.inner.read().await;
    let body = render_prometheus_metrics(&guard);
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

fn render_prometheus_metrics(state: &LocalNodeState) -> String {
    let height = state.runtime.cached_block_count();
    let share_pool = state.runtime.pool_size();
    let bounty_side_pool = state.bounty_side_pool.total_share_count();
    let started_at = state.started_at_ms;
    let mut out = String::new();
    out.push_str("# HELP boole_node_height Current block chain height.\n");
    out.push_str("# TYPE boole_node_height gauge\n");
    out.push_str(&format!("boole_node_height {height}\n"));
    out.push_str("# HELP boole_node_share_pool_size Current admission share-pool size.\n");
    out.push_str("# TYPE boole_node_share_pool_size gauge\n");
    out.push_str(&format!("boole_node_share_pool_size {share_pool}\n"));
    out.push_str(
        "# HELP boole_node_bounty_side_pool_total Unpromoted bounty shares in the side-pool.\n",
    );
    out.push_str("# TYPE boole_node_bounty_side_pool_total gauge\n");
    out.push_str(&format!(
        "boole_node_bounty_side_pool_total {bounty_side_pool}\n"
    ));
    out.push_str("# HELP boole_node_started_at_ms Unix epoch ms when the node booted.\n");
    out.push_str("# TYPE boole_node_started_at_ms gauge\n");
    out.push_str(&format!("boole_node_started_at_ms {started_at}\n"));

    // P0.5 slice 67 — process-wide outcome counters. Counter type so a
    // scraper computes rate()/increase() over the monotonic series.
    let submits_accepted = SUBMITS_ACCEPTED.load(Ordering::Relaxed);
    let submits_rejected = SUBMITS_REJECTED.load(Ordering::Relaxed);
    let proofs_accepted = PROOFS_ACCEPTED.load(Ordering::Relaxed);
    let proofs_rejected = PROOFS_REJECTED.load(Ordering::Relaxed);
    let panic_total = boole_core::telemetry::panic_total();
    out.push_str("# HELP boole_submits_total Share submissions by outcome.\n");
    out.push_str("# TYPE boole_submits_total counter\n");
    out.push_str(&format!(
        "boole_submits_total{{outcome=\"accepted\"}} {submits_accepted}\n"
    ));
    out.push_str(&format!(
        "boole_submits_total{{outcome=\"rejected\"}} {submits_rejected}\n"
    ));
    out.push_str("# HELP boole_proofs_total Bounty proof submissions by outcome.\n");
    out.push_str("# TYPE boole_proofs_total counter\n");
    out.push_str(&format!(
        "boole_proofs_total{{outcome=\"accepted\"}} {proofs_accepted}\n"
    ));
    out.push_str(&format!(
        "boole_proofs_total{{outcome=\"rejected\"}} {proofs_rejected}\n"
    ));
    out.push_str("# HELP boole_panic_total In-process panics caught by the panic hook.\n");
    out.push_str("# TYPE boole_panic_total counter\n");
    out.push_str(&format!("boole_panic_total {panic_total}\n"));
    out
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
        Err(err) => {
            record_submit_outcome(false);
            return error_response(err);
        }
    };
    // N2.1 — ownership proof is mandatory by default. A submit that carried
    // no agent-wallet `session` block (`checked_session == None`) has only a
    // bare prover pk and cannot prove it owns the reward it claims. Reject it
    // before admission unless the operator explicitly enabled the legacy
    // anonymous path. The session-bearing path already proved ownership
    // (reward-recipient binding + signature) inside `submit_session_gate`.
    if checked_session.is_none() && !guard.allow_anonymous_submit {
        record_submit_outcome(false);
        return error_response(HttpError::unauthenticated_submit());
    }
    // P1.3a — burn moved INTO submit_json before block append. Do not
    // re-burn here on accepted=true; that would double-append the same
    // (pk, nonce) and surface a spurious nonce_replayed envelope.
    match submit_json(&mut guard, &body, &peer_ip, checked_session.as_ref()) {
        Ok(value) => {
            record_submit_outcome(true);
            (StatusCode::OK, Json(value)).into_response()
        }
        Err(err) => {
            record_submit_outcome(false);
            error_response(anyhow_to_internal(err))
        }
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

const RECEIPTS_POST_PAYLOAD_SCHEMA: &str = "boole.receipts.commit.v1";

fn receipt_post_json(state: &mut LocalNodeState, body: &[u8]) -> Result<Value, HttpError> {
    // 1) Parse outer `boole.signed.v1` envelope.
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

    // 2) Wire-shape hex checks — keep `keys verify` vocabulary aligned.
    if !is_well_formed_hex32(pk) {
        return Err(HttpError::bad_envelope("pk must be 64 lowercase hex chars"));
    }
    if Hex64::from_hex(signature).is_err() {
        return Err(HttpError::bad_envelope(
            "signature must be 128 lowercase hex chars",
        ));
    }

    // 3) P2.10 — parse optional wire network_id and reject pre-crypto when
    //    it pins a different network than this node.
    let envelope_network_id = parse_envelope_network_id(envelope_obj, &state.network_id)?;

    // 4) Crypto verification: structural envelope intact but wrong sig is
    //    401, not 400. Network-bound digest when the wire envelope opted
    //    in via `network_id`; legacy digest otherwise.
    match verify_signature_with_network(pk, signature, payload, envelope_network_id) {
        Ok(true) => {}
        Ok(false) => return Err(HttpError::signature_invalid()),
        Err(detail) => return Err(HttpError::bad_envelope(detail)),
    }

    // 5) Inner payload schema gate.
    let payload_obj = payload
        .as_object()
        .ok_or_else(|| HttpError::bad_payload("payload", "payload must be a JSON object"))?;
    let payload_schema = payload_obj
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_payload("schema", "payload missing schema"))?;
    if payload_schema != RECEIPTS_POST_PAYLOAD_SCHEMA {
        return Err(HttpError::bad_payload(
            "schema",
            format!("expected {RECEIPTS_POST_PAYLOAD_SCHEMA}, got {payload_schema}"),
        ));
    }
    check_payload_valid_before(payload_obj)?;
    let nonce = check_payload_nonce(payload_obj)?.to_string();
    check_signed_envelope_nonce_not_replayed(state, pk, &nonce)?;
    let receipt_value = payload_obj.get("receiptCommitment").ok_or_else(|| {
        HttpError::bad_payload("receiptCommitment", "payload missing receiptCommitment")
    })?;
    let receipt: ReceiptCommitment = serde_json::from_value(receipt_value.clone())
        .map_err(|err| HttpError::bad_payload("receiptCommitment", err.to_string()))?;
    let signer_pk = pk.to_string();

    // 5) Existing durability + in-memory store mutation. The nonce burn
    //    runs before the receipt append so a crash mid-write cannot
    //    leave a replay window: a burned nonce without a persisted
    //    receipt safely rejects a retry as `nonce_replayed`.
    burn_signed_envelope_nonce(state, &signer_pk, &nonce)?;
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
    enforce_verify_answer_payment(
        header_str(headers, "Payment-Signature"),
        request_hash.clone(),
        pay_to.clone(),
        x402_version.clone(),
    )?;

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

const SESSIONS_REGISTER_PAYLOAD_SCHEMA: &str = "boole.sessions.register.v1";
const SESSIONS_REVOKE_PAYLOAD_SCHEMA: &str = "boole.sessions.revoke.v1";
const BOUNTY_PROOF_PAYLOAD_SCHEMA: &str = "boole.bounty.proof.v1";

fn session_register_json(state: &mut LocalNodeState, body: &[u8]) -> Result<Value, HttpError> {
    // 0) Short-circuit when the registry is not configured. Operators who
    //    have opted out should get the explicit `session_registry_disabled`
    //    reason regardless of whether the caller bothered to sign the
    //    body — wallet UX should not require a signing key just to
    //    discover that the route is off.
    if state.session_registry_path.is_none() || state.session_store.is_none() {
        return Err(HttpError::session_registry_disabled());
    }

    // 1) Parse outer `boole.signed.v1` envelope.
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

    // 2) P2.10 — cross-network gate before crypto.
    let envelope_network_id = parse_envelope_network_id(envelope_obj, &state.network_id)?;

    // 3) Crypto: signature must verify against payload bytes (network-bound
    //    when the wire envelope opted in).
    match verify_signature_with_network(pk, signature, payload, envelope_network_id) {
        Ok(true) => {}
        Ok(false) => return Err(HttpError::signature_invalid()),
        Err(detail) => return Err(HttpError::bad_envelope(detail)),
    }

    // 4) Inner payload validation.
    let payload_obj = payload
        .as_object()
        .ok_or_else(|| HttpError::bad_payload("payload", "payload must be a JSON object"))?;
    let payload_schema = payload_obj
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_payload("schema", "payload missing schema"))?;
    if payload_schema != SESSIONS_REGISTER_PAYLOAD_SCHEMA {
        return Err(HttpError::bad_payload(
            "schema",
            format!("expected {SESSIONS_REGISTER_PAYLOAD_SCHEMA}, got {payload_schema}"),
        ));
    }
    check_payload_valid_before(payload_obj)?;
    let nonce = check_payload_nonce(payload_obj)?.to_string();
    check_signed_envelope_nonce_not_replayed(state, pk, &nonce)?;
    let session_value = payload_obj
        .get("session")
        .ok_or_else(|| HttpError::missing_field("session"))?;
    let session: SessionState = serde_json::from_value(session_value.clone())
        .map_err(|err| HttpError::bad_payload("session", err.to_string()))?;
    let current_height = payload_obj
        .get("currentHeight")
        .and_then(Value::as_u64)
        .ok_or_else(|| HttpError::missing_field("currentHeight"))?;
    let signer_pk = pk.to_string();

    // P1.6 (audit) — AUTHORIZATION: a valid signature proves WHO signed, not
    // that they may register THIS session. Only the session's declared owner
    // may register it, otherwise anyone with any key could register a session
    // binding an arbitrary `ownerPk`/`fixedRewardRecipient`.
    if signer_pk != session.owner_pk {
        return Err(HttpError::unauthorized_signer(
            "envelope signer pk must equal session.ownerPk to register a session",
        ));
    }

    // 4) Burn the per-signer nonce before the session-ledger append so a
    //    crash mid-write rejects a retry with the same `(signerPk, nonce)`
    //    pair instead of silently re-registering the session.
    burn_signed_envelope_nonce(state, &signer_pk, &nonce)?;
    let path = state
        .session_registry_path
        .clone()
        .ok_or_else(HttpError::session_registry_disabled)?;
    let store = state
        .session_store
        .as_mut()
        .ok_or_else(HttpError::session_registry_disabled)?;
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
    // 0) URL-shape gate + operator-config gate run before envelope
    //    parsing so wallets receive the precise reason without having to
    //    sign a request just to learn the route is unusable.
    if !is_well_formed_hex32(session_pk) {
        return Err(HttpError::malformed_pk());
    }
    if state.session_registry_path.is_none() || state.session_store.is_none() {
        return Err(HttpError::session_registry_disabled());
    }

    // 1) Parse outer `boole.signed.v1` envelope.
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

    // 2) P2.10 — cross-network gate before crypto.
    let envelope_network_id = parse_envelope_network_id(envelope_obj, &state.network_id)?;

    // 3) Crypto: signature must verify against payload bytes (network-bound
    //    when the wire envelope opted in).
    match verify_signature_with_network(pk, signature, payload, envelope_network_id) {
        Ok(true) => {}
        Ok(false) => return Err(HttpError::signature_invalid()),
        Err(detail) => return Err(HttpError::bad_envelope(detail)),
    }

    // 4) Inner payload validation.
    let payload_obj = payload
        .as_object()
        .ok_or_else(|| HttpError::bad_payload("payload", "payload must be a JSON object"))?;
    let payload_schema = payload_obj
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_payload("schema", "payload missing schema"))?;
    if payload_schema != SESSIONS_REVOKE_PAYLOAD_SCHEMA {
        return Err(HttpError::bad_payload(
            "schema",
            format!("expected {SESSIONS_REVOKE_PAYLOAD_SCHEMA}, got {payload_schema}"),
        ));
    }
    check_payload_valid_before(payload_obj)?;
    let nonce = check_payload_nonce(payload_obj)?.to_string();
    check_signed_envelope_nonce_not_replayed(state, pk, &nonce)?;
    let payload_session_pk = payload_obj
        .get("sessionPk")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_payload("sessionPk", "payload missing sessionPk"))?;
    if payload_session_pk != session_pk {
        // URL sessionPk binds the signed payload so a valid signature
        // cannot be replayed against a different session's URL.
        return Err(HttpError::bad_payload(
            "sessionPk",
            "URL sessionPk does not match payload sessionPk",
        ));
    }
    let height = payload_obj
        .get("height")
        .and_then(Value::as_u64)
        .ok_or_else(|| HttpError::bad_payload("height", "payload missing height"))?;
    let signer_pk = pk.to_string();

    // P1.6 (audit) — AUTHORIZATION: only the session's owner may revoke it.
    // Looked up BEFORE the burn so an unauthorized/not-found request leaves the
    // `(signerPk, nonce)` reusable. A valid signature from any other key must
    // not be able to revoke someone else's session.
    match state
        .session_store
        .as_ref()
        .and_then(|store| store.get(session_pk))
        .map(|s| s.owner_pk.clone())
    {
        Some(owner) if owner == signer_pk => {}
        Some(_) => {
            return Err(HttpError::unauthorized_signer(
                "envelope signer pk must equal the session's ownerPk to revoke it",
            ))
        }
        None => return Err(HttpError::session_not_found(session_pk.to_string())),
    }

    // 4) Burn per-signer nonce before the revoke append so a crash
    //    mid-write rejects a retry instead of double-revoking.
    burn_signed_envelope_nonce(state, &signer_pk, &nonce)?;
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

/// Parsed wallet-submit envelope after the state-free validation gate.
///
/// RM2.3 (R3): the envelope parse + field/format validation is split out of
/// `submit_session_gate` so it is directly unit-testable without booting an
/// HTTP node or constructing a `LocalNodeState`. The owned `envelope` lets the
/// stateful suffix re-read the `body` and `session` blocks it needs.
#[derive(Debug)]
struct ParsedSubmitSession {
    submitted_by: String,
    reward_recipient: String,
    nonce: String,
    envelope: Value,
}

/// State-free prefix of the submit-session gate: decode the request envelope,
/// detect whether it carries a wallet `session` block, and validate the
/// required fields' presence and key format. Returns `Ok(None)` for non-wallet
/// callers (malformed JSON, no `session` object) so the legacy `submit_json`
/// path stays in charge, and `Err` for a wallet envelope missing or malforming
/// a required field. No `LocalNodeState` access happens here.
fn parse_submit_session_envelope(body: &[u8]) -> Result<Option<ParsedSubmitSession>, HttpError> {
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
    let session_obj = match envelope_obj.get("session").and_then(Value::as_object) {
        Some(obj) => obj,
        _ => return Ok(None),
    };
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
    let parsed = ParsedSubmitSession {
        submitted_by: submitted_by.to_string(),
        reward_recipient: reward_recipient.to_string(),
        nonce: nonce.to_string(),
        envelope,
    };
    Ok(Some(parsed))
}

fn submit_session_gate(
    state: &mut LocalNodeState,
    body: &[u8],
) -> Result<Option<CheckedSubmitSession>, HttpError> {
    let parsed = match parse_submit_session_envelope(body)? {
        Some(parsed) => parsed,
        None => return Ok(None),
    };
    let envelope_obj = parsed
        .envelope
        .as_object()
        .expect("parsed envelope is an object");
    let session_obj = envelope_obj
        .get("session")
        .and_then(Value::as_object)
        .expect("parsed envelope carries a session object");
    let submitted_by = parsed.submitted_by.as_str();
    let reward_recipient = parsed.reward_recipient.as_str();
    let nonce = parsed.nonce.as_str();

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
        &state.network_id,
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
    node_network_id: &str,
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
    // P2.10 — the nested `boole.signer.work.v1` envelope is in-scope per
    // ADR-0003. Cross-check its optional `network_id` against the node's
    // pinned id before recomputing the network-bound digest.
    let signed_network_id = parse_envelope_network_id(signed_obj, node_network_id)?;
    match verify_signature_with_network(pk, signature, payload, signed_network_id) {
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

/// P2.6 - recompute disk-vs-runtime agreement at request time so the
/// `/status.replayMatchesRuntime` field detects post-boot drift (file
/// tampering, partial truncation, future refactor regressions) instead
/// of merely reporting the boot snapshot. Returns `false` on any
/// recover/replay error or shape mismatch.
fn compute_replay_matches_runtime(state: &LocalNodeState) -> bool {
    let Ok(recovered) = FileBlockStore::recover(&state.block_path) else {
        return false;
    };
    if recovered.size() == 0 {
        return state.runtime.cached_block_count() == 0 && state.runtime.current_c().is_some();
    }
    let Ok(replay) = replay_blocks(recovered.blocks()) else {
        return false;
    };
    (replay.height as usize) == state.runtime.cached_block_count()
        && Some(replay.latest_c.as_str()) == state.runtime.current_c()
}

/// N1.5 (G6) — the only claim boundary this node asserts. It has no public
/// mining mode to configure, so the honest label is a constant rather than a
/// `LocalNodeConfig` field (avoids churning the wide Default-less config for
/// a value that can never be anything else here).
const CLAIM_BOUNDARY: &str = "closed-local-smoke";

fn status_json(state: &LocalNodeState) -> anyhow::Result<Value> {
    // Serve from the in-memory block cache. After boot the cache is
    // authoritative; commits update it synchronously via {check, append,
    // apply_unchecked}, and replay invariants (chain linkage, latest_c)
    // are checked at boot via replay_blocks. `replayMatchesRuntime` is
    // recomputed per request via `compute_replay_matches_runtime` so the
    // field detects post-boot drift (e.g., external file tampering).
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
        "replayMatchesRuntime": compute_replay_matches_runtime(state),
        "sharePoolSize": state.runtime.pool_size(),
        "familyManifestCount": state.family_manifest_registry.len(),
        "bountySidePoolTotal": state.bounty_side_pool.total_share_count(),
        "promotedBountySharesCount": promoted.len(),
        "nodeStartedAt": state.started_at_ms,
        // P2.6 b — Surface the operator's Lean-checker choice so a
        // 503 on `/ready` (reason: lean_checker_not_configured) is
        // diagnosable purely from HTTP without scraping logs. N0-pre.7 —
        // the boolean disabled flag stays (it leaks no path), but the
        // `blockStorePath` and `lean_checker_dir` absolute paths were
        // removed: an unauthenticated caller must not learn the operator's
        // on-disk layout. Path-level diagnostics move to a future
        // authenticated operator tier.
        "lean_checker_disabled": state.lean_checker_disabled,
        // N1.5 (G6) — claim-boundary + difficulty-mode honesty labels so a
        // reviewer can distinguish closed-local-smoke from public mining.
        "claimBoundary": CLAIM_BOUNDARY,
        "difficultyMode": state.runtime.effective_difficulty_for_head()?.mode,
        "publicMiningEvidence": false,
        "publicScoringEligible": false,
        "ineligibilityReasons": Vec::<String>::new(),
    }))
}

fn head_json(state: &LocalNodeState) -> anyhow::Result<Value> {
    let height = state.runtime.cached_block_count();
    let report = &state.report;
    // N1.1 (G1/G2) — emit the height-effective retargeted difficulty + epoch/
    // mode labels. In static-calibrated mode keep the exact report.T_block so
    // /head output is byte-unchanged when retarget is disabled; only the
    // retarget-engaged path swaps in the runtime-effective value.
    let difficulty = state.runtime.effective_difficulty_for_head()?;
    let t_block = if difficulty.mode == "static-calibrated" {
        json!(report.T_block)
    } else {
        json!(difficulty.t_block)
    };
    Ok(json!({
        "ok": true,
        "height": height,
        "c": current_head(state),
        "T_ticket": report.T_ticket,
        "T_share": report.T_share,
        "T_block": t_block,
        "T_submit": report.T_submit,
        "MinShareScoreMultiplier": report.MinShareScoreMultiplier,
        "M": report.M,
        "K_max": report.K_max,
        "L": report.L,
        "D_max": report.D_max,
        "difficultyEpoch": difficulty.difficulty_epoch,
        "difficultyMode": difficulty.mode,
        "difficultyRetarget": difficulty.retarget,
        // N1.5 (G6) — honesty labels: this node is a closed-local node, not a
        // public-network mining surface. A reviewer can tell its responses
        // apart from public mining evidence without scraping logs.
        "claimBoundary": CLAIM_BOUNDARY,
        "publicMiningEvidence": false,
        "publicScoringEligible": false,
        "ineligibilityReasons": Vec::<String>::new(),
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

/// P1.7 — bounty proof submission must not hold the write lock while
/// the verifier runs. The handler splits the request into three phases:
///
///   1. **Phase 1 (read lock)**: parse the body, look up the bounty +
///      verifier, peek the dedup table, reject terminal bounties. Read
///      lock only, so concurrent `/ready`, `/status`, and other reader
///      handlers stay responsive.
///   2. **Phase 2 (no locks)**: run `BountyProofVerifier::verify`. This
///      is the call that can sleep for hundreds of milliseconds (Lean
///      child invocation, network-bound mock verifier, etc.). Pre-P1.7
///      it ran inside the write lock and starved every other handler;
///      now it runs with no `LocalNodeState` lock held at all.
///   3. **Phase 3 (write lock)**: mutate registry, side-pool, and the
///      bounty event ledger. `BountyRegistry::submit_proof` re-checks
///      dedup and terminal status internally, so a racing submitter
///      that resolved the bounty during phase 2 is surfaced through
///      `outcome.duplicate` (we skip the side-pool insert and ledger
///      append to avoid double-credit) instead of stomping the prior
///      result.
async fn bounty_proof_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    body: Bytes,
) -> Response {
    let prep = {
        let guard = state.inner.read().await;
        match bounty_proof_prepare(&guard, &id, &body) {
            Ok(p) => p,
            Err(err) => {
                record_proof_outcome(false);
                return error_response(err);
            }
        }
    };
    let prepared = match prep {
        PreparedProof::Duplicate(value) => {
            // Idempotent re-submission of an already-accepted proof.
            record_proof_outcome(true);
            return (StatusCode::OK, Json(value)).into_response();
        }
        PreparedProof::RunVerifier(p) => *p,
    };

    // P1.7 — run the synchronous, subprocess-spawning Lean verifier on a
    // dedicated blocking thread so a flood of concurrent proofs cannot pin
    // the async worker pool (L5: "each verify on its own task"). The route
    // timeout can then preempt this await; the verifier's own internal
    // deadline + `ChildKillOnDrop` reap the `lake` child even if it does.
    let verifier = Arc::clone(&prepared.verifier);
    let bounty = prepared.bounty.clone();
    let envelope = prepared.envelope.clone();
    let outcome = match tokio::task::spawn_blocking(move || {
        verifier.verify_with_evidence(&bounty, &envelope)
    })
    .await
    {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            record_proof_outcome(false);
            return error_response(HttpError::verifier_error(e));
        }
        Err(join_err) => {
            record_proof_outcome(false);
            return error_response(HttpError::verifier_error(format!(
                "verifier task panicked: {join_err}"
            )));
        }
    };

    let mut guard = state.inner.write().await;
    match bounty_proof_finalize(&mut guard, &id, prepared, outcome) {
        Ok(value) => {
            record_proof_outcome(true);
            (StatusCode::OK, Json(value)).into_response()
        }
        Err(err) => {
            record_proof_outcome(false);
            error_response(err)
        }
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

    // 3) P2.10 — cross-network gate before crypto.
    let envelope_network_id = parse_envelope_network_id(envelope_obj, &state.network_id)?;

    // 4) Crypto: structurally valid envelope but wrong sig is 401, not 400.
    //    Network-bound digest when the wire envelope opted in via
    //    `network_id`; legacy digest otherwise.
    match verify_signature_with_network(pk, signature, payload, envelope_network_id) {
        Ok(true) => {}
        Ok(false) => return Err(HttpError::signature_invalid()),
        Err(detail) => return Err(HttpError::bad_envelope(detail)),
    }

    // 5) Inner payload validation. The CLI builds this; the wire format
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
    check_payload_valid_before(payload_obj)?;
    let nonce = check_payload_nonce(payload_obj)?.to_string();
    check_signed_envelope_nonce_not_replayed(state, pk, &nonce)?;
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
    let signer_pk = pk.to_string();

    // P1.6 (audit) — AUTHORIZATION: when the operator configured an allowlist
    // (`--operator-signer-pks`), only those keys may announce bounties. An empty
    // allowlist keeps the route permissionless (open testnet bounty board), so
    // this closes the closed-board gap without breaking the permissionless mode.
    if !state.operator_signer_pks.is_empty()
        && !state.operator_signer_pks.iter().any(|p| p == &signer_pk)
    {
        return Err(HttpError::unauthorized_signer(
            "bounty announce requires the signer to be on the operator allowlist",
        ));
    }

    // 5) Burn the per-signer nonce before the registry mutates so a
    //    crash during create leaves the nonce burned and the retry is
    //    rejected with `nonce_replayed` rather than re-attempting the
    //    same announce under a stale signing intent.
    burn_signed_envelope_nonce(state, &signer_pk, &nonce)?;
    // 6) Acquire registry mutation. validate_create surfaces field-level
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
    // 1b) P2.10 — cross-network gate before crypto.
    let envelope_network_id = parse_envelope_network_id(envelope_obj, &state.network_id)?;
    match verify_signature_with_network(pk, signature, payload, envelope_network_id) {
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
    check_payload_valid_before(payload_obj)?;
    let nonce = check_payload_nonce(payload_obj)?.to_string();
    check_signed_envelope_nonce_not_replayed(state, pk, &nonce)?;
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
    let signer_pk = pk.to_string();

    // P1.6 (audit) — AUTHORIZATION: when the operator configured an allowlist
    // (`--operator-signer-pks`), only those keys may change a bounty's status.
    // An empty allowlist keeps the route permissionless. Checked before the burn
    // so an unauthorized request leaves the `(signerPk, nonce)` reusable.
    if !state.operator_signer_pks.is_empty()
        && !state.operator_signer_pks.iter().any(|p| p == &signer_pk)
    {
        return Err(HttpError::unauthorized_signer(
            "bounty status change requires the signer to be on the operator allowlist",
        ));
    }

    // 4) Burn the per-signer nonce before the status mutation so a
    //    crash during update_status leaves the nonce burned and the
    //    retry is rejected with `nonce_replayed`.
    burn_signed_envelope_nonce(state, &signer_pk, &nonce)?;
    // 5) Apply the transition. The registry enforces transition rules; map
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

/// Phase 1 outcome of the bounty proof handler split. Either the dedup
/// table answers the request without spending verifier time, or we
/// have a fully validated request ready to enter the unlocked verify
/// phase.
enum PreparedProof {
    /// Idempotent re-post — the dedup table already remembers this
    /// `(bounty_id, proof_hash)` from a prior accept/reject. Return the
    /// canonical envelope verbatim; no write lock and no verifier
    /// dispatch needed.
    Duplicate(Value),
    /// First-seen proof. Phase 2 still needs to run the verifier with
    /// no locks held, then phase 3 applies the mutation under the
    /// write lock. Boxed because the prepared struct is ~288 bytes
    /// (largely the cloned `Bounty`); without indirection the enum
    /// payload alignment would balloon the `Duplicate` arm too.
    RunVerifier(Box<PreparedProofToVerify>),
}

struct PreparedProofToVerify {
    bounty: boole_core::Bounty,
    proof_hash: String,
    prover: String,
    envelope: Value,
    verifier: Arc<dyn BountyProofVerifier>,
    /// P1.6b — `(signer_pk, nonce)` pair captured during phase 1 so the
    /// write-lock phase can atomically burn the pair before the registry
    /// mutation. Set to `(pk, payload.nonce)` once phase 1 has validated
    /// the freshness gates.
    signer_pk: String,
    nonce: String,
}

/// Phase 1 of the P1.7 bounty proof flow. Runs under a read lock: parse
/// the body, look up the bounty + verifier, peek the dedup table, and
/// reject terminal bounties. Mirrors the validation order of the
/// pre-P1.7 monolithic helper so callers see identical 4xx/5xx envelopes
/// for the same inputs.
fn bounty_proof_prepare(
    state: &LocalNodeState,
    id: &str,
    body: &[u8],
) -> Result<PreparedProof, HttpError> {
    // 1) 404 — bounty must exist (catalog or registry-replayed). Bounty
    //    existence is public catalog data so this gate runs before
    //    envelope parsing to keep the error surface aligned with the
    //    pre-P1.6d contract.
    let bounty = state
        .bounty_registry
        .get(id)
        .ok_or_else(|| HttpError::bounty_not_found(id))?;

    // 2) Parse the outer `boole.signed.v1` envelope.
    let outer: Value = serde_json::from_slice(body)
        .map_err(|err| HttpError::bad_envelope(format!("body is not valid JSON: {err}")))?;
    let outer_obj = outer
        .as_object()
        .ok_or_else(|| HttpError::bad_envelope("envelope must be a JSON object"))?;
    let schema = outer_obj
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_envelope("envelope missing schema"))?;
    if schema != SIGNED_ENVELOPE_SCHEMA {
        return Err(HttpError::bad_envelope(format!(
            "expected schema {SIGNED_ENVELOPE_SCHEMA}, got {schema}"
        )));
    }
    let pk = outer_obj
        .get("pk")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_envelope("envelope missing pk"))?;
    let signature = outer_obj
        .get("signature")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_envelope("envelope missing signature"))?;
    let payload = outer_obj
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

    // 3) P2.10 — cross-network gate before crypto.
    let envelope_network_id = parse_envelope_network_id(outer_obj, &state.network_id)?;

    // 4) Crypto: signature must verify against payload bytes (network-bound
    //    when the wire envelope opted in).
    match verify_signature_with_network(pk, signature, payload, envelope_network_id) {
        Ok(true) => {}
        Ok(false) => return Err(HttpError::signature_invalid()),
        Err(detail) => return Err(HttpError::bad_envelope(detail)),
    }

    // 5) Inner payload validation.
    let payload_obj = payload
        .as_object()
        .ok_or_else(|| HttpError::bad_payload("payload", "payload must be a JSON object"))?;
    let payload_schema = payload_obj
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_payload("schema", "payload missing schema"))?;
    if payload_schema != BOUNTY_PROOF_PAYLOAD_SCHEMA {
        return Err(HttpError::bad_payload(
            "schema",
            format!("expected {BOUNTY_PROOF_PAYLOAD_SCHEMA}, got {payload_schema}"),
        ));
    }
    check_payload_valid_before(payload_obj)?;
    let nonce = check_payload_nonce(payload_obj)?.to_string();
    let payload_bounty_id = payload_obj
        .get("bountyId")
        .and_then(Value::as_str)
        .ok_or_else(|| HttpError::bad_payload("bountyId", "payload missing bountyId"))?;
    if payload_bounty_id != id {
        return Err(HttpError::bad_payload(
            "bountyId",
            "URL bountyId does not match payload bountyId",
        ));
    }
    let proof_hash = payload_obj
        .get("proofHash")
        .and_then(Value::as_str)
        .ok_or_else(HttpError::bad_proof_hash)?
        .to_string();
    if Hex32::from_hex(&proof_hash).is_err() {
        return Err(HttpError::bad_proof_hash());
    }
    let prover = payload_obj
        .get("prover")
        .and_then(Value::as_str)
        .ok_or_else(HttpError::bad_prover)?
        .to_string();
    if Hex32::from_hex(&prover).is_err() {
        return Err(HttpError::bad_prover());
    }
    if prover != pk {
        // Envelope signer must match the claimed prover so a third
        // party cannot post a proof crediting somebody else's reward.
        return Err(HttpError::bad_payload(
            "prover",
            "envelope pk does not match payload prover",
        ));
    }
    let envelope = payload_obj.get("envelope").cloned().unwrap_or(Value::Null);

    // 3) Dedup peek — wins over terminal status, nonce-replay, and
    //    verifier dispatch so an HTTP retry of the same envelope (same
    //    proofHash, same nonce) idempotently returns the cached
    //    outcome instead of failing with `nonce_replayed`.
    if let Some(accepted) = state.bounty_registry.has_proof(id, &proof_hash) {
        let value = json!({
            "ok": true,
            "accepted": accepted,
            "duplicate": true,
            "bounty": serde_json::to_value(&bounty)
                .expect("Bounty serializes to JSON via serde"),
        });
        return Ok(PreparedProof::Duplicate(value));
    }

    // 4) P1.6b — soft per-signer replay probe. Runs after the dedup
    //    peek so HTTP idempotency wins over freshness, but before the
    //    verifier and terminal gates so a stolen envelope re-aimed at a
    //    fresh proofHash never reaches `lake exec`. The atomic
    //    `(signer_pk, nonce)` burn happens in phase 3 regardless of the
    //    verifier's verdict — a rejected proof still consumes its nonce.
    check_signed_envelope_nonce_not_replayed(state, pk, &nonce)?;

    // 5) 501 — unknown verifier kind. Caller knows to retry with a node
    //    that has the verifier wired in.
    let verifier = state
        .bounty_verifiers
        .get(&bounty.verifier.kind)
        .cloned()
        .ok_or_else(|| HttpError::no_verifier(&bounty.verifier.kind))?;

    // 6) 409 — terminal bounty. Comes after dedup so a duplicate post on
    //    a now-solved bounty short-circuits with `duplicate=true`.
    if bounty.status != "open" {
        return Err(HttpError::bounty_terminal(&bounty.status));
    }

    let signer_pk = pk.to_string();
    Ok(PreparedProof::RunVerifier(Box::new(
        PreparedProofToVerify {
            bounty,
            proof_hash,
            prover,
            envelope,
            verifier,
            signer_pk,
            nonce,
        },
    )))
}

/// Phase 3 of the P1.7 bounty proof flow. Runs under the write lock
/// once phase 2's verifier call has returned. `BountyRegistry::submit_proof`
/// re-checks dedup and terminal status, so a racing submitter that
/// resolved the bounty during the unlocked verify window is surfaced
/// through `outcome.duplicate` instead of double-mutating.
fn bounty_proof_finalize(
    state: &mut LocalNodeState,
    id: &str,
    prepared: PreparedProofToVerify,
    outcome: VerifyOutcome,
) -> Result<Value, HttpError> {
    let PreparedProofToVerify {
        bounty,
        proof_hash,
        prover,
        envelope,
        verifier: _,
        signer_pk,
        nonce,
    } = prepared;
    let VerifyOutcome { accepted, evidence } = outcome;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // P1.6b — atomic burn under the write lock. Two concurrent
    // submitters that raced past phase 1's soft probe with the same
    // `(signer_pk, nonce)` will see one Ok(true) and one 409 here; the
    // burn writes to the ledger before submit_proof so a crash mid-call
    // leaves the nonce consumed and a retry is rejected as
    // `nonce_replayed`.
    burn_signed_envelope_nonce(state, &signer_pk, &nonce)?;

    // submit_proof internally re-checks dedup and terminal status, so a
    // concurrent submitter that landed the same proof hash during the
    // unlocked verify phase is surfaced as `outcome.duplicate = true`
    // (we skip the side-pool insert and ledger append below to avoid
    // double-credit), and a concurrent submitter that solved the bounty
    // with a different hash is surfaced as Err("cannot submit proof to
    // terminal bounty ...") which we map to a 5xx for the caller.
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

    // On a first-seen accept, route the share into the per-family
    // side-pool. The Hard Guard holds because (a) this writes to
    // `bounty_side_pool`, never to `runtime` or `share_pool`, and (b)
    // `build_block_selection` does not consume from the side-pool.
    // `family_id == bounty.domain` per the bounty/manifest fixture
    // convention; if the domain has no registered manifest we still
    // record the share so S22 can audit "would have promoted but no
    // manifest" cases. The `!outcome.duplicate` guard means a racing
    // submitter that already recorded this proof keeps its credit and
    // we do not insert again.
    if outcome.accepted && !outcome.duplicate {
        // S23a — stamp the matching bounty's reward onto the share so
        // the promotion gate can compute capped credit without a second
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

    // Audit-log append. Skipped on duplicates so a concurrent submitter's
    // ledger row is not double-written. Failure here is fatal — the
    // in-memory state has already mutated; surfacing a 500 at this point
    // is preferable to silently dropping the durability promise.
    if !outcome.duplicate {
        let credit = if accepted {
            bounty.reward.clone()
        } else {
            "0".to_string()
        };
        let mut event = json!({
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
        // P1.4 — for Lean-verified bounties, persist the inputs needed
        // to re-run `lake exec boole_check` from the audit log alone:
        // the verbatim `leanSource` from the envelope and the bounty's
        // pinned `verifierHash`. `boole state verify --deep` (master plan
        // line 110-141) reads the ledger and re-checks acceptance offline;
        // without these two fields the audit log cannot reproduce the
        // verdict and the node has to be trusted across restarts. The
        // branch is keyed on `verifier.kind == "lean"` because other
        // verifier kinds carry different evidence shapes and adding
        // null/empty placeholders would muddy schema migration later.
        if bounty.verifier.kind == "lean" {
            if let Some(obj) = event.as_object_mut() {
                if let Some(lean_source) = envelope.get("leanSource").and_then(Value::as_str) {
                    obj.insert(
                        "leanSource".to_string(),
                        Value::String(lean_source.to_string()),
                    );
                }
                if let Some(verifier_hash) = bounty
                    .verifier
                    .metadata
                    .get("verifierHash")
                    .and_then(Value::as_str)
                {
                    obj.insert(
                        "verifierHash".to_string(),
                        Value::String(verifier_hash.to_string()),
                    );
                }
                // P1.4 slice-20 — merge verifier-side evidence (e.g.
                // `checkerArtifactHash` from `LeanRunner`). The verifier
                // is the only source for these fields because they are
                // derived from the physical checker artifact, not from
                // the bounty record or the submitter's envelope. Slice-19
                // keys (`leanSource`, `verifierHash`) win on collision so
                // a misbehaving verifier cannot overwrite the audit log's
                // canonical input echoes.
                for (k, v) in evidence.into_iter() {
                    obj.entry(k).or_insert(v);
                }
            }
        }
        if let Some(path) = state.bounty_event_ledger_path.as_ref() {
            FileBountyEventLedger::append(path, &event)
                .map_err(|err| HttpError::internal(format!("bounty audit append: {err}")))?;
        }
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

/// N2.3 — the server's canonical hash of a submitted proof: SHA-256 over the
/// decoded proof package bytes (`body["bytes"]`). Mirrors the admission-layer
/// `canon_hash` and is computed entirely server-side, so it is a forge-proof
/// dedup key: two submits carrying the same proof collide regardless of pk or
/// any client-supplied field. `normalize_pow_fields` only rewrites `n`/`j`/
/// `nonceS`, never `bytes`, so this matches admission's value byte-for-byte.
fn proof_canon_hash(body: &serde_json::Map<String, Value>) -> String {
    let bytes_hex = body.get("bytes").and_then(Value::as_str).unwrap_or("");
    let package = hex::decode(bytes_hex).unwrap_or_default();
    hex::encode(Sha256::digest(&package))
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
    // N3-pre.3 (review #3) — a submit body that omits `ts` now defaults to
    // real wall-clock time rather than the repo-wide fixed test constant
    // `1_800_000_000_000`. That constant predates the future-drift bound
    // below and would otherwise be rejected as "from the future" on every
    // real run, since it is not tied to whenever the node actually boots.
    let ts_raw = submit_body
        .get("ts")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| now_unix_ms() as u64);
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
            // N0-pre.10 — stable machine-readable rejection code (additive;
            // the human-readable `decision` Debug string stays for back-compat).
            // The miner branches on this (`stale_c` => mid-cycle head refresh)
            // instead of substring-matching the prose.
            "code": decision.reject_code(),
            "c": current_head(state),
        }));
    };
    // N2.3 — proof dedup. Reject a second credit for the same proof (same
    // server-computed canonical bytes) under any pk BEFORE any durable write.
    // Cross-pk farming of one proof must not earn two credits. The key is the
    // node's own hash over the decoded proof package, never a client field.
    let proof_canon_hash = proof_canon_hash(&body);
    if state
        .proof_dedup_ledger
        .as_ref()
        .is_some_and(|ledger| ledger.contains(&proof_canon_hash))
    {
        return Ok(json!({
            "ok": false,
            "accepted": false,
            "reason": "duplicate_proof",
            "code": "duplicate_proof",
            "c": current_head(state),
        }));
    }
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
    // N3-pre.3 (review #3) — self-produce node boundary: reject a block
    // whose self-reported `ts` has drifted too far into the future before
    // any further state mutation. The deterministic median-time-past rule
    // (replay layer, `boole_core::verify_block_ts_median_time_past`) never
    // touches wall-clock time; this is the one guard that does, and it
    // lives only here at the boundary.
    check_block_ts_future_drift(ts_raw, now_unix_ms() as u64)?;
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
    // N2.3 — record the now-credited proof's canon hash so a later submit of
    // the same proof (under any pk) is rejected by the check above. Recorded
    // only after the block is committed, so a NoProposer/Ambiguous accept
    // (which returns earlier without a credit) does not consume the proof's
    // single-credit slot.
    if let (Some(path), Some(ledger)) = (
        state.proof_dedup_ledger_path.clone(),
        state.proof_dedup_ledger.as_mut(),
    ) {
        ledger.append_credit(&path, &proof_canon_hash)?;
    }
    // S23c — mirror the credit rows into the bounty event ledger so the
    // divergence sweep (S23d) has a parallel source to compare against.
    // P1.5b — additionally emit one `share_promoted` event per promoted
    // share (including zero-credit shares) so the boot loader can
    // subtract already-committed shares from the durable audit replay
    // and avoid silently re-inserting them into the live side-pool.
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
        // P1.3b — write the `share_promoted` rows from the now-persisted block
        // field, not the in-memory `selection.shares`, so the on-disk block and
        // the bounty-event ledger are derived from the same source (the block
        // is fsync'd before this point). The values are identical in the
        // non-crash path; this is the correctness anchor that lets the boot
        // heal re-derive these rows after a crash mid-commit.
        for share in &committed.block.promoted_bounty_shares {
            let event = json!({
                "schemaVersion": 1,
                "kind": "share_promoted",
                "height": committed.block.height,
                "familyId": share.family_id,
                "bountyId": share.bounty_id,
                "proofHash": share.proof_hash,
                "prover": share.prover,
            });
            FileBountyEventLedger::append(bounty_event_path, &event)?;
        }
    }
    // P1.5a — drop shares already promoted into a committed block so the
    // next block does not re-promote the same proof and double-credit the
    // prover. We drain by the full `selection.shares` slice (not by the
    // narrower `promoted_bounty_credits`) because zero-credit shares
    // still count as "promoted into this block" — they have flowed
    // through the selection gate and consumed the family's per-block
    // share quota.
    state.bounty_side_pool.remove_promoted(&selection.shares);
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
        "replayMatchesRuntime": compute_replay_matches_runtime(state),
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
            promoted_bounty_shares: Vec::new(),
        };

        let err = submit_receipt_json(&session, &block, HASH_1)
            .expect_err("missing replay credit must fail receipt creation");
        assert!(
            err.to_string().contains("rewardRecipient not credited"),
            "unexpected error: {err}"
        );
    }

    // RM2.3 (R3) — the submit-session envelope parse/validation gate is a
    // pure function (no LocalNodeState), so it is directly unit-testable
    // without booting an HTTP node. These cover the state-free decision
    // points the route tests previously exercised only over HTTP.
    #[test]
    fn parse_submit_session_envelope_returns_none_for_non_wallet_bodies() {
        // Malformed JSON → None (legacy submit path handles it).
        assert!(parse_submit_session_envelope(b"not json")
            .expect("malformed json is passthrough")
            .is_none());
        // Valid JSON without a `session` block → None (pre-wallet caller).
        let body = serde_json::to_vec(&serde_json::json!({"bytes": "00"})).unwrap();
        assert!(parse_submit_session_envelope(&body)
            .expect("no-session is passthrough")
            .is_none());
    }

    #[test]
    fn parse_submit_session_envelope_rejects_missing_and_malformed_fields() {
        let missing = serde_json::to_vec(&serde_json::json!({
            "session": {"rewardRecipient": PK_B, "nonce": "n1"}
        }))
        .unwrap();
        let err =
            parse_submit_session_envelope(&missing).expect_err("missing submittedBy must reject");
        assert_eq!(err.reason, "missing_field", "got: {:?}", err.reason);

        let bad_pk = serde_json::to_vec(&serde_json::json!({
            "session": {"submittedBy": "zz", "rewardRecipient": PK_B, "nonce": "n1"}
        }))
        .unwrap();
        let err = parse_submit_session_envelope(&bad_pk).expect_err("malformed pk must reject");
        assert_eq!(err.reason, "malformed_pk", "got: {:?}", err.reason);
    }

    #[test]
    fn parse_submit_session_envelope_extracts_well_formed_session() {
        let body = serde_json::to_vec(&serde_json::json!({
            "body": {"k": "v"},
            "session": {"submittedBy": PK_A, "rewardRecipient": PK_B, "nonce": "n-42"}
        }))
        .unwrap();
        let parsed = parse_submit_session_envelope(&body)
            .expect("valid envelope parses")
            .expect("a session block is present");
        assert_eq!(parsed.submitted_by, PK_A);
        assert_eq!(parsed.reward_recipient, PK_B);
        assert_eq!(parsed.nonce, "n-42");
    }

    // N3-pre.3 (review #3) — the wall-clock future-drift bound is the ONLY
    // check in the self-produce path that reads real time, so it is tested
    // as a pure function of an explicit `now_ms` rather than over HTTP with
    // `SystemTime::now()` — keeping it deterministic and independent of
    // whatever `ts` convention the rest of the fixture suite uses.
    #[test]
    fn self_produce_rejects_ts_beyond_future_drift() {
        let now_ms = 1_800_000_000_000u64;

        // Within the drift bound (1 minute ahead of now) → accepted.
        check_block_ts_future_drift(now_ms + 60_000, now_ms)
            .expect("ts within the future-drift bound must be accepted");

        // Exactly at the bound → still accepted (bound is inclusive).
        check_block_ts_future_drift(now_ms + BLOCK_TS_MAX_FUTURE_DRIFT_MS, now_ms)
            .expect("ts exactly at the future-drift bound must be accepted");

        // A ts far beyond the bound (a proposer trying to pre-stage a
        // large forward drift ahead of a later median-time-past window)
        // must be rejected.
        let err = check_block_ts_future_drift(now_ms + BLOCK_TS_MAX_FUTURE_DRIFT_MS + 1, now_ms)
            .expect_err("ts beyond the future-drift bound must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("future-drift"),
            "error should name the future-drift rule, got: {msg}"
        );
    }

    #[test]
    fn is_proof_route_matches_only_canonical_three_segment_form() {
        // Canonical proof route → true.
        assert!(is_proof_route("/bounties/test-id/proof"));
        assert!(is_proof_route("/bounties/abc123/proof"));
        // /bounties/{id} GET with id="proof" → NOT the proof route.
        assert!(!is_proof_route("/bounties/proof"));
        // Deeper paths (no registered route) → NOT the proof route.
        assert!(!is_proof_route("/bounties/x/y/proof"));
        // Other routes / prefixes → false.
        assert!(!is_proof_route("/bounties/test-id"));
        assert!(!is_proof_route("/bounties/test-id/status"));
        assert!(!is_proof_route("/bounties"));
        assert!(!is_proof_route("/submit"));
        assert!(!is_proof_route("/bounties//proof"));
    }
}
