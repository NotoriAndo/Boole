//! X1.1 — mock/local `/verify-answer` 402 flow.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

const AGENT_PK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const PAY_TO: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const ACCEPTED_X402_VERSION: &str = "x402.draft-2";

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn x402_versions_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/x402/versions.json")
        .canonicalize()
        .expect("x402 versions fixture")
}

fn fresh_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-x11-verify-answer-{label}-{}-{}",
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
    receipt_path: PathBuf,
}

fn boot_verify_answer_node(max_requests: usize) -> Boot {
    let dir = fresh_dir("boot");
    let block_path = dir.join("blocks.ndjson");
    let receipt_path = dir.join("receipt-commitments.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let receipt_path_for_thread = receipt_path.clone();
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
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: Some(receipt_path_for_thread),
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                genesis_override: None,
                state_dir: None,
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Boot {
        addr,
        handle,
        dir,
        receipt_path,
    }
}

fn verify_answer_payload() -> Value {
    json!({
        "agentPk": AGENT_PK,
        "familyId": "v1-lenbound",
        "verifierId": "mock-verified-answer-v01",
        "verifierHashVersion": "mock-v0",
        "answer": "the mock local answer body is not stored in receipt state",
        "payTo": PAY_TO
    })
}

fn http_post_verify_answer(
    addr: SocketAddr,
    body: &Value,
    payment_signature: Option<&str>,
    x402_version: Option<&str>,
) -> (u16, Value) {
    let body_str = serde_json::to_string(body).expect("body json");
    let mut request = format!(
        "POST /verify-answer HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body_str.len()
    );
    if let Some(version) = x402_version {
        request.push_str(&format!("X-Boole-X402-Version: {version}\r\n"));
    }
    if let Some(signature) = payment_signature {
        request.push_str(&format!("Payment-Signature: {signature}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(&body_str);
    http_request(addr, request)
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
fn x402_versions_fixture_pins_mock_flow_version() {
    let raw = std::fs::read_to_string(x402_versions_path()).expect("versions fixture");
    let value: Value = serde_json::from_str(&raw).expect("versions JSON");
    assert_eq!(value["acceptedVersions"], json!([ACCEPTED_X402_VERSION]));
}

#[test]
fn verify_answer_without_payment_returns_402_typed_envelope() {
    let boot = boot_verify_answer_node(1);

    let (status, value) = http_post_verify_answer(
        boot.addr,
        &verify_answer_payload(),
        None,
        Some(ACCEPTED_X402_VERSION),
    );

    assert_eq!(status, 402, "expected 402, got {status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "payment_required");
    assert_eq!(value["scheme"], "boole-native-test");
    assert_eq!(value["x402Version"], ACCEPTED_X402_VERSION);
    assert_eq!(value["amount"], "1");
    assert_eq!(value["payTo"], PAY_TO);
    assert!(value["requestHash"].as_str().is_some_and(|s| s.len() == 64));
    assert!(
        !boot.receipt_path.exists(),
        "missing payment must not create receipt commitment ledger"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
}

#[test]
fn verify_answer_rejects_unsupported_x402_version_before_payment() {
    let boot = boot_verify_answer_node(1);

    let (status, value) = http_post_verify_answer(
        boot.addr,
        &verify_answer_payload(),
        Some("boole-native-test:paid"),
        Some("x402.future-99"),
    );

    assert_eq!(status, 400, "expected 400, got {status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "x402_version_unsupported");
    assert_eq!(value["x402Version"], "x402.future-99");
    assert_eq!(value["acceptedVersions"], json!([ACCEPTED_X402_VERSION]));
    assert!(!boot.receipt_path.exists());

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
}

#[test]
fn verify_answer_rejects_bad_fake_payment_without_receipt() {
    let boot = boot_verify_answer_node(1);

    let (status, value) = http_post_verify_answer(
        boot.addr,
        &verify_answer_payload(),
        Some("not-a-valid-test-payment"),
        Some(ACCEPTED_X402_VERSION),
    );

    assert_eq!(status, 403, "expected 403, got {status}: {value}");
    assert_eq!(value["ok"], false);
    assert_eq!(value["reason"], "payment_invalid");
    assert_eq!(value["scheme"], "boole-native-test");
    assert_eq!(value["x402Version"], ACCEPTED_X402_VERSION);
    assert!(!boot.receipt_path.exists());

    boot.handle.join().expect("server thread").expect("exits");
    let _ = std::fs::remove_dir_all(&boot.dir);
}

#[test]
fn verify_answer_with_valid_fake_payment_creates_receipt_commitment() {
    let boot = boot_verify_answer_node(1);

    let (status, value) = http_post_verify_answer(
        boot.addr,
        &verify_answer_payload(),
        Some("boole-native-test:paid"),
        Some(ACCEPTED_X402_VERSION),
    );

    assert_eq!(status, 200, "expected 200, got {status}: {value}");
    assert_eq!(value["ok"], true);
    assert_eq!(value["verified"], true);
    assert_eq!(value["scheme"], "boole-native-test");
    assert_eq!(value["x402Version"], ACCEPTED_X402_VERSION);
    assert_eq!(value["familyId"], "v1-lenbound");
    assert_eq!(value["verifierScope"], "declared_family_only");
    assert!(value["receiptId"].as_str().is_some_and(|s| s.len() == 64));
    assert_eq!(
        value["receiptId"], value["receiptCommitment"]["receiptId"],
        "response id must match commitment id"
    );
    assert_eq!(value["receiptCommitment"]["agentPk"], AGENT_PK);
    assert_eq!(value["receiptCommitment"]["familyId"], "v1-lenbound");
    assert_eq!(value["receiptCommitment"]["feeCharged"], "1");
    assert_eq!(value["receiptCommitment"]["rewardRecipient"], PAY_TO);
    assert_eq!(
        value["receiptCommitment"]["x402Version"],
        ACCEPTED_X402_VERSION
    );
    assert!(
        !serde_json::to_string(&value)
            .expect("json")
            .contains("humanAnswer"),
        "verify-answer response must not expose raw humanAnswer field: {value}"
    );

    boot.handle.join().expect("server thread").expect("exits");
    let ledger = std::fs::read_to_string(&boot.receipt_path).expect("receipt ledger created");
    let lines: Vec<_> = ledger.lines().collect();
    assert_eq!(lines.len(), 1, "one receipt commitment row: {ledger}");
    let row: Value = serde_json::from_str(lines[0]).expect("receipt row json");
    assert_eq!(row, value["receiptCommitment"]);
    let row_text = serde_json::to_string(&row).expect("row json string");
    assert!(!row_text.contains("the mock local answer body"));
    assert!(!row_text.contains("humanAnswer"));

    let _ = std::fs::remove_dir_all(&boot.dir);
}
