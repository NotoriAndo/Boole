//! S13b — `POST /bounties` announce flow.
//!
//! Drives a real local node configured with the mock-bounty fixture and
//! exercises the eight observable branches of the announce handler:
//! happy path, audit-log durability, boot replay, boot replay overlap with
//! the static catalog, signature tampering, outer schema mismatch, inner
//! schema mismatch, and duplicate-id rejection.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{Bounty, BountyProofVerifier, SigningKeyV2, SIGNED_ENVELOPE_SCHEMA};
use boole_node::FileBountyEventLedger;
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Map, Value};

const ANNOUNCE_SCHEMA: &str = "boole.bounty.announce.v1";
const PROBLEM_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";

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

struct BootResult {
    addr: SocketAddr,
    handle: thread::JoinHandle<anyhow::Result<()>>,
    dir: PathBuf,
}

fn boot(
    max_requests: usize,
    bounties_path: Option<PathBuf>,
    bounty_event_path: PathBuf,
) -> BootResult {
    let dir = bounty_event_path.parent().expect("parent").to_path_buf();
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path_for_thread = block_path.clone();
    let verifiers = mock_verifiers();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path: block_path_for_thread,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path,
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
    BootResult { addr, handle, dir }
}

fn fresh_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-s13b-announce-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir
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
    let (_, body_text) = raw.split_once("\r\n\r\n").expect("body");
    let parsed: Value = serde_json::from_str(body_text).expect("body json");
    (status, parsed)
}

fn announce_payload(id: &str, problem_hash: &str, ts: u64) -> Value {
    json!({
        "schema": ANNOUNCE_SCHEMA,
        "id": id,
        "domain": "code.spec-template",
        "problemHash": problem_hash,
        "verifier": {
            "kind": "mock-accept",
            "metadata": {
                "verifierHash": "2222222222222222222222222222222222222222222222222222222222222222"
            }
        },
        "reward": "100",
        "deadline": 1900000000000_u64,
        "ts": ts,
        "validBefore": valid_before_far_future(),
    })
}

fn valid_before_far_future() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 3600)
        .unwrap_or(u64::MAX / 2)
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

