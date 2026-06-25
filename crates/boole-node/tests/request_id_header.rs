//! P0.5 slice 66 — every HTTP response carries an `x-request-id` header.
//!
//! The L8 observability contract requires a request id that ties a log
//! line, a tracing span, and an operator's `curl` together. This slice
//! installs a node-wide middleware that stamps a unique id on every
//! served response (and enters a tracing span carrying it). Echoing the
//! id into every response *envelope* and every ledger line is a larger,
//! per-handler change deferred to a follow-up slice; the header is the
//! minimal, non-invasive surface that satisfies the propagation contract
//! today.
//!
//! Contract pinned here:
//!   * every response carries a non-empty `x-request-id` header;
//!   * two distinct requests get two distinct ids (per-request, not
//!     per-process constant).

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

/// Returns (status, x-request-id header value or empty).
fn http_get_with_request_id(addr: SocketAddr, path: &str) -> (u16, String) {
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
    let headers = raw.split_once("\r\n\r\n").map(|(h, _)| h).unwrap_or(&raw);
    let request_id = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name.trim().eq_ignore_ascii_case("x-request-id") {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();
    (status, request_id)
}

#[test]
fn every_response_carries_a_unique_request_id_header() {
    let dir = std::env::temp_dir().join(format!(
        "boole-request-id-{}-{}",
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

    let (status_a, id_a) = http_get_with_request_id(addr, "/live");
    assert_eq!(status_a, 200, "GET /live must return 200");
    assert!(
        !id_a.is_empty(),
        "every response must carry a non-empty x-request-id header"
    );

    let (status_b, id_b) = http_get_with_request_id(addr, "/live");
    assert_eq!(status_b, 200, "second GET /live must return 200");
    assert!(
        !id_b.is_empty(),
        "second response must also carry an x-request-id header"
    );

    assert_ne!(
        id_a, id_b,
        "x-request-id must be unique per request, not a per-process constant"
    );

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    let _ = std::fs::remove_dir_all(&dir);
}
