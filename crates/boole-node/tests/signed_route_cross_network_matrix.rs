//! P1.6 closure — cross-network rejection matrix across every mutating
//! signed-envelope route.
//!
//! `cross_network_rejection.rs` (slice 59) proved the
//! `parse_envelope_network_id` → `403 cross_network_rejected` path for
//! exactly one route: `POST /bounties` (announce). All six mutating
//! routes run the same `parse_envelope_network_id` check against the
//! node's pinned `network_id` *before* any crypto, so a regression on
//! any one of them would silently let a testnet-signed envelope through
//! on a dev node (or vice versa). This file pins the rejection on the
//! five routes the slice-59 test did not cover:
//!
//!   * `POST /receipts`
//!   * `POST /sessions`
//!   * `POST /sessions/{sessionPk}/revoke`
//!   * `POST /bounties/{id}/status`
//!   * `POST /bounties/{id}/proof`
//!
//! Each test boots a real local node pinned to `network_id =
//! "boole-testnet"`, then drives the route with an envelope signed via
//! `sign_for_network(payload, Some("boole-dev"))` and a wire
//! `network_id: "boole-dev"`. The node must reject with `403
//! cross_network_rejected` carrying `expected: "boole-testnet"` and
//! `got: "boole-dev"` before running the ed25519 check.

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

const NODE_NETWORK_ID: &str = "boole-testnet";
const FOREIGN_NETWORK_ID: &str = "boole-dev";
const PROBLEM_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const VERIFIER_HASH: &str = "2222222222222222222222222222222222222222222222222222222222222222";

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

fn fresh_nonce() -> String {
    format!("nonce-{}", rand_suffix())
}

fn valid_before_fresh() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 60)
        .unwrap_or(u64::MAX / 2)
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

struct Boot {
    addr: SocketAddr,
    handle: thread::JoinHandle<anyhow::Result<()>>,
    dir: PathBuf,
}

/// Knobs each route needs flipped on in `LocalNodeConfig`. Everything
/// else stays `None`/default so the node is the minimal surface for the
/// route under test, always pinned to `network_id = boole-testnet`.
#[derive(Default)]
struct BootOpts {
    with_bounty_events: bool,
    with_session_registry: bool,
    with_signed_nonce_ledger: bool,
    /// Load the mock bounty catalog (id `gamma-1`). The proof route
    /// resolves the bounty out of the catalog before the network gate,
    /// so its cross-network test needs a real bounty to reach the gate.
    with_mock_bounties: bool,
}

fn boot(label: &str, opts: BootOpts) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-p1-6-xnet-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");

    let bounty_event_ledger_path = opts
        .with_bounty_events
        .then(|| dir.join("bounty-events.ndjson"));
    let bounty_verifiers = opts.with_bounty_events.then(mock_verifiers);
    let session_registry_path = opts
        .with_session_registry
        .then(|| dir.join("sessions.ndjson"));
    let signed_nonce_ledger_path = opts
        .with_signed_nonce_ledger
        .then(|| dir.join("signed-nonces.ndjson"));
    let bounties_path = opts.with_mock_bounties.then(mock_bounty_fixture_path);

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();

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
                bounties_path,
                bounty_event_ledger_path,
                bounty_verifiers,
                family_manifests_dir: None,
                session_registry_path,
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                max_requests: Some(1),
                operator_signer_pks: vec![],
                genesis_override: None,
                state_dir: None,
                network_id: Some(NODE_NETWORK_ID.to_string()),
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: true,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Boot { addr, handle, dir }
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

/// Sign `payload` for the FOREIGN network and stamp the foreign wire
/// `network_id`, so the pinned node must policy-reject before crypto.
fn foreign_network_envelope(payload: &Value, key: &SigningKeyV2) -> Value {
    let signed = key
        .sign_for_network(payload, Some(FOREIGN_NETWORK_ID))
        .expect("sign_for_network");
    json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
        "network_id": FOREIGN_NETWORK_ID,
    })
}

fn assert_cross_network_rejected(status: u16, resp: &Value) {
    assert_eq!(
        status, 403,
        "mismatched wire network_id must be policy-rejected pre-crypto: {resp}"
    );
    assert_eq!(
        resp["ok"], false,
        "rejection envelope ok must be false: {resp}"
    );
    assert_eq!(
        resp["reason"], "cross_network_rejected",
        "reason must be cross_network_rejected: {resp}"
    );
    assert_eq!(
        resp["expected"], NODE_NETWORK_ID,
        "expected pinned id: {resp}"
    );
    assert_eq!(resp["got"], FOREIGN_NETWORK_ID, "got foreign id: {resp}");
}

