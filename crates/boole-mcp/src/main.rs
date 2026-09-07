//! P2.1 / P2.2 — `boole-mcp` binary. Carves out the Model Context
//! Protocol server crate and (P2.2) wires the first two read-only
//! tools that proxy to the upstream boole-node:
//!
//!   * `bounty.list`  -> upstream GET /work
//!   * `receipt.get`  -> upstream GET /receipts/{receipt_id}
//!   * `boole.verify_native` -> upstream POST /native-shadow/submissions on a
//!     separately configured numeric-loopback-only native service URL
//!
//! Surface:
//!   * `serve --node-url <url> --native-shadow-url <loopback-url>
//!     --listen <numeric-loopback:port>`; the native bridge refuses a remote,
//!     wildcard or hostname listener before bind
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
//!   * native transport unknown -> 502 with no invented verdict; manually
//!     resubmit the identical six fields to recover any durable redelivery
//!   * not-implemented     -> 501 {"error":"not-implemented","tool":"<name>"}
//!
//! No signing or key material lives here. Native submission may consume one
//! node-owned challenge and execute its checker, but only through the separate
//! loopback native service; wallet, payment, block and reward mutation remain
//! outside this MCP process.

use std::fs::{File, OpenOptions};
use std::io::{BufReader, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

use anyhow::{Context, Result};
use axum::{
    body::{Body, Bytes},
    extract::{rejection::JsonRejection, FromRequest, Request, State},
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
use toml_edit::{value as toml_value, DocumentMut, Item, Table, Value as TomlValue};

use boole_core::Hex32;
use boole_mcp::{
    build_in_process_mining_deps, handle_jsonrpc_sync, mcp_tools_array, read_mcp_frame,
    write_mcp_frame, InProcessMiningInputs, NATIVE_VERIFIER_RESPONSE_MAX_BYTES,
    NATIVE_VERIFIER_TIMEOUT_SECS,
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
        /// Separate closed-local native verifier URL. Hostnames, redirects,
        /// proxies and non-loopback addresses are refused.
        #[arg(long)]
        native_shadow_url: Option<String>,
        /// HTTP MCP bind address. When the native bridge is configured this
        /// must be a numeric loopback SocketAddr; legacy-only serve retains
        /// its historical listener behavior.
        #[arg(long, default_value = "127.0.0.1:0")]
        listen: String,
    },
    /// Real MCP stdio transport: speak newline-delimited JSON-RPC 2.0
    /// over stdin/stdout. This is the transport that MCP clients
    /// (Claude, Cursor, etc.) expect when launching `boole-mcp` as a
    /// subprocess. The HTTP `serve` subcommand is kept for direct use.
    Stdio {
        /// Upstream boole-node base URL for proxy tools (bounty.list,
        /// receipt.get). Optional; omit when only mining tools are needed.
        #[arg(long)]
        node_url: Option<String>,
        /// Separate closed-local native verifier URL.
        #[arg(long)]
        native_shadow_url: Option<String>,
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
            IdeTarget::Claude => &[".claude.json"],
            IdeTarget::Codex => &[".codex", "config.toml"],
            IdeTarget::Cursor => &[".cursor", "mcp.json"],
            IdeTarget::Opencode => &[".config", "opencode", "opencode.json"],
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
    native_shadow_url: Option<String>,
    native_client: reqwest::Client,
    /// P2.1 slice 54 — last `boole.mine` outcome so `boole.status` can
    /// report `completed` with the full honest counter set (protocol +
    /// agent runtime) instead of `idle` after a session has run in this
    /// process. `None` before any mine call; replaced wholesale on each
    /// successful invocation.
    last_mining_summary: Mutex<Option<MiningLoopOutcome>>,
    /// The stdio transport permits one in-process mine at a time.  Keeping
    /// the cancellation token in shared state lets the reader remain
    /// responsive while the blocking mining loop is running elsewhere.
    active_mining: Mutex<Option<ActiveMiningRequest>>,
}

struct ActiveMiningRequest {
    request_id: Value,
    cancel: Arc<AtomicBool>,
    completed: Arc<tokio::sync::Notify>,
}

const MAX_MCP_MINING_CYCLES: u64 = 100_000;
const MCP_MINING_DEADLINE: Duration = Duration::from_secs(30);
const MCP_MINING_EOF_GRACE: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
struct InvokeRequest {
    tool: String,
    #[serde(default)]
    args: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeSubmissionArguments {
    #[serde(rename = "schema")]
    _schema: String,
    #[serde(rename = "familyVersion")]
    _family_version: String,
    #[serde(rename = "templateId")]
    _template_id: String,
    #[serde(rename = "challengeSha256")]
    _challenge_sha256: String,
    epoch: serde_json::Number,
    #[serde(rename = "rawAnswer")]
    _raw_answer: String,
}

impl NativeSubmissionArguments {
    fn epoch_is_integer(&self) -> bool {
        self.epoch.is_i64() || self.epoch.is_u64()
    }
}

#[derive(Deserialize)]
struct NativeHttpInvocation {
    tool: String,
    args: NativeSubmissionArguments,
}

#[derive(Deserialize)]
struct NativeHttpProbe {
    tool: String,
}

#[derive(Deserialize)]
struct NativeStdioInvocation {
    method: String,
    params: NativeStdioParams,
}

#[derive(Deserialize)]
struct NativeStdioProbe {
    method: String,
    params: NativeStdioProbeParams,
}

#[derive(Deserialize)]
struct NativeStdioProbeParams {
    name: String,
}

#[derive(Deserialize)]
struct NativeStdioEnvelopeProbe {
    jsonrpc: String,
    id: Value,
}

#[derive(Deserialize)]
struct NativeStdioParams {
    name: String,
    arguments: NativeSubmissionArguments,
}

#[derive(Clone, Copy)]
enum NativeInvocationTransport {
    Http,
    Stdio,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NativeArgumentsPrecheck {
    NotNative,
    Exact,
    Invalid,
}

/// Inspect a native invocation from its original bytes before serde_json can
/// collapse duplicate object keys into a `Value`. This is deliberately scoped
/// to `boole.verify_native`; legacy tools retain their historical JSON parsing.
fn precheck_raw_native_arguments(
    raw: &[u8],
    transport: NativeInvocationTransport,
) -> NativeArgumentsPrecheck {
    match transport {
        NativeInvocationTransport::Http => {
            let targets_native = serde_json::from_slice::<NativeHttpProbe>(raw)
                .is_ok_and(|request| request.tool == "boole.verify_native");
            if !targets_native {
                return NativeArgumentsPrecheck::NotNative;
            }
            if serde_json::from_slice::<NativeHttpInvocation>(raw).is_ok_and(|request| {
                request.tool == "boole.verify_native" && request.args.epoch_is_integer()
            }) {
                NativeArgumentsPrecheck::Exact
            } else {
                NativeArgumentsPrecheck::Invalid
            }
        }
        NativeInvocationTransport::Stdio => {
            let targets_native =
                serde_json::from_slice::<NativeStdioProbe>(raw).is_ok_and(|request| {
                    request.method == "tools/call" && request.params.name == "boole.verify_native"
                });
            if !targets_native {
                return NativeArgumentsPrecheck::NotNative;
            }
            if serde_json::from_slice::<NativeStdioInvocation>(raw).is_ok_and(|request| {
                request.method == "tools/call"
                    && request.params.name == "boole.verify_native"
                    && request.params.arguments.epoch_is_integer()
            }) {
                NativeArgumentsPrecheck::Exact
            } else {
                NativeArgumentsPrecheck::Invalid
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // P0.5 slice 65 — install the telemetry subscriber before any work so
    // the MCP server's events are observable. Default-silent unless RUST_LOG
    // opts in.
    boole_core::telemetry::init(boole_core::telemetry::BinaryName::Mcp);
    let cli = Cli::parse();
    match cli.command {
        Command::Serve {
            node_url,
            native_shadow_url,
            listen,
        } => serve(&node_url, native_shadow_url.as_deref(), &listen).await,
        Command::Stdio {
            node_url,
            native_shadow_url,
        } => run_stdio(node_url, native_shadow_url).await,
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

fn stdio_entry() -> Value {
    let bin = std::env::current_exe()
        .expect("resolve current executable for MCP server registration")
        .to_string_lossy()
        .to_string();
    json!({
        "command": bin,
        "args": [
            "stdio",
            "--node-url",
            "http://127.0.0.1:8080",
            "--native-shadow-url",
            "http://127.0.0.1:8082"
        ],
    })
}

fn inline_table_as_table(inline: &toml_edit::InlineTable) -> Table {
    let mut table = Table::new();
    for (key, value) in inline.iter() {
        table.insert(key, Item::Value(value.clone()));
    }
    table
}

fn ensure_table<'a>(item: &'a mut Item, name: &str) -> Result<&'a mut Table> {
    if item.is_none() {
        *item = Item::Table(Table::new());
    } else if let Some(inline) = item.as_inline_table() {
        *item = Item::Table(inline_table_as_table(inline));
    }
    item.as_table_mut()
        .with_context(|| format!("{name} must be a TOML table"))
}

fn merge_codex_config(existing: &str, entry: &Value) -> Result<String> {
    let mut document = existing
        .parse::<DocumentMut>()
        .map_err(|_| anyhow::anyhow!("Codex configuration could not be parsed"))?;
    let root = document.as_table_mut();
    let mcp_servers = ensure_table(
        root.entry("mcp_servers").or_insert(Item::None),
        "mcp_servers",
    )?;
    let boole = ensure_table(
        mcp_servers.entry("boole").or_insert(Item::None),
        "mcp_servers.boole",
    )?;
    let command = entry["command"]
        .as_str()
        .expect("stdio command is a string");
    let args = entry["args"].as_array().expect("stdio args are an array");
    let mut args_toml = toml_edit::Array::new();
    for argument in args {
        args_toml.push(argument.as_str().expect("stdio arg is a string"));
    }
    boole["command"] = toml_value(command);
    boole["args"] = Item::Value(TomlValue::Array(args_toml));
    Ok(document.to_string())
}

static INSTALL_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn atomic_write_settings(path: &std::path::Path, content: &str) -> Result<()> {
    let parent = path.parent().context("settings path has no parent")?;
    std::fs::create_dir_all(parent).with_context(|| format!("mkdir -p {}", parent.display()))?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("settings path has no UTF-8 filename")?;

    #[cfg(unix)]
    let desired_mode = std::fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions().mode() & 0o7777)
        .unwrap_or(0o600);

    let mut temporary = None;
    for _ in 0..64 {
        let sequence = INSTALL_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".{filename}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        match options.open(&candidate) {
            Ok(file) => {
                temporary = Some((candidate, file));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("create {}", candidate.display()));
            }
        }
    }
    let (temporary_path, mut file) = temporary.context("create unique settings temporary file")?;
    let write_result = (|| -> Result<()> {
        file.write_all(content.as_bytes())
            .with_context(|| format!("write {}", temporary_path.display()))?;
        #[cfg(unix)]
        file.set_permissions(std::fs::Permissions::from_mode(desired_mode))
            .with_context(|| format!("set mode on {}", temporary_path.display()))?;
        file.sync_all()
            .with_context(|| format!("sync {}", temporary_path.display()))?;
        Ok(())
    })();
    drop(file);
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error);
    }
    if let Err(error) = std::fs::rename(&temporary_path, path) {
        let _ = std::fs::remove_file(&temporary_path);
        return Err(error).with_context(|| format!("rename into {}", path.display()));
    }
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .with_context(|| format!("sync parent directory {}", parent.display()))?;
    Ok(())
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
    let mut entry = stdio_entry();
    if matches!(target, IdeTarget::Claude) {
        entry["type"] = Value::String("stdio".to_string());
    }

    if matches!(target, IdeTarget::Codex) {
        let existing = if settings_path.exists() {
            std::fs::read_to_string(&settings_path)
                .with_context(|| format!("read {}", settings_path.display()))?
        } else {
            String::new()
        };
        let merged = match merge_codex_config(&existing, &entry) {
            Ok(merged) => merged,
            Err(_) => {
                eprintln!(
                    "{}",
                    install_envelope_err(
                        "codex-config-merge-failed",
                        json!({"settings_path": settings_path.to_string_lossy()})
                    )
                );
                std::process::exit(1);
            }
        };
        if dry_run {
            println!(
                "{}",
                install_envelope_ok(json!({
                    "dry_run": true,
                    "target": target.slug(),
                    "settings_path": settings_path.to_string_lossy(),
                    "planned_content": merged,
                }))
            );
            return Ok(());
        }
        atomic_write_settings(&settings_path, &merged)?;
        println!(
            "{}",
            install_envelope_ok(json!({
                "dry_run": false,
                "target": target.slug(),
                "settings_path": settings_path.to_string_lossy(),
            }))
        );
        return Ok(());
    }

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

    let root = settings
        .as_object_mut()
        .expect("settings root is object (checked above)");
    if matches!(target, IdeTarget::Opencode) {
        let mcp = root.entry("mcp".to_string()).or_insert_with(|| json!({}));
        if !mcp.is_object() {
            eprintln!(
                "{}",
                install_envelope_err(
                    "opencode-mcp-not-object",
                    json!({"settings_path": settings_path.to_string_lossy()})
                )
            );
            std::process::exit(1);
        }
        let command = entry["command"]
            .as_str()
            .expect("stdio command is a string");
        let args = entry["args"].as_array().expect("stdio args are an array");
        let command = std::iter::once(Value::String(command.to_string()))
            .chain(args.iter().cloned())
            .collect::<Vec<_>>();
        mcp.as_object_mut()
            .expect("mcp is object (checked above)")
            .insert(
                "boole".to_string(),
                json!({"type": "local", "command": command}),
            );
    } else {
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
        mcp_servers
            .as_object_mut()
            .expect("mcpServers is object (checked above)")
            .insert("boole".to_string(), entry);
    }

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

    atomic_write_settings(&settings_path, &serialized)?;

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

fn validate_native_shadow_url(raw: Option<&str>, node_url: &str) -> Result<Option<String>> {
    let Some(raw) = raw else { return Ok(None) };
    let parsed = reqwest::Url::parse(raw).context("parse --native-shadow-url")?;
    anyhow::ensure!(
        parsed.scheme() == "http",
        "--native-shadow-url must use http on numeric loopback"
    );
    let loopback = parsed
        .host_str()
        .and_then(|host| {
            let numeric_host = host
                .strip_prefix('[')
                .and_then(|host| host.strip_suffix(']'))
                .unwrap_or(host);
            numeric_host.parse::<std::net::IpAddr>().ok()
        })
        .is_some_and(|address| address.is_loopback());
    anyhow::ensure!(
        loopback,
        "--native-shadow-url must use a numeric loopback address"
    );
    anyhow::ensure!(
        parsed.username().is_empty() && parsed.password().is_none(),
        "--native-shadow-url must not contain credentials"
    );
    anyhow::ensure!(
        parsed.path() == "/" && parsed.query().is_none() && parsed.fragment().is_none(),
        "--native-shadow-url must be an origin without path, query or fragment"
    );
    if let Ok(node) = reqwest::Url::parse(node_url) {
        anyhow::ensure!(
            node.origin() != parsed.origin(),
            "--native-shadow-url and --node-url must use distinct origins"
        );
    }
    Ok(Some(raw.trim_end_matches('/').to_string()))
}

fn validate_native_serve_listen(native_shadow_url: Option<&str>, listen: &str) -> Result<()> {
    if native_shadow_url.is_none() {
        return Ok(());
    }
    let address = listen.parse::<SocketAddr>().with_context(|| {
        "when --native-shadow-url is configured, --listen must be a numeric loopback socket address"
    })?;
    anyhow::ensure!(
        address.ip().is_loopback(),
        "when --native-shadow-url is configured, --listen must use a numeric loopback address"
    );
    Ok(())
}

fn native_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(NATIVE_VERIFIER_TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .build()?)
}

async fn serve(node_url: &str, native_shadow_url: Option<&str>, listen: &str) -> Result<()> {
    let native_shadow_url = validate_native_shadow_url(native_shadow_url, node_url)?;
    validate_native_serve_listen(native_shadow_url.as_deref(), listen)?;
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
        native_shadow_url,
        native_client: native_client()?,
        last_mining_summary: Mutex::new(None),
        active_mining: Mutex::new(None),
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
    /// HTTP 500 — an in-process tool worker did not complete safely.
    Internal(Value),
    /// Preserve a native service status and JSON body without translating
    /// its adjudication vocabulary into the legacy MCP/node vocabulary.
    Native(StatusCode, Value),
}

#[derive(Debug, PartialEq, Eq)]
enum MiningRunError {
    DeadlineExceeded,
    WorkerFailed,
}

struct CancelMiningOnDrop(Arc<AtomicBool>);

impl Drop for CancelMiningOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

async fn run_mining_bounded(
    max_cycles: u64,
    cancel: Arc<AtomicBool>,
) -> Result<MiningLoopOutcome, MiningRunError> {
    run_mining_bounded_for(max_cycles, cancel, MCP_MINING_DEADLINE).await
}

async fn run_mining_bounded_for(
    max_cycles: u64,
    cancel: Arc<AtomicBool>,
    deadline: Duration,
) -> Result<MiningLoopOutcome, MiningRunError> {
    let loop_cancel = Arc::clone(&cancel);
    let mut worker = tokio::task::spawn_blocking(move || {
        run_mining_summary_with_cancel(max_cycles, Some(loop_cancel))
    });
    tokio::select! {
        completed = &mut worker => completed.map_err(|_| MiningRunError::WorkerFailed),
        _ = tokio::time::sleep(deadline) => {
            cancel.store(true, Ordering::SeqCst);
            let _ = worker.await;
            Err(MiningRunError::DeadlineExceeded)
        }
    }
}

fn mining_max_cycles(args: &Value) -> Result<u64, ToolResult> {
    let Some(object) = args.as_object() else {
        return Err(ToolResult::BadRequest(json!({
            "error": "invalid-arg",
            "arg": "max_cycles"
        })));
    };
    match object.get("max_cycles") {
        None => Ok(0),
        Some(value) => match value.as_u64() {
            Some(cycles) if cycles <= MAX_MCP_MINING_CYCLES => Ok(cycles),
            _ => Err(ToolResult::BadRequest(json!({
                "error": "invalid-arg",
                "arg": "max_cycles",
                "max": MAX_MCP_MINING_CYCLES
            }))),
        },
    }
}

fn mining_outcome_value(outcome: &MiningLoopOutcome) -> Value {
    let p = &outcome.protocol;
    let a = &outcome.agent;
    json!({
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
    })
}

/// Shared async tool dispatcher used by both the HTTP `invoke` handler
/// and the stdio `tools/call` handler.
///
/// Stateful operations (boole.mine, boole.status) access `state` directly.
/// Legacy proxy operations (bounty.list, receipt.get) use `state.client` +
/// `state.node_url`; native verification uses only its separately constrained
/// client and loopback origin.
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
        "boole.verify_native" => proxy_native_submission(state, args).await,
        "boole.status" => {
            if state
                .active_mining
                .lock()
                .expect("active_mining mutex poisoned")
                .is_some()
            {
                return ToolResult::Ok(json!({"state": "running"}));
            }
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
            let max_cycles = match mining_max_cycles(args) {
                Ok(cycles) => cycles,
                Err(error) => return error,
            };
            // If Axum drops this request future because the HTTP client went
            // away, the guard flips the miner's existing cancellation token.
            let cancel = Arc::new(AtomicBool::new(false));
            let _cancel_on_drop = CancelMiningOnDrop(Arc::clone(&cancel));
            let outcome = match run_mining_bounded(max_cycles, cancel).await {
                Ok(outcome) => outcome,
                Err(MiningRunError::DeadlineExceeded) => {
                    return ToolResult::Internal(json!({"error": "mining-deadline-exceeded"}));
                }
                Err(MiningRunError::WorkerFailed) => {
                    return ToolResult::Internal(json!({"error": "mining-task-failed"}));
                }
            };
            {
                let mut guard = state
                    .last_mining_summary
                    .lock()
                    .expect("last_mining_summary mutex poisoned");
                *guard = Some(outcome.clone());
            }
            ToolResult::Ok(mining_outcome_value(&outcome))
        }
        other => ToolResult::BadRequest(json!({"error":"unknown-tool","tool":other})),
    }
}

