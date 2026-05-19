//! P2.6 c (2026-05-18 design review concern #4 follow-on).
//!
//! Master plan line 298: "`/ready` returns 503 unless every ledger is
//! loaded."
//!
//! When the operator opts the node into production-mode by setting
//! `--state-dir`, the agent-wallet ledger paths
//! (`session_registry`, `submit_nonce_ledger`, `submit_receipt_ledger`,
//! `receipt_commitment_ledger`) are no longer optional — a production
//! node that holds the state-dir lock but cannot persist sessions,
//! nonces, receipts, or commitments would silently lose audit-critical
//! data on restart. `/ready` must return `503 Service Unavailable` with
//! `reason: "ledgers_not_loaded"` so an orchestrator never routes
//! traffic to a half-wired production node.
//!
//! Legacy embedding mode (`state_dir: None`) is unaffected: the four
//! ledgers remain opt-in, and `/ready` reports
//! `checks.ledgers_loaded: true` regardless of which of the four
//! agent-wallet paths are configured.
//!
//! This test boots a node with `state_dir: Some(_)` and
//! `submit_nonce_ledger_path: None` (the misconfigured combination —
//! one of the four ledgers missing) and asserts the structured
//! envelope. Replay and lean checker preconditions pass here so the
//! body must surface the ledger failure as the first-failing reason.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::Value;

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("write timeout");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
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
    let body = raw
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .unwrap_or_default();
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    (status, parsed)
}

#[test]
fn ready_returns_503_when_state_dir_set_but_an_agent_wallet_ledger_missing() {
    let dir = std::env::temp_dir().join(format!(
        "boole-ready-ledgers-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let state_dir = dir.join("state");
    std::fs::create_dir_all(&state_dir).expect("state dir");
    let block_path = dir.join("blocks.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (ready_tx, ready_rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_for_thread = block_path.clone();
    let state_dir_for_thread = state_dir.clone();
    let session_registry = dir.join("sessions.ndjson");
    let submit_receipt_ledger = dir.join("submit-receipts.ndjson");
    let receipt_commitment_ledger = dir.join("receipt-commitments.ndjson");

    let handle = thread::spawn(move || {
        ready_tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path: block_for_thread,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: Some(1),
                operator_signer_pks: vec![],
                session_registry_path: Some(session_registry),
                submit_nonce_ledger_path: None,
                submit_receipt_ledger_path: Some(submit_receipt_ledger),
                receipt_commitment_ledger_path: Some(receipt_commitment_ledger),
                genesis_override: None,
                state_dir: Some(state_dir_for_thread),
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
            },
        )
    });
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));

    let (status, body) = http_get(addr, "/ready");
    assert_eq!(
        status, 503,
        "GET /ready on a state-dir node missing one of the four \
         agent-wallet ledgers must return 503; a 200 would let an \
         orchestrator route traffic to a node that cannot persist \
         every session-bound submission. Body: {body}"
    );
    assert_eq!(
        body.get("ok"),
        Some(&Value::Bool(false)),
        "/ready failure body must report ok=false, got {body}"
    );
    assert_eq!(
        body.get("probe").and_then(Value::as_str),
        Some("ready"),
        "/ready failure body must still tag probe=\"ready\", got {body}"
    );
    assert_eq!(
        body.get("reason").and_then(Value::as_str),
        Some("ledgers_not_loaded"),
        "/ready failure body must name the failure class so operators \
         can diagnose without scraping logs, got {body}"
    );
    assert_eq!(
        body.pointer("/checks/ledgers_loaded"),
        Some(&Value::Bool(false)),
        "/ready failure body must expose checks.ledgers_loaded = false \
         so this precondition appears alongside the others without \
         breaking the shape, got {body}"
    );
    assert_eq!(
        body.pointer("/checks/replay_matches_runtime"),
        Some(&Value::Bool(true)),
        "/ready must continue to report the replay precondition's \
         status (true on a clean boot here) so the envelope conveys \
         the full readiness picture, not just the first failure. \
         Got {body}"
    );
    assert_eq!(
        body.pointer("/checks/lean_checker_configured"),
        Some(&Value::Bool(true)),
        "/ready must continue to report the lean precondition's \
         status (true here since --lean-checker-disabled was set) so \
         the envelope conveys the full readiness picture, got {body}"
    );

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    let _ = std::fs::remove_dir_all(&dir);
}
