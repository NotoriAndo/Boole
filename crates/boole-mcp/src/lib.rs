//! P2.1 closure — in-process trait impls that let `boole-mcp` drive
//! `boole-miner`'s mining loop without an HTTP loopback to
//! `boole-node`.
//!
//! Pieces:
//!   * **slice 47** — `InProcessChainHead` (`ChainHeadFetcher`).
//!   * **slice 48** — `InProcessSubmitter` (`Submitter`) + capture
//!     buffers so a test or the future `boole.mine` tool can read
//!     back what shares/blocks the miner emitted.
//!   * **slice 49** — `build_in_process_mining_deps` factory that
//!     bundles both impls + caller-injected heavy collaborators
//!     (driver, verifier, emitter, canonicalizer) into a
//!     `MiningLoopDeps` ready for `run_mining_loop`, and hands back a
//!     clonable `CaptureLog` so the caller can inspect submitter
//!     captures after the submitter has moved behind the
//!     `Box<dyn Submitter>` trait object owned by `MiningLoopDeps`.
//!
//! MCP stdio transport pieces (S4 / S5):
//!   * `write_mcp_frame` / `read_mcp_frame` — Content-Length framing per
//!     the LSP/MCP base protocol.
//!   * `mcp_tools_array` — the canonical 4-tool list; feeds both HTTP
//!     `GET /mcp/tools` and stdio `tools/list` so they are always in sync.
//!   * `handle_jsonrpc_sync` — synchronous JSON-RPC 2.0 dispatcher for
//!     stateless requests (initialize, tools/list, tools/call on status).
//!     The binary's async loop calls this from a spawn_blocking task.
//!
//! Actual mining-loop invocation + the `boole.mine` / `boole.status`
//! MCP tool wiring ride on follow-up slices.

use std::io::{self, BufRead, Write};
use std::sync::{Arc, Mutex};

use boole_core::Hex32;
use boole_miner::{
    AnnounceTicketInputs, AnnounceTicketResult, Canonicalizer, ChainHead, ChainHeadError,
    ChainHeadFetcher, MiningLoopDeps, ProverDriver, SubmitInputs, SubmitResult, Submitter,
    TargetEmitter, Verifier,
};
use serde_json::{json, Value};

/// P2.1 slice 55 — canonical runtime-smoke scenario fixture embedded at
/// build time. Keeps the binary self-sufficient: a user running
/// `boole-mcp serve` does not need anything from `fixtures/` on their
/// host. Closes P2.1 closure criterion 3. Future slices source
/// `default_in_process_inputs` thresholds from this byte slice instead
/// of hardcoded BigUint constants.
pub const RUNTIME_SMOKE_FIXTURE_BYTES: &[u8] =
    include_bytes!("../../../fixtures/protocol/runtime-smoke/v1.json");

/// `ChainHeadFetcher` impl that returns a single pinned `ChainHead`.
/// Suitable for boole-mcp's mining tools when the head is sourced from
/// boole-mcp's own state instead of an external boole-node `GET /head`
/// HTTP call.
pub struct InProcessChainHead {
    head: ChainHead,
}

impl InProcessChainHead {
    pub fn new(head: ChainHead) -> Self {
        Self { head }
    }
}

impl ChainHeadFetcher for InProcessChainHead {
    fn fetch_head(&self) -> Result<ChainHead, ChainHeadError> {
        Ok(self.head.clone())
    }
}

/// One captured `announce_ticket` call. Owned strings because the
/// `AnnounceTicketInputs` lifetime ends as soon as the call returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedAnnounce {
    pub c_hex: String,
    pub pk_hex: String,
    pub n_hex: String,
}

/// One captured `submit` call. `canon_bytes` is cloned to an owned
/// `Vec<u8>` for the same lifetime reason as `CapturedAnnounce`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedSubmit {
    pub c_hex: String,
    pub pk_hex: String,
    pub n_hex: String,
    pub j_hex: String,
    pub nonce_s_hex: String,
    pub canon_bytes: Vec<u8>,
}

/// Shared, clonable view onto an `InProcessSubmitter`'s capture
/// buffers. Cheap to clone (Arc internally) so a caller can hand the
/// submitter into `Box<dyn Submitter>` while retaining a separate
/// handle to read announce / submit captures.
#[derive(Debug, Clone, Default)]
pub struct CaptureLog {
    inner: Arc<CaptureLogInner>,
}

