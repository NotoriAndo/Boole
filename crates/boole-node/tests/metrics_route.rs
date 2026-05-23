//! P2.6 — `/metrics` exposes a Prometheus text-format scrape surface.
//! Operators need a stable, parseable counter/gauge feed; a single
//! /status JSON blob is not consumable by Prometheus, Grafana Agent,
//! or VictoriaMetrics scrapers without a translation layer.
//!
//! Contract (Prometheus exposition format v0.0.4):
//!   * Content-Type: `text/plain; version=0.0.4`
//!   * Status: 200
//!   * Body lists each metric with `# HELP` and `# TYPE` headers, then
//!     `<name> <value>` samples.
//!
//! The slice does not yet wire counters that mutate during the
//! process lifetime; it surfaces the immediately-available
//! `node_started_at_ms` / `height` / `share_pool_size` /
//! `bounty_side_pool_total` gauges so scrapers can graph baseline
//! state from boot.

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

fn http_get_raw(addr: SocketAddr, path: &str) -> (u16, String, String) {
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
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .map(|(h, b)| (h.to_string(), b.to_string()))
        .unwrap_or((raw, String::new()));
    let content_type = head
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-type:")
                .map(|rest| rest.trim().to_string())
        })
        .unwrap_or_default();
    (status, content_type, body)
}

#[test]
fn metrics_endpoint_emits_prometheus_text_format() {
    let dir = std::env::temp_dir().join(format!(
        "boole-metrics-{}-{}",
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
            },
        )
    });
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));

    let (status, content_type, body) = http_get_raw(addr, "/metrics");
    assert_eq!(status, 200, "GET /metrics must return 200, got body={body}");
    assert!(
        content_type.contains("text/plain"),
        "content-type must be text/plain (got {content_type:?})"
    );
    assert!(
        content_type.contains("version=0.0.4"),
        "content-type must pin Prometheus exposition version 0.0.4 (got {content_type:?})"
    );

    for required in [
        "# HELP boole_node_height",
        "# TYPE boole_node_height gauge",
        "boole_node_height ",
        "# HELP boole_node_share_pool_size",
        "# TYPE boole_node_share_pool_size gauge",
        "boole_node_share_pool_size ",
        "# HELP boole_node_bounty_side_pool_total",
        "# TYPE boole_node_bounty_side_pool_total gauge",
        "boole_node_bounty_side_pool_total ",
        "# HELP boole_node_started_at_ms",
        "# TYPE boole_node_started_at_ms gauge",
        "boole_node_started_at_ms ",
    ] {
        assert!(
            body.contains(required),
            "metrics body missing required line {required:?}; body=\n{body}"
        );
    }

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    let _ = std::fs::remove_dir_all(&dir);
}
