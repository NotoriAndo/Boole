//! P2.1 / P2.2 — `boole-mcp` binary. Carves out the Model Context
//! Protocol server crate and (P2.2) wires the first two read-only
//! tools that proxy to the upstream boole-node:
//!
//!   * `bounty.list`  -> upstream GET /work
//!   * `receipt.get`  -> upstream GET /receipts/{receipt_id}
//!
//! Surface:
//!   * `serve --node-url <url> --listen <host:port>`
//!   * resolved bind address echoed to stderr as
//!     `boole-mcp listening on http://<addr>` so the launcher can grab
//!     the ephemeral port when `:0` is requested
//!   * GET  /healthz       -> 200 `boole-mcp.v0`
//!   * GET  /mcp/tools     -> 200 JSON tool registry
//!   * POST /mcp/invoke    -> 200 with raw upstream JSON, or typed
//!     4xx/5xx error envelope (see below)
//!
//! P2.1 (slice 51+) adds two mining-side tools that **do not proxy** to
//! the upstream node; the round-trip runs in-process via the
//! `boole-mcp` lib's `InProcessChainHead`/`InProcessSubmitter` impls.
//!
//!   * `boole.mine`   -> drives a zero-cycle in-process round-trip
//!     through `run_mining_loop` and returns the `ProtocolReport`
//!     counters (cycles_run, tickets_found, shares_accepted,
//!     network_errors). Slice 54 also stores the summary into
//!     AppState so subsequent `boole.status` calls reflect it.
//!   * `boole.status` -> reports current mining session state. Returns
//!     200 `{"state":"idle"}` before any session has run, or
//!     `{"state":"completed","last_summary":{...}}` once `boole.mine`
//!     has executed in this process (slice 54).
//!
//! Typed error shapes (always JSON):
//!   * unknown tool        -> 400 {"error":"unknown-tool","tool":"<name>"}
//!   * missing required arg-> 400 {"error":"missing-arg","arg":"<name>"}
//!   * upstream unreachable-> 502 {"error":"upstream-unreachable"}
//!   * not-implemented     -> 501 {"error":"not-implemented","tool":"<name>"}
//!
//! No signing, no key material, no mutation routes -- this is a
//! read-only proxy. The mutation/wallet surface lives in the signed
//! boole-cli / boole-wallet-agent path, not here.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use clap::{Parser, Subcommand, ValueEnum};
use num_bigint::BigUint;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tokio::net::TcpListener;

use boole_core::Hex32;
use boole_mcp::{build_in_process_mining_deps, InProcessMiningInputs};
use boole_miner::{
    run_mining_loop, AnnounceTicketResult, ChainHead, FamilyV1LengthBoundTargetEmitter,
    GenerateResult, MiningLoopOptions, MiningLoopOutcome, ProverDriver, RejectingVerifier,
    Strategy, StructuralCanonicalizer, SubmitResult, VerifyReason,
};

/// `boole-mcp --version` text. Captured at build time by `build.rs`
/// so an operator can pin down exactly which binary is registered into
/// their IDE config (`mcpServers.boole.command`).
const VERSION_STRING: &str = concat!(
    "boole-mcp ",
    env!("CARGO_PKG_VERSION"),
    " (sha=",
    env!("BOOLE_MCP_GIT_SHA"),
    " build=",
    env!("BOOLE_MCP_BUILD_UTC"),
    ")",
);

#[derive(Parser)]
#[command(name = "boole-mcp", about = "Boole MCP server", version = VERSION_STRING)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        node_url: String,
        #[arg(long, default_value = "127.0.0.1:0")]
        listen: String,
    },
    /// P2.2 — register `boole-mcp` as an MCP server in the target IDE's
    /// settings file. Idempotent merge: re-running this is a no-op when
    /// the entry is already current; other settings are preserved.
    Install {
        #[arg(long, value_enum)]
        target: IdeTarget,
        /// Show the planned settings JSON on stdout without writing.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum IdeTarget {
    Claude,
    Codex,
    Cursor,
    Opencode,
}

