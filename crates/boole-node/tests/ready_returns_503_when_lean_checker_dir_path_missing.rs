//! P2.6 readiness predicate audit follow-on.
//!
//! Master plan line 297: `/ready` must return 503 unless "Lean checker
//! dir is set or explicitly disabled". The pre-audit gate accepted
//! `lean_checker_dir.is_some()` regardless of whether that path
//! actually existed on disk, which let a typoed `--lean-checker-dir`
//! flag silently boot a node whose proof verification path is broken.
//!
//! This test boots a node with `lean_checker_dir = Some(<nonexistent>)`
//! and `lean_checker_disabled = false`. The node must reject the
//! `/ready` probe with 503 and a typed reason so an orchestrator never
//! routes traffic to a node whose proof checker would fail every
//! submission. `--lean-checker-disabled` remains the supported escape
//! hatch when no checker is wanted.
//!
//! Parity with the existing `lean_checker_not_configured` test: the
//! `checks.lean_checker_configured` field must surface `false` even
//! when the missing precondition is "set but path missing" so the
//! envelope shape stays stable for downstream tools.

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
fn ready_returns_503_when_lean_checker_dir_is_set_but_path_does_not_exist() {
    let dir = std::env::temp_dir().join(format!(
        "boole-ready-lean-missing-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let bogus_lean_dir = dir.join("nonexistent-lean-checker-dir");
    assert!(
        !bogus_lean_dir.exists(),
        "precondition: the typoed lean-checker path must not exist before boot"
    );

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (ready_tx, ready_rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_for_thread = block_path.clone();
    let bogus_for_thread = bogus_lean_dir.clone();

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
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                genesis_override: None,
                state_dir: None,
                network_id: None,
                lean_checker_dir: Some(bogus_for_thread),
                lean_checker_disabled: false,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: true,
            },
        )
    });
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));

    let (status, body) = http_get(addr, "/ready");
    assert_eq!(
        status, 503,
        "GET /ready on a node booted with a non-existent --lean-checker-dir \
         must return 503; a 200 would let an orchestrator route traffic to \
         a node whose proof verification path is broken at the OS level. \
         Body: {body}"
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
        Some("lean_checker_not_configured"),
        "/ready must reuse the existing reason slug so dashboards keyed on \
         it surface both misconfiguration shapes (\"flag not passed\" and \
         \"flag passed but path missing\") uniformly, got {body}"
    );
    assert_eq!(
        body.pointer("/checks/lean_checker_configured"),
        Some(&Value::Bool(false)),
        "/ready failure body must report checks.lean_checker_configured = \
         false when the configured path is missing on disk so the envelope \
         shape stays identical to the unset-flag case, got {body}"
    );

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    let _ = std::fs::remove_dir_all(&dir);
}
