//! P0.5 slice 67 — `/metrics` exposes process-wide outcome counters in
//! addition to the boot-time state gauges.
//!
//! The L8 observability contract asks for typed counters so a scraper can
//! `rate()`/`increase()` over submit and proof outcomes and alert on
//! panics. This test pins that the Prometheus body declares the three
//! counters with the correct `# TYPE ... counter` header and the
//! `outcome` label dimension. (Driving an accepted submit to bump the
//! value requires a valid signed envelope; the value-increment path is
//! covered by the handler-level wiring and exercised in the full gate's
//! runtime-smoke. Here we pin the exposition contract.)

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn http_get_body(addr: SocketAddr, path: &str) -> (u16, String) {
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
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

#[test]
fn metrics_exposes_outcome_counters() {
    let dir = std::env::temp_dir().join(format!(
        "boole-metrics-counters-{}-{}",
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
                proof_dedup_ledger_path: None,
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
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: true,
            },
        )
    });
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));

    let (status, body) = http_get_body(addr, "/metrics");
    assert_eq!(status, 200, "GET /metrics must return 200");

    // submit counter: typed `counter`, both outcome labels present.
    assert!(
        body.contains("# TYPE boole_submits_total counter"),
        "metrics must declare boole_submits_total as a counter:\n{body}"
    );
    assert!(
        body.contains("boole_submits_total{outcome=\"accepted\"}"),
        "metrics must expose the accepted-submit series:\n{body}"
    );
    assert!(
        body.contains("boole_submits_total{outcome=\"rejected\"}"),
        "metrics must expose the rejected-submit series:\n{body}"
    );

    // proof counter: typed `counter`, both outcome labels present.
    assert!(
        body.contains("# TYPE boole_proofs_total counter"),
        "metrics must declare boole_proofs_total as a counter:\n{body}"
    );
    assert!(
        body.contains("boole_proofs_total{outcome=\"accepted\"}"),
        "metrics must expose the accepted-proof series:\n{body}"
    );
    assert!(
        body.contains("boole_proofs_total{outcome=\"rejected\"}"),
        "metrics must expose the rejected-proof series:\n{body}"
    );

    // panic counter: typed `counter`, wired now (incremented by slice 68).
    assert!(
        body.contains("# TYPE boole_panic_total counter"),
        "metrics must declare boole_panic_total as a counter:\n{body}"
    );
    assert!(
        body.contains("boole_panic_total 0"),
        "boole_panic_total must read 0 on a clean process:\n{body}"
    );

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    let _ = std::fs::remove_dir_all(&dir);
}