impl IdeTarget {
    /// Path of the IDE's settings file relative to `$HOME`. The
    /// install handler creates intermediate directories.
    fn settings_rel_path(&self) -> &'static [&'static str] {
        match self {
            IdeTarget::Claude => &[".claude", "settings.json"],
            IdeTarget::Codex => &[".codex", "config.json"],
            IdeTarget::Cursor => &[".cursor", "mcp.json"],
            IdeTarget::Opencode => &[".config", "opencode", "config.json"],
        }
    }

    fn slug(&self) -> &'static str {
        match self {
            IdeTarget::Claude => "claude",
            IdeTarget::Codex => "codex",
            IdeTarget::Cursor => "cursor",
            IdeTarget::Opencode => "opencode",
        }
    }
}

struct AppState {
    node_url: String,
    client: reqwest::Client,
    /// P2.1 slice 54 — last `boole.mine` outcome so `boole.status` can
    /// report `completed` with the full honest counter set (protocol +
    /// agent runtime) instead of `idle` after a session has run in this
    /// process. `None` before any mine call; replaced wholesale on each
    /// successful invocation.
    last_mining_summary: Mutex<Option<MiningLoopOutcome>>,
}

#[derive(Deserialize)]
struct InvokeRequest {
    tool: String,
    #[serde(default)]
    args: Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    // P0.5 slice 65 — install the telemetry subscriber before any work so
    // the MCP server's events are observable. Default-silent unless RUST_LOG
    // opts in.
    boole_core::telemetry::init(boole_core::telemetry::BinaryName::Mcp);
    let cli = Cli::parse();
    match cli.command {
        Command::Serve { node_url, listen } => serve(&node_url, &listen).await,
        Command::Install { target, dry_run } => run_install(target, dry_run),
    }
}

fn install_envelope_ok(result: Value) -> String {
    let envelope = json!({
        "ok": true,
        "version": "v1",
        "command": "install",
        "result": result,
    });
    serde_json::to_string(&envelope).expect("install envelope serializes")
}

fn install_envelope_err(reason: &str, extras: Value) -> String {
    let mut error = Map::new();
    error.insert("reason".to_string(), Value::String(reason.to_string()));
    if let Value::Object(map) = extras {
        for (k, v) in map {
            if k == "reason" {
                continue;
            }
            error.insert(k, v);
        }
    }
    let envelope = json!({
        "ok": false,
        "version": "v1",
        "command": "install",
        "error": Value::Object(error),
    });
    serde_json::to_string(&envelope).expect("install envelope serializes")
}

