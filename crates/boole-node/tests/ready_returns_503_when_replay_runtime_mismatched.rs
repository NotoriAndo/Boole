//! P2.6 readiness predicate audit (2026-05-18 design review concern #4).
//!
//! `/ready` must not be a static `200 OK`. It must evaluate the real
//! readiness preconditions on every request and surface a structured
//! `503 Service Unavailable` envelope when any precondition fails, so
//! orchestrators (systemd, k8s, supervisord) can correctly gate traffic
//! and the failure class is named in the body for operator diagnosis.
//!
//! This slice covers the first precondition: `replayMatchesRuntime`.
//! When the on-disk block file diverges from the in-memory runtime
//! (process-external tampering, partial truncation, FS rollback), the
//! probe must report `503 { ok: false, probe: "ready", reason:
//! "replay_runtime_mismatch", checks: { replay_matches_runtime: false } }`.
//!
//! The fault is injected the same way the existing
//! `status_replay_matches_runtime_live` test injects it: by
//! bypass-appending a phantom block to the durable block file via
//! `FileBlockStore::append`, which the in-memory runtime did not
//! observe through the commit path.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_core::PersistedBlock;
use boole_node::{serve_local_node, FileBlockStore, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::Value;

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn replay_fixture_block() -> PersistedBlock {
    #[derive(serde::Deserialize)]
    struct Fixture {
        blocks: Vec<PersistedBlock>,
    }
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("replay fixture parses");
    fixture
        .blocks
        .into_iter()
        .next()
        .expect("replay fixture has at least one block")
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
fn ready_returns_503_when_replay_runtime_diverges_post_boot() {
    let dir = std::env::temp_dir().join(format!(
        "boole-ready-replay-{}-{}",
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
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));

    let (status, body) = http_get(addr, "/ready");
    assert_eq!(
        status, 200,
        "first GET /ready on a clean boot must return 200 (replay matches \
         runtime; body: {body})"
    );
    assert_eq!(
        body.get("ok"),
        Some(&Value::Bool(true)),
        "/ready body must report ok=true on a healthy boot, got {body}"
    );
    assert_eq!(
        body.get("probe").and_then(Value::as_str),
        Some("ready"),
        "/ready body must tag probe=\"ready\", got {body}"
    );
    assert_eq!(
        body.pointer("/checks/replay_matches_runtime"),
        Some(&Value::Bool(true)),
        "/ready body must expose checks.replay_matches_runtime=true on a \
         healthy boot so operators can audit which preconditions passed, \
         got {body}"
    );

    let phantom = replay_fixture_block();
    FileBlockStore::append(&block_path, &phantom)
        .expect("bypass-append a phantom block to durable disk");

    let (status2, body2) = http_get(addr, "/ready");
    assert_eq!(
        status2, 503,
        "after bypass-appending a phantom block to disk, /ready must \
         return 503 Service Unavailable; a static 200 would let \
         orchestrators route traffic to a divergent node. Body: {body2}"
    );
    assert_eq!(
        body2.get("ok"),
        Some(&Value::Bool(false)),
        "/ready failure body must report ok=false, got {body2}"
    );
    assert_eq!(
        body2.get("probe").and_then(Value::as_str),
        Some("ready"),
        "/ready failure body must still tag probe=\"ready\" so the \
         orchestrator can correlate the response with the probe target, \
         got {body2}"
    );
    assert_eq!(
        body2.get("reason").and_then(Value::as_str),
        Some("replay_runtime_mismatch"),
        "/ready failure body must name the failure class so operators \
         can diagnose without scraping logs, got {body2}"
    );
    assert_eq!(
        body2.pointer("/checks/replay_matches_runtime"),
        Some(&Value::Bool(false)),
        "/ready failure body must expose checks.replay_matches_runtime \
         = false so future preconditions can be added alongside it \
         without breaking the shape, got {body2}"
    );

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    let _ = std::fs::remove_dir_all(&dir);
}
