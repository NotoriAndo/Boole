//! P1.6 closure — per-signer nonce-replay rejection matrix across the
//! mutating signed-envelope routes that the per-route test files did not
//! already cover.
//!
//! `bounty_announce_route.rs` and `bounty_proof_route.rs` already pin
//! `409 nonce_replayed` on `POST /bounties` and `POST
//! /bounties/{id}/proof`. The remaining three mutating routes —
//! `POST /receipts`, `POST /sessions/{sessionPk}/revoke`, and
//! `POST /bounties/{id}/status` — all call the same
//! `check_signed_envelope_nonce_not_replayed` + `burn_signed_envelope_nonce`
//! pair, but only when the node is booted with a `signed_nonce_ledger`.
//! Without that ledger the burn is a no-op and a replay is silently
//! accepted, so each test here boots with the ledger configured, sends a
//! well-formed signed request that the route accepts (200), then resends
//! a DIFFERENT payload re-using the SAME `(signerPk, nonce)` and asserts
//! the second request is rejected `409 nonce_replayed` carrying the
//! signer pk and the replayed nonce.
//!
//! The reused-nonce-with-distinct-payload shape matters: it proves the
//! ledger keys on `(signerPk, nonce)` and not on a payload hash, so an
//! attacker cannot get a fresh acceptance by tweaking the body while
//! replaying a captured nonce.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{
    Bounty, BountyProofVerifier, ReceiptCommitment, ReceiptCommitmentInput, SigningKeyV2,
};
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

const PK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PK_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ROOT: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PK_D: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const ARTIFACT_HASH: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const REQUEST_HASH: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const REWARD_RECIPIENT: &str = "4444444444444444444444444444444444444444444444444444444444444444";

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

/// Per-route boot knobs. Every boot pins a `signed_nonce_ledger` so the
/// nonce burn is real; routes differ only in which store they need.
#[derive(Default)]
struct BootOpts {
    with_receipt_store: bool,
    with_session_registry: bool,
    with_bounty_events: bool,
    with_mock_bounties: bool,
}

fn boot(label: &str, max_requests: usize, opts: BootOpts) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-p1-6-nonce-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let signed_nonce_ledger_path = Some(dir.join("signed-nonces.ndjson"));
    let receipt_commitment_ledger_path =
        opts.with_receipt_store.then(|| dir.join("receipts.ndjson"));
    let session_registry_path = opts
        .with_session_registry
        .then(|| dir.join("sessions.ndjson"));
    let bounty_event_ledger_path = opts
        .with_bounty_events
        .then(|| dir.join("bounty-events.ndjson"));
    let bounty_verifiers = opts.with_bounty_events.then(mock_verifiers);
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
                receipt_commitment_ledger_path,
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
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

fn signed_envelope(payload: &Value, key: &SigningKeyV2) -> Value {
    let signed = key.sign(payload).expect("sign");
    json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    })
}

fn assert_nonce_replayed(status: u16, resp: &Value, signer_pk: &str, nonce: &str) {
    assert_eq!(
        status, 409,
        "replayed (signerPk, nonce) must be rejected 409: {resp}"
    );
    assert_eq!(
        resp["ok"], false,
        "replay envelope ok must be false: {resp}"
    );
    assert_eq!(resp["reason"], "nonce_replayed", "reason: {resp}");
    assert_eq!(resp["signerPk"], signer_pk, "signerPk echoed: {resp}");
    assert_eq!(resp["nonce"], nonce, "replayed nonce echoed: {resp}");
}

fn finish(b: Boot) {
    b.handle.join().expect("server thread").expect("server ok");
    let _ = std::fs::remove_dir_all(&b.dir);
}

fn fixture_commitment() -> ReceiptCommitment {
    ReceiptCommitment::new(ReceiptCommitmentInput {
        agent_pk: PK_A.to_string(),
        family_id: "v1-lenbound".to_string(),
        verifier_id: "lean-runner-v01".to_string(),
        verifier_hash_version: "v0".to_string(),
        artifact_hash: ARTIFACT_HASH.to_string(),
        request_hash: REQUEST_HASH.to_string(),
        result: "accepted".to_string(),
        fee_charged: "1".to_string(),
        reward_recipient: REWARD_RECIPIENT.to_string(),
    })
    .expect("valid fixture commitment")
}

/// Mirror of session_route.rs::fixture_session — the full
/// `boole.sessions.register.v1` session object the registry validates.
fn fixture_session() -> Value {
    json!({
        "sessionPk": PK_A,
        "ownerPk": PK_B,
        "agentPk": PK_C,
        "fixedRewardRecipient": PK_D,
        "allowedFamilyRoot": ROOT,
        "maxFeePerRequest": "12",
        "activationHeight": 0,
        "expiryHeight": 100,
        "revoked": false,
        "policyHash": ROOT,
    })
}