fn run_install(target: IdeTarget, dry_run: bool) -> Result<()> {
    let home = match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h),
        None => {
            eprintln!("{}", install_envelope_err("home-not-set", Value::Null));
            std::process::exit(1);
        }
    };
    let mut settings_path = home;
    for seg in target.settings_rel_path() {
        settings_path.push(seg);
    }
    let bin = std::env::current_exe()
        .context("resolve current executable for mcpServers.boole.command")?;
    let bin_str = bin.to_string_lossy().to_string();

    // Read existing settings JSON, treating missing/empty as {}. Any
    // parse error surfaces a typed envelope on stderr so the operator
    // can repair the file by hand rather than silently overwrite it.
    let mut settings: Value = if settings_path.exists() {
        let txt = std::fs::read_to_string(&settings_path)
            .with_context(|| format!("read {}", settings_path.display()))?;
        if txt.trim().is_empty() {
            json!({})
        } else {
            match serde_json::from_str::<Value>(&txt) {
                Ok(v) if v.is_object() => v,
                Ok(_) => {
                    eprintln!(
                        "{}",
                        install_envelope_err(
                            "settings-not-object",
                            json!({"settings_path": settings_path.to_string_lossy()})
                        )
                    );
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!(
                        "{}",
                        install_envelope_err(
                            "settings-parse-failed",
                            json!({
                                "settings_path": settings_path.to_string_lossy(),
                                "detail": e.to_string(),
                            })
                        )
                    );
                    std::process::exit(1);
                }
            }
        }
    } else {
        json!({})
    };

    let entry = json!({
        "command": bin_str,
        "args": ["serve", "--node-url", "http://127.0.0.1:8080"],
    });

    let root = settings
        .as_object_mut()
        .expect("settings root is object (checked above)");
    let mcp_servers = root
        .entry("mcpServers".to_string())
        .or_insert_with(|| json!({}));
    if !mcp_servers.is_object() {
        eprintln!(
            "{}",
            install_envelope_err(
                "mcp-servers-not-object",
                json!({"settings_path": settings_path.to_string_lossy()})
            )
        );
        std::process::exit(1);
    }
    let mcp_obj = mcp_servers
        .as_object_mut()
        .expect("mcpServers is object (checked above)");
    mcp_obj.insert("boole".to_string(), entry);

    let serialized =
        serde_json::to_string_pretty(&settings).context("serialize updated settings")?;

    if dry_run {
        println!(
            "{}",
            install_envelope_ok(json!({
                "dry_run": true,
                "target": target.slug(),
                "settings_path": settings_path.to_string_lossy(),
                "planned_content": settings,
            }))
        );
        return Ok(());
    }

    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }

    // Atomic write: stage to a sibling .tmp then rename, so a crash
    // mid-write cannot leave the operator's IDE config truncated.
    let tmp_path = settings_path.with_extension("json.tmp");
    std::fs::write(&tmp_path, serialized.as_bytes())
        .with_context(|| format!("write {}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, &settings_path)
        .with_context(|| format!("rename into {}", settings_path.display()))?;

    println!(
        "{}",
        install_envelope_ok(json!({
            "dry_run": false,
            "target": target.slug(),
            "settings_path": settings_path.to_string_lossy(),
        }))
    );
    Ok(())
}

async fn serve(node_url: &str, listen: &str) -> Result<()> {
    let listener = TcpListener::bind(listen).await?;
    let addr: SocketAddr = listener.local_addr()?;
    eprintln!("boole-mcp listening on http://{addr}");
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .build()?;
    let state = Arc::new(AppState {
        node_url: node_url.trim_end_matches('/').to_string(),
        client,
        last_mining_summary: Mutex::new(None),
    });
    let app = build_router(state);
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/mcp/tools", get(tools_list))
        .route("/mcp/invoke", post(invoke))
        .fallback(not_found)
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "boole-mcp.v0")
}

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "")
}

async fn tools_list() -> impl IntoResponse {
    let body = json!({
        "tools": [
            {
                "name": "bounty.list",
                "description": "List currently-open bounties/work units from the upstream boole-node.",
                "input_schema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            },
            {
                "name": "receipt.get",
                "description": "Fetch a single proof receipt by id from the upstream boole-node.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "receipt_id": { "type": "string" }
                    },
                    "required": ["receipt_id"],
                    "additionalProperties": false
                }
            },
            {
                "name": "boole.mine",
                "description": "Drive a closed-local mining round-trip in-process: real v1-lenbound instance generation → deterministic driver stand-in → ProofIntakeV1 → Canonicalizer → RejectingVerifier. Optional `max_cycles` (default 0 = zero-cycle plumbing smoke; >=1 runs that many full closed-local cycles). Returns honest pipeline counters: verify_accepted=0 is correct (no Lean toolchain). Not public mining; not a solve claim.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "max_cycles": {
                            "type": "integer",
                            "minimum": 0,
                            "description": "Number of ticket cycles to run (default 0)."
                        }
                    },
                    "additionalProperties": false
                }
            },
            {
                "name": "boole.status",
                "description": "Report current in-process mining session state (idle, in-progress, last summary). Pure read.",
                "input_schema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            }
        ]
    });
    (StatusCode::OK, Json(body))
}

