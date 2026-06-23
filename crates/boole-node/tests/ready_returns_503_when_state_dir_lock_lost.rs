//! P2.6 d (2026-05-18 design review concern #4 closer).
//!
//! Master plan line 299: "`/ready` returns 503 unless ... state-dir
//! lock is held."
//!
//! The state-dir advisory lock (`<dir>/state.lock` flock) prevents a
//! second `boole-node` from attaching to the same on-disk state. Boot
//! acquires the lock and holds the file handle for the lifetime of the
//! process; the kernel keeps our exclusive flock as long as the file
//! descriptor stays open.
//!
//! Runtime drift case: an operator (or a misbehaving cleanup script,
//! or an `rm -rf <state-dir>`) removes the lock file out from under
//! the running node. Our flock is still held by the open FD, but the
//! contract is broken — a fresh `boole-node` at the same path can now
//! create a new `state.lock` and acquire its own exclusive lock,
//! letting two processes race on every ledger. `/ready` must surface
//! this as `503 Service Unavailable` with `reason:
//! "state_dir_lock_lost"` so an orchestrator stops routing traffic
//! before the second process can latch on.
//!
//! This test boots a node with `state_dir` configured, asserts the
//! first `/ready` returns 200 (lock file present), deletes
//! `<state_dir>/state.lock` to simulate the drift, and asserts the
//! second `/ready` returns the structured failure envelope. Replay,
//! lean, and ledgers preconditions all pass here so the body must
//! name the state-dir failure as the first-failing reason.

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
fn ready_returns_503_when_state_dir_lock_file_removed_at_runtime() {
    let dir = std::env::temp_dir().join(format!(
        "boole-ready-lock-{}-{}",
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
    let submit_nonce_ledger = dir.join("submit-nonces.ndjson");
    let signed_nonce_ledger = dir.join("signed-nonces.ndjson");
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
                max_requests: Some(2),
                operator_signer_pks: vec![],
                session_registry_path: Some(session_registry),
                submit_nonce_ledger_path: Some(submit_nonce_ledger),
                signed_nonce_ledger_path: Some(signed_nonce_ledger),
                submit_receipt_ledger_path: Some(submit_receipt_ledger),
                receipt_commitment_ledger_path: Some(receipt_commitment_ledger),
                genesis_override: None,
                state_dir: Some(state_dir_for_thread),
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: true,
            },
        )
    });
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));

    let lock_path = state_dir.join("state.lock");
    assert!(
        lock_path.is_file(),
        "boot must create the advisory lock file at {} so the drift \
         test below has something to remove",
        lock_path.display()
    );

    let (status, body) = http_get(addr, "/ready");
    assert_eq!(
        status, 200,
        "first GET /ready on a clean state-dir boot must return 200 \
         (lock file present, every other precondition satisfied; \
         body: {body})"
    );
    assert_eq!(
        body.get("ok"),
        Some(&Value::Bool(true)),
        "/ready body must report ok=true on a healthy state-dir boot, \
         got {body}"
    );
    assert_eq!(
        body.pointer("/checks/state_dir_lock_held"),
        Some(&Value::Bool(true)),
        "/ready body must expose checks.state_dir_lock_held=true on a \
         healthy boot so operators can audit which preconditions \
         passed, got {body}"
    );

    std::fs::remove_file(&lock_path).expect("remove state.lock to simulate drift");

    let (status2, body2) = http_get(addr, "/ready");
    assert_eq!(
        status2, 503,
        "after removing <state-dir>/state.lock at runtime, /ready \
         must return 503; a 200 would let a second boole-node attach \
         to the same state and race on every ledger. Body: {body2}"
    );
    assert_eq!(
        body2.get("ok"),
        Some(&Value::Bool(false)),
        "/ready failure body must report ok=false, got {body2}"
    );
    assert_eq!(
        body2.get("probe").and_then(Value::as_str),
        Some("ready"),
        "/ready failure body must still tag probe=\"ready\", got {body2}"
    );
    assert_eq!(
        body2.get("reason").and_then(Value::as_str),
        Some("state_dir_lock_lost"),
        "/ready failure body must name the failure class so operators \
         can diagnose without scraping logs, got {body2}"
    );
    assert_eq!(
        body2.pointer("/checks/state_dir_lock_held"),
        Some(&Value::Bool(false)),
        "/ready failure body must expose checks.state_dir_lock_held \
         = false so this precondition appears alongside the others \
         without breaking the shape, got {body2}"
    );
    assert_eq!(
        body2.pointer("/checks/replay_matches_runtime"),
        Some(&Value::Bool(true)),
        "/ready must continue to report the replay precondition's \
         status (true here — only the lock file was removed) so the \
         envelope conveys the full readiness picture. Got {body2}"
    );
    assert_eq!(
        body2.pointer("/checks/lean_checker_configured"),
        Some(&Value::Bool(true)),
        "/ready must continue to report the lean precondition's \
         status (true here since --lean-checker-disabled was set), \
         got {body2}"
    );
    assert_eq!(
        body2.pointer("/checks/ledgers_loaded"),
        Some(&Value::Bool(true)),
        "/ready must continue to report the ledgers precondition's \
         status (true here since all four agent-wallet ledgers were \
         configured), got {body2}"
    );

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    let _ = std::fs::remove_dir_all(&dir);
}
