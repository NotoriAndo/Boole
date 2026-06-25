//! P2.10 slice 59 — cross-network rejection on signed-envelope routes.
//!
//! Boots a real local node pinned to `network_id = "boole-testnet"` and
//! drives `POST /bounties` (the `boole.bounty.announce.v1` route) with
//! three envelope shapes:
//!
//! 1. Legacy (no wire `network_id` field, signed with the legacy
//!    `SigningKeyV2::sign` path) — must keep returning 200 so existing
//!    clients keep working during the migration window.
//! 2. Matching wire `network_id: "boole-testnet"` signed via
//!    `sign_for_network(payload, Some("boole-testnet"))` — must return
//!    200 (network-bound digest verifies against the pinned node).
//! 3. Mismatched wire `network_id: "boole-dev"` signed via
//!    `sign_for_network(payload, Some("boole-dev"))` — must return 403
//!    with the typed `cross_network_rejected` envelope before any crypto
//!    runs, carrying `expected: "boole-testnet"` and `got: "boole-dev"`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{Bounty, BountyProofVerifier, SigningKeyV2};
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

const ANNOUNCE_SCHEMA: &str = "boole.bounty.announce.v1";
const PROBLEM_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const NODE_NETWORK_ID: &str = "boole-testnet";

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
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

struct BootResult {
    addr: SocketAddr,
    handle: thread::JoinHandle<anyhow::Result<()>>,
    dir: PathBuf,
}

fn boot(network_id: Option<&str>, max_requests: usize) -> BootResult {
    let dir = std::env::temp_dir().join(format!(
        "boole-p2-10-cross-network-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let event_path = dir.join("bounty-events.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let verifiers = mock_verifiers();
    let pinned = network_id.map(|s| s.to_string());

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
                bounties_path: None,
                bounty_event_ledger_path: Some(event_path),
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
                network_id: pinned,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: true,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    BootResult { addr, handle, dir }
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
    let (_, body_text) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no body break in: {raw}"));
    let parsed: Value =
        serde_json::from_str(body_text).unwrap_or_else(|_| panic!("body not json: {body_text}"));
    (status, parsed)
}

fn valid_before_fresh() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 60)
        .unwrap_or(u64::MAX / 2)
}

fn announce_payload(id: &str, ts: u64) -> Value {
    json!({
        "schema": ANNOUNCE_SCHEMA,
        "id": id,
        "domain": "code.spec-template",
        "problemHash": PROBLEM_HASH,
        "verifier": {
            "kind": "mock-accept",
            "metadata": {
                "verifierHash": "2222222222222222222222222222222222222222222222222222222222222222"
            }
        },
        "reward": "100",
        "deadline": 1900000000000_u64,
        "ts": ts,
        "validBefore": valid_before_fresh(),
        "nonce": format!("nonce-{}", rand_suffix()),
    })
}

fn legacy_signed_envelope(payload: &Value, key: &SigningKeyV2) -> Value {
    let signed = key.sign(payload).expect("sign");
    json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    })
}

fn network_signed_envelope(payload: &Value, key: &SigningKeyV2, network_id: &str) -> Value {
    let signed = key
        .sign_for_network(payload, Some(network_id))
        .expect("sign_for_network");
    json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
        "network_id": network_id,
    })
}

#[test]
fn legacy_envelope_without_network_id_keeps_returning_200() {
    let booted = boot(Some(NODE_NETWORK_ID), 1);
    let key = SigningKeyV2::from_dev_id("p2-10-legacy");
    let payload = announce_payload("p2-10-legacy", 1800000300000);
    let envelope = legacy_signed_envelope(&payload, &key);

    let (status, resp) = http_post(booted.addr, "/bounties", &envelope);
    assert_eq!(
        status, 200,
        "legacy (no wire network_id) must keep verifying via the absent-id fallback: {resp}"
    );
    assert_eq!(resp["ok"], true);
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn matching_network_id_envelope_returns_200() {
    let booted = boot(Some(NODE_NETWORK_ID), 1);
    let key = SigningKeyV2::from_dev_id("p2-10-match");
    let payload = announce_payload("p2-10-match", 1800000400000);
    let envelope = network_signed_envelope(&payload, &key, NODE_NETWORK_ID);

    let (status, resp) = http_post(booted.addr, "/bounties", &envelope);
    assert_eq!(
        status, 200,
        "matching wire network_id must verify under the network-bound digest: {resp}"
    );
    assert_eq!(resp["ok"], true);
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn mismatched_network_id_envelope_returns_403_cross_network_rejected() {
    let booted = boot(Some(NODE_NETWORK_ID), 1);
    let key = SigningKeyV2::from_dev_id("p2-10-mismatch");
    let payload = announce_payload("p2-10-mismatch", 1800000500000);
    let envelope = network_signed_envelope(&payload, &key, "boole-dev");

    let (status, resp) = http_post(booted.addr, "/bounties", &envelope);
    assert_eq!(
        status, 403,
        "mismatched wire network_id must be policy-rejected pre-crypto: {resp}"
    );
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "cross_network_rejected");
    assert_eq!(resp["expected"], NODE_NETWORK_ID);
    assert_eq!(resp["got"], "boole-dev");
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}