async fn invoke(
    State(state): State<Arc<AppState>>,
    request: Request,
) -> Result<(StatusCode, Json<Value>), JsonRejection> {
    // Retain the raw bytes for the native-only duplicate-key pass, then feed a
    // reconstructed request through axum's original Json extractor. This keeps
    // the pre-existing Content-Type, body-limit and JsonRejection behavior for
    // every legacy tool instead of replacing it with a bespoke parser.
    let headers = request.headers().clone();
    let body = Bytes::from_request(request, &state)
        .await
        .map_err(JsonRejection::from)?;
    let native_arguments = precheck_raw_native_arguments(&body, NativeInvocationTransport::Http);
    let mut replay = Request::new(Body::from(body.clone()));
    *replay.headers_mut() = headers;
    let Json(req) = Json::<InvokeRequest>::from_request(replay, &state).await?;
    if req.tool == "boole.verify_native" && native_arguments != NativeArgumentsPrecheck::Exact {
        return Ok((
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid-native-submission-arguments"})),
        ));
    }
    Ok(match dispatch_tool(&state, &req.tool, &req.args).await {
        ToolResult::Ok(v) => (StatusCode::OK, Json(v)),
        ToolResult::BadRequest(v) => (StatusCode::BAD_REQUEST, Json(v)),
        ToolResult::BadGateway(v) => (StatusCode::BAD_GATEWAY, Json(v)),
        ToolResult::Internal(v) => (StatusCode::INTERNAL_SERVER_ERROR, Json(v)),
        ToolResult::Native(status, v) => (status, Json(v)),
    })
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
fn run_mining_summary_with_cancel(
    max_cycles: u64,
    cancel: Option<Arc<AtomicBool>>,
) -> MiningLoopOutcome {
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
        cancel,
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

async fn proxy_native_submission(state: &AppState, args: &Value) -> ToolResult {
    if !has_exact_native_submission_shape(args) {
        return ToolResult::BadRequest(json!({
            "error": "invalid-native-submission-arguments"
        }));
    }
    let Some(base_url) = state.native_shadow_url.as_deref() else {
        return ToolResult::BadGateway(json!({"error":"native-upstream-not-configured"}));
    };
    let url = format!("{base_url}/native-shadow/submissions");
    let mut response = match state
        .native_client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .json(args)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => return native_outcome_unknown("native-transport-failed"),
    };
    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if response
        .content_length()
        .is_some_and(|length| length > NATIVE_VERIFIER_RESPONSE_MAX_BYTES as u64)
    {
        return native_outcome_unknown_with_limit("native-response-too-large");
    }
    let mut body = Vec::new();
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(_) => return native_outcome_unknown("native-response-read-failed"),
        };
        if body.len().saturating_add(chunk.len()) > NATIVE_VERIFIER_RESPONSE_MAX_BYTES {
            return native_outcome_unknown_with_limit("native-response-too-large");
        }
        body.extend_from_slice(&chunk);
    }
    let parsed = match serde_json::from_slice(&body) {
        Ok(parsed) => parsed,
        Err(_) => return native_outcome_unknown("native-response-invalid-json"),
    };
    ToolResult::Native(status, parsed)
}

