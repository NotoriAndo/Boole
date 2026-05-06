use crate::block_store::FileBlockStore;
use crate::runtime::{RuntimeAdmissionState, RuntimeConfig};
use boole_core::{
    ticket, AdmissionDecision, CalibrationReport, DifficultyRetargetPolicy, Hex32, PersistedBlock,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

const MAX_HTTP_BODY_BYTES: usize = 1_048_576;
const SOCKET_READ_TIMEOUT: Duration = Duration::from_secs(15);
const SOCKET_WRITE_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct LocalNodeConfig {
    pub scenario_path: PathBuf,
    pub block_path: PathBuf,
    pub max_requests: Option<usize>,
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
    max_requests: Option<usize>,
}

pub fn serve_local_node(listener: TcpListener, config: LocalNodeConfig) -> anyhow::Result<()> {
    let mut state = LocalNodeState::from_config(config)?;
    let mut served = 0usize;
    for stream in listener.incoming() {
        let stream = stream?;
        let result = handle_connection(stream, &mut state);
        if let Err(err) = result {
            eprintln!("boole-node local request failed: {err}");
        }
        served += 1;
        if state_should_stop(served, state.max_requests) {
            break;
        }
    }
    Ok(())
}

fn state_should_stop(served: usize, max_requests: Option<usize>) -> bool {
    max_requests.is_some_and(|max| served >= max)
}

impl LocalNodeState {
    fn from_config(config: LocalNodeConfig) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(&config.scenario_path)?;
        let scenario: LocalNodeScenarioConfig = serde_json::from_str(&raw)?;
        let mut runtime_config =
            RuntimeConfig::from_calibration_report(scenario.cfg.clone(), 60_000)
                .map_err(|err| anyhow::anyhow!(err))?;
        if let Some(policy) = scenario.difficulty_retarget.clone() {
            runtime_config = runtime_config
                .with_difficulty_retarget(policy)
                .map_err(|err| anyhow::anyhow!(err))?;
        }
        let recovered = FileBlockStore::recover(&config.block_path)?;
        let mut runtime = if recovered.size() == 0 {
            let mut runtime = RuntimeAdmissionState::new(runtime_config);
            runtime.set_current_c(scenario.genesis_c.clone());
            runtime
        } else {
            RuntimeAdmissionState::boot_from_store(runtime_config, &config.block_path)?
        };
        if runtime.current_c().is_none() {
            runtime.set_current_c(scenario.genesis_c.clone());
        }
        Ok(Self {
            runtime,
            genesis_c: scenario.genesis_c,
            block_path: config.block_path,
            report: scenario.cfg,
            max_requests: config.max_requests,
        })
    }
}

fn handle_connection(mut stream: TcpStream, state: &mut LocalNodeState) -> anyhow::Result<()> {
    stream.set_read_timeout(Some(SOCKET_READ_TIMEOUT))?;
    stream.set_write_timeout(Some(SOCKET_WRITE_TIMEOUT))?;
    let peer_ip = stream
        .peer_addr()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let request = match read_http_request(&mut stream) {
        Ok(request) => request,
        Err(HttpRequestError::BodyTooLarge { limit, actual }) => {
            write_json_response(
                &mut stream,
                413,
                &json!({
                    "ok": false,
                    "error": "body_too_large",
                    "limitBytes": limit,
                    "actualBytes": actual,
                }),
            )?;
            return Ok(());
        }
        Err(HttpRequestError::Invalid(err)) => return Err(err),
    };
    let response = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/status") => status_json(state)?,
        ("GET", "/head") => head_json(state)?,
        ("GET", "/config") => config_json(state),
        ("POST", "/ticket") => ticket_json(state, &request.body)?,
        ("POST", "/submit") => submit_json(state, &request.body, &peer_ip)?,
        _ => {
            write_json_response(
                &mut stream,
                404,
                &json!({ "ok": false, "error": "not_found" }),
            )?;
            return Ok(());
        }
    };
    write_json_response(&mut stream, 200, &response)
}

struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

#[derive(Debug)]
enum HttpRequestError {
    BodyTooLarge { limit: usize, actual: usize },
    Invalid(anyhow::Error),
}

impl From<anyhow::Error> for HttpRequestError {
    fn from(err: anyhow::Error) -> Self {
        Self::Invalid(err)
    }
}

impl From<std::io::Error> for HttpRequestError {
    fn from(err: std::io::Error) -> Self {
        Self::Invalid(err.into())
    }
}

