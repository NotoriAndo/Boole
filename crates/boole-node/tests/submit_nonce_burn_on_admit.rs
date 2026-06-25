//! N2.2 — characterization guard for `/submit` nonce burn-on-admit (P1.3a).
//!
//! P1.3a moved the submit-nonce burn INTO the `AdmissionDecision::Accepted`
//! branch of `submit_json`: a submit that admission rejects returns before the
//! burn, so its `(submittedBy, nonce)` pair stays reusable; only an accepted
//! submit consumes the nonce, exactly once. That behavior already exists, so
//! these tests are characterization (regression) guards — they must be GREEN
//! the day they are written. A RED here is a P1.3a regression (a nonce burned
//! on reject, or an accepted nonce not burned), not a missing feature, so the
//! fix is to investigate the burn site, not to "implement" the test.
//!
//! Pairs with `scripts/test_nonce_burn_before_block_contract.py`, which pins
//! the same ordering at the source level.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_core::{canonical_payload_hash_hex, SigningKeyV2};
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

const PK_REWARD: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const ROOT_HEX: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
// A third, distinct role pk: the node rejects a session whose sessionPk,
// ownerPk, and agentPk are not all unique ("session role keys must be unique").
const PK_AGENT_RAW: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const SESSIONS_REGISTER_PAYLOAD_SCHEMA: &str = "boole.sessions.register.v1";

fn fresh_nonce() -> String {
    format!("nonce-{}", rand_suffix())
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn fresh_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-n22-nonce-burn-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("tmp dir");
    dir
}

fn scenario_path() -> PathBuf {
    repo_root().join("fixtures/protocol/runtime-smoke/v1.json")
}

/// The runtime-smoke v1 fixture share — admission accepts it (with the Lean
/// checker disabled) so it can stand in for a winning submit.
fn scenario_body() -> Value {
    let raw = fs::read_to_string(scenario_path()).expect("scenario file");
    let scenario: Value = serde_json::from_str(&raw).expect("scenario json");
    scenario["steps"][0]["body"].clone()
}

struct Boot {
    addr: SocketAddr,
    handle: thread::JoinHandle<anyhow::Result<()>>,
}

struct BootPaths {
    dir: PathBuf,
    sessions: PathBuf,
    nonces: PathBuf,
    rewards: PathBuf,
}

impl BootPaths {
    fn new(label: &str) -> Self {
        let dir = fresh_dir(label);
        let sessions = dir.join("sessions.ndjson");
        let nonces = dir.join("submit-nonces.ndjson");
        let rewards = dir.join("rewards.ndjson");
        Self {
            dir,
            sessions,
            nonces,
            rewards,
        }
    }
}

fn boot_with(paths: &BootPaths, max_requests: usize) -> Boot {
    let block_path = paths.dir.join("blocks.ndjson");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let sessions = paths.sessions.clone();
    let nonces = paths.nonces.clone();
    let rewards = paths.rewards.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                proof_dedup_ledger_path: None,
                scenario_path: scenario,
                block_path,
                reward_ledger_path: Some(rewards),
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                session_registry_path: Some(sessions),
                submit_nonce_ledger_path: Some(nonces),
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
                allow_anonymous_submit: true,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Boot { addr, handle }
}

fn http_post(addr: SocketAddr, path: &str, body: &Value) -> (u16, Value) {
    let body_str = serde_json::to_string(body).expect("body json");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
        body_str.len()
    );
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("write timeout");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = Vec::new();
    match stream.read_to_end(&mut buf) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::ConnectionReset && !buf.is_empty() => {}
        Err(err) => panic!("read response: {err}"),
    }
    let raw = String::from_utf8(buf).expect("utf8 response");
    let status = raw
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("status not parseable: {raw}"));
    let (_, body_text) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("response missing body: {raw}"));
    let body: Value = serde_json::from_str(body_text)
        .unwrap_or_else(|err| panic!("body not JSON: {err}, raw={body_text}"));
    (status, body)
}

fn valid_before_fresh() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 60)
        .unwrap_or(u64::MAX / 2)
}

fn signed_register_envelope(payload: &Value, key: &SigningKeyV2) -> Value {
    let signed = key.sign(payload).expect("sign register payload");
    json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    })
}

