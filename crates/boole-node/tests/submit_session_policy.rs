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
use boole_node::local_node::{serve_local_node, LocalNodeConfig};
use serde_json::{json, Value};

const PK_AGENT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PK_OWNER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PK_AGENT_RAW: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const ROOT_HEX: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const PK_OTHER: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
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
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                genesis_override: None,
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

fn register_session(addr: SocketAddr, session_pk: &str, fixed_reward_recipient: &str) {
    let body = json!({
        "session": fixture_session(session_pk, fixed_reward_recipient),
        "currentHeight": 0,
    });
    let (status, value) = http_post(addr, "/sessions", &body);
    assert_eq!(
        status, 200,
        "register failed: status={status}, body={value}"
    );
    assert_eq!(value["ok"], true);
}

fn revoke_session(addr: SocketAddr, session_pk: &str) {
    let (status, value) = http_post(
        addr,
        &format!("/sessions/{session_pk}/revoke"),
        &json!({"height": 1}),
    );
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
        "rewardRecipient": PK_OWNER,
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

    register_session(boot.addr, PK_AGENT, PK_OWNER);
    revoke_session(boot.addr, PK_AGENT);

    let body = scenario_body();
    let session = json!({
        "submittedBy": PK_AGENT,
        "rewardRecipient": PK_OWNER,
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

    register_session(boot.addr, PK_AGENT, PK_OWNER);

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
    register_session(boot.addr, &session_pk, PK_OWNER);

    let body = scenario_body();
    let session = signed_work_session(&key, &body, "n-replay", PK_OWNER);

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
    register_session(first.addr, &session_pk, PK_OWNER);
    let body = scenario_body();
    let session = signed_work_session(&key, &body, "n-restart", PK_OWNER);
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

    register_session(boot.addr, PK_AGENT, PK_OWNER);
    let body = scenario_body();
    let session = json!({
        "submittedBy": PK_AGENT,
        "rewardRecipient": PK_OWNER,
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
    register_session(boot.addr, &session_pk, PK_OWNER);
    let body = scenario_body();
    let mut session = signed_work_session(&key, &body, "n-tamper", PK_OWNER);
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
    register_session(boot.addr, &session_pk, PK_OWNER);
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
        "rewardRecipient": PK_OWNER,
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
    let session = json!({
        "sessionPk": session_pk,
        "ownerPk": PK_OWNER,
        "agentPk": PK_AGENT_RAW,
        "fixedRewardRecipient": PK_OWNER,
        "allowedFamilyRoot": ROOT_HEX,
        "maxFeePerRequest": "12",
        "activationHeight": 0,
        "expiryHeight": 1,
        "revoked": false,
        "policyHash": ROOT_HEX,
    });
    let (register_status, register_value) = http_post(
        boot.addr,
        "/sessions",
        &json!({"session": session, "currentHeight": 0}),
    );
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
    let session_submit = signed_work_session(&key, &body, "n-expired-at-submit", PK_OWNER);
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
    register_session(boot.addr, &session_pk, PK_OWNER);

    let mut bad_body = scenario_body();
    bad_body["c"] = json!(ROOT_HEX);
    let bad_session = signed_work_session(&key, &bad_body, "n-retry-after-reject", PK_OWNER);
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
    let good_session = signed_work_session(&key, &good_body, "n-retry-after-reject", PK_OWNER);
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
