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

use std::io::{BufReader, Write};
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
use boole_mcp::{
    build_in_process_mining_deps, handle_jsonrpc_sync, mcp_tools_array, read_mcp_frame,
    write_mcp_frame, InProcessMiningInputs,
};
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
    /// Real MCP stdio transport: speak JSON-RPC 2.0 with Content-Length
    /// framing over stdin/stdout. This is the transport that MCP clients
    /// (Claude, Cursor, etc.) expect when launching `boole-mcp` as a
    /// subprocess. The HTTP `serve` subcommand is kept for direct use.
    Stdio {
        /// Upstream boole-node base URL for proxy tools (bounty.list,
        /// receipt.get). Optional; omit when only mining tools are needed.
        #[arg(long)]
        node_url: Option<String>,
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
        Command::Stdio { node_url } => run_stdio(node_url).await,
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

    // Use the stdio subcommand so MCP clients (Claude, Cursor, etc.) get the
    // real JSON-RPC 2.0 stdio transport instead of HTTP.  Pass --node-url so
    // the proxy tools (bounty.list, receipt.get) keep working.
    let entry = json!({
        "command": bin_str,
        "args": ["stdio", "--node-url", "http://127.0.0.1:8080"],
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
    // Use the shared tools array from lib so HTTP and stdio surfaces stay in
    // sync. The HTTP route keeps both `input_schema` (snake, existing contract)
    // and `inputSchema` (camel, MCP spec) in the response.
    let body = json!({ "tools": mcp_tools_array() });
    (StatusCode::OK, Json(body))
}

/// Typed result from the shared tool dispatcher.
enum ToolResult {
    Ok(Value),
    /// HTTP 400 — bad request (missing arg, unknown tool).
    BadRequest(Value),
    /// HTTP 502 — upstream unreachable (proxy tools only).
    BadGateway(Value),
}

/// Shared async tool dispatcher used by both the HTTP `invoke` handler
/// and the stdio `tools/call` handler.
///
/// Stateful operations (boole.mine, boole.status) access `state` directly.
/// Proxy operations (bounty.list, receipt.get) use `state.client` +
/// `state.node_url`.
async fn dispatch_tool(state: &AppState, tool: &str, args: &Value) -> ToolResult {
    match tool {
        "bounty.list" => match proxy_get(state, "/work").await {
            (StatusCode::OK, Json(v)) => ToolResult::Ok(v),
            (StatusCode::BAD_GATEWAY, Json(v)) => ToolResult::BadGateway(v),
            (_, Json(v)) => ToolResult::BadRequest(v),
        },
        "receipt.get" => match args.get("receipt_id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => {
                let path = format!("/receipts/{id}");
                match proxy_get(state, &path).await {
                    (StatusCode::OK, Json(v)) => ToolResult::Ok(v),
                    (StatusCode::BAD_GATEWAY, Json(v)) => ToolResult::BadGateway(v),
                    (_, Json(v)) => ToolResult::BadRequest(v),
                }
            }
            _ => ToolResult::BadRequest(json!({"error":"missing-arg","arg":"receipt_id"})),
        },
        "boole.status" => {
            let guard = state
                .last_mining_summary
                .lock()
                .expect("last_mining_summary mutex poisoned");
            let val = match guard.as_ref() {
                Some(outcome) => {
                    let p = &outcome.protocol;
                    let a = &outcome.agent;
                    json!({
                        "state": "completed",
                        "last_summary": {
                            // Protocol counters
                            "cycles_run": p.cycles_run,
                            "tickets_found": p.tickets_found,
                            "verify_accepted": p.verify_accepted,
                            "verify_rejected": p.verify_rejected,
                            "shares_accepted": p.shares_accepted,
                            "network_errors": p.network_errors,
                            "canonicalize_errors": p.canonicalize_errors,
                            "loop_class": p.loop_class,
                            // Agent runtime counters
                            "driver_answered": a.driver_answered,
                            "proof_intake_accepted": a.proof_intake_accepted,
                            "proof_intake_rejected": a.proof_intake_rejected,
                        }
                    })
                }
                None => json!({"state": "idle"}),
            };
            ToolResult::Ok(val)
        }
        "boole.mine" => {
            // Optional `max_cycles` (default 0 = zero-cycle plumbing smoke;
            // >= 1 drives a closed-local real round-trip through the in-process
            // bundle with a real v1-lenbound target emitter).
            let max_cycles = args.get("max_cycles").and_then(|v| v.as_u64()).unwrap_or(0);
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
            ToolResult::Ok(json!({
                // Protocol counters
                "cycles_run": p.cycles_run,
                "tickets_found": p.tickets_found,
                "verify_accepted": p.verify_accepted,
                "verify_rejected": p.verify_rejected,
                "shares_accepted": p.shares_accepted,
                "network_errors": p.network_errors,
                "canonicalize_errors": p.canonicalize_errors,
                "loop_class": p.loop_class,
                // Agent runtime counters (driver → ProofIntakeV1 pipeline)
                "driver_answered": a.driver_answered,
                "proof_intake_accepted": a.proof_intake_accepted,
                "proof_intake_rejected": a.proof_intake_rejected,
            }))
        }
        other => ToolResult::BadRequest(json!({"error":"unknown-tool","tool":other})),
    }
}

async fn invoke(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InvokeRequest>,
) -> (StatusCode, Json<Value>) {
    match dispatch_tool(&state, &req.tool, &req.args).await {
        ToolResult::Ok(v) => (StatusCode::OK, Json(v)),
        ToolResult::BadRequest(v) => (StatusCode::BAD_REQUEST, Json(v)),
        ToolResult::BadGateway(v) => (StatusCode::BAD_GATEWAY, Json(v)),
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
    // reproducible. E#1 note: the CLI `--deterministic-nonces` flag is
    // dev-tools-gated; setting the library field here is fine because
    // boole-mcp drives a closed-local in-process smoke, not a network
    // miner — no share leaves this process.
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
            difficulty_epoch: 0,
            mode: "static-calibrated".to_string(),
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

// ── S6: stdio subcommand ──────────────────────────────────────────────────

/// Wrap a tool result `Value` in the MCP `tools/call` content envelope.
fn tool_result_to_mcp_content(id: &Value, result: &ToolResult) -> String {
    let (text, is_error) = match result {
        ToolResult::Ok(v) => (
            serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
            false,
        ),
        ToolResult::BadRequest(v) => (
            serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
            true,
        ),
        ToolResult::BadGateway(v) => (
            serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
            true,
        ),
    };
    let resp = json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": text }],
            "isError": is_error
        }
    });
    resp.to_string()
}

