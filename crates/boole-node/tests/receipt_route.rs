//! V1.2 — node receipt commitment store and read route.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_core::{ReceiptCommitment, ReceiptCommitmentInput, SigningKeyV2};
use boole_node::FileReceiptStore;
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

fn fresh_nonce() -> String {
    format!("nonce-{}", rand_suffix())
}

const AGENT_PK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const ARTIFACT_HASH: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const REQUEST_HASH: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const REWARD_RECIPIENT: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const RECEIPTS_POST_PAYLOAD_SCHEMA: &str = "boole.receipts.commit.v1";

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn fresh_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-v12-receipt-route-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir
}

fn fixture_input() -> ReceiptCommitmentInput {
    ReceiptCommitmentInput {
        agent_pk: AGENT_PK.to_string(),
        family_id: "v1-lenbound".to_string(),
        verifier_id: "lean-runner-v01".to_string(),
        verifier_hash_version: "v0".to_string(),
        artifact_hash: ARTIFACT_HASH.to_string(),
        request_hash: REQUEST_HASH.to_string(),
        result: "accepted".to_string(),
        fee_charged: "1".to_string(),
        reward_recipient: REWARD_RECIPIENT.to_string(),
    }
}

fn fixture_commitment() -> ReceiptCommitment {
    ReceiptCommitment::new(fixture_input()).expect("valid fixture commitment")
}

struct Boot {
    addr: SocketAddr,
    handle: thread::JoinHandle<anyhow::Result<()>>,
    dir: PathBuf,
}

fn boot_with_receipt_store(max_requests: usize, receipt_store: Option<PathBuf>) -> Boot {
    let dir = fresh_dir("boot");
    let block_path = dir.join("blocks.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path_for_thread = block_path.clone();
    let receipt_store_for_thread = receipt_store.clone();
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
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: receipt_store_for_thread,
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

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    http_request(addr, request)
}

fn http_post(addr: SocketAddr, path: &str, body: &Value) -> (u16, Value) {
    let body_str = serde_json::to_string(body).expect("body json");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
        body_str.len()
    );
    http_request(addr, request)
}

