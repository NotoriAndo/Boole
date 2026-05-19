//! P2.6 — `/live` and `/ready` must be distinct, lightweight liveness
//! and readiness probes so orchestrators (systemd, k8s, supervisord)
//! can differentiate "process running" from "process ready to serve".
//! `/health` keeps its existing detailed shape; these are the cheap
//! probes that never block on `RwLock` reads or scenario IO.
//!
//! Contract:
//!   * `GET /live`  → `200 OK`, `{"ok": true, "probe": "live"}`
//!   * `GET /ready` → `200 OK`, `{"ok": true, "probe": "ready"}`
//!
//! Failure-to-route surfaces here as the typed `404` envelope from the
//! existing `fallback_handler`; either probe returning a non-200 is a
//! regression in the route table.

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
fn live_and_ready_routes_return_distinct_probe_envelopes() {
    let dir = std::env::temp_dir().join(format!(
        "boole-live-ready-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (ready_tx, ready_rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_for_thread = block_path.clone();

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
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                genesis_override: None,
                state_dir: None,
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
            },
        )
    });
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));

    let (live_status, live_body) = http_get(addr, "/live");
    assert_eq!(live_status, 200, "GET /live must return 200");
    assert_eq!(
        live_body.get("ok"),
        Some(&Value::Bool(true)),
        "GET /live body must report ok=true, got {live_body}"
    );
    assert_eq!(
        live_body.get("probe").and_then(Value::as_str),
        Some("live"),
        "GET /live body must tag probe=live, got {live_body}"
    );

    let (ready_status, ready_body) = http_get(addr, "/ready");
    assert_eq!(ready_status, 200, "GET /ready must return 200");
    assert_eq!(
        ready_body.get("ok"),
        Some(&Value::Bool(true)),
        "GET /ready body must report ok=true, got {ready_body}"
    );
    assert_eq!(
        ready_body.get("probe").and_then(Value::as_str),
        Some("ready"),
        "GET /ready body must tag probe=ready, got {ready_body}"
    );

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    let _ = std::fs::remove_dir_all(&dir);
}