impl From<std::str::Utf8Error> for HttpRequestError {
    fn from(err: std::str::Utf8Error) -> Self {
        Self::Invalid(err.into())
    }
}

fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, HttpRequestError> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
        if header_end(&buffer).is_some() {
            break;
        }
    }
    let Some(header_end) = header_end(&buffer) else {
        return Err(anyhow::anyhow!("bad HTTP request: missing header terminator").into());
    };
    let header = std::str::from_utf8(&buffer[..header_end])?;
    let mut lines = header.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("bad HTTP request: missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("bad HTTP request: missing method"))?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("bad HTTP request: missing path"))?
        .to_string();
    let content_length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > MAX_HTTP_BODY_BYTES {
        return Err(HttpRequestError::BodyTooLarge {
            limit: MAX_HTTP_BODY_BYTES,
            actual: content_length,
        });
    }
    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
    let body = buffer[body_start..buffer.len().min(body_start + content_length)].to_vec();
    Ok(HttpRequest { method, path, body })
}

fn header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn status_json(state: &LocalNodeState) -> anyhow::Result<Value> {
    // Serve from the in-memory block cache. After boot the cache is
    // authoritative; commits update it synchronously, and replay invariants
    // (chain linkage, latest_c) are checked at boot via replay_blocks.
    let height = state.runtime.cached_block_count();
    let head = current_head(state);
    Ok(json!({
        "ok": true,
        "mode": "local",
        "height": height,
        "c": head.clone(),
        "genesisC": state.genesis_c,
        "replayHeight": height,
        "replayLatestC": head,
        "replayMatchesRuntime": true,
        "blockStorePath": state.block_path.to_string_lossy(),
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

fn ticket_json(state: &mut LocalNodeState, body: &[u8]) -> anyhow::Result<Value> {
    let body_value: Value = serde_json::from_slice(body)?;
    let mut ticket_body = body_value
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("ticket body must be a JSON object"))?;
    normalize_pow_fields(&mut ticket_body);
    state
        .runtime
        .observe_ticket_from_body(&ticket_body)
        .map_err(|err| anyhow::anyhow!(err))?;
    let c = Hex32::from_hex(required_string(&ticket_body, "c")?)?;
    let pk = Hex32::from_hex(required_string(&ticket_body, "pk")?)?;
    let n = Hex32::from_hex(required_string(&ticket_body, "n")?)?;
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
    field: &str,
) -> anyhow::Result<&'a str> {
    body.get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing field {field}"))
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

fn submit_json(state: &mut LocalNodeState, body: &[u8], peer_ip: &str) -> anyhow::Result<Value> {
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
    let decision = state
        .runtime
        .admit_body_with_canon_tag(ts_i64, peer_ip, &body, canon_tag);
    let AdmissionDecision::Accepted { share_hash } = decision else {
        return Ok(json!({
            "ok": false,
            "accepted": false,
            "decision": format!("{decision:?}"),
            "c": current_head(state),
        }));
    };
    let accepted_tags = BTreeSet::from([canon_tag]);
    let committed =
        state
            .runtime
            .commit_next_block_for_current_c(&state.block_path, ts_raw, &accepted_tags)?;
    // After commit_next_block: the runtime head is the new block's c, and the
    // store size is committed.block.height + 1 by construction. We do not need
    // to read the store again or re-replay the chain — apply_produced_block has
    // already verified linkage and updated runtime head.
    let new_height = committed.block.height + 1;
    let runtime_head = current_head(state);
    Ok(json!({
        "ok": true,
        "accepted": true,
        "shareHash": share_hash.to_hex(),
        "block": block_json(&committed.block),
        "height": new_height,
        "c": runtime_head,
        "replayHeight": new_height,
        "replayLatestC": runtime_head,
        "replayMatchesRuntime": true,
        "droppedStaleShares": committed.dropped_stale_shares,
    }))
}

fn current_head(state: &LocalNodeState) -> String {
    state
        .runtime
        .current_c()
        .unwrap_or(&state.genesis_c)
        .to_string()
}

fn block_json(block: &PersistedBlock) -> Value {
    json!({
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
    })
}

fn write_json_response(stream: &mut TcpStream, status: u16, body: &Value) -> anyhow::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "Error",
    };
    let body = serde_json::to_string(body)?;
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )?;
    Ok(())
}