/// Registers a session bound to `reward_recipient` and returns the *session*
/// key, with which the caller signs matching `signedWork` envelopes (its pk is
/// the session's `sessionPk` and the submit's `submittedBy`). The register is
/// authorized by a separate owner key (the node checks the signer against
/// `ownerPk`), and `agentPk` is a third distinct role pk.
fn register_session(addr: SocketAddr, reward_recipient: &str) -> SigningKeyV2 {
    let session_key = SigningKeyV2::from_dev_id("n22-session");
    let owner = SigningKeyV2::from_dev_id("n22-owner");
    let session = json!({
        "sessionPk": session_key.pk_hex(),
        "ownerPk": owner.pk_hex(),
        "agentPk": PK_AGENT_RAW,
        "fixedRewardRecipient": reward_recipient,
        "allowedFamilyRoot": ROOT_HEX,
        "maxFeePerRequest": "12",
        "activationHeight": 0,
        "expiryHeight": 100,
        "revoked": false,
        "policyHash": ROOT_HEX,
    });
    let payload = json!({
        "schema": SESSIONS_REGISTER_PAYLOAD_SCHEMA,
        "session": session,
        "currentHeight": 0,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let envelope = signed_register_envelope(&payload, &owner);
    let (status, value) = http_post(addr, "/sessions", &envelope);
    assert_eq!(
        status, 200,
        "register failed: status={status}, body={value}"
    );
    assert_eq!(value["ok"], true);
    session_key
}

fn submit_envelope(body: Value, session: Value) -> Value {
    json!({ "body": body, "session": session })
}

/// A session submit metadata block carrying a W3 `signedWork` envelope bound to
/// `body` and `nonce`. The binding is over whatever `body` is passed, so a
/// caller can sign a deliberately admission-failing body and still clear the
/// signature/session checks (the reject then lands in admission, not earlier).
fn signed_work_session(
    key: &SigningKeyV2,
    body: &Value,
    nonce: &str,
    reward_recipient: &str,
) -> Value {
    let payload = json!({
        "schema": "boole.signer.work.v1",
        "route": "/submit",
        "familyId": "boole.protocol-invariant.v01",
        "verifierId": "lean-runner-v01",
        "fee": "0",
        "requestHash": canonical_payload_hash_hex(body),
        "nonce": nonce,
        "workPayload": body,
    });
    let signed = key.sign(&payload).expect("sign work payload");
    json!({
        "submittedBy": key.pk_hex(),
        "rewardRecipient": reward_recipient,
        "nonce": nonce,
        "signedWork": {
            "schema": signed.schema,
            "payload": signed.payload,
            "pk": signed.pk,
            "signature": signed.signature,
        }
    })
}

/// An accepted submit burns its nonce exactly once: the winning share is
/// admitted (200, accepted=true) and a replay of the same `(submittedBy,
/// nonce)` pair is rejected with `nonce_replayed` (409). Pins that the burn
/// fires on the accept path.
#[test]
fn accepted_submit_burns_nonce_once() {
    // Exactly three requests below (register + winning submit + replay);
    // `boot_with` stops the server after `max_requests`, so an over-count would
    // hang the final `join()` waiting for a request that never comes.
    let paths = BootPaths::new("accepted-burns");
    let boot = boot_with(&paths, 3);

    let key = register_session_returning(&boot);
    let body = scenario_body();
    let nonce = fresh_nonce();
    let session = signed_work_session(&key, &body, &nonce, PK_REWARD);

    let (status_first, value_first) = http_post(
        boot.addr,
        "/submit",
        &submit_envelope(body.clone(), session.clone()),
    );
    assert_eq!(
        status_first, 200,
        "accepted submit must return 200: status={status_first}, body={value_first}"
    );
    assert_eq!(
        value_first["accepted"], true,
        "the winning share must be accepted before its nonce is burned: {value_first}"
    );

    let (status_replay, value_replay) =
        http_post(boot.addr, "/submit", &submit_envelope(body, session));
    assert_eq!(
        status_replay, 409,
        "the burned nonce must reject a replay: status={status_replay}, body={value_replay}"
    );
    assert_eq!(value_replay["ok"], false);
    assert_eq!(
        value_replay["reason"], "nonce_replayed",
        "replay must be the typed nonce-replay reject: {value_replay}"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

/// A submit that admission rejects must NOT consume its nonce: the same
/// `(submittedBy, nonce)` pair is still usable for a later winning submit.
/// The reject reaches admission (the signature binds the tampered body, so the
/// session/signature checks pass) and returns before the P1.3a burn site.
#[test]
fn rejected_submit_keeps_nonce_reusable() {
    // Exactly three requests below (register + rejected submit + winning
    // resubmit); `boot_with` stops the server after `max_requests`, so an
    // over-count would hang the final `join()` waiting for a request that
    // never comes.
    let paths = BootPaths::new("rejected-reusable");
    let boot = boot_with(&paths, 3);

    let key = register_session_returning(&boot);
    let nonce = fresh_nonce();

    // A share whose `c` does not match the chain head: admission rejects it.
    // Sign the signedWork over this exact (tampered) body so the binding holds
    // and the reject lands in admission, not in the signature/session gate.
    let mut bad_body = scenario_body();
    bad_body["c"] = json!("1111111111111111111111111111111111111111111111111111111111111111");
    let bad_session = signed_work_session(&key, &bad_body, &nonce, PK_REWARD);

    let (status_bad, value_bad) = http_post(
        boot.addr,
        "/submit",
        &submit_envelope(bad_body, bad_session),
    );
    assert_ne!(
        value_bad["accepted"],
        json!(true),
        "the tampered share must not be accepted: status={status_bad}, body={value_bad}"
    );
    assert_ne!(
        value_bad["reason"],
        json!("nonce_replayed"),
        "the first submit cannot itself be a replay: {value_bad}"
    );

    // Same nonce, now a winning share. If the rejected attempt had burned the
    // nonce this would come back 409 nonce_replayed; instead it must admit.
    let good_body = scenario_body();
    let good_session = signed_work_session(&key, &good_body, &nonce, PK_REWARD);
    let (status_good, value_good) = http_post(
        boot.addr,
        "/submit",
        &submit_envelope(good_body, good_session),
    );
    assert_eq!(
        status_good, 200,
        "the un-burned nonce must remain reusable: status={status_good}, body={value_good}"
    );
    assert_eq!(
        value_good["accepted"], true,
        "the winning resubmit on the same nonce must be admitted: {value_good}"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

/// Registers a session and returns the signing key (small wrapper so each test
/// reads top-down).
fn register_session_returning(boot: &Boot) -> SigningKeyV2 {
    register_session(boot.addr, PK_REWARD)
}