fn native_outcome_unknown(detail: &str) -> ToolResult {
    ToolResult::BadGateway(json!({
        "error": "native-upstream-outcome-unknown",
        "detail": detail,
        "retry": "resubmit-exact-six-fields"
    }))
}

fn native_outcome_unknown_with_limit(detail: &str) -> ToolResult {
    ToolResult::BadGateway(json!({
        "error": "native-upstream-outcome-unknown",
        "detail": detail,
        "retry": "resubmit-exact-six-fields",
        "maxBytes": NATIVE_VERIFIER_RESPONSE_MAX_BYTES
    }))
}

fn has_exact_native_submission_shape(args: &Value) -> bool {
    // This is deliberately only the MCP boundary's exact-key/JSON-type check.
    // The native service remains the sole authority for schema identity,
    // family/challenge/digest meaning and every field's length/content bounds.
    const STRING_FIELDS: [&str; 5] = [
        "schema",
        "familyVersion",
        "templateId",
        "challengeSha256",
        "rawAnswer",
    ];
    let Some(object) = args.as_object() else {
        return false;
    };
    if object.len() != STRING_FIELDS.len() + 1 {
        return false;
    }
    STRING_FIELDS
        .iter()
        .all(|name| object.get(*name).is_some_and(Value::is_string))
        && object
            .get("epoch")
            .is_some_and(|epoch| epoch.as_i64().is_some() || epoch.as_u64().is_some())
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
        ToolResult::Internal(v) => (
            serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
            true,
        ),
        ToolResult::Native(status, v) => (
            serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
            !status.is_success(),
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

fn jsonrpc_error(id: &Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
    .to_string()
}

async fn write_stdio_response(stdout: Arc<Mutex<std::io::Stdout>>, response: String) {
    tokio::task::spawn_blocking(move || {
        let mut out = stdout.lock().expect("stdout mutex poisoned");
        write_mcp_frame(&mut *out, &response).ok();
        out.flush().ok();
    })
    .await
    .expect("stdout writer task panicked");
}

/// Start one in-process mining loop without tying up the stdio reader.  Its
/// terminal response is emitted exactly once by this completion task, using
/// the ID of the original `tools/call` request.
fn start_stdio_mining(
    state: Arc<AppState>,
    request_id: Value,
    max_cycles: u64,
    stdout: Arc<Mutex<std::io::Stdout>>,
) -> bool {
    let cancel = Arc::new(AtomicBool::new(false));
    let completed = Arc::new(tokio::sync::Notify::new());
    {
        let mut active = state
            .active_mining
            .lock()
            .expect("active_mining mutex poisoned");
        if active.is_some() {
            return false;
        }
        *active = Some(ActiveMiningRequest {
            request_id: request_id.clone(),
            cancel: Arc::clone(&cancel),
            completed: Arc::clone(&completed),
        });
    }

    tokio::spawn(async move {
        let outcome = run_mining_bounded(max_cycles, Arc::clone(&cancel)).await;

        let response = match outcome {
            Err(MiningRunError::DeadlineExceeded) => {
                jsonrpc_error(&request_id, -32000, "Mining deadline exceeded")
            }
            Ok(_) if cancel.load(Ordering::SeqCst) => {
                jsonrpc_error(&request_id, -32800, "Request cancelled")
            }
            Ok(outcome) => {
                {
                    let mut summary = state
                        .last_mining_summary
                        .lock()
                        .expect("last_mining_summary mutex poisoned");
                    *summary = Some(outcome.clone());
                }
                tool_result_to_mcp_content(
                    &request_id,
                    &ToolResult::Ok(mining_outcome_value(&outcome)),
                )
            }
            Err(MiningRunError::WorkerFailed) => {
                jsonrpc_error(&request_id, -32603, "Mining task failed")
            }
        };

        // Keep the slot occupied until the terminal response is serialized,
        // so a second mine cannot slip in during completion/cancel races.
        write_stdio_response(stdout, response).await;
        {
            let mut active = state
                .active_mining
                .lock()
                .expect("active_mining mutex poisoned");
            if active.as_ref().is_some_and(|running| {
                running.request_id == request_id && Arc::ptr_eq(&running.cancel, &cancel)
            }) {
                *active = None;
            }
        }
        // `notify_one` retains a permit when EOF reaches this after the job
        // has finished, unlike `notify_waiters`; that closes the EOF race.
        completed.notify_one();
    });
    true
}

fn cancel_stdio_mining(state: &AppState, request_id: &Value) {
    let active = state
        .active_mining
        .lock()
        .expect("active_mining mutex poisoned");
    if let Some(running) = active
        .as_ref()
        .filter(|running| running.request_id == *request_id)
    {
        running.cancel.store(true, Ordering::SeqCst);
    }
}

/// Cancel and bounded-drain an active mine before *any* stdio termination.
/// This covers clean EOF as well as framing failures, because Tokio otherwise
/// waits for the outstanding blocking worker while shutting down the runtime.
async fn cancel_active_stdio_mining_and_drain(state: &AppState) {
    let completed = {
        let active = state
            .active_mining
            .lock()
            .expect("active_mining mutex poisoned");
        active.as_ref().map(|running| {
            running.cancel.store(true, Ordering::SeqCst);
            Arc::clone(&running.completed)
        })
    };
    if let Some(completed) = completed {
        // The miner checks its token at loop/target checkpoints.  Do not hold
        // shutdown forever if a future dependency regresses that guarantee.
        let observed = completed.notified();
        let _ = tokio::time::timeout(MCP_MINING_EOF_GRACE, observed).await;
    }
}

/// Run the MCP stdio transport loop.
///
/// Reads newline-delimited JSON-RPC 2.0 messages from stdin, dispatches
/// them, and writes framed responses to stdout.  Stateless messages
/// (initialize, tools/list, unknown methods) are handled by the lib's
/// `handle_jsonrpc_sync`. Stateful and proxy tool calls are handled via
/// `dispatch_tool`, which has access to `AppState` and keeps the native and
/// legacy upstream origins distinct.
///
/// The loop exits cleanly on EOF (read_mcp_frame returns None).
///
/// Design: stdin reads are done via `tokio::task::spawn_blocking` because the
/// standard `BufRead` framing API is synchronous.  `boole.mine` already uses
/// `spawn_blocking` internally, so this fits the existing pattern and avoids
/// an async IO dependency.  A mine call itself runs in a separate background
/// task so the single stdio reader can still service protocol requests and its
/// cancellation notification.
async fn run_stdio(node_url: Option<String>, native_shadow_url: Option<String>) -> Result<()> {
    let node_url = node_url
        .as_deref()
        .unwrap_or("http://127.0.0.1:8080")
        .trim_end_matches('/')
        .to_string();
    let native_shadow_url = validate_native_shadow_url(native_shadow_url.as_deref(), &node_url)?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .build()?;
    let state = Arc::new(AppState {
        node_url,
        client,
        native_shadow_url,
        native_client: native_client()?,
        last_mining_summary: Mutex::new(None),
        active_mining: Mutex::new(None),
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

        let msg = match frame_result {
            Ok(Some(s)) => s,
            Ok(None) => {
                // Clean EOF — MCP client closed stdin.
                cancel_active_stdio_mining_and_drain(&state).await;
                break;
            }
            Err(error) => {
                // A malformed UTF-8 or truncated frame still closes this
                // transport. Cancel first; returning directly would leave
                // the blocking mine live until its deadline at runtime drop.
                cancel_active_stdio_mining_and_drain(&state).await;
                return Err(error);
            }
        };

        let native_arguments =
            precheck_raw_native_arguments(msg.as_bytes(), NativeInvocationTransport::Stdio);

        // Try the stateless handler first (initialize, tools/list, unknown
        // methods, notifications).
        let req_val: Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(_) => {
                // Malformed JSON — handle_jsonrpc_sync will produce the
                // -32700 error object.
                if let Some(resp_str) = handle_jsonrpc_sync(&msg) {
                    write_stdio_response(Arc::clone(&stdout), resp_str).await;
                }
                continue;
            }
        };

        let method = req_val.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let request_id = req_val.get("id").cloned();
        if method == "notifications/cancelled" {
            if let Some(cancelled_id) = req_val
                .get("params")
                .and_then(|params| params.get("requestId"))
            {
                cancel_stdio_mining(&state, cancelled_id);
            }
            // This is an MCP notification: it never has a response, and a
            // missing/nonmatching requestId is deliberately a no-op.
            continue;
        }
        let is_native_tool_call = method == "tools/call"
            && req_val
                .get("params")
                .and_then(|params| params.get("name"))
                .and_then(Value::as_str)
                == Some("boole.verify_native");
        let native_envelope_is_exact = !is_native_tool_call
            || serde_json::from_str::<NativeStdioEnvelopeProbe>(&msg).is_ok_and(|request| {
                request.jsonrpc == "2.0" && (request.id.is_string() || request.id.is_number())
            });

        // JSON-RPC notifications have no response channel. Do not execute any
        // tools for an id-less call, especially the one-use native mutation.
        if method == "tools/call" && request_id.is_none() {
            continue;
        }
        if is_native_tool_call && !native_envelope_is_exact {
            let response = json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {
                    "code": -32600,
                    "message": "Invalid Request: tools/call requires jsonrpc 2.0 and a string or number id"
                }
            })
            .to_string();
            write_stdio_response(Arc::clone(&stdout), response).await;
            continue;
        }
        let id = request_id.unwrap_or(Value::Null);

        // Stateful tools/call goes through dispatch_tool.
        if method == "tools/call" {
            let params = req_val.get("params").cloned().unwrap_or(json!({}));
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            if tool_name == "boole.mine" {
                match mining_max_cycles(&arguments) {
                    Ok(max_cycles) => {
                        if start_stdio_mining(
                            Arc::clone(&state),
                            id.clone(),
                            max_cycles,
                            Arc::clone(&stdout),
                        ) {
                            continue;
                        }
                        let response = tool_result_to_mcp_content(
                            &id,
                            &ToolResult::BadRequest(json!({"error": "mining-busy"})),
                        );
                        write_stdio_response(Arc::clone(&stdout), response).await;
                        continue;
                    }
                    Err(error) => {
                        let response = tool_result_to_mcp_content(&id, &error);
                        write_stdio_response(Arc::clone(&stdout), response).await;
                        continue;
                    }
                }
            }
            let result = if tool_name == "boole.verify_native"
                && native_arguments != NativeArgumentsPrecheck::Exact
            {
                ToolResult::BadRequest(json!({
                    "error": "invalid-native-submission-arguments"
                }))
            } else {
                dispatch_tool(&state, tool_name, &arguments).await
            };
            let resp_str = tool_result_to_mcp_content(&id, &result);
            write_stdio_response(Arc::clone(&stdout), resp_str).await;
            continue;
        }

        // All other methods go through the stateless handler.
        if let Some(resp_str) = handle_jsonrpc_sync(&msg) {
            write_stdio_response(Arc::clone(&stdout), resp_str).await;
        }
        // Notifications produce None — no frame to write.
    }

    Ok(())
}

#[cfg(test)]
mod mining_execution_tests {
    use super::*;

    #[test]
    fn dropped_http_request_sets_the_mining_cancel_token() {
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let _guard = CancelMiningOnDrop(Arc::clone(&cancel));
        }
        assert!(cancel.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn bounded_execution_stops_a_long_run_at_its_deadline() {
        let cancel = Arc::new(AtomicBool::new(false));
        let error = run_mining_bounded_for(100_000, Arc::clone(&cancel), Duration::ZERO)
            .await
            .expect_err("a zero deadline must not await the long mining run");
        assert_eq!(error, MiningRunError::DeadlineExceeded);
        assert!(cancel.load(Ordering::SeqCst));
    }
}