/// Run the MCP stdio transport loop.
///
/// Reads Content-Length-framed JSON-RPC 2.0 messages from stdin, dispatches
/// them, and writes framed responses to stdout.  Stateless messages
/// (initialize, tools/list, unknown methods) are handled by the lib's
/// `handle_jsonrpc_sync`.  Stateful tool calls (boole.mine, boole.status,
/// bounty.list, receipt.get) are handled via `dispatch_tool` which has access
/// to `AppState`.
///
/// The loop exits cleanly on EOF (read_mcp_frame returns None).
///
/// Design: stdin reads are done via `tokio::task::spawn_blocking` because the
/// standard `BufRead` framing API is synchronous.  `boole.mine` already uses
/// `spawn_blocking` internally, so this fits the existing pattern and avoids
/// an async IO dependency for a protocol that is inherently sequential (one
/// request at a time on a single stdio pipe).
async fn run_stdio(node_url: Option<String>) -> Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .build()?;
    let state = Arc::new(AppState {
        node_url: node_url
            .as_deref()
            .unwrap_or("http://127.0.0.1:8080")
            .trim_end_matches('/')
            .to_string(),
        client,
        last_mining_summary: Mutex::new(None),
    });

    // Wrap stdin in a BufReader inside a Mutex so it can be sent across
    // spawn_blocking calls.  Each iteration reads exactly one frame.
    let stdin = Arc::new(Mutex::new(BufReader::new(std::io::stdin())));
    let stdout = Arc::new(Mutex::new(std::io::stdout()));

    loop {
        // Read one frame (blocking).
        let stdin_clone = Arc::clone(&stdin);
        let frame_result = tokio::task::spawn_blocking(move || {
            let mut guard = stdin_clone.lock().expect("stdin mutex poisoned");
            read_mcp_frame(&mut *guard)
        })
        .await
        .expect("stdin reader task panicked");

        let msg = match frame_result? {
            Some(s) => s,
            None => {
                // Clean EOF — MCP client closed stdin.
                break;
            }
        };

        // Try the stateless handler first (initialize, tools/list, unknown
        // methods, notifications).
        let req_val: Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(_) => {
                // Malformed JSON — handle_jsonrpc_sync will produce the
                // -32700 error object.
                if let Some(resp_str) = handle_jsonrpc_sync(&msg) {
                    let stdout_clone = Arc::clone(&stdout);
                    tokio::task::spawn_blocking(move || {
                        let mut out = stdout_clone.lock().expect("stdout mutex poisoned");
                        write_mcp_frame(&mut *out, &resp_str).ok();
                        out.flush().ok();
                    })
                    .await
                    .expect("stdout writer task panicked");
                }
                continue;
            }
        };

        let method = req_val.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = req_val.get("id").cloned().unwrap_or(Value::Null);

        // Stateful tools/call goes through dispatch_tool.
        if method == "tools/call" {
            let params = req_val.get("params").cloned().unwrap_or(json!({}));
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            let result = dispatch_tool(&state, tool_name, &arguments).await;
            let resp_str = tool_result_to_mcp_content(&id, &result);
            let stdout_clone = Arc::clone(&stdout);
            tokio::task::spawn_blocking(move || {
                let mut out = stdout_clone.lock().expect("stdout mutex poisoned");
                write_mcp_frame(&mut *out, &resp_str).ok();
                out.flush().ok();
            })
            .await
            .expect("stdout writer task panicked");
            continue;
        }

        // All other methods go through the stateless handler.
        if let Some(resp_str) = handle_jsonrpc_sync(&msg) {
            let stdout_clone = Arc::clone(&stdout);
            tokio::task::spawn_blocking(move || {
                let mut out = stdout_clone.lock().expect("stdout mutex poisoned");
                write_mcp_frame(&mut *out, &resp_str).ok();
                out.flush().ok();
            })
            .await
            .expect("stdout writer task panicked");
        }
        // Notifications produce None — no frame to write.
    }

    Ok(())
}
