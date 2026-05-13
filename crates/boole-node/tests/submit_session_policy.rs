//! N2.1 — Session-bound `/submit` admission gate.
//!
//! Boots a local node with the session registry and the new submit-nonce
//! ledger wired in, then exercises the agent-wallet plan's typed-error
//! contract:
//!
//!   - Legacy submit with no `session` metadata still takes the existing
//!     admission path (200 envelope, regardless of POW outcome).
//!   - Submit naming an unknown `submittedBy` is rejected with
//!     `session_unknown` (404).
//!   - Submit against a revoked session is rejected with
//!     `session_revoked` (403).
//!   - Submit whose `rewardRecipient` does not match the session's
//!     `fixedRewardRecipient` is rejected with
//!     `reward_recipient_mismatch` (403).
//!   - Replayed `(submittedBy, nonce)` pair is rejected with
//!     `nonce_replayed` (409), and the dedup state survives a process
//!     restart by replaying the NDJSON ledger.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::local_node::{serve_local_node, LocalNodeConfig};
use serde_json::{json, Value};

const PK_AGENT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PK_OWNER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PK_AGENT_RAW: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ROOT_HEX: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PK_OTHER: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn fresh_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-n21-submit-session-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("tmp dir");
    dir
}

fn scenario_path() -> PathBuf {
    repo_root().join("fixtures/protocol/runtime-smoke/v1.json")
}

fn scenario_body() -> Value {
    let raw = fs::read_to_string(scenario_path()).expect("scenario file");
    let scenario: Value = serde_json::from_str(&raw).expect("scenario json");
    scenario["steps"][0]["body"].clone()
}

struct Boot {
    addr: SocketAddr,
    handle: thread::JoinHandle<anyhow::Result<()>>,
}

struct BootPaths {
    dir: PathBuf,
    sessions: PathBuf,
    nonces: PathBuf,
}

impl BootPaths {
    fn new(label: &str) -> Self {
        let dir = fresh_dir(label);
        let sessions = dir.join("sessions.ndjson");
        let nonces = dir.join("submit-nonces.ndjson");
        Self {
            dir,
            sessions,
            nonces,
        }
    }
}

fn boot_with(paths: &BootPaths, max_requests: usize) -> Boot {
    let block_path = paths.dir.join("blocks.ndjson");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let sessions = paths.sessions.clone();
    let nonces = paths.nonces.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                session_registry_path: Some(sessions),
                submit_nonce_ledger_path: Some(nonces),
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                genesis_override: None,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Boot { addr, handle }
}

fn http_post(addr: SocketAddr, path: &str, body: &Value) -> (u16, Value) {
    let body_str = serde_json::to_string(body).expect("body json");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
        body_str.len()
    );
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("write timeout");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = Vec::new();
    match stream.read_to_end(&mut buf) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::ConnectionReset && !buf.is_empty() => {}
        Err(err) => panic!("read response: {err}"),
    }
    let raw = String::from_utf8(buf).expect("utf8 response");
    let status = raw
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("status not parseable: {raw}"));
    let (_, body_text) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("response missing body: {raw}"));
    let body: Value = serde_json::from_str(body_text)
        .unwrap_or_else(|err| panic!("body not JSON: {err}, raw={body_text}"));
    (status, body)
}

fn fixture_session(session_pk: &str, fixed_reward_recipient: &str) -> Value {
    json!({
        "sessionPk": session_pk,
        "ownerPk": PK_OWNER,
        "agentPk": PK_AGENT_RAW,
        "fixedRewardRecipient": fixed_reward_recipient,
        "allowedFamilyRoot": ROOT_HEX,
        "maxFeePerRequest": "12",
        "activationHeight": 0,
        "expiryHeight": 100,
        "revoked": false,
        "policyHash": ROOT_HEX,
    })
}

fn register_session(addr: SocketAddr, session_pk: &str, fixed_reward_recipient: &str) {
    let body = json!({
        "session": fixture_session(session_pk, fixed_reward_recipient),
        "currentHeight": 0,
    });
    let (status, value) = http_post(addr, "/sessions", &body);
    assert_eq!(
        status, 200,
        "register failed: status={status}, body={value}"
    );
    assert_eq!(value["ok"], true);
}

fn revoke_session(addr: SocketAddr, session_pk: &str) {
    let (status, value) = http_post(
        addr,
        &format!("/sessions/{session_pk}/revoke"),
        &json!({"height": 1}),
    );
    assert_eq!(status, 200, "revoke failed: status={status}, body={value}");
    assert_eq!(value["session"]["revoked"], true);
}

fn submit_envelope(body: Value, session: Option<Value>) -> Value {
    let mut envelope = json!({"body": body});
    if let Some(s) = session {
        envelope["session"] = s;
    }
    envelope
}