#[derive(Debug, Default)]
struct CaptureLogInner {
    announces: Mutex<Vec<CapturedAnnounce>>,
    submits: Mutex<Vec<CapturedSubmit>>,
}

impl CaptureLog {
    pub fn captured_announces(&self) -> Vec<CapturedAnnounce> {
        self.inner.announces.lock().unwrap().clone()
    }

    pub fn captured_submits(&self) -> Vec<CapturedSubmit> {
        self.inner.submits.lock().unwrap().clone()
    }

    fn record_announce(&self, a: CapturedAnnounce) {
        self.inner.announces.lock().unwrap().push(a);
    }

    fn record_submit(&self, s: CapturedSubmit) {
        self.inner.submits.lock().unwrap().push(s);
    }
}

/// `Submitter` impl that returns pinned results and records every
/// `announce_ticket` / `submit` call for later inspection. Captures
/// live in a clonable `CaptureLog` so the caller keeps a read handle
/// even after the submitter moves behind `Box<dyn Submitter>`.
pub struct InProcessSubmitter {
    announce_result: AnnounceTicketResult,
    submit_result: SubmitResult,
    capture: CaptureLog,
}

impl InProcessSubmitter {
    pub fn new(announce_result: AnnounceTicketResult, submit_result: SubmitResult) -> Self {
        Self {
            announce_result,
            submit_result,
            capture: CaptureLog::default(),
        }
    }

    /// Hand out a clonable handle onto the capture log. The submitter
    /// keeps its own handle; both halves see every recorded call.
    pub fn capture_log(&self) -> CaptureLog {
        self.capture.clone()
    }

    pub fn captured_announces(&self) -> Vec<CapturedAnnounce> {
        self.capture.captured_announces()
    }

    pub fn captured_submits(&self) -> Vec<CapturedSubmit> {
        self.capture.captured_submits()
    }
}

impl Submitter for InProcessSubmitter {
    fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
        self.capture.record_announce(CapturedAnnounce {
            c_hex: inputs.c_hex.to_string(),
            pk_hex: inputs.pk_hex.to_string(),
            n_hex: inputs.n_hex.to_string(),
        });
        self.announce_result.clone()
    }

    fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
        self.capture.record_submit(CapturedSubmit {
            c_hex: inputs.c_hex.to_string(),
            pk_hex: inputs.pk_hex.to_string(),
            n_hex: inputs.n_hex.to_string(),
            j_hex: inputs.j_hex.to_string(),
            nonce_s_hex: inputs.nonce_s_hex.to_string(),
            canon_bytes: inputs.canon_bytes.to_vec(),
        });
        self.submit_result.clone()
    }
}

/// Inputs that fully describe an in-process mining-deps composition.
/// `prompt_builder`, `log`, and `sleeper` are intentionally omitted —
/// they stay `None` on the produced `MiningLoopDeps` and the caller
/// (the future `boole.mine` tool, slice 50+) decides whether to wire
/// them.
pub struct InProcessMiningInputs {
    pub pk: Hex32,
    pub head: ChainHead,
    pub announce_result: AnnounceTicketResult,
    pub submit_result: SubmitResult,
    pub emitter: Box<dyn TargetEmitter>,
    pub driver: Box<dyn ProverDriver>,
    pub verifier: Box<dyn Verifier>,
    pub canonicalizer: Box<dyn Canonicalizer>,
}

/// Bundle of `MiningLoopDeps` ready for `run_mining_loop` and a
/// clonable `CaptureLog` the caller retains for inspection.
pub struct InProcessMiningBundle {
    pub deps: MiningLoopDeps,
    pub capture: CaptureLog,
}

/// Compose `InProcessChainHead` + `InProcessSubmitter` + the
/// caller-injected heavy collaborators into a single `MiningLoopDeps`
/// the future `boole.mine` tool can hand straight to
/// `boole_miner::run_mining_loop`.
pub fn build_in_process_mining_deps(inputs: InProcessMiningInputs) -> InProcessMiningBundle {
    let submitter = InProcessSubmitter::new(inputs.announce_result, inputs.submit_result);
    let capture = submitter.capture_log();
    let deps = MiningLoopDeps {
        pk: inputs.pk,
        chain_head: Box::new(InProcessChainHead::new(inputs.head)),
        emitter: inputs.emitter,
        driver: inputs.driver,
        verifier: inputs.verifier,
        canonicalizer: inputs.canonicalizer,
        submit_client: Box::new(submitter),
        prompt_builder: None,
        log: None,
        sleeper: None,
    };
    InProcessMiningBundle { deps, capture }
}

