//! P2.6 — `/status.replayMatchesRuntime` must reflect the **current**
//! disk-vs-runtime consistency state, not a static boot-time snapshot.
//!
//! Operators rely on the field to detect post-boot drift such as:
//!   * Process-external tampering of the block file
//!   * Filesystem rollback or partial truncation
//!   * Disk-runtime divergence introduced by a future refactor
//!
//! Contract: after the field has been observed `true` against an empty
//! chain, writing a *new, runtime-bypassed* block to the durable file
//! must flip the next /status read to `false`. A static boot-time value
//! cannot satisfy this — only a per-request recomputation can.

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
fn status_replay_matches_runtime_recomputes_on_each_request() {
    let dir = std::env::temp_dir().join(format!(
        "boole-node-replay-live-{}-{}",
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
            },
        )
    });
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));

    let (status, body) = http_get(addr, "/status");
    assert_eq!(status, 200, "first GET /status must return 200");
    assert_eq!(
        body.get("replayMatchesRuntime"),
        Some(&Value::Bool(true)),
        "with no blocks committed, replay and runtime trivially agree, got {body}"
    );

    let phantom = replay_fixture_block();
    FileBlockStore::append(&block_path, &phantom)
        .expect("bypass-append a phantom block to durable disk");

    let (status2, body2) = http_get(addr, "/status");
    assert_eq!(status2, 200, "second GET /status must return 200");
    assert_eq!(
        body2.get("replayMatchesRuntime"),
        Some(&Value::Bool(false)),
        "after bypass-appending a phantom block to disk, the live check \
         must report mismatch; a static boot-time value would still be true. \
         Body: {body2}"
    );

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    let _ = std::fs::remove_dir_all(&dir);
}