fn receipt_payload(commitment: &ReceiptCommitment) -> Value {
    json!({
        "schema": RECEIPTS_POST_PAYLOAD_SCHEMA,
        "receiptCommitment": commitment,
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

fn signed_envelope(payload: &Value, key: &SigningKeyV2) -> Value {
    let signed = key.sign(payload).expect("sign");
    json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    })
}

fn http_request(addr: SocketAddr, request: String) -> (u16, Value) {
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

#[test]
fn receipt_store_recovers_commitment_from_ndjson() {
    let dir = fresh_dir("store-recovery");
    let path = dir.join("receipts.ndjson");
    let commitment = fixture_commitment();
    FileReceiptStore::append(&path, &commitment).expect("append commitment");

    let store = FileReceiptStore::recover(&path).expect("recover receipt store");
    let recovered = store
        .get(&commitment.receipt_id)
        .expect("commitment available by receipt id");

    assert_eq!(recovered, &commitment);
    assert_eq!(store.size(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn receipt_route_get_returns_stored_commitment() {
    let dir = fresh_dir("get-route");
    let path = dir.join("receipts.ndjson");
    let commitment = fixture_commitment();
    FileReceiptStore::append(&path, &commitment).expect("append commitment");
    let boot = boot_with_receipt_store(1, Some(path));

    let (status, value) = http_get(boot.addr, &format!("/receipts/{}", commitment.receipt_id));
    assert_eq!(status, 200, "expected 200, got {status}: {value}");
    assert_eq!(value["ok"], true);
    assert_eq!(
        value["receiptCommitment"]["receiptId"],
        commitment.receipt_id
    );
    assert_eq!(value["receiptCommitment"]["agentPk"], AGENT_PK);
    assert!(
        !serde_json::to_string(&value)
            .expect("json")
            .contains("humanAnswer"),
        "route must not surface raw human answers: {value}"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn receipt_route_get_unknown_receipt_returns_404() {
    let dir = fresh_dir("unknown-route");
    let path = dir.join("receipts.ndjson");
    let boot = boot_with_receipt_store(1, Some(path));
    let unknown = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    let (status, value) = http_get(boot.addr, &format!("/receipts/{unknown}"));
    assert_eq!(status, 404, "expected 404, got {status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "receipt_not_found");
    assert_eq!(value["receiptId"], unknown);

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn receipt_route_post_rejects_raw_human_answer_field() {
    let dir = fresh_dir("post-rejects-raw");
    let path = dir.join("receipts.ndjson");
    let boot = boot_with_receipt_store(1, Some(path.clone()));
    let key = SigningKeyV2::from_dev_id("receipt-poster-human-answer");
    let mut commitment_value = serde_json::to_value(fixture_commitment()).expect("commitment json");
    commitment_value["humanAnswer"] =
        json!("raw model/proof text must not enter node receipt state");
    let payload = json!({
        "schema": RECEIPTS_POST_PAYLOAD_SCHEMA,
        "receiptCommitment": commitment_value,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let envelope = signed_envelope(&payload, &key);

    let (status, value) = http_post(boot.addr, "/receipts", &envelope);
    assert_eq!(status, 400, "expected 400, got {status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "bad_payload");
    assert_eq!(value["field"], "receiptCommitment");
    assert!(
        !path.exists(),
        "rejected raw answer payload must not create receipt ledger"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn receipt_route_post_with_valid_signed_envelope_accepts_and_stores() {
    let dir = fresh_dir("post-signed-ok");
    let path = dir.join("receipts.ndjson");
    let boot = boot_with_receipt_store(1, Some(path.clone()));
    let key = SigningKeyV2::from_dev_id("receipt-poster-happy");
    let commitment = fixture_commitment();
    let payload = receipt_payload(&commitment);
    let envelope = signed_envelope(&payload, &key);

    let (status, value) = http_post(boot.addr, "/receipts", &envelope);
    assert_eq!(status, 200, "expected 200, got {status}: {value}");
    assert_eq!(value["ok"], true);
    assert_eq!(
        value["receiptCommitment"]["receiptId"],
        commitment.receipt_id
    );
    assert!(path.exists(), "signed receipt must be persisted to ledger");

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn receipt_route_post_tampered_payload_returns_401_signature_invalid() {
    let dir = fresh_dir("post-tampered");
    let path = dir.join("receipts.ndjson");
    let boot = boot_with_receipt_store(1, Some(path.clone()));
    let key = SigningKeyV2::from_dev_id("receipt-poster-tampered");
    let commitment = fixture_commitment();
    let payload = receipt_payload(&commitment);
    let mut envelope = signed_envelope(&payload, &key);
    envelope["payload"]["receiptCommitment"]["feeCharged"] = json!("999");

    let (status, value) = http_post(boot.addr, "/receipts", &envelope);
    assert_eq!(status, 401, "tampered payload → 401: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "signature_invalid");
    assert!(
        !path.exists(),
        "tampered envelope must not create receipt ledger"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn receipt_route_post_wrong_outer_envelope_schema_returns_400_bad_envelope() {
    let dir = fresh_dir("post-bad-envelope");
    let path = dir.join("receipts.ndjson");
    let boot = boot_with_receipt_store(1, Some(path.clone()));
    let key = SigningKeyV2::from_dev_id("receipt-poster-bad-envelope");
    let payload = receipt_payload(&fixture_commitment());
    let mut envelope = signed_envelope(&payload, &key);
    envelope["schema"] = json!("not.signed.v1");

    let (status, value) = http_post(boot.addr, "/receipts", &envelope);
    assert_eq!(status, 400, "wrong outer schema → 400: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "bad_envelope");
    assert!(
        !path.exists(),
        "bad envelope must not create receipt ledger"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn receipt_route_post_missing_valid_before_returns_400_bad_payload() {
    // P1.6a — inner payload must declare a u64 unix-seconds `validBefore`
    // so a leaked signed envelope cannot be replayed against a freshly
    // booted node hours/days later. Schema gate passes (correct inner
    // schema) so we land squarely on the freshness check.
    let dir = fresh_dir("post-missing-valid-before");
    let path = dir.join("receipts.ndjson");
    let boot = boot_with_receipt_store(1, Some(path.clone()));
    let key = SigningKeyV2::from_dev_id("receipt-poster-missing-valid-before");
    let commitment = fixture_commitment();
    let payload = json!({
        "schema": RECEIPTS_POST_PAYLOAD_SCHEMA,
        "receiptCommitment": commitment,
    });
    let envelope = signed_envelope(&payload, &key);

    let (status, value) = http_post(boot.addr, "/receipts", &envelope);
    assert_eq!(status, 400, "missing validBefore → 400: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "bad_payload");
    assert_eq!(value["field"], "validBefore");
    assert!(
        !path.exists(),
        "missing validBefore must not create receipt ledger"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn receipt_route_post_expired_valid_before_returns_401_envelope_expired() {
    let dir = fresh_dir("post-expired-valid-before");
    let path = dir.join("receipts.ndjson");
    let boot = boot_with_receipt_store(1, Some(path.clone()));
    let key = SigningKeyV2::from_dev_id("receipt-poster-expired-valid-before");
    let commitment = fixture_commitment();
    let payload = json!({
        "schema": RECEIPTS_POST_PAYLOAD_SCHEMA,
        "receiptCommitment": commitment,
        "validBefore": 1_u64,
        "nonce": fresh_nonce(),
    });
    let envelope = signed_envelope(&payload, &key);

    let (status, value) = http_post(boot.addr, "/receipts", &envelope);
    assert_eq!(status, 401, "expired validBefore → 401: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "envelope_expired");
    assert_eq!(value["validBefore"], 1);
    assert!(
        value["now"].as_u64().is_some(),
        "envelope_expired must include server `now`: {value}"
    );
    assert!(
        !path.exists(),
        "expired validBefore must not create receipt ledger"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn receipt_route_post_wrong_inner_payload_schema_returns_400_bad_payload() {
    let dir = fresh_dir("post-bad-payload-schema");
    let path = dir.join("receipts.ndjson");
    let boot = boot_with_receipt_store(1, Some(path.clone()));
    let key = SigningKeyV2::from_dev_id("receipt-poster-bad-payload-schema");
    let commitment = fixture_commitment();
    let payload = json!({
        "schema": "not.receipts.commit.v1",
        "receiptCommitment": commitment,
    });
    let envelope = signed_envelope(&payload, &key);

    let (status, value) = http_post(boot.addr, "/receipts", &envelope);
    assert_eq!(status, 400, "wrong inner schema → 400: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "bad_payload");
    assert_eq!(value["field"], "schema");
    assert!(
        !path.exists(),
        "wrong inner schema must not create receipt ledger"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&dir);
}