// ── S4: Content-Length framing ────────────────────────────────────────────

/// Write a single MCP/LSP base-protocol Content-Length frame to `writer`.
///
/// Format: `Content-Length: {len}\r\n\r\n{body}`.  The caller is
/// responsible for flushing `writer` after the call when needed.
pub fn write_mcp_frame(writer: &mut impl Write, body: &str) -> io::Result<()> {
    let bytes = body.as_bytes();
    write!(writer, "Content-Length: {}\r\n\r\n", bytes.len())?;
    writer.write_all(bytes)?;
    Ok(())
}

/// Read a single MCP/LSP base-protocol Content-Length frame from `reader`.
///
/// Returns `Ok(None)` on a clean EOF before any bytes are consumed.
/// Returns an error if the headers are malformed (no `Content-Length`
/// present, or the body is shorter than declared).
pub fn read_mcp_frame(reader: &mut impl BufRead) -> anyhow::Result<Option<String>> {
    let mut content_length: Option<usize> = None;
    let mut first = true;

    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            // Clean EOF.
            if first {
                return Ok(None);
            }
            anyhow::bail!("unexpected EOF before blank header line");
        }
        first = false;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            // Blank line — headers done.
            break;
        }
        if let Some(val) = trimmed.strip_prefix("Content-Length:") {
            let parsed: usize = val
                .trim()
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid Content-Length value: {e}"))?;
            content_length = Some(parsed);
        }
        // Ignore unrecognised headers (e.g. Content-Type) per spec.
    }

    let len = content_length.ok_or_else(|| anyhow::anyhow!("missing Content-Length header"))?;
    // N0-pre.6 — cap the declared frame size before allocating, so a hostile
    // `Content-Length` (e.g. 4 GiB) cannot drive a pre-allocation OOM bomb on
    // the untrusted stdio transport.
    const MCP_FRAME_MAX_BYTES: usize = 16 * 1024 * 1024;
    if len > MCP_FRAME_MAX_BYTES {
        anyhow::bail!("Content-Length {len} exceeds the {MCP_FRAME_MAX_BYTES}-byte frame cap");
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    let s = String::from_utf8(body)
        .map_err(|e| anyhow::anyhow!("frame body is not valid UTF-8: {e}"))?;
    Ok(Some(s))
}

// ── S5: Shared tools list ────────────────────────────────────────────────

/// The canonical 4-tool array shared by HTTP `GET /mcp/tools` and MCP
/// stdio `tools/list`.  Each tool carries BOTH `input_schema` (snake_case,
/// for the existing HTTP surface) and `inputSchema` (camelCase, required by
/// the MCP spec for stdio clients).  Both fields carry identical content.
pub fn mcp_tools_array() -> Vec<Value> {
    let tools_raw: &[(&str, &str, Value)] = &[
        (
            "bounty.list",
            "List currently-open bounties/work units from the upstream boole-node.",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        ),
        (
            "receipt.get",
            "Fetch a single proof receipt by id from the upstream boole-node.",
            json!({
                "type": "object",
                "properties": {
                    "receipt_id": { "type": "string" }
                },
                "required": ["receipt_id"],
                "additionalProperties": false
            }),
        ),
        (
            "boole.mine",
            "Drive a closed-local mining round-trip in-process: real v1-lenbound instance \
             generation → deterministic driver stand-in → ProofIntakeV1 → Canonicalizer → \
             RejectingVerifier. Optional `max_cycles` (default 0 = zero-cycle plumbing smoke; \
             >=1 runs that many full closed-local cycles). Returns honest pipeline counters: \
             verify_accepted=0 is correct (no Lean toolchain). \
             Not public mining; not a solve claim.",
            json!({
                "type": "object",
                "properties": {
                    "max_cycles": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Number of ticket cycles to run (default 0)."
                    }
                },
                "additionalProperties": false
            }),
        ),
        (
            "boole.status",
            "Report current in-process mining session state (idle, in-progress, last summary). \
             Pure read.",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        ),
    ];

    tools_raw
        .iter()
        .map(|(name, desc, schema)| {
            json!({
                "name": name,
                "description": desc,
                // snake_case: backward-compat for HTTP /mcp/tools consumers
                "input_schema": schema,
                // camelCase: required by MCP spec for stdio tools/list
                "inputSchema": schema,
            })
        })
        .collect()
}