async fn invoke(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InvokeRequest>,
) -> (StatusCode, Json<Value>) {
    match req.tool.as_str() {
        "bounty.list" => proxy_get(&state, "/work").await,
        "receipt.get" => match req.args.get("receipt_id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => {
                let path = format!("/receipts/{id}");
                proxy_get(&state, &path).await
            }
            _ => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"missing-arg","arg":"receipt_id"})),
            ),
        },
        "boole.status" => {
            let guard = state
                .last_mining_summary
                .lock()
                .expect("last_mining_summary mutex poisoned");
            match guard.as_ref() {
                Some(outcome) => {
                    let p = &outcome.protocol;
                    let a = &outcome.agent;
                    (
                        StatusCode::OK,
                        Json(json!({
                            "state": "completed",
                            "last_summary": {
                                // Protocol counters
                                "cycles_run": p.cycles_run,
                                "tickets_found": p.tickets_found,
                                "verify_accepted": p.verify_accepted,
                                "verify_rejected": p.verify_rejected,
                                "shares_accepted": p.shares_accepted,
                                "network_errors": p.network_errors,
                                "loop_class": p.loop_class,
                                // Agent runtime counters
                                "driver_answered": a.driver_answered,
                                "proof_intake_accepted": a.proof_intake_accepted,
                                "proof_intake_rejected": a.proof_intake_rejected,
                            }
                        })),
                    )
                }
                None => (StatusCode::OK, Json(json!({"state": "idle"}))),
            }
        }
        "boole.mine" => {
            // Optional `max_cycles` (default 0 = zero-cycle plumbing smoke;
            // >= 1 drives a closed-local real round-trip through the in-process
            // bundle with a real v1-lenbound target emitter).
            let max_cycles = req
                .args
                .get("max_cycles")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let outcome = tokio::task::spawn_blocking(move || run_mining_summary(max_cycles))
                .await
                .expect("mining task panicked");
            {
                let mut guard = state
                    .last_mining_summary
                    .lock()
                    .expect("last_mining_summary mutex poisoned");
                *guard = Some(outcome.clone());
            }
            let p = &outcome.protocol;
            let a = &outcome.agent;
            (
                StatusCode::OK,
                Json(json!({
                    // Protocol counters
                    "cycles_run": p.cycles_run,
                    "tickets_found": p.tickets_found,
                    "verify_accepted": p.verify_accepted,
                    "verify_rejected": p.verify_rejected,
                    "shares_accepted": p.shares_accepted,
                    "network_errors": p.network_errors,
                    "loop_class": p.loop_class,
                    // Agent runtime counters (driver → ProofIntakeV1 pipeline)
                    "driver_answered": a.driver_answered,
                    "proof_intake_accepted": a.proof_intake_accepted,
                    "proof_intake_rejected": a.proof_intake_rejected,
                })),
            )
        }
        other => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"unknown-tool","tool":other})),
        ),
    }
}

/// Deterministic closed-local prover stand-in.
///
/// This is NOT an LLM, NOT a network call, and NOT a Lean-verified solver.
/// It returns a fixed intake-valid term-mode answer so the closed-local smoke
/// can traverse the full pipeline (emitter → driver → ProofIntakeV1 →
/// Canonicalizer → RejectingVerifier) without any external dependencies.
///
/// The answer `fun xs => nodup_dedup _` passes ProofIntakeV1 because:
///   - first token is `fun` (not a bare tactic keyword)
///   - no backticks, `sorry`, or `admit`
///
/// verify_accepted will always be 0 with `RejectingVerifier` — that is
/// CORRECT and EXPECTED for a closed-local smoke.
struct CanonicalProofDriver;

impl ProverDriver for CanonicalProofDriver {
    fn name(&self) -> &str {
        "canonical-proof-stand-in"
    }

    fn strategy(&self) -> Strategy {
        Strategy::Frontier
    }

    fn generate(&self, _prompt: &str) -> GenerateResult {
        GenerateResult::Answered {
            answer: "fun xs => nodup_dedup _".to_string(),
            elapsed: Duration::from_millis(0),
            tokens_used: None,
        }
    }
}

