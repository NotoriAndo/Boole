use crate::block_store::FileBlockStore;
use crate::runtime::{RuntimeAdmissionState, RuntimeConfig};
use boole_core::{replay_blocks, AdmissionDecision, CalibrationReport, PersistedBlock};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;

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
    genesis_c: String,
}

struct LocalNodeState {
    runtime: RuntimeAdmissionState,
    genesis_c: String,
    block_path: PathBuf,
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
        let runtime_config = RuntimeConfig::from_calibration_report(scenario.cfg, 60_000)
            .map_err(|err| anyhow::anyhow!(err))?;
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
            max_requests: config.max_requests,
        })
    }
}

fn handle_connection(mut stream: TcpStream, state: &mut LocalNodeState) -> anyhow::Result<()> {
    let request = read_http_request(&mut stream)?;
    let response = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/status") => status_json(state)?,
        ("GET", "/head") => head_json(state)?,
        ("GET", "/config") => config_json(state),
        ("POST", "/submit") => submit_json(state, &request.body)?,
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

fn read_http_request(stream: &mut TcpStream) -> anyhow::Result<HttpRequest> {
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
        anyhow::bail!("bad HTTP request: missing header terminator");
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
    let recovered = FileBlockStore::recover(&state.block_path)?;
    let replay = replay_blocks(recovered.blocks())?;
    Ok(json!({
        "ok": true,
        "mode": "local",
        "height": recovered.size(),
        "c": current_head(state),
        "genesisC": state.genesis_c,
        "replayHeight": replay.height,
        "replayLatestC": replay.latest_c,
        "replayMatchesRuntime": replay.latest_c == current_head(state),
        "blockStorePath": state.block_path.to_string_lossy(),
    }))
}

fn head_json(state: &LocalNodeState) -> anyhow::Result<Value> {
    let recovered = FileBlockStore::recover(&state.block_path)?;
    Ok(json!({
        "ok": true,
        "height": recovered.size(),
        "c": current_head(state),
    }))
}

fn config_json(state: &LocalNodeState) -> Value {
    let policy = &state.runtime.config.policy;
    json!({
        "ok": true,
        "T_submit": policy.thresholds.t_submit.to_string(),
        "T_share": policy.thresholds.t_share.to_string(),
        "T_block": policy.thresholds.t_block.to_string(),
        "T_ticket": policy.thresholds.t_ticket.to_string(),
        "K_max": policy.k_max,
        "ShareCapPerPK_Block": policy.share_cap_per_pk_block,
    })
}

fn submit_json(state: &mut LocalNodeState, body: &[u8]) -> anyhow::Result<Value> {
    let body_value: Value = serde_json::from_slice(body)?;
    let submit_body = body_value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("submit body must be a JSON object"))?;
    let ip = submit_body
        .get("ip")
        .and_then(Value::as_str)
        .unwrap_or("127.0.0.1");
    let canon_tag = submit_body
        .get("canonTag")
        .and_then(Value::as_u64)
        .unwrap_or(0) as u8;
    let ts = submit_body
        .get("ts")
        .and_then(Value::as_u64)
        .unwrap_or(1_800_000_000_000);
    let body = submit_body
        .get("body")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(|| submit_body.clone());

    state
        .runtime
        .observe_ticket_from_body(&body)
        .map_err(|err| anyhow::anyhow!(err))?;
    let decision = state
        .runtime
        .admit_body_with_canon_tag(ts as i64, ip, &body, canon_tag);
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
            .commit_next_block_for_current_c(&state.block_path, ts, &accepted_tags)?;
    let recovered = FileBlockStore::recover(&state.block_path)?;
    let replay = replay_blocks(recovered.blocks())?;
    let runtime_head = current_head(state);
    Ok(json!({
        "ok": true,
        "accepted": true,
        "shareHash": share_hash.to_hex(),
        "block": block_json(&committed.block),
        "height": recovered.size(),
        "c": runtime_head,
        "replayHeight": replay.height,
        "replayLatestC": replay.latest_c,
        "replayMatchesRuntime": replay.latest_c == runtime_head,
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
        "ts": block.ts,
    })
}

fn write_json_response(stream: &mut TcpStream, status: u16, body: &Value) -> anyhow::Result<()> {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
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