#[test]
fn valid_signed_envelope_creates_bounty_and_appears_in_list() {
    let dir = fresh_dir("happy");
    let event_path = dir.join("bounty-events.ndjson");
    let booted = boot(2, None, event_path.clone());
    let key = SigningKeyV2::from_dev_id("announcer-1");
    let payload = announce_payload("new-bounty-1", PROBLEM_HASH, 1800000300000);
    let envelope = signed_envelope(&payload, &key);

    let (status, resp) = http_post(booted.addr, "/bounties", &envelope);
    assert_eq!(status, 200, "expected 200, got {status}: {resp}");
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["bounty"]["id"], "new-bounty-1");
    assert_eq!(resp["bounty"]["status"], "open");
    assert_eq!(resp["bounty"]["domain"], "code.spec-template");
    assert_eq!(resp["bounty"]["reward"], "100");

    let (status, list) = http_get(booted.addr, "/bounties/new-bounty-1");
    assert_eq!(
        status, 200,
        "GET /bounties/new-bounty-1 must succeed: {list}"
    );
    assert_eq!(list["bounty"]["id"], "new-bounty-1");

    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn audit_log_contains_create_kind_with_announcer_pk() {
    let dir = fresh_dir("audit");
    let event_path = dir.join("bounty-events.ndjson");
    let booted = boot(1, None, event_path.clone());
    let key = SigningKeyV2::from_dev_id("announcer-2");
    let pk_hex = key.pk_hex();
    let payload = announce_payload("audit-bounty", PROBLEM_HASH, 1800000400000);
    let envelope = signed_envelope(&payload, &key);

    let (status, _resp) = http_post(booted.addr, "/bounties", &envelope);
    assert_eq!(status, 200);
    booted.handle.join().expect("server").expect("server ok");

    let recovered = FileBountyEventLedger::recover(&event_path).expect("recover");
    assert_eq!(recovered.len(), 1, "expected 1 audit event: {recovered:?}");
    let ev = &recovered[0];
    assert_eq!(ev["kind"], "create");
    assert_eq!(ev["workId"], "audit-bounty");
    assert_eq!(ev["problemHash"], PROBLEM_HASH);
    assert_eq!(ev["verifierKind"], "mock-accept");
    assert_eq!(ev["announcerPk"], pk_hex);
    assert_eq!(ev["bounty"]["id"], "audit-bounty");
    assert_eq!(ev["bounty"]["status"], "open");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn boot_replay_restores_announced_bounty_from_audit_log() {
    let dir = fresh_dir("replay");
    let event_path = dir.join("bounty-events.ndjson");

    let key = SigningKeyV2::from_dev_id("announcer-3");
    let payload = announce_payload("replayed-bounty", PROBLEM_HASH, 1800000500000);

    // Boot 1: announce dynamically. The handler is responsible for writing
    // the create event to disk; boot 2 then replays it.
    let booted1 = boot(1, None, event_path.clone());
    let envelope = signed_envelope(&payload, &key);
    let (status, _r) = http_post(booted1.addr, "/bounties", &envelope);
    assert_eq!(status, 200);
    booted1
        .handle
        .join()
        .expect("server 1")
        .expect("server 1 ok");

    // Boot 2: same audit log path, no in-process state. GET must surface
    // the bounty solely from audit-log replay.
    let booted2 = boot(1, None, event_path.clone());
    let (status, resp) = http_get(booted2.addr, "/bounties/replayed-bounty");
    assert_eq!(status, 200, "replay must restore bounty: {resp}");
    assert_eq!(resp["bounty"]["id"], "replayed-bounty");
    assert_eq!(resp["bounty"]["status"], "open");
    booted2
        .handle
        .join()
        .expect("server 2")
        .expect("server 2 ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn boot_replay_overlap_static_catalog_wins_without_panic() {
    // Static catalog has gamma-1; we plant a `kind:"create"` audit event for
    // the same id with a different reward so the post-replay "static wins"
    // policy is observable. Boot must succeed (no panic) and GET must return
    // the static reward.
    let dir = fresh_dir("overlap");
    let event_path = dir.join("bounty-events.ndjson");
    let static_path = mock_bounty_fixture_path();

    let bounty = json!({
        "id": "gamma-1",
        "domain": "code.audit-overlap",
        "problemHash": PROBLEM_HASH,
        "verifier": {"kind": "mock-accept", "metadata": {}},
        "reward": "999",
        "deadline": 1900000000000_u64,
        "status": "open",
        "createdAt": 1800000600000_u64,
        "updatedAt": 1800000600000_u64,
    });
    let event = json!({
        "schemaVersion": 1,
        "kind": "create",
        "workId": "gamma-1",
        "problemHash": PROBLEM_HASH,
        "verifierKind": "mock-accept",
        "ts": 1800000600000_u64,
        "announcerPk": "abab000000000000000000000000000000000000000000000000000000000000",
        "bounty": bounty,
    });
    FileBountyEventLedger::append(&event_path, &event).expect("seed audit log");

    let booted = boot(1, Some(static_path), event_path);
    let (status, resp) = http_get(booted.addr, "/bounties/gamma-1");
    assert_eq!(status, 200, "boot must succeed despite overlap: {resp}");
    // Static catalog reward is "100" (see fixtures/.../v1-mock.json gamma-1).
    assert_eq!(
        resp["bounty"]["reward"], "100",
        "static catalog must win on overlap: {resp}"
    );
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn tampered_payload_returns_401_signature_invalid() {
    let dir = fresh_dir("tamper");
    let event_path = dir.join("bounty-events.ndjson");
    let booted = boot(1, None, event_path);
    let key = SigningKeyV2::from_dev_id("announcer-4");
    let payload = announce_payload("tamper-bounty", PROBLEM_HASH, 1800000700000);
    let mut envelope = signed_envelope(&payload, &key);
    // Mutate payload after signing: same fields but reward changed. The
    // signature was computed over the original payload, so verification
    // must fail with 401 signature_invalid (not 400 — the structure is
    // intact).
    envelope["payload"]["reward"] = json!("999");

    let (status, resp) = http_post(booted.addr, "/bounties", &envelope);
    assert_eq!(status, 401, "tampered payload → 401: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "signature_invalid");
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn wrong_outer_envelope_schema_returns_400_bad_envelope() {
    let dir = fresh_dir("badenv");
    let event_path = dir.join("bounty-events.ndjson");
    let booted = boot(1, None, event_path);
    let key = SigningKeyV2::from_dev_id("announcer-5");
    let payload = announce_payload("envschema-bounty", PROBLEM_HASH, 1800000800000);
    let mut envelope = signed_envelope(&payload, &key);
    envelope["schema"] = json!("not.signed.v1");

    let (status, resp) = http_post(booted.addr, "/bounties", &envelope);
    assert_eq!(status, 400, "wrong outer schema → 400: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bad_envelope");
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);

    let _ = SIGNED_ENVELOPE_SCHEMA;
    let _: Map<String, Value> = Map::new();
}

#[test]
fn wrong_inner_payload_schema_returns_400_bad_payload() {
    let dir = fresh_dir("badpayload");
    let event_path = dir.join("bounty-events.ndjson");
    let booted = boot(1, None, event_path);
    let key = SigningKeyV2::from_dev_id("announcer-6");
    let mut payload = announce_payload("payloadschema-bounty", PROBLEM_HASH, 1800000900000);
    payload["schema"] = json!("not.announce.v1");
    // Re-sign with the mutated payload so verify_signature passes — we want
    // to isolate the inner-schema rejection branch.
    let envelope = signed_envelope(&payload, &key);

    let (status, resp) = http_post(booted.addr, "/bounties", &envelope);
    assert_eq!(status, 400, "wrong inner schema → 400: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bad_payload");
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn missing_valid_before_returns_400_bad_payload() {
    // P1.6a freshness gate: inner payload must declare u64 validBefore.
    let dir = fresh_dir("missing-valid-before");
    let event_path = dir.join("bounty-events.ndjson");
    let booted = boot(1, None, event_path);
    let key = SigningKeyV2::from_dev_id("announcer-missing-valid-before");
    let payload = json!({
        "schema": ANNOUNCE_SCHEMA,
        "id": "missing-valid-before",
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
        "ts": 1800000300000_u64,
    });
    let envelope = signed_envelope(&payload, &key);

    let (status, resp) = http_post(booted.addr, "/bounties", &envelope);
    assert_eq!(status, 400, "missing validBefore → 400: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bad_payload");
    assert_eq!(resp["field"], "validBefore");
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn expired_valid_before_returns_401_envelope_expired() {
    let dir = fresh_dir("expired-valid-before");
    let event_path = dir.join("bounty-events.ndjson");
    let booted = boot(1, None, event_path);
    let key = SigningKeyV2::from_dev_id("announcer-expired-valid-before");
    let mut payload = announce_payload("expired-valid-before", PROBLEM_HASH, 1800001100000);
    payload["validBefore"] = json!(1_u64);
    let envelope = signed_envelope(&payload, &key);

    let (status, resp) = http_post(booted.addr, "/bounties", &envelope);
    assert_eq!(status, 401, "expired validBefore → 401: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "envelope_expired");
    assert_eq!(resp["validBefore"], 1);
    assert!(resp["now"].as_u64().is_some());
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn duplicate_id_returns_409_bounty_already_exists() {
    let dir = fresh_dir("dup");
    let event_path = dir.join("bounty-events.ndjson");
    let booted = boot(2, None, event_path);
    let key = SigningKeyV2::from_dev_id("announcer-7");
    let payload = announce_payload("dup-bounty", PROBLEM_HASH, 1800001000000);
    let envelope = signed_envelope(&payload, &key);

    let (s1, _) = http_post(booted.addr, "/bounties", &envelope);
    assert_eq!(s1, 200, "first announce must succeed");

    let (s2, resp) = http_post(booted.addr, "/bounties", &envelope);
    assert_eq!(s2, 409, "duplicate → 409: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bounty_already_exists");
    assert_eq!(resp["id"], "dup-bounty");
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}