/// Drive the in-process bundle through `run_mining_loop` for `max_cycles`
/// ticket cycles.
///
/// A `max_cycles` of zero short-circuits the loop body (zero-cycle plumbing
/// smoke); a value of one or more drives that many closed-local round-trips
/// with a real v1-lenbound target emitter and the `CanonicalProofDriver`
/// stand-in. The `RejectingVerifier` means the cycle completes with
/// verify_rejected >= 1 and verify_accepted == 0 — correct for a closed-local
/// smoke with no Lean toolchain.
///
/// Returns the full `MiningLoopOutcome` (protocol + agent counters) so the
/// caller can surface the honest pipeline boundary to the MCP client.
fn run_mining_summary(max_cycles: u64) -> MiningLoopOutcome {
    let bundle = build_in_process_mining_deps(default_in_process_inputs());
    // Bound every grinder so a >0-cycle fixture run terminates promptly.
    // The cycle still completes (and `cycles_run` increments) whether or
    // not a share target is hit. Deterministic nonces keep the outcome
    // reproducible.
    let bounded = boole_miner::GrinderConfig {
        max_attempts: Some(4096),
        ..Default::default()
    };
    let opts = MiningLoopOptions {
        max_cycles: Some(max_cycles),
        deterministic_nonces: true,
        ticket_grind: bounded,
        share_grind: bounded,
        submit_grind: bounded,
        ..Default::default()
    };
    run_mining_loop(bundle.deps, opts)
}

/// In-process inputs for the closed-local mining smoke.
///
/// Uses `FamilyV1LengthBoundTargetEmitter` (real v1-lenbound instance
/// generation) and `CanonicalProofDriver` (deterministic intake-valid answer,
/// no LLM or network). ChainHead thresholds are all-ones so the ticket grind
/// succeeds deterministically on the first attempt, giving `tickets_found >= 1`
/// on any >0-cycle run.
///
/// `RejectingVerifier` and `StructuralCanonicalizer` are kept so CI never
/// requires a Lean toolchain; `verify_accepted == 0` is correct and expected.
fn default_in_process_inputs() -> InProcessMiningInputs {
    // All-ones thresholds: difficulty_weight((1<<256)-1) = 1, which satisfies
    // has_open_thresholds → loop_class = "smoke".  The ticket grind succeeds
    // on the first deterministic nonce attempt, so tickets_found >= 1 for any
    // >0-cycle run without an expensive PoW search.
    let all_ones = BigUint::from_bytes_be(&[0xffu8; 32]);
    InProcessMiningInputs {
        pk: Hex32::from_bytes([0u8; 32]),
        head: ChainHead {
            c: Hex32::from_bytes([0u8; 32]),
            t_ticket: all_ones.clone(),
            t_share: all_ones.clone(),
            t_block: all_ones.clone(),
            t_submit: all_ones,
            min_share_score: BigUint::from(1u32),
            m: 7,
            d: 11,
            profile: "v1-lenbound".to_string(),
            n: Some(3),
        },
        announce_result: AnnounceTicketResult::Observed {
            hash_hex: "0xticket".to_string(),
        },
        submit_result: SubmitResult::Accepted {
            share_hash_hex: "0xshare".to_string(),
        },
        emitter: Box::new(FamilyV1LengthBoundTargetEmitter::new()),
        driver: Box::new(CanonicalProofDriver),
        verifier: Box::new(RejectingVerifier::new(VerifyReason::ElaborateFailed)),
        canonicalizer: Box::new(StructuralCanonicalizer),
    }
}

async fn proxy_get(state: &AppState, path: &str) -> (StatusCode, Json<Value>) {
    let url = format!("{}{}", state.node_url, path);
    match state.client.get(&url).send().await {
        Ok(resp) => {
            let status = resp.status();
            match resp.text().await {
                Ok(body) => {
                    let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                    let mapped =
                        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
                    (mapped, Json(parsed))
                }
                Err(_) => (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"error":"upstream-unreachable"})),
                ),
            }
        }
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error":"upstream-unreachable"})),
        ),
    }
}
