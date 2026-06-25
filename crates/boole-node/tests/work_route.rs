//! S10 — `GET /work` and `GET /work/:id` routes.
//!
//! Boots a node with `LocalNodeConfig.work_manifests_path` set to the
//! v1 fixture, and confirms:
//!   - `/work` returns 200 with the loaded manifest list.
//!   - `/work/:id` returns 200 + the matching manifest on hit.
//!   - `/work/:id` returns 404 + typed envelope on miss.
//!   - When `work_manifests_path: None`, `/work` still serves 200 with
//!     an empty list (catalog is optional, not required).

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

fn work_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/work/v1.json")
        .canonicalize()
        .expect("work fixture path")
}

fn boot_with_work(
    work_path: Option<PathBuf>,
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-s10-work-route-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path_for_thread = block_path.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                proof_dedup_ledger_path: None,
                scenario_path: scenario,
                block_path: block_path_for_thread,
                reward_ledger_path: None,
                work_manifests_path: work_path,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: Some(max_requests),
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
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    (addr, handle, dir)
}

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf).to_string();
    let status_line = raw.lines().next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let (_, body_text) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no body break in: {raw}"));
    let parsed: Value =
        serde_json::from_str(body_text).unwrap_or_else(|_| panic!("body not json: {body_text}"));
    (status, parsed)
}

#[test]
fn work_list_returns_loaded_manifests() {
    let (addr, handle, dir) = boot_with_work(Some(work_fixture_path()), 1);
    let (status, body) = http_get(addr, "/work");
    assert_eq!(status, 200, "expected 200, got {status}: {body}");
    assert_eq!(body["ok"], true);
    let work = body["work"]
        .as_array()
        .unwrap_or_else(|| panic!("work must be array: {body}"));
    assert_eq!(work.len(), 2);
    assert_eq!(work[0]["workId"], "lean-bounty-1");
    assert_eq!(work[0]["familyId"], "lean.protocol-invariant");
    assert_eq!(work[0]["status"], "open");
    assert_eq!(work[1]["workId"], "smart-contract-invariant-v01-direct");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn work_by_id_returns_match() {
    let (addr, handle, dir) = boot_with_work(Some(work_fixture_path()), 1);
    let (status, body) = http_get(addr, "/work/lean-bounty-1");
    assert_eq!(status, 200, "expected 200, got {status}: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["work"]["workId"], "lean-bounty-1");
    assert_eq!(body["work"]["familyId"], "lean.protocol-invariant");
    assert_eq!(
        body["work"]["verifier"]["metadata"]["verifierHash"],
        "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"
    );

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn work_by_id_unknown_returns_404_typed() {
    let (addr, handle, dir) = boot_with_work(Some(work_fixture_path()), 1);
    let (status, body) = http_get(addr, "/work/no-such-work");
    assert_eq!(status, 404, "expected 404, got {status}: {body}");
    assert_eq!(body["ok"], false);
    assert_eq!(body["reason"], "work_not_found");
    assert_eq!(body["id"], "no-such-work");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn work_routes_empty_when_no_manifests_path() {
    let (addr, handle, dir) = boot_with_work(None, 2);
    let (status, body) = http_get(addr, "/work");
    assert_eq!(status, 200, "expected 200, got {status}: {body}");
    assert_eq!(body["ok"], true);
    let work = body["work"]
        .as_array()
        .unwrap_or_else(|| panic!("work must be array: {body}"));
    assert!(
        work.is_empty(),
        "expected empty list when no manifests path: {body}"
    );

    let (status_404, body_404) = http_get(addr, "/work/anything");
    assert_eq!(status_404, 404);
    assert_eq!(body_404["reason"], "work_not_found");
    assert_eq!(body_404["id"], "anything");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}
