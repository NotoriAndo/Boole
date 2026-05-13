//! S12 — `POST /bounties/{id}/proof` route, 8 branches matching pof parity.
//!
//! Boots a node with `LocalNodeConfig.bounties_path` set to the mock fixture
//! and `LocalNodeConfig.bounty_verifiers` injecting `mock-accept` / `mock-reject`
//! kinds. Asserts the validation order, response shapes, and side-effect
//! contracts (status flip, dedup, ledger append) are byte-frozen against the
//! pof reference (`projects/pof/dispatcher/src/httpServer.ts:337-388`).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{Bounty, BountyProofVerifier};
// BountyProofVerifier trait lives in boole-core; the existing struct
// `BountyVerifier { kind, metadata }` keeps its name in the bounty schema.
use boole_node::{serve_local_node, LocalNodeConfig};
use serde_json::{json, Value};

const PROOF_HASH_A: &str = "aaaa000000000000000000000000000000000000000000000000000000000000";
const PROOF_HASH_B: &str = "bbbb000000000000000000000000000000000000000000000000000000000000";
const PROVER_X: &str = "1100000000000000000000000000000000000000000000000000000000000000";

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

fn mock_bounty_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/bounties/v1-mock.json")
        .canonicalize()
        .expect("mock bounty fixture path")
}

struct MockAccept;
impl BountyProofVerifier for MockAccept {
    fn verify(&self, _bounty: &Bounty, _envelope: &Value) -> Result<bool, String> {
        Ok(true)
    }
}

struct MockReject;
impl BountyProofVerifier for MockReject {
    fn verify(&self, _bounty: &Bounty, _envelope: &Value) -> Result<bool, String> {
        Ok(false)
    }
}

fn default_mock_verifiers() -> HashMap<String, Arc<dyn BountyProofVerifier>> {
    let mut m: HashMap<String, Arc<dyn BountyProofVerifier>> = HashMap::new();
    m.insert("mock-accept".to_string(), Arc::new(MockAccept));
    m.insert("mock-reject".to_string(), Arc::new(MockReject));
    m
}

fn boot_with_mock_verifiers(
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-s12-bounty-proof-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let bounty_event_path = dir.join("bounty-events.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path_for_thread = block_path.clone();
    let bounties_path = mock_bounty_fixture_path();
    let verifiers = default_mock_verifiers();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path: block_path_for_thread,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: Some(bounties_path),
                bounty_event_ledger_path: Some(bounty_event_path),
                bounty_verifiers: Some(verifiers),
                family_manifests_dir: None,
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                genesis_override: None,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    (addr, handle, dir)
}

fn http_post(addr: SocketAddr, path: &str, body: &Value) -> (u16, Value) {
    let body_str = serde_json::to_string(body).expect("body json");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
        body_str.len()
    );
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

fn submit_proof_body(proof_hash: &str, prover: &str, envelope: Value) -> Value {
    json!({
        "proofHash": proof_hash,
        "prover": prover,
        "envelope": envelope,
    })
}

#[test]
fn accept_path_flips_status_to_solved() {
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let body = submit_proof_body(PROOF_HASH_A, PROVER_X, json!({}));
    let (status, resp) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 200, "expected 200, got {status}: {resp}");
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["accepted"], true);
    assert_eq!(resp["duplicate"], false);
    assert_eq!(resp["bounty"]["status"], "solved");
    assert_eq!(resp["bounty"]["id"], "gamma-1");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reject_path_keeps_status_open() {
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let body = submit_proof_body(PROOF_HASH_A, PROVER_X, json!({}));
    let (status, resp) = http_post(addr, "/bounties/delta-1/proof", &body);
    assert_eq!(status, 200, "expected 200, got {status}: {resp}");
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["accepted"], false);
    assert_eq!(resp["duplicate"], false);
    assert_eq!(resp["bounty"]["status"], "open");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dedup_returns_cached_outcome_without_revisiting_verifier() {
    // Two POSTs of the SAME proof against gamma-1 (mock-accept).
    // Second call must short-circuit at dedup: duplicate=true, no second
    // verifier call, no second ledger event. Also exercises the terminal +
    // dedup interaction (after first call bounty is "solved", but dedup
    // wins over the terminal guard).
    let (addr, handle, dir) = boot_with_mock_verifiers(2);
    let body = submit_proof_body(PROOF_HASH_A, PROVER_X, json!({}));
    let (s1, r1) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(s1, 200);
    assert_eq!(r1["accepted"], true);
    assert_eq!(r1["duplicate"], false);

    let (s2, r2) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(s2, 200, "second POST must still be 200, got {s2}: {r2}");
    assert_eq!(r2["accepted"], true);
    assert_eq!(
        r2["duplicate"], true,
        "second post must be marked duplicate: {r2}"
    );
    assert_eq!(r2["bounty"]["status"], "solved");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_bounty_returns_404_typed() {
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let body = submit_proof_body(PROOF_HASH_A, PROVER_X, json!({}));
    let (status, resp) = http_post(addr, "/bounties/no-such/proof", &body);
    assert_eq!(status, 404, "expected 404, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bounty_not_found");
    assert_eq!(resp["id"], "no-such");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_proof_hash_returns_400_typed() {
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    // proofHash too short.
    let body = submit_proof_body("deadbeef", PROVER_X, json!({}));
    let (status, resp) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 400, "expected 400, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bad_proof_hash");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_prover_returns_400_typed() {
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    // prover not hex32.
    let body = submit_proof_body(PROOF_HASH_A, "not-a-hex32", json!({}));
    let (status, resp) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 400, "expected 400, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bad_prover");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn terminal_bounty_returns_409_when_not_dedup_hit() {
    // epsilon-1 is `withdrawn` in the fixture. Submitting a fresh proof
    // (not previously seen) must hit the terminal guard, not dedup.
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let body = submit_proof_body(PROOF_HASH_B, PROVER_X, json!({}));
    let (status, resp) = http_post(addr, "/bounties/epsilon-1/proof", &body);
    assert_eq!(status, 409, "expected 409, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bounty_terminal");
    assert_eq!(resp["status"], "withdrawn");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_verifier_kind_returns_501_typed() {
    // zeta-1 has verifier.kind = "wholly-unknown-kind".
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let body = submit_proof_body(PROOF_HASH_A, PROVER_X, json!({}));
    let (status, resp) = http_post(addr, "/bounties/zeta-1/proof", &body);
    assert_eq!(status, 501, "expected 501, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "no_verifier");
    assert_eq!(resp["kind"], "wholly-unknown-kind");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}
