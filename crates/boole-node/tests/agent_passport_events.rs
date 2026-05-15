//! P1.1 — primitive agent passport events from verify-answer receipts.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::FileReceiptStore;
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

const AGENT_PK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const REWARD_RECIPIENT: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const SESSION_PK_SHOULD_NOT_RECEIVE_REWARD: &str =
    "5555555555555555555555555555555555555555555555555555555555555555";
const ACCEPTED_X402_VERSION: &str = "x402.draft-2";

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn fresh_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-p11-agent-passport-events-{label}-{}-{}",
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

fn boot_node(max_requests: usize) -> Boot {
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

fn verify_answer_payload(answer: &str) -> Value {
    json!({
        "agentPk": AGENT_PK,
        "sessionPk": SESSION_PK_SHOULD_NOT_RECEIVE_REWARD,
        "familyId": "v1-lenbound",
        "verifierId": "mock-verified-answer-v01",
        "verifierHashVersion": "mock-v0",
        "answer": answer,
        "payTo": REWARD_RECIPIENT
    })
}

fn post_verify_answer(addr: SocketAddr, body: &Value) -> (u16, Value) {
    let body_str = serde_json::to_string(body).expect("body json");
    let request = format!(
        "POST /verify-answer HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nX-Boole-X402-Version: {ACCEPTED_X402_VERSION}\r\nPayment-Signature: boole-native-test:paid\r\n\r\n{body_str}",
        body_str.len()
    );
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
fn accepted_verify_answer_emits_work_accepted_and_reward_credit_events() {
    let boot = boot_node(1);

    let (status, value) = post_verify_answer(boot.addr, &verify_answer_payload("accepted answer"));
    assert_eq!(status, 200, "expected 200, got {status}: {value}");
    assert_eq!(value["verified"], true);
    let receipt_id = value["receiptId"].as_str().expect("receipt id");
    let events = value["agentEvents"].as_array().expect("agentEvents array");

    assert_eq!(
        events.len(),
        2,
        "accepted verify-answer should emit work+reward events: {events:?}"
    );
    assert_eq!(events[0]["schema"], "boole.agent.event.v1");
    assert_eq!(events[0]["kind"], "workAccepted");
    assert_eq!(events[0]["agentPk"], AGENT_PK);
    assert_eq!(events[0]["familyId"], "v1-lenbound");
    assert_eq!(events[0]["receiptId"], receipt_id);

    assert_eq!(events[1]["schema"], "boole.agent.event.v1");
    assert_eq!(events[1]["kind"], "rewardCredited");
    assert_eq!(events[1]["rewardRecipient"], REWARD_RECIPIENT);
    assert_eq!(events[1]["amount"], "1");
    assert_eq!(events[1]["reason"], "verify_answer_mock_fee");
    assert_ne!(
        events[1]["rewardRecipient"],
        SESSION_PK_SHOULD_NOT_RECEIVE_REWARD
    );

    boot.handle.join().expect("server thread").expect("exits");
    let store = FileReceiptStore::recover(&boot.receipt_path).expect("recover receipts");
    let recovered_events = serde_json::to_value(store.agent_events()).expect("events serialize");
    assert_eq!(recovered_events, value["agentEvents"]);
    let _ = std::fs::remove_dir_all(&boot.dir);
}

#[test]
fn rejected_verify_answer_emits_work_rejected_without_reward_credit() {
    let boot = boot_node(1);

    let (status, value) = post_verify_answer(boot.addr, &verify_answer_payload("reject"));
    assert_eq!(status, 200, "expected 200, got {status}: {value}");
    assert_eq!(value["verified"], false);
    assert_eq!(value["receiptCommitment"]["result"], "rejected");
    let receipt_id = value["receiptId"].as_str().expect("receipt id");
    let events = value["agentEvents"].as_array().expect("agentEvents array");

    assert_eq!(
        events.len(),
        1,
        "rejected work should not emit reward credit: {events:?}"
    );
    assert_eq!(events[0]["schema"], "boole.agent.event.v1");
    assert_eq!(events[0]["kind"], "workRejected");
    assert_eq!(events[0]["agentPk"], AGENT_PK);
    assert_eq!(events[0]["familyId"], "v1-lenbound");
    assert_eq!(events[0]["receiptId"], receipt_id);

    boot.handle.join().expect("server thread").expect("exits");
    let store = FileReceiptStore::recover(&boot.receipt_path).expect("recover receipts");
    let recovered_events = serde_json::to_value(store.agent_events()).expect("events serialize");
    assert_eq!(recovered_events, value["agentEvents"]);
    let _ = std::fs::remove_dir_all(&boot.dir);
}