fn finish(booted: Boot) {
    booted
        .handle
        .join()
        .expect("server thread")
        .expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn receipts_cross_network_envelope_returns_403() {
    let booted = boot("receipts", BootOpts::default());
    let key = SigningKeyV2::from_dev_id("p1-6-xnet-receipts");
    let payload = json!({
        "schema": "boole.receipts.commit.v1",
        "receiptCommitment": {
            "schema": "boole.receipt.commitment.v1",
            "agentPk": "1111111111111111111111111111111111111111111111111111111111111111",
            "familyId": "v1-lenbound",
            "verifierId": "lean-runner-v01",
            "verifierHashVersion": "v0",
            "artifactHash": "2222222222222222222222222222222222222222222222222222222222222222",
            "requestHash": "3333333333333333333333333333333333333333333333333333333333333333",
            "result": "accepted",
            "feeCharged": "1",
            "rewardRecipient": "4444444444444444444444444444444444444444444444444444444444444444"
        },
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let envelope = foreign_network_envelope(&payload, &key);
    let (status, resp) = http_post(booted.addr, "/receipts", &envelope);
    assert_cross_network_rejected(status, &resp);
    finish(booted);
}

#[test]
fn sessions_register_cross_network_envelope_returns_403() {
    let booted = boot(
        "sessions-register",
        BootOpts {
            with_session_registry: true,
            ..Default::default()
        },
    );
    let key = SigningKeyV2::from_dev_id("p1-6-xnet-sess-reg");
    let payload = json!({
        "schema": "boole.sessions.register.v1",
        "session": {
            "sessionPk": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "ownerPk": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "scopes": ["submit"],
            "issuedAtHeight": 0,
            "expiresAtHeight": 100
        },
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let envelope = foreign_network_envelope(&payload, &key);
    let (status, resp) = http_post(booted.addr, "/sessions", &envelope);
    assert_cross_network_rejected(status, &resp);
    finish(booted);
}

#[test]
fn sessions_revoke_cross_network_envelope_returns_403() {
    let booted = boot(
        "sessions-revoke",
        BootOpts {
            with_session_registry: true,
            ..Default::default()
        },
    );
    let key = SigningKeyV2::from_dev_id("p1-6-xnet-sess-rev");
    let session_pk = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let payload = json!({
        "schema": "boole.sessions.revoke.v1",
        "sessionPk": session_pk,
        "height": 0,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let envelope = foreign_network_envelope(&payload, &key);
    let (status, resp) = http_post(
        booted.addr,
        &format!("/sessions/{session_pk}/revoke"),
        &envelope,
    );
    assert_cross_network_rejected(status, &resp);
    finish(booted);
}

#[test]
fn bounty_status_cross_network_envelope_returns_403() {
    let booted = boot(
        "bounty-status",
        BootOpts {
            with_bounty_events: true,
            ..Default::default()
        },
    );
    let key = SigningKeyV2::from_dev_id("p1-6-xnet-status");
    let payload = json!({
        "schema": "boole.bounty.status.v1",
        "id": "p1-6-xnet-status",
        "status": "paused",
        "ts": 1800000600000_u64,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let envelope = foreign_network_envelope(&payload, &key);
    let (status, resp) = http_post(booted.addr, "/bounties/p1-6-xnet-status/status", &envelope);
    assert_cross_network_rejected(status, &resp);
    finish(booted);
}

#[test]
fn bounty_proof_cross_network_envelope_returns_403() {
    // The proof route resolves the bounty from the catalog before the
    // network gate, so boot with the mock catalog (id `gamma-1`) and
    // target it; otherwise the route short-circuits with 404
    // bounty_not_found before the cross-network check can fire.
    let booted = boot(
        "bounty-proof",
        BootOpts {
            with_bounty_events: true,
            with_mock_bounties: true,
            ..Default::default()
        },
    );
    let key = SigningKeyV2::from_dev_id("p1-6-xnet-proof");
    let proof_hash = "5555555555555555555555555555555555555555555555555555555555555555";
    let payload = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": "gamma-1",
        "proofHash": proof_hash,
        "domain": "code.spec-template",
        "problemHash": PROBLEM_HASH,
        "verifier": { "kind": "mock-accept", "metadata": { "verifierHash": VERIFIER_HASH } },
        "ts": 1800000700000_u64,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let envelope = foreign_network_envelope(&payload, &key);
    let (status, resp) = http_post(booted.addr, "/bounties/gamma-1/proof", &envelope);
    assert_cross_network_rejected(status, &resp);
    finish(booted);
}
