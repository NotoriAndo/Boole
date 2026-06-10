//! N1.2 — `/sessions` HTTP route surface for the agent-wallet plan.
//!
//! Boots a local node with an explicit `session_registry_path`, then
//! exercises the four observable surfaces:
//!
//!   - `POST /sessions` registers a `SessionState` and returns the
//!     public view (no secret-shaped fields).
//!   - `GET /sessions/{sessionPk}` returns the public state for a
//!     registered session.
//!   - `POST /sessions/{sessionPk}/revoke` marks the session revoked
//!     and a subsequent `GET` reflects the new state.
//!   - Malformed or non-canonical uppercase `sessionPk` is rejected with
//!     `malformed_pk` (mirrors `account_balance_json` so the vocabulary is
//!     consistent).
//!   - When `session_registry_path` is `None`, the routes return
//!     `session_registry_disabled` instead of silently succeeding.

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

fn fresh_nonce() -> String {
    format!("nonce-{}", rand_suffix())
}

const SESSIONS_REGISTER_PAYLOAD_SCHEMA: &str = "boole.sessions.register.v1";
const SESSIONS_REVOKE_PAYLOAD_SCHEMA: &str = "boole.sessions.revoke.v1";

fn signed_register_envelope(payload: &Value, key: &SigningKeyV2) -> Value {
    let signed = key.sign(payload).expect("sign register payload");
    json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    })
}

fn signed_revoke_envelope(payload: &Value, key: &SigningKeyV2) -> Value {
    let signed = key.sign(payload).expect("sign revoke payload");
    json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    })
}

fn register_payload(session: Value, current_height: u64) -> Value {
    json!({
        "schema": SESSIONS_REGISTER_PAYLOAD_SCHEMA,
        "session": session,
        "currentHeight": current_height,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    })
}

fn revoke_payload(session_pk: &str, height: u64) -> Value {
    json!({
        "schema": SESSIONS_REVOKE_PAYLOAD_SCHEMA,
        "sessionPk": session_pk,
        "height": height,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    })
}

fn valid_before_fresh() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 60)
        .unwrap_or(u64::MAX / 2)
}

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

fn fresh_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-n12-session-route-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir
}

struct Boot {
    addr: SocketAddr,
    handle: thread::JoinHandle<anyhow::Result<()>>,
    dir: PathBuf,
}

fn boot_with_registry(max_requests: usize, registry: Option<PathBuf>) -> Boot {
    boot_with_signed_nonce_ledger(max_requests, registry, None)
}

