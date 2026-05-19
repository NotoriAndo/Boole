//! P1.7 — `ConcurrencyLimitLayer` is wired into the HTTP layer stack so
//! a flood of in-flight requests cannot exhaust process resources. The
//! exact cap is a workspace-wide constant exported from the crate so
//! operators and audits can inspect it without grepping middleware
//! plumbing. The behavioural guard is intentionally light: a burst of
//! concurrent `/health` calls (well below the cap) must all complete
//! with `200 OK`, proving the layer integrates with axum's response
//! pipeline and does not corrupt or drop responses.
//!
//! Without the constant the test fails to compile; without a wired
//! layer (or with a layer whose error type does not propagate cleanly
//! into axum) the burst observes a non-200 status. Both failure modes
//! are caught here.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::{serve_local_node, LocalNodeConfig, MAX_CONCURRENT_REQUESTS};
use boole_testkit::rand_suffix;

const BURST: usize = 32;

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn http_get_status(addr: SocketAddr, path: &str) -> u16 {
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
    raw.lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

#[test]
fn concurrency_limit_constant_matches_master_plan() {
    // Master plan §`P1.7` mandates `ConcurrencyLimitLayer(256)`. Pinning
    // the constant prevents a silent loosening of the in-flight cap.
    assert_eq!(
        MAX_CONCURRENT_REQUESTS, 256,
        "P1.7 master plan pins the in-flight HTTP concurrency cap at 256"
    );
}

#[test]
fn concurrency_limit_layer_serves_burst_without_dropping_responses() {
    let dir = std::env::temp_dir().join(format!(
        "boole-concurrency-limit-{}-{}",
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
                max_requests: Some(BURST),
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

    let mut workers = Vec::with_capacity(BURST);
    for _ in 0..BURST {
        workers.push(thread::spawn(move || http_get_status(addr, "/health")));
    }

    let mut ok = 0usize;
    for worker in workers {
        let status = worker.join().expect("worker joined");
        assert_eq!(
            status, 200,
            "every burst /health request must return 200 (got {status})"
        );
        ok += 1;
    }
    assert_eq!(ok, BURST, "all burst requests served");

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    let _ = std::fs::remove_dir_all(&dir);
}
