//! N2.1 — Session-bound `/submit` admission gate.
//!
//! Boots a local node with the session registry and the new submit-nonce
//! ledger wired in, then exercises the agent-wallet plan's typed-error
//! contract:
//!
//!   - Legacy submit with no `session` metadata still takes the existing
//!     admission path (200 envelope, regardless of POW outcome).
//!   - Submit naming an unknown `submittedBy` is rejected with
//!     `session_unknown` (404).
//!   - Submit against a revoked session is rejected with
//!     `session_revoked` (403).
//!   - Submit whose `rewardRecipient` does not match the session's
//!     `fixedRewardRecipient` is rejected with
//!     `reward_recipient_mismatch` (403).
//!   - Replayed `(submittedBy, nonce)` pair is rejected with
//!     `nonce_replayed` (409), and the dedup state survives a process
//!     restart by replaying the NDJSON ledger.
//!   - Active session-bound submit requires a valid W3 `signedWork`
//!     (`boole.signed.v1` around `boole.signer.work.v1`) whose pk,
//!     route, nonce, requestHash, and workPayload bind to the submitted
//!     body.
//!   - Session activation/expiry is checked again at submit-time height.
//!   - A nonce is burned only after the underlying admission returns
//!     `accepted: true`; rejected admission can be retried with the same
//!     signed nonce.

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

fn fresh_nonce() -> String {
    format!("nonce-{}", rand_suffix())
}

const PK_AGENT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PK_OWNER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PK_AGENT_RAW: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ROOT_HEX: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PK_OTHER: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const PK_REWARD: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn fresh_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-n21-submit-session-{label}-{}-{}",
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
    receipts: PathBuf,
}