/// `fixture_session` owned by `owner`. The P1.6 authorization check requires
/// the register/revoke envelope signer's pk to equal the session's `ownerPk`.
fn owned_session(owner: &SigningKeyV2) -> Value {
    let mut session = fixture_session();
    session["ownerPk"] = json!(owner.pk_hex());
    session
}

#[test]
fn receipts_replayed_nonce_returns_409() {
    let booted = boot(
        "receipts",
        2,
        BootOpts {
            with_receipt_store: true,
            ..Default::default()
        },
    );
    let key = SigningKeyV2::from_dev_id("p1-6-nonce-receipts");
    let pk_hex = key.pk_hex();
    let reused = "ffffffffffffffffffffffffffffffff";

    let mut first = json!({
        "schema": "boole.receipts.commit.v1",
        "receiptCommitment": fixture_commitment(),
        "validBefore": valid_before_fresh(),
        "nonce": reused,
    });
    let (s1, r1) = http_post(booted.addr, "/receipts", &signed_envelope(&first, &key));
    assert_eq!(s1, 200, "first receipt must succeed: {r1}");

    // Distinct payload (bump validBefore) but reuse the same nonce.
    first["validBefore"] = json!(valid_before_fresh() + 1);
    let (s2, r2) = http_post(booted.addr, "/receipts", &signed_envelope(&first, &key));
    assert_nonce_replayed(s2, &r2, &pk_hex, reused);
    finish(booted);
}

#[test]
fn sessions_revoke_replayed_nonce_returns_409() {
    let booted = boot(
        "sessions-revoke",
        3,
        BootOpts {
            with_session_registry: true,
            ..Default::default()
        },
    );
    let key = SigningKeyV2::from_dev_id("p1-6-nonce-sess-rev");
    let pk_hex = key.pk_hex();
    let reused = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

    // Register the session first so revoke has a target.
    let register = json!({
        "schema": "boole.sessions.register.v1",
        "session": owned_session(&key),
        "currentHeight": 0,
        "validBefore": valid_before_fresh(),
        "nonce": format!("reg-{}", rand_suffix()),
    });
    let (sr, rr) = http_post(booted.addr, "/sessions", &signed_envelope(&register, &key));
    assert_eq!(sr, 200, "register must succeed: {rr}");

    let mut revoke = json!({
        "schema": "boole.sessions.revoke.v1",
        "sessionPk": PK_A,
        "height": 7,
        "validBefore": valid_before_fresh(),
        "nonce": reused,
    });
    let (s1, r1) = http_post(
        booted.addr,
        &format!("/sessions/{PK_A}/revoke"),
        &signed_envelope(&revoke, &key),
    );
    assert_eq!(s1, 200, "first revoke must succeed: {r1}");

    // Distinct payload (bump height) but reuse the same nonce.
    revoke["height"] = json!(8);
    let (s2, r2) = http_post(
        booted.addr,
        &format!("/sessions/{PK_A}/revoke"),
        &signed_envelope(&revoke, &key),
    );
    assert_nonce_replayed(s2, &r2, &pk_hex, reused);
    finish(booted);
}

#[test]
fn bounty_status_replayed_nonce_returns_409() {
    let booted = boot(
        "bounty-status",
        2,
        BootOpts {
            with_bounty_events: true,
            with_mock_bounties: true,
            ..Default::default()
        },
    );
    let key = SigningKeyV2::from_dev_id("p1-6-nonce-status");
    let pk_hex = key.pk_hex();
    let reused = "dddddddddddddddddddddddddddddddd";

    let mut first = json!({
        "schema": "boole.bounty.status.v1",
        "id": "gamma-1",
        "newStatus": "withdrawn",
        "ts": 1800001100000_u64,
        "validBefore": valid_before_fresh(),
        "nonce": reused,
    });
    let (s1, r1) = http_post(
        booted.addr,
        "/bounties/gamma-1/status",
        &signed_envelope(&first, &key),
    );
    assert_eq!(s1, 200, "first status change must succeed: {r1}");

    // Distinct payload (bump ts) but reuse the same nonce.
    first["ts"] = json!(1800001100001_u64);
    let (s2, r2) = http_post(
        booted.addr,
        "/bounties/gamma-1/status",
        &signed_envelope(&first, &key),
    );
    assert_nonce_replayed(s2, &r2, &pk_hex, reused);
    finish(booted);
}