#[test]
fn submit_without_session_metadata_uses_legacy_path() {
    let paths = BootPaths::new("legacy");
    let boot = boot_with(&paths, 1);

    let body = scenario_body();
    let (status, value) = http_post(boot.addr, "/submit", &submit_envelope(body, None));
    assert_eq!(
        status, 200,
        "legacy submit must return 200, got {status}: {value}"
    );
    assert!(
        value.get("accepted").is_some(),
        "legacy submit must carry the existing admission envelope: {value}"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_with_unknown_session_pk_returns_session_unknown() {
    let paths = BootPaths::new("unknown");
    let boot = boot_with(&paths, 1);

    let body = scenario_body();
    let session = json!({
        "submittedBy": PK_AGENT,
        "rewardRecipient": PK_OWNER,
        "nonce": "n-unknown",
    });
    let (status, value) = http_post(boot.addr, "/submit", &submit_envelope(body, Some(session)));
    assert_eq!(status, 404, "got status={status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "session_unknown");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_with_revoked_session_returns_session_revoked() {
    let paths = BootPaths::new("revoked");
    let boot = boot_with(&paths, 3);

    register_session(boot.addr, PK_AGENT, PK_OWNER);
    revoke_session(boot.addr, PK_AGENT);

    let body = scenario_body();
    let session = json!({
        "submittedBy": PK_AGENT,
        "rewardRecipient": PK_OWNER,
        "nonce": "n-revoked",
    });
    let (status, value) = http_post(boot.addr, "/submit", &submit_envelope(body, Some(session)));
    assert_eq!(status, 403, "got status={status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "session_revoked");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_with_reward_recipient_mismatch_is_rejected() {
    let paths = BootPaths::new("recipient");
    let boot = boot_with(&paths, 2);

    register_session(boot.addr, PK_AGENT, PK_OWNER);

    let body = scenario_body();
    let session = json!({
        "submittedBy": PK_AGENT,
        "rewardRecipient": PK_OTHER,
        "nonce": "n-recipient",
    });
    let (status, value) = http_post(boot.addr, "/submit", &submit_envelope(body, Some(session)));
    assert_eq!(status, 403, "got status={status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "reward_recipient_mismatch");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_replayed_nonce_returns_nonce_replayed() {
    let paths = BootPaths::new("replay");
    let boot = boot_with(&paths, 3);

    register_session(boot.addr, PK_AGENT, PK_OWNER);

    let body = scenario_body();
    let session = json!({
        "submittedBy": PK_AGENT,
        "rewardRecipient": PK_OWNER,
        "nonce": "n-replay",
    });

    let (status_first, value_first) = http_post(
        boot.addr,
        "/submit",
        &submit_envelope(body.clone(), Some(session.clone())),
    );
    assert_eq!(
        status_first, 200,
        "first submit must reach admission: status={status_first}, body={value_first}"
    );

    let (status_second, value_second) =
        http_post(boot.addr, "/submit", &submit_envelope(body, Some(session)));
    assert_eq!(
        status_second, 409,
        "duplicate nonce must return 409: status={status_second}, body={value_second}"
    );
    assert_eq!(value_second["ok"], false);
    assert_eq!(value_second["reason"], "nonce_replayed");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_nonce_dedup_survives_restart() {
    let paths = BootPaths::new("restart");

    // First boot: register session + submit once. Both calls share the
    // session registry and nonce ledger paths so the second boot can
    // replay the same NDJSON files.
    let first = boot_with(&paths, 2);
    register_session(first.addr, PK_AGENT, PK_OWNER);
    let session = json!({
        "submittedBy": PK_AGENT,
        "rewardRecipient": PK_OWNER,
        "nonce": "n-restart",
    });
    let body = scenario_body();
    let (status_first, value_first) = http_post(
        first.addr,
        "/submit",
        &submit_envelope(body.clone(), Some(session.clone())),
    );
    assert_eq!(
        status_first, 200,
        "first submit must reach admission: status={status_first}, body={value_first}"
    );
    first.handle.join().expect("server thread").expect("exits");

    // Second boot: same ledgers, replay the same (submittedBy, nonce)
    // pair. Restart-replay must rehydrate the dedup set so the second
    // submit is rejected with the same `nonce_replayed` envelope.
    let second = boot_with(&paths, 1);
    let (status_second, value_second) = http_post(
        second.addr,
        "/submit",
        &submit_envelope(body, Some(session)),
    );
    assert_eq!(
        status_second, 409,
        "post-restart replay must still return 409: status={status_second}, body={value_second}"
    );
    assert_eq!(value_second["reason"], "nonce_replayed");
    second.handle.join().expect("server thread").expect("exits");

    let _ = fs::remove_dir_all(&paths.dir);
}
