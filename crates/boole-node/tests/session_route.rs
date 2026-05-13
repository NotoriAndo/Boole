//! N1.2 — `/sessions` HTTP route surface for the agent-wallet plan.
//!
//! Boots a local node with an explicit `session_registry_path`, then
//! exercises the four observable surfaces:
//!
//!   - `POST /sessions` registers a `SessionState` and returns the
//!     public view (no secret-shaped fields).
//!   - `GET /sessions/{sessionPk}` returns the public state for a
//!     registered session.
//!   - `POST /sessions/{sessionPk}/revoke` marks the session revoked
//!     and a subsequent `GET` reflects the new state.
//!   - Malformed or non-canonical uppercase `sessionPk` is rejected with
//!     `malformed-pk` (mirrors `account_balance_json` so the vocabulary is
//!     consistent).
//!   - When `session_registry_path` is `None`, the routes return
//!     `session_registry_disabled` instead of silently succeeding.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::local_node::{serve_local_node, LocalNodeConfig};
use serde_json::{json, Value};

const PK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PK_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ROOT: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PK_D: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn fresh_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-n12-session-route-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir
}

struct Boot {
    addr: SocketAddr,
    handle: thread::JoinHandle<anyhow::Result<()>>,
    dir: PathBuf,
}

fn boot_with_registry(max_requests: usize, registry: Option<PathBuf>) -> Boot {
    let dir = fresh_dir("boot");
    let block_path = dir.join("blocks.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path_for_thread = block_path.clone();
    let registry_for_thread = registry.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path: block_path_for_thread,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                session_registry_path: registry_for_thread,
                submit_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                genesis_override: None,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Boot { addr, handle, dir }
}

fn http_post(addr: SocketAddr, path: &str, body: &Value) -> (u16, Value) {
    let body_str = serde_json::to_string(body).expect("body json");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
        body_str.len()
    );
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf).to_string();
    let status: u16 = raw
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let (_, body_text) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no body break in: {raw}"));
    let parsed: Value =
        serde_json::from_str(body_text).unwrap_or_else(|_| panic!("body not json: {body_text}"));
    (status, parsed)
}

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf).to_string();
    let status: u16 = raw
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let (_, body_text) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no body break in: {raw}"));
    let parsed: Value =
        serde_json::from_str(body_text).unwrap_or_else(|_| panic!("body not json: {body_text}"));
    (status, parsed)
}

fn fixture_session() -> Value {
    json!({
        "sessionPk": PK_A,
        "ownerPk": PK_B,
        "agentPk": PK_C,
        "fixedRewardRecipient": PK_D,
        "allowedFamilyRoot": ROOT,
        "maxFeePerRequest": "12",
        "activationHeight": 0,
        "expiryHeight": 100,
        "revoked": false,
        "policyHash": ROOT,
    })
}

#[test]
fn session_route_register_returns_ok_for_valid_session() {
    let dir = fresh_dir("register-ledger");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(1, Some(registry));

    let body = json!({"session": fixture_session(), "currentHeight": 0});
    let (status, value) = http_post(boot.addr, "/sessions", &body);
    assert_eq!(status, 200, "expected 200, got {status}: {value}");
    assert_eq!(value["ok"], true);
    assert_eq!(value["session"]["sessionPk"], PK_A);
    assert_eq!(value["session"]["revoked"], false);

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_get_returns_public_state_no_secret() {
    let dir = fresh_dir("get-ledger");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(2, Some(registry));

    let body = json!({"session": fixture_session(), "currentHeight": 0});
    let (status_post, _) = http_post(boot.addr, "/sessions", &body);
    assert_eq!(status_post, 200);

    let (status, value) = http_get(boot.addr, &format!("/sessions/{PK_A}"));
    assert_eq!(status, 200, "expected 200, got {status}: {value}");
    assert_eq!(value["ok"], true);
    assert_eq!(value["session"]["sessionPk"], PK_A);
    assert_eq!(value["session"]["ownerPk"], PK_B);
    assert_eq!(value["session"]["agentPk"], PK_C);
    let body_text = serde_json::to_string(&value).expect("json");
    assert!(
        !body_text.contains("\"sk\""),
        "response must not contain `sk`; got {body_text}"
    );
    assert!(
        !body_text.contains("\"sessionSk\""),
        "response must not contain `sessionSk`; got {body_text}"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_revoke_sets_revoked_true() {
    let dir = fresh_dir("revoke-ledger");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(3, Some(registry));

    let body = json!({"session": fixture_session(), "currentHeight": 0});
    let (status_post, _) = http_post(boot.addr, "/sessions", &body);
    assert_eq!(status_post, 200);

    let (status_revoke, value_revoke) = http_post(
        boot.addr,
        &format!("/sessions/{PK_A}/revoke"),
        &json!({"height": 42}),
    );
    assert_eq!(status_revoke, 200, "got {status_revoke}: {value_revoke}");
    assert_eq!(value_revoke["ok"], true);
    assert_eq!(value_revoke["session"]["revoked"], true);

    let (status_get, value_get) = http_get(boot.addr, &format!("/sessions/{PK_A}"));
    assert_eq!(status_get, 200);
    assert_eq!(value_get["session"]["revoked"], true);

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_rejects_malformed_session_pk() {
    let dir = fresh_dir("malformed-ledger");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(1, Some(registry));

    let (status, value) = http_get(boot.addr, "/sessions/not-hex");
    assert_eq!(status, 400, "got {status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "malformed-pk");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_rejects_uppercase_session_pk_as_noncanonical() {
    let dir = fresh_dir("uppercase-ledger");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(1, Some(registry));

    let uppercase = PK_A.to_ascii_uppercase();
    let (status, value) = http_get(boot.addr, &format!("/sessions/{uppercase}"));
    assert_eq!(
        status, 400,
        "uppercase sessionPk must be noncanonical: {value}"
    );
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "malformed-pk");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_returns_disabled_when_registry_unconfigured() {
    let boot = boot_with_registry(1, None);

    let body = json!({"session": fixture_session(), "currentHeight": 0});
    let (status, value) = http_post(boot.addr, "/sessions", &body);
    assert_eq!(status, 400, "got {status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "session_registry_disabled");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
}
