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
//!   * `boole.mine`   -> drives a fixture mining round-trip (slice 53+).
//!     Currently answers `not-implemented`.
//!   * `boole.status` -> reports current mining session state. With no
//!     session ever started, returns 200 `{"state":"idle"}` (slice 52).
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
use std::sync::Arc;
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
    run_mining_loop, AnnounceTicketResult, ChainHead, MiningLoopOptions, MockDriver, MockResponse,
    ProtocolReport, RejectingVerifier, StructuralCanonicalizer, StubTargetEmitter, SubmitResult,
    VerifyReason,
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

#[derive(Clone)]
struct AppState {
    node_url: String,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct InvokeRequest {
    tool: String,
    #[serde(default)]
    args: Value,
}

#[tokio::main]
async fn main() -> Result<()> {
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
                "description": "Drive a fixture mining round-trip in-process (no HTTP loopback to boole-node). Returns the mining loop summary once the in-process runtime wiring lands.",
                "input_schema": {
                    "type": "object",
                    "properties": {},
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
        "boole.status" => (StatusCode::OK, Json(json!({"state": "idle"}))),
        "boole.mine" => {
            let protocol = tokio::task::spawn_blocking(run_zero_cycle_mining_summary)
                .await
                .expect("zero-cycle mining task panicked");
            (
                StatusCode::OK,
                Json(json!({
                    "cycles_run": protocol.cycles_run,
                    "tickets_found": protocol.tickets_found,
                    "shares_accepted": protocol.shares_accepted,
                    "network_errors": protocol.network_errors,
                })),
            )
        }
        other => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"unknown-tool","tool":other})),
        ),
    }
}

/// P2.1 slice 53 — drive the slice 49 in-process bundle through
/// `run_mining_loop` with `max_cycles: Some(0)`. The body short-circuits
/// before any driver / verifier / Lean work, so this exercises the full
/// MCP -> `MiningLoopDeps` -> `run_mining_loop` plumbing end-to-end
/// without paying any mining cost. Returns just the protocol counters
/// because that's what the slice 53 envelope contract exposes; the
/// agent-side report rides on slice 54+.
fn run_zero_cycle_mining_summary() -> ProtocolReport {
    let bundle = build_in_process_mining_deps(default_in_process_inputs());
    let opts = MiningLoopOptions {
        max_cycles: Some(0),
        ..Default::default()
    };
    run_mining_loop(bundle.deps, opts).protocol
}

/// Fixture `InProcessMiningInputs` for the slice 53 zero-cycle smoke.
/// Field values match the slice 49 / 50 in-process tests so deps shape
/// stays in sync; nothing in here is reachable when `max_cycles == 0`.
fn default_in_process_inputs() -> InProcessMiningInputs {
    InProcessMiningInputs {
        pk: Hex32::from_bytes([0u8; 32]),
        head: ChainHead {
            c: Hex32::from_bytes([0u8; 32]),
            t_ticket: BigUint::from(1u32) << 240,
            t_share: BigUint::from(1u32) << 232,
            t_block: BigUint::from(1u32) << 224,
            t_submit: BigUint::from(1u32) << 248,
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
        emitter: Box::new(StubTargetEmitter::new("stub")),
        driver: Box::new(MockDriver::new(vec![MockResponse::Text(
            "fun xs => xs".to_string(),
        )])),
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
