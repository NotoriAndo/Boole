//! S9 — `GET /account/{pk}/balance` route.
//!
//! Boots a node with a pre-seeded reward ledger derived from
//! `fixtures/protocol/replay/v1.json` and confirms the route returns the
//! expected balance shape, treats unknown pk as `balance="0"` (not 404),
//! and rejects malformed pk shapes with a 400 envelope.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::Value;

// The runtime-smoke step-0 share is mined AND proposed by PK_B, so the
// committed block credits it twice (share + proposer) — balance "2".
const PK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PK_UNKNOWN: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

// SC.5 — no pre-seeded golden store: the node boot now replays its own
// store under the strict genesis-aware contract, and the golden replay
// fixtures (legacy v1 chain; v2 with t_block == t_share) are chains no
// valid scenario genesis can produce. The tests instead COMMIT a block
// through the live node (the smoke step-0 share) and query balances the
// node itself derived — the same route surface, seeded honestly.
fn smoke_step0_submit_payload() -> Value {
    let raw = std::fs::read_to_string(scenario_path()).expect("read scenario");
    let scenario: Value = serde_json::from_str(&raw).expect("scenario json");
    let body = scenario["steps"][0]["body"].clone();
    serde_json::json!({"body": body, "canonTag": 0})
}

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn boot_with_seeded_ledger(
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-s9-account-route-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let reward_path = dir.join("rewards.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path_for_thread = block_path.clone();
    let reward_path_for_thread = reward_path.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                proof_dedup_ledger_path: None,
                scenario_path: scenario,
                block_path: block_path_for_thread,
                reward_ledger_path: Some(reward_path_for_thread),
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                genesis_override: None,
                state_dir: None,
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: true,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    (addr, handle, dir)
}

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf).to_string();
    let status_line = raw.lines().next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let (_, body_text) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no body break in: {raw}"));
    let parsed: Value =
        serde_json::from_str(body_text).unwrap_or_else(|_| panic!("body not json: {body_text}"));
    (status, parsed)
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
    let status_line = raw.lines().next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let (_, body_text) = raw.split_once("\r\n\r\n").unwrap_or(("", "{}"));
    let parsed: Value = serde_json::from_str(body_text).unwrap_or(Value::Null);
    (status, parsed)
}

fn commit_smoke_block(addr: SocketAddr) {
    let (status, resp) = http_post(addr, "/submit", &smoke_step0_submit_payload());
    assert_eq!(status, 200, "smoke share must commit block 0: {resp}");
    assert_eq!(resp["accepted"], true, "smoke share accepted: {resp}");
}

#[test]
fn account_balance_returns_recovered_balance_for_known_pk() {
    let (addr, handle, dir) = boot_with_seeded_ledger(2);
    commit_smoke_block(addr);
    let (status, body) = http_get(addr, &format!("/account/{PK_B}/balance"));
    assert_eq!(status, 200, "expected 200, got {status}: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["pk"], PK_B);
    assert_eq!(
        body["balance"], "2",
        "PK_B mined the block-0 share and proposed the block (1 + 1)"
    );
    assert_eq!(body["asOfHeight"], 0);
    let as_of_c = body["asOfC"].as_str().expect("asOfC string");
    assert_eq!(as_of_c.len(), 64);

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn account_balance_returns_zero_for_unknown_pk_not_404() {
    let (addr, handle, dir) = boot_with_seeded_ledger(1);
    let (status, body) = http_get(addr, &format!("/account/{PK_UNKNOWN}/balance"));
    assert_eq!(status, 200, "unknown pk must be 200, not 404: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["pk"], PK_UNKNOWN);
    assert_eq!(body["balance"], "0");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn account_balance_rejects_malformed_pk_with_400() {
    let (addr, handle, dir) = boot_with_seeded_ledger(1);
    let (status, body) = http_get(addr, "/account/notalongenoughhex/balance");
    assert_eq!(status, 400, "malformed pk must be 400: {body}");
    assert_eq!(body["ok"], false);
    assert_eq!(body["reason"], "malformed_pk");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn account_balance_returns_proposer_credit_too() {
    let (addr, handle, dir) = boot_with_seeded_ledger(2);
    commit_smoke_block(addr);
    let (status, body) = http_get(addr, &format!("/account/{PK_B}/balance"));
    assert_eq!(status, 200, "expected 200, got {status}: {body}");
    assert_eq!(
        body["balance"], "2",
        "the proposer credit is folded into PK_B's balance alongside the share credit"
    );
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}
