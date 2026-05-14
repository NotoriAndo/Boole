//! S14 — `POST /bounties/:id/status` operator status transition.
//!
//! Drives a real local node with the mock-bounty fixture and exercises
//! the six observable branches of the status handler: happy path with
//! audit-log durability, unknown `newStatus` value, URL/payload id
//! mismatch, terminal-state rejection, boot replay restoring the
//! post-transition status, and signature tampering.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{Bounty, BountyProofVerifier, SigningKeyV2};
use boole_node::FileBountyEventLedger;
use boole_node::{serve_local_node, LocalNodeConfig};
use serde_json::{json, Value};

const STATUS_SCHEMA: &str = "boole.bounty.status.v1";

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
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
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    BootResult { addr, handle, dir }
}

fn fresh_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-s14-status-{label}-{}-{}",
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

fn status_payload(id: &str, new_status: &str, ts: u64) -> Value {
    json!({
        "schema": STATUS_SCHEMA,
        "id": id,
        "newStatus": new_status,
        "ts": ts,
    })
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
fn valid_status_change_updates_bounty_and_appends_audit_event() {
    let dir = fresh_dir("happy");
    let event_path = dir.join("bounty-events.ndjson");
    let booted = boot(2, Some(mock_bounty_fixture_path()), event_path.clone());
    let key = SigningKeyV2::from_dev_id("operator-1");
    let pk_hex = key.pk_hex();
    let payload = status_payload("gamma-1", "withdrawn", 1800001100000);
    let envelope = signed_envelope(&payload, &key);

    let (status, resp) = http_post(booted.addr, "/bounties/gamma-1/status", &envelope);
    assert_eq!(status, 200, "expected 200, got {status}: {resp}");
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["bounty"]["id"], "gamma-1");
    assert_eq!(resp["bounty"]["status"], "withdrawn");

    let (status, list) = http_get(booted.addr, "/bounties/gamma-1");
    assert_eq!(status, 200);
    assert_eq!(list["bounty"]["status"], "withdrawn");

    booted.handle.join().expect("server").expect("server ok");

    let recovered = FileBountyEventLedger::recover(&event_path).expect("recover");
    assert_eq!(recovered.len(), 1, "expected 1 audit event: {recovered:?}");
    let ev = &recovered[0];
    assert_eq!(ev["kind"], "status_change");
    assert_eq!(ev["workId"], "gamma-1");
    assert_eq!(ev["prevStatus"], "open");
    assert_eq!(ev["newStatus"], "withdrawn");
    assert_eq!(ev["announcerPk"], pk_hex);
    assert_eq!(ev["verifierKind"], "mock-accept");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn unknown_status_value_returns_400_bad_status_value() {
    let dir = fresh_dir("badstatus");
    let event_path = dir.join("bounty-events.ndjson");
    let booted = boot(1, Some(mock_bounty_fixture_path()), event_path);
    let key = SigningKeyV2::from_dev_id("operator-2");
    let payload = status_payload("gamma-1", "frozen", 1800001200000);
    let envelope = signed_envelope(&payload, &key);

    let (status, resp) = http_post(booted.addr, "/bounties/gamma-1/status", &envelope);
    assert_eq!(status, 400, "unknown status → 400: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bad_status_value");
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn url_id_payload_id_mismatch_returns_400_bounty_id_mismatch() {
    let dir = fresh_dir("idmismatch");
    let event_path = dir.join("bounty-events.ndjson");
    let booted = boot(1, Some(mock_bounty_fixture_path()), event_path);
    let key = SigningKeyV2::from_dev_id("operator-3");
    // Sign a payload for delta-1 but send it to /bounties/gamma-1/status.
    // Signature is valid; the cross-check must reject before update_status.
    let payload = status_payload("delta-1", "withdrawn", 1800001300000);
    let envelope = signed_envelope(&payload, &key);

    let (status, resp) = http_post(booted.addr, "/bounties/gamma-1/status", &envelope);
    assert_eq!(status, 400, "id mismatch → 400: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bounty_id_mismatch");
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn terminal_bounty_returns_409_bounty_terminal() {
    let dir = fresh_dir("terminal");
    let event_path = dir.join("bounty-events.ndjson");
    // epsilon-1 is loaded with status=withdrawn from the static fixture.
    // Any further transition must be rejected with 409 bounty_terminal.
    let booted = boot(1, Some(mock_bounty_fixture_path()), event_path);
    let key = SigningKeyV2::from_dev_id("operator-4");
    let payload = status_payload("epsilon-1", "open", 1800001400000);
    let envelope = signed_envelope(&payload, &key);

    let (status, resp) = http_post(booted.addr, "/bounties/epsilon-1/status", &envelope);
    assert_eq!(status, 409, "terminal → 409: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bounty_terminal");
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}

#[test]
fn boot_replay_restores_post_transition_status() {
    let dir = fresh_dir("replay");
    let event_path = dir.join("bounty-events.ndjson");

    let key = SigningKeyV2::from_dev_id("operator-5");
    let payload = status_payload("gamma-1", "withdrawn", 1800001500000);

    // Boot 1: flip gamma-1 open → withdrawn via the route. The handler
    // must persist a status_change audit event so a fresh boot can replay it.
    let booted1 = boot(1, Some(mock_bounty_fixture_path()), event_path.clone());
    let envelope = signed_envelope(&payload, &key);
    let (status, _r) = http_post(booted1.addr, "/bounties/gamma-1/status", &envelope);
    assert_eq!(status, 200);
    booted1
        .handle
        .join()
        .expect("server 1")
        .expect("server 1 ok");

    // Boot 2: same audit log path + same static catalog. Replay must apply
    // the status_change on top of the static gamma-1 record so GET surfaces
    // the post-transition status, not the static "open".
    let booted2 = boot(1, Some(mock_bounty_fixture_path()), event_path);
    let (status, resp) = http_get(booted2.addr, "/bounties/gamma-1");
    assert_eq!(status, 200, "replay must restore bounty: {resp}");
    assert_eq!(
        resp["bounty"]["status"], "withdrawn",
        "replay must apply status_change: {resp}"
    );
    booted2
        .handle
        .join()
        .expect("server 2")
        .expect("server 2 ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tampered_payload_returns_401_signature_invalid() {
    let dir = fresh_dir("tamper");
    let event_path = dir.join("bounty-events.ndjson");
    let booted = boot(1, Some(mock_bounty_fixture_path()), event_path);
    let key = SigningKeyV2::from_dev_id("operator-6");
    let payload = status_payload("gamma-1", "open", 1800001600000);
    let mut envelope = signed_envelope(&payload, &key);
    // Mutate payload after signing: same id but different newStatus. The
    // signature was computed over the original payload, so verification
    // must fail with 401 signature_invalid.
    envelope["payload"]["newStatus"] = json!("withdrawn");

    let (status, resp) = http_post(booted.addr, "/bounties/gamma-1/status", &envelope);
    assert_eq!(status, 401, "tampered payload → 401: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "signature_invalid");
    booted.handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&booted.dir);
}
