//! S12 — Boot-time recovery: audit log replays accepted proofs as
//! bounty status flips, so a node restart shows the correct in-memory
//! status without needing to re-run the verifier.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{canonical_payload_hash_hex, Bounty, BountyProofVerifier, SigningKeyV2};
use boole_node::FileBountyEventLedger;
use boole_node::{serve_local_node, LocalNodeConfig};
// P0.1a — first proven call site for boole_testkit. The local rand_suffix()
// duplicate has been removed; later P0.1 slices migrate the remaining
// 30+ call sites to the same shared helper.
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

fn fresh_nonce() -> String {
    format!("nonce-{}", rand_suffix())
}

fn prover_key() -> SigningKeyV2 {
    SigningKeyV2::from_dev_id("bounty-recovery-test-prover")
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

fn mock_verifiers() -> HashMap<String, Arc<dyn BountyProofVerifier>> {
    let mut m: HashMap<String, Arc<dyn BountyProofVerifier>> = HashMap::new();
    m.insert("mock-accept".to_string(), Arc::new(MockAccept));
    m
}

fn boot_at(
    bounty_event_path: PathBuf,
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path = bounty_event_path
        .parent()
        .expect("parent")
        .join("blocks.ndjson");
    let bounties_path = mock_bounty_fixture_path();
    let verifiers = mock_verifiers();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                proof_dedup_ledger_path: None,
                scenario_path: scenario,
                block_path,
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
    (addr, handle)
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
    let status: u16 = raw
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let (_, body_text) = raw.split_once("\r\n\r\n").expect("body");
    let parsed: Value = serde_json::from_str(body_text).expect("body json");
    (status, parsed)
}

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let mut stream = TcpStream::connect(addr).expect("connect");
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
    let (_, body_text) = raw.split_once("\r\n\r\n").expect("body");
    let parsed: Value = serde_json::from_str(body_text).expect("body json");
    (status, parsed)
}

/// Build a `boole.signed.v1` envelope around a `boole.bounty.proof.v1`
/// payload signed by `key`.
fn signed_proof_body(key: &SigningKeyV2, bounty_id: &str, envelope: Value) -> Value {
    // §SC W1.b — the node re-derives the proof hash from the envelope's
    // canonical JSON and rejects mismatches, so the tests compute it the
    // same way instead of claiming a dummy value.
    let proof_hash = canonical_payload_hash_hex(&envelope);
    let payload = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": bounty_id,
        "proofHash": proof_hash,
        "prover": key.pk_hex(),
        "envelope": envelope,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let signed = key.sign(&payload).expect("sign proof payload");
    json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    })
}

fn valid_before_fresh() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 60)
        .unwrap_or(u64::MAX / 2)
}

#[test]
fn second_boot_replays_audit_log_to_restore_solved_status() {
    let dir = std::env::temp_dir().join(format!(
        "boole-s12-recovery-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let bounty_event_path = dir.join("bounty-events.ndjson");
    let key = prover_key();

    // Boot 1: submit accepted proof against gamma-1 → status flips to solved
    // and audit event is appended to the file.
    let (addr1, handle1) = boot_at(bounty_event_path.clone(), 1);
    let body = signed_proof_body(&key, "gamma-1", json!({}));
    let (s1, r1) = http_post(addr1, "/bounties/gamma-1/proof", &body);
    assert_eq!(s1, 200);
    assert_eq!(r1["bounty"]["status"], "solved");
    handle1.join().expect("server 1 join").expect("server 1 ok");

    // Confirm the audit log has exactly one event on disk.
    let recovered = FileBountyEventLedger::recover(&bounty_event_path).expect("recover");
    assert_eq!(recovered.len(), 1, "expected 1 event: {recovered:?}");
    assert_eq!(recovered[0]["accepted"], true);
    assert_eq!(recovered[0]["workId"], "gamma-1");

    // Boot 2: same audit log path. Replay must restore status=solved
    // BEFORE any verifier call. We exercise this by issuing a GET (no
    // verifier dispatch) and asserting status.
    let (addr2, handle2) = boot_at(bounty_event_path.clone(), 1);
    let (s2, r2) = http_get(addr2, "/bounties/gamma-1");
    assert_eq!(s2, 200);
    assert_eq!(r2["bounty"]["status"], "solved", "status must replay: {r2}");
    handle2.join().expect("server 2 join").expect("server 2 ok");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn recovered_event_is_byte_equal_to_appended_event() {
    let dir = std::env::temp_dir().join(format!(
        "boole-s12-recovery-byteq-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let bounty_event_path = dir.join("bounty-events.ndjson");
    let key = prover_key();

    let (addr, handle) = boot_at(bounty_event_path.clone(), 1);
    let body = signed_proof_body(&key, "gamma-1", json!({}));
    let (status, _) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 200);
    handle.join().expect("server").expect("server ok");

    let recovered = FileBountyEventLedger::recover(&bounty_event_path).expect("recover");
    assert_eq!(recovered.len(), 1);
    let ev = &recovered[0];
    assert_eq!(ev["schemaVersion"], 1);
    assert_eq!(ev["kind"], "proof");
    assert_eq!(ev["workId"], "gamma-1");
    assert_eq!(
        ev["problemHash"],
        "1111111111111111111111111111111111111111111111111111111111111111"
    );
    assert_eq!(ev["verifierKind"], "mock-accept");
    assert_eq!(ev["proofHash"], canonical_payload_hash_hex(&json!({})));
    assert_eq!(ev["solverPk"], key.pk_hex());
    assert_eq!(ev["accepted"], true);
    assert_eq!(ev["reward"], "100");
    assert_eq!(ev["credit"], "100");
    assert!(
        ev["ts"].is_u64() || ev["ts"].is_i64(),
        "ts must be number: {ev}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