impl BootPaths {
    fn new(label: &str) -> Self {
        let dir = fresh_dir(label);
        let sessions = dir.join("sessions.ndjson");
        let nonces = dir.join("submit-nonces.ndjson");
        let rewards = dir.join("rewards.ndjson");
        let receipts = dir.join("submit-receipts.ndjson");
        Self {
            dir,
            sessions,
            nonces,
            rewards,
            receipts,
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
    let receipts = paths.receipts.clone();
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
                submit_receipt_ledger_path: Some(receipts),
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

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
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

fn fixture_session(session_pk: &str, fixed_reward_recipient: &str) -> Value {
    json!({
        "sessionPk": session_pk,
        "ownerPk": PK_OWNER,
        "agentPk": PK_AGENT_RAW,
        "fixedRewardRecipient": fixed_reward_recipient,
        "allowedFamilyRoot": ROOT_HEX,
        "maxFeePerRequest": "12",
        "activationHeight": 0,
        "expiryHeight": 100,
        "revoked": false,
        "policyHash": ROOT_HEX,
    })
}

const SESSIONS_REGISTER_PAYLOAD_SCHEMA: &str = "boole.sessions.register.v1";

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

// P1.6 (audit) — the register route now authorizes the signer against the
// session's `ownerPk`, so the registrar must own the session it registers.
// Returns the owner key so callers can sign a matching revoke (only the owner
// may revoke).
fn register_session(
    addr: SocketAddr,
    session_pk: &str,
    fixed_reward_recipient: &str,
) -> SigningKeyV2 {
    let key = SigningKeyV2::from_dev_id("session-registrar-helper");
    let mut session = fixture_session(session_pk, fixed_reward_recipient);
    session["ownerPk"] = json!(key.pk_hex());
    let payload = json!({
        "schema": SESSIONS_REGISTER_PAYLOAD_SCHEMA,
        "session": session,
        "currentHeight": 0,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let envelope = signed_register_envelope(&payload, &key);
    let (status, value) = http_post(addr, "/sessions", &envelope);
    assert_eq!(
        status, 200,
        "register failed: status={status}, body={value}"
    );
    assert_eq!(value["ok"], true);
    key
}

// Only the session's owner may revoke it, so the caller passes the same key
// that `register_session` returned.
fn revoke_session(addr: SocketAddr, session_pk: &str, owner: &SigningKeyV2) {
    let payload = json!({
        "schema": "boole.sessions.revoke.v1",
        "sessionPk": session_pk,
        "height": 1,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let signed = owner.sign(&payload).expect("sign revoke");
    let envelope = json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    });
    let (status, value) = http_post(addr, &format!("/sessions/{session_pk}/revoke"), &envelope);
    assert_eq!(status, 200, "revoke failed: status={status}, body={value}");
    assert_eq!(value["session"]["revoked"], true);
}

fn submit_envelope(body: Value, session: Option<Value>) -> Value {
    let mut envelope = json!({"body": body});
    if let Some(s) = session {
        envelope["session"] = s;
    }
    envelope
}

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

fn read_receipt_lines(path: &PathBuf) -> Vec<Value> {
    let text = fs::read_to_string(path).expect("receipt ledger should exist");
    text.lines()
        .map(|line| serde_json::from_str(line).expect("receipt line should be valid JSON"))
        .collect()
}

#[test]
fn submit_without_session_metadata_uses_legacy_path() {
    let paths = BootPaths::new("legacy");
    let boot = boot_with(&paths, 1);

    let body = scenario_body();
    let (status, value) = http_post(boot.addr, "/submit", &submit_envelope(body, None));
    assert_eq!(
        status, 200,
        "legacy submit must return 200, got {status}: {value}"
    );
    assert!(
        value.get("accepted").is_some(),
        "legacy submit must carry the existing admission envelope: {value}"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_with_unknown_session_pk_returns_session_unknown() {
    let paths = BootPaths::new("unknown");
    let boot = boot_with(&paths, 1);

    let body = scenario_body();
    let session = json!({
        "submittedBy": PK_AGENT,
        "rewardRecipient": PK_REWARD,
        "nonce": "n-unknown",
    });
    let (status, value) = http_post(boot.addr, "/submit", &submit_envelope(body, Some(session)));
    assert_eq!(status, 404, "got status={status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "session_unknown");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_with_revoked_session_returns_session_revoked() {
    let paths = BootPaths::new("revoked");
    let boot = boot_with(&paths, 3);

    let owner = register_session(boot.addr, PK_AGENT, PK_REWARD);
    revoke_session(boot.addr, PK_AGENT, &owner);

    let body = scenario_body();
    let session = json!({
        "submittedBy": PK_AGENT,
        "rewardRecipient": PK_REWARD,
        "nonce": "n-revoked",
    });
    let (status, value) = http_post(boot.addr, "/submit", &submit_envelope(body, Some(session)));
    assert_eq!(status, 403, "got status={status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "session_revoked");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_with_reward_recipient_mismatch_is_rejected() {
    let paths = BootPaths::new("recipient");
    let boot = boot_with(&paths, 2);

    register_session(boot.addr, PK_AGENT, PK_REWARD);

    let body = scenario_body();
    let session = json!({
        "submittedBy": PK_AGENT,
        "rewardRecipient": PK_OTHER,
        "nonce": "n-recipient",
    });
    let (status, value) = http_post(boot.addr, "/submit", &submit_envelope(body, Some(session)));
    assert_eq!(status, 403, "got status={status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "reward_recipient_mismatch");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_replayed_nonce_returns_nonce_replayed() {
    let paths = BootPaths::new("replay");
    let boot = boot_with(&paths, 3);

    let key = SigningKeyV2::from_dev_id("n21-replay");
    let session_pk = key.pk_hex();
    register_session(boot.addr, &session_pk, PK_REWARD);

    let body = scenario_body();
    let session = signed_work_session(&key, &body, "n-replay", PK_REWARD);

    let (status_first, value_first) = http_post(
        boot.addr,
        "/submit",
        &submit_envelope(body.clone(), Some(session.clone())),
    );
    assert_eq!(
        status_first, 200,
        "first submit must reach admission: status={status_first}, body={value_first}"
    );
    assert_eq!(
        value_first["accepted"], true,
        "first submit must be accepted before nonce is burned: {value_first}"
    );

    let (status_second, value_second) =
        http_post(boot.addr, "/submit", &submit_envelope(body, Some(session)));
    assert_eq!(
        status_second, 409,
        "duplicate nonce must return 409: status={status_second}, body={value_second}"
    );
    assert_eq!(value_second["ok"], false);
    assert_eq!(value_second["reason"], "nonce_replayed");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_nonce_dedup_survives_restart() {
    let paths = BootPaths::new("restart");

    // First boot: register session + submit once. Both calls share the
    // session registry and nonce ledger paths so the second boot can
    // replay the same NDJSON files.
    let first = boot_with(&paths, 2);
    let key = SigningKeyV2::from_dev_id("n21-restart");
    let session_pk = key.pk_hex();
    register_session(first.addr, &session_pk, PK_REWARD);
    let body = scenario_body();
    let session = signed_work_session(&key, &body, "n-restart", PK_REWARD);
    let (status_first, value_first) = http_post(
        first.addr,
        "/submit",
        &submit_envelope(body.clone(), Some(session.clone())),
    );
    assert_eq!(
        status_first, 200,
        "first submit must reach admission: status={status_first}, body={value_first}"
    );
    assert_eq!(
        value_first["accepted"], true,
        "first submit must be accepted before nonce is persisted: {value_first}"
    );
    first.handle.join().expect("server thread").expect("exits");

    // Second boot: same ledgers, replay the same (submittedBy, nonce)
    // pair. Restart-replay must rehydrate the dedup set so the second
    // submit is rejected with the same `nonce_replayed` envelope.
    let second = boot_with(&paths, 1);
    let (status_second, value_second) = http_post(
        second.addr,
        "/submit",
        &submit_envelope(body, Some(session)),
    );
    assert_eq!(
        status_second, 409,
        "post-restart replay must still return 409: status={status_second}, body={value_second}"
    );
    assert_eq!(value_second["reason"], "nonce_replayed");
    second.handle.join().expect("server thread").expect("exits");

    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_with_active_session_requires_signed_work_envelope() {
    let paths = BootPaths::new("requires-signed-work");
    let boot = boot_with(&paths, 2);

    register_session(boot.addr, PK_AGENT, PK_REWARD);
    let body = scenario_body();
    let session = json!({
        "submittedBy": PK_AGENT,
        "rewardRecipient": PK_REWARD,
        "nonce": "n-missing-signed-work",
    });

    let (status, value) = http_post(boot.addr, "/submit", &submit_envelope(body, Some(session)));
    assert_eq!(
        status, 400,
        "missing signedWork must be rejected: status={status}, body={value}"
    );
    assert_eq!(value["reason"], "missing_field");
    assert_eq!(value["field"], "session.signedWork");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_rejects_tampered_signed_work_payload() {
    let paths = BootPaths::new("tampered-signed-work");
    let boot = boot_with(&paths, 2);

    let key = SigningKeyV2::from_dev_id("n21-tampered");
    let session_pk = key.pk_hex();
    register_session(boot.addr, &session_pk, PK_REWARD);
    let body = scenario_body();
    let mut session = signed_work_session(&key, &body, "n-tamper", PK_REWARD);
    session["signedWork"]["payload"]["workPayload"]["nonceS"] =
        json!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");

    let (status, value) = http_post(boot.addr, "/submit", &submit_envelope(body, Some(session)));
    assert_eq!(
        status, 401,
        "tampered signed payload must fail signature verification: status={status}, body={value}"
    );
    assert_eq!(value["reason"], "signature_invalid");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_rejects_request_hash_mismatch() {
    let paths = BootPaths::new("request-hash-mismatch");
    let boot = boot_with(&paths, 2);

    let key = SigningKeyV2::from_dev_id("n21-request-hash");
    let session_pk = key.pk_hex();
    register_session(boot.addr, &session_pk, PK_REWARD);
    let body = scenario_body();
    let payload = json!({
        "schema": "boole.signer.work.v1",
        "route": "/submit",
        "familyId": "boole.protocol-invariant.v01",
        "verifierId": "lean-runner-v01",
        "fee": "0",
        "requestHash": ROOT_HEX,
        "nonce": "n-bad-request-hash",
        "workPayload": body,
    });
    let signed = key
        .sign(&payload)
        .expect("sign mismatched requestHash payload");
    let session = json!({
        "submittedBy": session_pk,
        "rewardRecipient": PK_REWARD,
        "nonce": "n-bad-request-hash",
        "signedWork": {
            "schema": signed.schema,
            "payload": signed.payload,
            "pk": signed.pk,
            "signature": signed.signature,
        }
    });

    let (status, value) = http_post(
        boot.addr,
        "/submit",
        &submit_envelope(scenario_body(), Some(session)),
    );
    assert_eq!(
        status, 400,
        "requestHash mismatch must be rejected: status={status}, body={value}"
    );
    assert_eq!(value["reason"], "bad_payload");
    assert_eq!(value["field"], "session.signedWork.payload.requestHash");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn submit_rejects_session_expired_at_current_node_height() {
    let paths = BootPaths::new("expired-at-submit");
    let boot = boot_with(&paths, 3);

    let key = SigningKeyV2::from_dev_id("n21-expired-at-submit");
    let session_pk = key.pk_hex();
    // P1.6 (audit) — the registrar must own the session, so ownerPk is the
    // register signer's pk (declared before the session object that uses it).
    let register_key = SigningKeyV2::from_dev_id("session-registrar-expiry-inline");
    let session = json!({
        "sessionPk": session_pk,
        "ownerPk": register_key.pk_hex(),
        "agentPk": PK_AGENT_RAW,
        "fixedRewardRecipient": PK_REWARD,
        "allowedFamilyRoot": ROOT_HEX,
        "maxFeePerRequest": "12",
        "activationHeight": 0,
        "expiryHeight": 1,
        "revoked": false,
        "policyHash": ROOT_HEX,
    });
    let register_payload = json!({
        "schema": SESSIONS_REGISTER_PAYLOAD_SCHEMA,
        "session": session,
        "currentHeight": 0,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let register_envelope = signed_register_envelope(&register_payload, &register_key);
    let (register_status, register_value) = http_post(boot.addr, "/sessions", &register_envelope);
    assert_eq!(
        register_status, 200,
        "register must succeed before expiry: {register_value}"
    );

    let legacy_body = scenario_body();
    let (legacy_status, legacy_value) =
        http_post(boot.addr, "/submit", &submit_envelope(legacy_body, None));
    assert_eq!(
        legacy_status, 200,
        "legacy submit should advance node height: {legacy_value}"
    );
    assert_eq!(
        legacy_value["accepted"], true,
        "legacy submit fixture must be accepted: {legacy_value}"
    );

    let body = scenario_body();
    let session_submit = signed_work_session(&key, &body, "n-expired-at-submit", PK_REWARD);
    let (status, value) = http_post(
        boot.addr,
        "/submit",
        &submit_envelope(body, Some(session_submit)),
    );
    assert_eq!(
        status, 403,
        "expired session must be denied at submit-time height: {value}"
    );
    assert_eq!(value["reason"], "session_denied");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn rejected_admission_does_not_burn_submit_nonce() {
    let paths = BootPaths::new("rejected-does-not-burn");
    let boot = boot_with(&paths, 3);

    let key = SigningKeyV2::from_dev_id("n21-rejected-does-not-burn");
    let session_pk = key.pk_hex();
    register_session(boot.addr, &session_pk, PK_REWARD);

    let mut bad_body = scenario_body();
    bad_body["c"] = json!(ROOT_HEX);
    let bad_session = signed_work_session(&key, &bad_body, "n-retry-after-reject", PK_REWARD);
    let (bad_status, bad_value) = http_post(
        boot.addr,
        "/submit",
        &submit_envelope(bad_body, Some(bad_session)),
    );
    assert_eq!(
        bad_status, 200,
        "bad admission should reach legacy admission: {bad_value}"
    );
    assert_eq!(
        bad_value["accepted"], false,
        "bad admission must be rejected by admission, not session gate: {bad_value}"
    );

    let good_body = scenario_body();
    let good_session = signed_work_session(&key, &good_body, "n-retry-after-reject", PK_REWARD);
    let (good_status, good_value) = http_post(
        boot.addr,
        "/submit",
        &submit_envelope(good_body, Some(good_session)),
    );
    assert_eq!(
        good_status, 200,
        "same nonce must be reusable after rejected admission: {good_value}"
    );
    assert_eq!(
        good_value["accepted"], true,
        "same nonce should burn only after accepted admission: {good_value}"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn session_bound_submit_credits_fixed_reward_recipient_not_body_pk() {
    let paths = BootPaths::new("reward-recipient-credit");
    let boot = boot_with(&paths, 4);

    let key = SigningKeyV2::from_dev_id("r11-reward-recipient");
    let session_pk = key.pk_hex();
    register_session(boot.addr, &session_pk, PK_OTHER);

    let body = scenario_body();
    let request_hash = canonical_payload_hash_hex(&body);
    assert_eq!(body["pk"], PK_OWNER, "fixture body should mine as PK_OWNER");
    let session = signed_work_session(&key, &body, "n-reward-recipient", PK_OTHER);
    let (submit_status, submit_value) =
        http_post(boot.addr, "/submit", &submit_envelope(body, Some(session)));
    assert_eq!(
        submit_status, 200,
        "submit must reach admission: {submit_value}"
    );
    assert_eq!(
        submit_value["accepted"], true,
        "session-bound submit must be accepted: {submit_value}"
    );
    assert_eq!(
        submit_value["block"]["proposerPk"], PK_OWNER,
        "block proposer identity remains the proof/mining pk"
    );
    assert_eq!(
        submit_value["block"]["proposerRewardPk"], PK_OTHER,
        "block audit artifact must expose the session reward recipient for proposer bonus"
    );
    assert_eq!(
        submit_value["block"]["selectedShareRewardPks"][0], PK_OTHER,
        "block audit artifact must expose the session reward recipient for selected share credit"
    );

    let receipt = &submit_value["receipt"];
    assert_eq!(receipt["schema"], "boole.submit.receipt.v1");
    assert_eq!(receipt["accepted"], true);
    assert_eq!(receipt["route"], "/submit");
    assert_eq!(receipt["sessionPk"], session_pk);
    assert_eq!(receipt["nonce"], "n-reward-recipient");
    assert_eq!(receipt["requestHash"], request_hash);
    assert_eq!(receipt["blockHeight"], submit_value["block"]["height"]);
    assert_eq!(receipt["blockC"], submit_value["block"]["c"]);
    assert_eq!(receipt["proposerPk"], PK_OWNER);
    assert_eq!(receipt["rewardRecipient"], PK_OTHER);
    assert_eq!(receipt["rewardAmount"], "2");

    let receipt_lines = read_receipt_lines(&paths.receipts);
    assert_eq!(
        receipt_lines.len(),
        1,
        "one accepted session-bound submit should append one receipt"
    );
    assert_eq!(
        receipt_lines[0], *receipt,
        "ledger receipt must match response receipt exactly"
    );

    let (recipient_status, recipient_balance) =
        http_get(boot.addr, &format!("/account/{PK_OTHER}/balance"));
    assert_eq!(
        recipient_status, 200,
        "recipient balance route failed: {recipient_balance}"
    );
    assert_eq!(
        recipient_balance["balance"], "2",
        "fixedRewardRecipient should receive share credit plus proposer bonus"
    );

    let (body_pk_status, body_pk_balance) =
        http_get(boot.addr, &format!("/account/{PK_OWNER}/balance"));
    assert_eq!(
        body_pk_status, 200,
        "body pk balance route failed: {body_pk_balance}"
    );
    assert_eq!(
        body_pk_balance["balance"], "0",
        "session-bound reward must not be credited to the raw submit body pk"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

// ------------------------------------------------------------------
// P1.6b — POST /sessions wire-rejection coverage. These tests pin the
// `boole.signed.v1` envelope contract on the registration route so that
// callers cannot bypass signature verification or inner-schema gating
// by reverting to the legacy plain-JSON body.
// ------------------------------------------------------------------

const PK_REGISTER_HAPPY: &str = "1010101010101010101010101010101010101010101010101010101010101010";
const PK_REGISTER_TAMPER: &str = "2020202020202020202020202020202020202020202020202020202020202020";
const PK_REGISTER_BADENV: &str = "3030303030303030303030303030303030303030303030303030303030303030";
const PK_REGISTER_BADPAYLOAD: &str =
    "4040404040404040404040404040404040404040404040404040404040404040";

#[test]
fn sessions_register_with_valid_signed_envelope_accepts_and_persists() {
    let paths = BootPaths::new("register-signed-ok");
    let boot = boot_with(&paths, 1);
    let key = SigningKeyV2::from_dev_id("session-registrar-happy");
    // P1.6 (audit) — the registrar must own the session it registers.
    let mut session = fixture_session(PK_REGISTER_HAPPY, PK_REWARD);
    session["ownerPk"] = json!(key.pk_hex());
    let payload = json!({
        "schema": SESSIONS_REGISTER_PAYLOAD_SCHEMA,
        "session": session,
        "currentHeight": 0,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let envelope = signed_register_envelope(&payload, &key);

    let (status, value) = http_post(boot.addr, "/sessions", &envelope);
    assert_eq!(status, 200, "expected 200, got {status}: {value}");
    assert_eq!(value["ok"], true);
    assert_eq!(value["session"]["sessionPk"], PK_REGISTER_HAPPY);
    assert!(paths.sessions.exists(), "register must persist ledger line");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn sessions_register_tampered_payload_returns_401_signature_invalid() {
    let paths = BootPaths::new("register-tampered");
    let boot = boot_with(&paths, 1);
    let key = SigningKeyV2::from_dev_id("session-registrar-tampered");
    let payload = json!({
        "schema": SESSIONS_REGISTER_PAYLOAD_SCHEMA,
        "session": fixture_session(PK_REGISTER_TAMPER, PK_REWARD),
        "currentHeight": 0,
    });
    let mut envelope = signed_register_envelope(&payload, &key);
    envelope["payload"]["currentHeight"] = json!(99);

    let (status, value) = http_post(boot.addr, "/sessions", &envelope);
    assert_eq!(status, 401, "tampered payload → 401: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "signature_invalid");
    assert!(
        !paths.sessions.exists(),
        "tampered envelope must not write session ledger"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn sessions_register_wrong_outer_envelope_schema_returns_400_bad_envelope() {
    let paths = BootPaths::new("register-bad-envelope");
    let boot = boot_with(&paths, 1);
    let key = SigningKeyV2::from_dev_id("session-registrar-bad-envelope");
    let payload = json!({
        "schema": SESSIONS_REGISTER_PAYLOAD_SCHEMA,
        "session": fixture_session(PK_REGISTER_BADENV, PK_REWARD),
        "currentHeight": 0,
    });
    let mut envelope = signed_register_envelope(&payload, &key);
    envelope["schema"] = json!("not.signed.v1");

    let (status, value) = http_post(boot.addr, "/sessions", &envelope);
    assert_eq!(status, 400, "wrong outer schema → 400: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "bad_envelope");
    assert!(
        !paths.sessions.exists(),
        "bad envelope must not write session ledger"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}

#[test]
fn sessions_register_wrong_inner_payload_schema_returns_400_bad_payload() {
    let paths = BootPaths::new("register-bad-payload-schema");
    let boot = boot_with(&paths, 1);
    let key = SigningKeyV2::from_dev_id("session-registrar-bad-payload-schema");
    let payload = json!({
        "schema": "not.sessions.register.v1",
        "session": fixture_session(PK_REGISTER_BADPAYLOAD, PK_REWARD),
        "currentHeight": 0,
    });
    let envelope = signed_register_envelope(&payload, &key);

    let (status, value) = http_post(boot.addr, "/sessions", &envelope);
    assert_eq!(status, 400, "wrong inner schema → 400: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "bad_payload");
    assert_eq!(value["field"], "schema");
    assert!(
        !paths.sessions.exists(),
        "wrong inner schema must not write session ledger"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&paths.dir);
}
