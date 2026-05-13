//! S11 — `GET /bounties` and `GET /bounties/:id` routes.
//!
//! Boots a node with `LocalNodeConfig.bounties_path` set to the v1
//! fixture, and confirms:
//!   - `/bounties` returns 200 with the loaded catalog list.
//!   - `/bounties/:id` returns 200 + the matching bounty on hit.
//!   - `/bounties/:id` returns 404 + typed envelope on miss.
//!   - When `bounties_path: None`, `/bounties` still serves 200 with
//!     an empty list (catalog is optional, not required).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::local_node::{serve_local_node, LocalNodeConfig};
use serde_json::Value;

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn bounty_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/bounties/v1.json")
        .canonicalize()
        .expect("bounty fixture path")
}

fn boot_with_bounties(
    bounties_path: Option<PathBuf>,
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-s11-bounty-route-{}-{}",
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
                scenario_path: scenario,
                block_path: block_path_for_thread,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                genesis_override: None,
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
fn bounty_list_returns_loaded_catalog() {
    let (addr, handle, dir) = boot_with_bounties(Some(bounty_fixture_path()), 1);
    let (status, body) = http_get(addr, "/bounties");
    assert_eq!(status, 200, "expected 200, got {status}: {body}");
    assert_eq!(body["ok"], true);
    let bounties = body["bounties"]
        .as_array()
        .unwrap_or_else(|| panic!("bounties must be array: {body}"));
    assert_eq!(bounties.len(), 2);
    assert_eq!(bounties[0]["id"], "alpha-1");
    assert_eq!(bounties[0]["domain"], "lean.protocol-invariant");
    assert_eq!(bounties[0]["status"], "open");
    assert_eq!(bounties[0]["reward"], "42");
    assert_eq!(bounties[1]["id"], "beta-1");
    assert_eq!(bounties[1]["status"], "solved");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bounty_by_id_returns_match() {
    let (addr, handle, dir) = boot_with_bounties(Some(bounty_fixture_path()), 1);
    let (status, body) = http_get(addr, "/bounties/alpha-1");
    assert_eq!(status, 200, "expected 200, got {status}: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["bounty"]["id"], "alpha-1");
    assert_eq!(body["bounty"]["domain"], "lean.protocol-invariant");
    assert_eq!(
        body["bounty"]["verifier"]["metadata"]["verifierHash"],
        "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"
    );
    assert_eq!(body["bounty"]["reward"], "42");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bounty_by_id_unknown_returns_404_typed() {
    let (addr, handle, dir) = boot_with_bounties(Some(bounty_fixture_path()), 1);
    let (status, body) = http_get(addr, "/bounties/no-such-bounty");
    assert_eq!(status, 404, "expected 404, got {status}: {body}");
    assert_eq!(body["ok"], false);
    assert_eq!(body["reason"], "bounty_not_found");
    assert_eq!(body["id"], "no-such-bounty");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bounty_routes_empty_when_no_catalog_path() {
    let (addr, handle, dir) = boot_with_bounties(None, 2);
    let (status, body) = http_get(addr, "/bounties");
    assert_eq!(status, 200, "expected 200, got {status}: {body}");
    assert_eq!(body["ok"], true);
    let bounties = body["bounties"]
        .as_array()
        .unwrap_or_else(|| panic!("bounties must be array: {body}"));
    assert!(
        bounties.is_empty(),
        "expected empty list when no catalog path: {body}"
    );

    let (status_404, body_404) = http_get(addr, "/bounties/anything");
    assert_eq!(status_404, 404);
    assert_eq!(body_404["reason"], "bounty_not_found");
    assert_eq!(body_404["id"], "anything");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}