// ── S5: Stateless JSON-RPC 2.0 dispatcher ────────────────────────────────

/// MCP protocol version this server speaks.  Pinned here so a version bump
/// is a deliberate, visible code change.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// `boole.status` idle envelope (no prior mining session in this state).
pub fn status_idle_value() -> Value {
    json!({"state": "idle"})
}

/// Synchronous JSON-RPC 2.0 dispatcher for stateless MCP messages.
///
/// Handles: `initialize`, `notifications/initialized`, `tools/list`,
/// `tools/call` (boole.status only — stateless), unknown methods, and
/// JSON parse errors.
///
/// Returns `None` for notifications (no `id` field present and method is a
/// notification).  Returns `Some(json_string)` for all other messages
/// (results or error objects).
///
/// The stateful `boole.mine` / `boole.status`-after-mine path is handled
/// in `main.rs`'s async dispatch which has access to `AppState`.
pub fn handle_jsonrpc_sync(msg: &str) -> Option<String> {
    // Parse JSON.
    let req: Value = match serde_json::from_str(msg) {
        Ok(v) => v,
        Err(_) => {
            let resp = json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": "Parse error" }
            });
            return Some(resp.to_string());
        }
    };

    let id = req.get("id").cloned();

    // Missing method → Invalid Request.
    let method = match req.get("method").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => {
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id.unwrap_or(Value::Null),
                "error": { "code": -32600, "message": "Invalid Request: missing method" }
            });
            return Some(resp.to_string());
        }
    };

    // Notifications have no `id`. Return None (no response).
    let is_notification = id.is_none();

    match method {
        "initialize" => {
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id.unwrap_or(Value::Null),
                "result": {
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": "boole-mcp",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            });
            Some(resp.to_string())
        }
        "notifications/initialized" => {
            // Notification — no response.
            None
        }
        "tools/list" => {
            // Build the tools array; for stdio we expose inputSchema (camelCase).
            let tools: Vec<Value> = mcp_tools_array()
                .into_iter()
                .map(|mut t| {
                    // For the MCP stdio tools/list we only need inputSchema (camelCase).
                    // Remove the snake_case key to keep the response clean.
                    if let Some(obj) = t.as_object_mut() {
                        obj.remove("input_schema");
                    }
                    t
                })
                .collect();
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id.unwrap_or(Value::Null),
                "result": { "tools": tools }
            });
            Some(resp.to_string())
        }
        "tools/call" => {
            // Stateless path: only boole.status (idle) is handled here.
            // Stateful dispatch (boole.mine, boole.status-after-mine, and
            // proxy tools) is done in main.rs's async dispatch which has
            // access to AppState.  This function is called from the async
            // dispatch after it resolves stateful calls; we hand it a
            // pre-serialised tool result to wrap in MCP content envelope.
            // If called standalone (e.g. from tests without AppState),
            // boole.status returns idle.
            let params = req.get("params").cloned().unwrap_or(json!({}));
            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            match tool_name {
                "boole.status" => {
                    let result_val = status_idle_value();
                    let content_text =
                        serde_json::to_string(&result_val).unwrap_or_else(|_| "{}".to_string());
                    let resp = json!({
                        "jsonrpc": "2.0",
                        "id": id.unwrap_or(Value::Null),
                        "result": {
                            "content": [{ "type": "text", "text": content_text }],
                            "isError": false
                        }
                    });
                    Some(resp.to_string())
                }
                other => {
                    // For unknown tool names in the stateless path we surface
                    // a tools/call error content block (isError: true).
                    let content_text =
                        serde_json::to_string(&json!({"error":"unknown-tool","tool": other}))
                            .unwrap_or_else(|_| "{}".to_string());
                    let resp = json!({
                        "jsonrpc": "2.0",
                        "id": id.unwrap_or(Value::Null),
                        "result": {
                            "content": [{ "type": "text", "text": content_text }],
                            "isError": true
                        }
                    });
                    Some(resp.to_string())
                }
            }
        }
        _other if is_notification => {
            // Generic notification — no response.
            None
        }
        other => {
            let resp = json!({
                "jsonrpc": "2.0",
                "id": id.unwrap_or(Value::Null),
                "error": { "code": -32601, "message": format!("Method not found: {other}") }
            });
            Some(resp.to_string())
        }
    }
}