fn boot_with_signed_nonce_ledger(
    max_requests: usize,
    registry: Option<PathBuf>,
    signed_nonce_ledger_path: Option<PathBuf>,
) -> Boot {
    let dir = fresh_dir("boot");
    let block_path = dir.join("blocks.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path_for_thread = block_path.clone();
    let registry_for_thread = registry.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path: block_path_for_thread,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                session_registry_path: registry_for_thread,
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path,
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
    let (_, body_text) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no body break in: {raw}"));
    let parsed: Value =
        serde_json::from_str(body_text).unwrap_or_else(|_| panic!("body not json: {body_text}"));
    (status, parsed)
}

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

/// `fixture_session` owned by `owner`. The P1.6 authorization check on
/// `POST /sessions` requires the envelope signer's pk to equal the session's
/// `ownerPk`, so any happy-path register (and the revoke that follows it) must
/// be signed by the owner — i.e. the session's `ownerPk` is the signer's pk.
fn owned_session(owner: &SigningKeyV2) -> Value {
    let mut session = fixture_session();
    session["ownerPk"] = json!(owner.pk_hex());
    session
}

#[test]
fn session_route_register_returns_ok_for_valid_session() {
    let dir = fresh_dir("register-ledger");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(1, Some(registry));

    let key = SigningKeyV2::from_dev_id("session-route-register-happy");
    let body = signed_register_envelope(&register_payload(owned_session(&key), 0), &key);
    let (status, value) = http_post(boot.addr, "/sessions", &body);
    assert_eq!(status, 200, "expected 200, got {status}: {value}");
    assert_eq!(value["ok"], true);
    assert_eq!(value["session"]["sessionPk"], PK_A);
    assert_eq!(value["session"]["revoked"], false);

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_get_returns_public_state_no_secret() {
    let dir = fresh_dir("get-ledger");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(2, Some(registry));

    let key = SigningKeyV2::from_dev_id("session-route-get-public-state");
    let body = signed_register_envelope(&register_payload(owned_session(&key), 0), &key);
    let (status_post, _) = http_post(boot.addr, "/sessions", &body);
    assert_eq!(status_post, 200);

    let (status, value) = http_get(boot.addr, &format!("/sessions/{PK_A}"));
    assert_eq!(status, 200, "expected 200, got {status}: {value}");
    assert_eq!(value["ok"], true);
    assert_eq!(value["session"]["sessionPk"], PK_A);
    assert_eq!(value["session"]["ownerPk"], key.pk_hex());
    assert_eq!(value["session"]["agentPk"], PK_C);
    let body_text = serde_json::to_string(&value).expect("json");
    assert!(
        !body_text.contains("\"sk\""),
        "response must not contain `sk`; got {body_text}"
    );
    assert!(
        !body_text.contains("\"sessionSk\""),
        "response must not contain `sessionSk`; got {body_text}"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_revoke_sets_revoked_true() {
    let dir = fresh_dir("revoke-ledger");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(3, Some(registry));

    let key = SigningKeyV2::from_dev_id("session-route-revoke-flow");
    let body = signed_register_envelope(&register_payload(owned_session(&key), 0), &key);
    let (status_post, _) = http_post(boot.addr, "/sessions", &body);
    assert_eq!(status_post, 200);

    let revoke_envelope = signed_revoke_envelope(&revoke_payload(PK_A, 42), &key);
    let (status_revoke, value_revoke) = http_post(
        boot.addr,
        &format!("/sessions/{PK_A}/revoke"),
        &revoke_envelope,
    );
    assert_eq!(status_revoke, 200, "got {status_revoke}: {value_revoke}");
    assert_eq!(value_revoke["ok"], true);
    assert_eq!(value_revoke["session"]["revoked"], true);

    let (status_get, value_get) = http_get(boot.addr, &format!("/sessions/{PK_A}"));
    assert_eq!(status_get, 200);
    assert_eq!(value_get["session"]["revoked"], true);

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_rejects_malformed_session_pk() {
    let dir = fresh_dir("malformed-ledger");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(1, Some(registry));

    let (status, value) = http_get(boot.addr, "/sessions/not-hex");
    assert_eq!(status, 400, "got {status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "malformed_pk");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_rejects_uppercase_session_pk_as_noncanonical() {
    let dir = fresh_dir("uppercase-ledger");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(1, Some(registry));

    let uppercase = PK_A.to_ascii_uppercase();
    let (status, value) = http_get(boot.addr, &format!("/sessions/{uppercase}"));
    assert_eq!(
        status, 400,
        "uppercase sessionPk must be noncanonical: {value}"
    );
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "malformed_pk");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_returns_disabled_when_registry_unconfigured() {
    let boot = boot_with_registry(1, None);

    let body = json!({"session": fixture_session(), "currentHeight": 0});
    let (status, value) = http_post(boot.addr, "/sessions", &body);
    assert_eq!(status, 400, "got {status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "session_registry_disabled");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
}

// ------------------------------------------------------------------
// P1.6c — signed envelope on POST /sessions/{sessionPk}/revoke.
//
// Mirrors slice 23 (/receipts) and slice 24 (/sessions): the route
// requires a `boole.signed.v1` outer envelope whose inner payload is
// `boole.sessions.revoke.v1` and binds `sessionPk` so a signed payload
// cannot be replayed against a different session's URL.
// ------------------------------------------------------------------

#[test]
fn session_route_revoke_with_valid_signed_envelope_accepts_and_persists() {
    let dir = fresh_dir("revoke-signed-ok");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(2, Some(registry));

    let key = SigningKeyV2::from_dev_id("session-route-revoke-happy");
    let register_envelope =
        signed_register_envelope(&register_payload(owned_session(&key), 0), &key);
    let (status_register, _) = http_post(boot.addr, "/sessions", &register_envelope);
    assert_eq!(status_register, 200);

    let revoke_envelope = signed_revoke_envelope(&revoke_payload(PK_A, 7), &key);
    let (status, value) = http_post(
        boot.addr,
        &format!("/sessions/{PK_A}/revoke"),
        &revoke_envelope,
    );
    assert_eq!(status, 200, "expected 200, got {status}: {value}");
    assert_eq!(value["ok"], true);
    assert_eq!(value["session"]["revoked"], true);

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_revoke_non_owner_signer_returns_403_unauthorized_signer() {
    // P1.6 (audit) — only the session's owner may revoke it. `attacker` holds a
    // valid key but does not own the registered session, so the revoke must be
    // 403 unauthorized_signer, never 200. Fails closed if the authz check is
    // removed.
    let dir = fresh_dir("revoke-non-owner");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(2, Some(registry));

    let owner = SigningKeyV2::from_dev_id("session-route-revoke-owner");
    let attacker = SigningKeyV2::from_dev_id("session-route-revoke-attacker");
    let register_envelope =
        signed_register_envelope(&register_payload(owned_session(&owner), 0), &owner);
    let (status_register, _) = http_post(boot.addr, "/sessions", &register_envelope);
    assert_eq!(status_register, 200);

    let revoke_envelope = signed_revoke_envelope(&revoke_payload(PK_A, 7), &attacker);
    let (status, value) = http_post(
        boot.addr,
        &format!("/sessions/{PK_A}/revoke"),
        &revoke_envelope,
    );
    assert_eq!(status, 403, "non-owner revoke must be 403: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "unauthorized_signer");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_revoke_unknown_session_returns_404_session_not_found() {
    // P1.6 (audit) — revoking a session that was never registered must be 404
    // session_not_found (the owner lookup finds nothing), not a silent success.
    let dir = fresh_dir("revoke-unknown");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(1, Some(registry));

    let key = SigningKeyV2::from_dev_id("session-route-revoke-unknown");
    let revoke_envelope = signed_revoke_envelope(&revoke_payload(PK_A, 7), &key);
    let (status, value) = http_post(
        boot.addr,
        &format!("/sessions/{PK_A}/revoke"),
        &revoke_envelope,
    );
    assert_eq!(status, 404, "unknown session revoke must be 404: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "session_not_found");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_revoke_tampered_payload_returns_401_signature_invalid() {
    let dir = fresh_dir("revoke-tampered");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(2, Some(registry));

    let key = SigningKeyV2::from_dev_id("session-route-revoke-tampered");
    let register_envelope =
        signed_register_envelope(&register_payload(owned_session(&key), 0), &key);
    let (status_register, _) = http_post(boot.addr, "/sessions", &register_envelope);
    assert_eq!(status_register, 200);

    let mut revoke_envelope = signed_revoke_envelope(&revoke_payload(PK_A, 7), &key);
    // Tamper height post-signing — sig was computed for height=7 but the
    // wire now claims height=99; signature verification must fail.
    revoke_envelope["payload"]["height"] = json!(99);
    let (status, value) = http_post(
        boot.addr,
        &format!("/sessions/{PK_A}/revoke"),
        &revoke_envelope,
    );
    assert_eq!(status, 401, "expected 401, got {status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "signature_invalid");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_revoke_wrong_outer_envelope_schema_returns_400_bad_envelope() {
    let dir = fresh_dir("revoke-bad-env");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(2, Some(registry));

    let key = SigningKeyV2::from_dev_id("session-route-revoke-bad-env");
    let register_envelope =
        signed_register_envelope(&register_payload(owned_session(&key), 0), &key);
    let (status_register, _) = http_post(boot.addr, "/sessions", &register_envelope);
    assert_eq!(status_register, 200);

    let mut revoke_envelope = signed_revoke_envelope(&revoke_payload(PK_A, 7), &key);
    revoke_envelope["schema"] = json!("boole.signed.v0");
    let (status, value) = http_post(
        boot.addr,
        &format!("/sessions/{PK_A}/revoke"),
        &revoke_envelope,
    );
    assert_eq!(status, 400, "expected 400, got {status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "bad_envelope");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_revoke_wrong_inner_payload_schema_returns_400_bad_payload() {
    let dir = fresh_dir("revoke-bad-payload");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(2, Some(registry));

    let key = SigningKeyV2::from_dev_id("session-route-revoke-bad-payload");
    let register_envelope =
        signed_register_envelope(&register_payload(owned_session(&key), 0), &key);
    let (status_register, _) = http_post(boot.addr, "/sessions", &register_envelope);
    assert_eq!(status_register, 200);

    let payload = json!({
        "schema": "boole.sessions.revoke.v0",
        "sessionPk": PK_A,
        "height": 7,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let envelope = signed_revoke_envelope(&payload, &key);
    let (status, value) = http_post(boot.addr, &format!("/sessions/{PK_A}/revoke"), &envelope);
    assert_eq!(status, 400, "expected 400, got {status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "bad_payload");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

// P1.6a — register/revoke inner payloads must carry `validBefore` so a
// leaked signed envelope cannot be replayed against a freshly booted
// node. Missing → 400 bad_payload field=validBefore; expired → 401
// envelope_expired with both `validBefore` and `now` extras.
#[test]
fn session_route_register_missing_valid_before_returns_400_bad_payload() {
    let dir = fresh_dir("register-missing-valid-before");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(1, Some(registry));

    let key = SigningKeyV2::from_dev_id("session-route-register-missing-valid-before");
    let payload = json!({
        "schema": SESSIONS_REGISTER_PAYLOAD_SCHEMA,
        "session": fixture_session(),
        "currentHeight": 0,
    });
    let envelope = signed_register_envelope(&payload, &key);
    let (status, value) = http_post(boot.addr, "/sessions", &envelope);
    assert_eq!(status, 400, "missing validBefore → 400: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "bad_payload");
    assert_eq!(value["field"], "validBefore");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_revoke_expired_valid_before_returns_401_envelope_expired() {
    let dir = fresh_dir("revoke-expired-valid-before");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(2, Some(registry));

    let key = SigningKeyV2::from_dev_id("session-route-revoke-expired-valid-before");
    let register_envelope =
        signed_register_envelope(&register_payload(owned_session(&key), 0), &key);
    let (status_register, _) = http_post(boot.addr, "/sessions", &register_envelope);
    assert_eq!(status_register, 200);

    let payload = json!({
        "schema": SESSIONS_REVOKE_PAYLOAD_SCHEMA,
        "sessionPk": PK_A,
        "height": 7,
        "validBefore": 1_u64,
        "nonce": fresh_nonce(),
    });
    let envelope = signed_revoke_envelope(&payload, &key);
    let (status, value) = http_post(boot.addr, &format!("/sessions/{PK_A}/revoke"), &envelope);
    assert_eq!(status, 401, "expired validBefore → 401: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "envelope_expired");
    assert_eq!(value["validBefore"], 1);
    assert!(value["now"].as_u64().is_some());

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_revoke_rejects_session_pk_url_payload_mismatch() {
    let dir = fresh_dir("revoke-pk-mismatch");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(2, Some(registry));

    let key = SigningKeyV2::from_dev_id("session-route-revoke-pk-mismatch");
    let register_envelope =
        signed_register_envelope(&register_payload(owned_session(&key), 0), &key);
    let (status_register, _) = http_post(boot.addr, "/sessions", &register_envelope);
    assert_eq!(status_register, 200);

    // Payload binds sessionPk=PK_B but URL targets PK_A. The handler must
    // refuse so an attacker cannot replay a payload signed for one
    // session against a different one.
    let payload = revoke_payload(PK_B, 7);
    let envelope = signed_revoke_envelope(&payload, &key);
    let (status, value) = http_post(boot.addr, &format!("/sessions/{PK_A}/revoke"), &envelope);
    assert_eq!(status, 400, "expected 400, got {status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "bad_payload");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_register_missing_nonce_returns_400_bad_payload_field_nonce() {
    let dir = fresh_dir("register-missing-nonce");
    let registry = dir.join("sessions.ndjson");
    let boot = boot_with_registry(1, Some(registry));
    let key = SigningKeyV2::from_dev_id("session-route-register-missing-nonce");
    let mut payload = register_payload(owned_session(&key), 0);
    payload.as_object_mut().expect("obj").remove("nonce");
    let envelope = signed_register_envelope(&payload, &key);

    let (status, value) = http_post(boot.addr, "/sessions", &envelope);
    assert_eq!(status, 400, "missing nonce → 400: {value}");
    assert_eq!(value["reason"], "bad_payload");
    assert_eq!(value["field"], "nonce");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_register_replayed_nonce_returns_409_nonce_replayed() {
    let dir = fresh_dir("register-replay");
    let registry = dir.join("sessions.ndjson");
    let signed_nonce_path = dir.join("signed-nonces.ndjson");
    let boot = boot_with_signed_nonce_ledger(2, Some(registry), Some(signed_nonce_path));
    let key = SigningKeyV2::from_dev_id("session-route-register-replay");
    let pk_hex = key.pk_hex();

    let mut payload_a = register_payload(owned_session(&key), 0);
    let reused_nonce = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
    payload_a["nonce"] = json!(reused_nonce);
    let env_a = signed_register_envelope(&payload_a, &key);
    let (s1, resp1) = http_post(boot.addr, "/sessions", &env_a);
    assert_eq!(s1, 200, "first register must succeed: {resp1}");

    // Different session payload (different sessionPk), reused nonce → must
    // be rejected as nonce_replayed before the inner-session handler runs.
    let alt_session = json!({
        "sessionPk": PK_C,
        "ownerPk": PK_A,
        "agentPk": PK_B,
        "fixedRewardRecipient": PK_A,
        "allowedFamilyRoot": ROOT,
        "maxFeePerRequest": "12",
        "activationHeight": 0,
        "expiryHeight": 100,
        "revoked": false,
        "policyHash": ROOT,
    });
    let mut payload_b = register_payload(alt_session, 0);
    payload_b["nonce"] = json!(reused_nonce);
    let env_b = signed_register_envelope(&payload_b, &key);
    let (s2, resp2) = http_post(boot.addr, "/sessions", &env_b);
    assert_eq!(s2, 409, "reused nonce → 409: {resp2}");
    assert_eq!(resp2["reason"], "nonce_replayed");
    assert_eq!(resp2["signerPk"], pk_hex);
    assert_eq!(resp2["nonce"], reused_nonce);

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn session_route_register_rejection_does_not_burn_nonce() {
    // P1.6 closure cell (f) — the per-signer nonce is burned ONLY on the commit
    // path (`burn_signed_envelope_nonce` runs immediately before the session-
    // ledger append), never on a business rejection. A register whose envelope
    // is well-formed, signed, and nonce-fresh but whose inner `session` fails
    // validation (which happens AFTER the nonce-replay check and BEFORE the
    // burn) must leave the `(signerPk, nonce)` reusable. This is the complement
    // of `session_route_register_replayed_nonce_returns_409_nonce_replayed`: a
    // committed write burns the nonce, a rejected write does not. With the
    // signed nonce ledger configured, a burn on the rejected request would turn
    // the retry into a `409 nonce_replayed`, so the 200 below is the proof.
    let dir = fresh_dir("register-reject-no-burn");
    let registry = dir.join("sessions.ndjson");
    let signed_nonce_path = dir.join("signed-nonces.ndjson");
    let boot = boot_with_signed_nonce_ledger(2, Some(registry), Some(signed_nonce_path));
    let key = SigningKeyV2::from_dev_id("session-route-register-reject-no-burn");
    let reused_nonce = "abababababababababababababababab";

    // Request A: a malformed `session` (missing every required field) fails
    // SessionState parsing after the nonce-replay check and before the burn.
    let mut bad = register_payload(json!({}), 0);
    bad["nonce"] = json!(reused_nonce);
    let (s1, resp1) = http_post(
        boot.addr,
        "/sessions",
        &signed_register_envelope(&bad, &key),
    );
    assert_ne!(
        s1, 200,
        "a malformed session must be rejected, got 200: {resp1}"
    );
    assert_eq!(
        resp1["ok"], false,
        "rejection envelope must be ok=false: {resp1}"
    );

    // Request B: same signer + same nonce, now a VALID session → must succeed,
    // proving request A did not burn the nonce.
    let mut good = register_payload(owned_session(&key), 0);
    good["nonce"] = json!(reused_nonce);
    let (s2, resp2) = http_post(
        boot.addr,
        "/sessions",
        &signed_register_envelope(&good, &key),
    );
    assert_eq!(
        s2, 200,
        "a nonce reused after a non-committing rejection must still be accepted: {resp2}"
    );
    assert_eq!(resp2["ok"], true, "second register must succeed: {resp2}");
    assert_eq!(resp2["session"]["sessionPk"], PK_A);

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}
