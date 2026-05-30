//! P1.6 closure — the two `POST /sessions` (register) failure cells the
//! per-route `session_route.rs` file does not already cover:
//!
//!   * `bad_signature` — a `boole.signed.v1` envelope whose signature was
//!     computed for one payload but whose wire payload was mutated after
//!     signing must be rejected `401 signature_invalid`. `session_route.rs`
//!     covers this for the REVOKE route
//!     (`session_route_revoke_tampered_payload_returns_401_signature_invalid`)
//!     but not for REGISTER.
//!   * `expired` — a register envelope whose inner `validBefore` is in the
//!     past must be rejected `401 envelope_expired`. `session_route.rs`
//!     covers a MISSING `validBefore` (400 bad_payload) and an expired
//!     REVOKE, but not an expired REGISTER.
//!
//! The register route's nonce-replay and cross-network cells are covered
//! by `signed_route_nonce_replay_matrix.rs` and
//! `signed_route_cross_network_matrix.rs` respectively; this file closes
//! the remaining two register-specific cells of the P1.6 matrix.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_core::SigningKeyV2;
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

const PK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PK_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ROOT: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PK_D: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn fresh_nonce() -> String {
    format!("nonce-{}", rand_suffix())
}

fn valid_before_far_future() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 3600)
        .unwrap_or(u64::MAX / 2)
}

struct Boot {
    addr: SocketAddr,
    handle: thread::JoinHandle<anyhow::Result<()>>,
    dir: PathBuf,
}

fn boot(label: &str, max_requests: usize) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-p1-6-register-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let registry = dir.join("sessions.ndjson");

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
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                session_registry_path: Some(registry),
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                genesis_override: None,
                state_dir: None,
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
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

/// The full `boole.sessions.register.v1` session object the registry
/// validates — mirrors session_route.rs::fixture_session.
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

fn register_payload(valid_before: u64) -> Value {
    json!({
        "schema": "boole.sessions.register.v1",
        "session": fixture_session(),
        "currentHeight": 0,
        "validBefore": valid_before,
        "nonce": fresh_nonce(),
    })
}

fn finish(b: Boot) {
    b.handle.join().expect("server thread").expect("server ok");
    let _ = std::fs::remove_dir_all(&b.dir);
}

#[test]
fn sessions_register_tampered_payload_returns_401_signature_invalid() {
    let booted = boot("bad-sig", 1);
    let key = SigningKeyV2::from_dev_id("p1-6-register-bad-sig");

    let mut envelope = signed_envelope(&register_payload(valid_before_far_future()), &key);
    // Mutate the inner payload after signing: the signature was computed
    // for currentHeight=0 but the wire now claims 99, so the ed25519
    // check over the recanonicalized payload must fail.
    envelope["payload"]["currentHeight"] = json!(99);

    let (status, resp) = http_post(booted.addr, "/sessions", &envelope);
    assert_eq!(status, 401, "tampered register payload must be 401: {resp}");
    assert_eq!(resp["ok"], false, "rejection ok must be false: {resp}");
    assert_eq!(
        resp["reason"], "signature_invalid",
        "reason must be signature_invalid: {resp}"
    );
    finish(booted);
}

#[test]
fn sessions_register_expired_valid_before_returns_401_envelope_expired() {
    let booted = boot("expired", 1);
    let key = SigningKeyV2::from_dev_id("p1-6-register-expired");

    // validBefore = 1 (1970) is far in the past, well outside the
    // clock-skew leeway, so the envelope must be rejected as expired.
    let envelope = signed_envelope(&register_payload(1), &key);

    let (status, resp) = http_post(booted.addr, "/sessions", &envelope);
    assert_eq!(status, 401, "expired register must be 401: {resp}");
    assert_eq!(resp["ok"], false, "rejection ok must be false: {resp}");
    assert_eq!(
        resp["reason"], "envelope_expired",
        "reason must be envelope_expired: {resp}"
    );
    finish(booted);
}
