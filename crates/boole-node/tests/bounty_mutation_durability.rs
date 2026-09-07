//! A failed route-owned audit write must not publish unrecoverable bounty state.
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use boole_core::{canonical_payload_hash_hex, Bounty, BountyProofVerifier, SigningKeyV2};
use boole_node::{serve_local_node_with_shutdown, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};
use tokio::sync::Notify;

struct Accept;
impl BountyProofVerifier for Accept {
    fn verify(&self, _: &Bounty, _: &Value) -> Result<bool, String> {
        Ok(true)
    }
}

struct Node {
    addr: SocketAddr,
    stop: Arc<Notify>,
    handle: Option<JoinHandle<anyhow::Result<()>>>,
}

impl Node {
    fn boot(dir: &Path) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener");
        let addr = listener.local_addr().expect("address");
        let stop = Arc::new(Notify::new());
        let notify = Arc::clone(&stop);
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/protocol");
        let config = LocalNodeConfig {
            scenario_path: fixtures.join("runtime-smoke/v1.json"),
            block_path: dir.join("blocks.ndjson"),
            reward_ledger_path: None,
            work_manifests_path: None,
            bounties_path: Some(fixtures.join("bounties/v1-mock.json")),
            bounty_event_ledger_path: Some(dir.join("bounty-events.ndjson")),
            bounty_verifiers: Some(HashMap::from([(
                "mock-accept".to_string(),
                Arc::new(Accept) as Arc<dyn BountyProofVerifier>,
            )])),
            family_manifests_dir: None,
            operator_signer_pks: vec![],
            session_registry_path: None,
            submit_nonce_ledger_path: None,
            signed_nonce_ledger_path: Some(dir.join("signed-nonces.ndjson")),
            proof_dedup_ledger_path: None,
            submit_receipt_ledger_path: None,
            receipt_commitment_ledger_path: None,
            max_requests: None,
            genesis_override: None,
            state_dir: None,
            network_id: None,
            lean_checker_dir: None,
            lean_checker_disabled: true,
            http_rate_limit_per_60s: None,
            allow_anonymous_submit: true,
        };
        let handle =
            thread::spawn(move || serve_local_node_with_shutdown(listener, config, notify));
        let node = Self {
            addr,
            stop,
            handle: Some(handle),
        };
        assert_eq!(node.request("GET", "/live", None).0, 200);
        node
    }

    fn request(&self, method: &str, route: &str, body: Option<&Value>) -> (u16, Value) {
        let body = body.map(Value::to_string).unwrap_or_default();
        let mut stream = TcpStream::connect(self.addr).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read deadline");
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .expect("write deadline");
        write!(stream, "{method} {route} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).expect("request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("response");
        let (headers, body) = response.split_once("\r\n\r\n").expect("HTTP body");
        let status = headers
            .split_whitespace()
            .nth(1)
            .expect("status")
            .parse()
            .expect("status number");
        (status, serde_json::from_str(body).expect("JSON body"))
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        self.stop.notify_one();
        if let Some(handle) = self.handle.take() {
            let result = handle.join().expect("node thread");
            if !thread::panicking() {
                result.expect("node shutdown");
            }
        }
    }
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "boole-bounty-durability-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        std::fs::create_dir(&dir).expect("private test directory");
        Self(dir)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn signed(mut payload: Value) -> Value {
    let key = SigningKeyV2::from_dev_id("bounty-durability-regression");
    payload["nonce"] = json!(format!("nonce-{}", rand_suffix()));
    payload["validBefore"] = json!(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_secs()
            + 60
    );
    if payload["schema"] == "boole.bounty.proof.v1" {
        payload["prover"] = json!(key.pk_hex());
    }
    let envelope = key.sign(&payload).expect("sign synthetic intent");
    json!({"schema":envelope.schema,"payload":envelope.payload,"pk":envelope.pk,"signature":envelope.signature})
}

fn proof() -> Value {
    let envelope = json!({"proof":"test proof"});
    signed(
        json!({"schema":"boole.bounty.proof.v1","bountyId":"gamma-1","proofHash":canonical_payload_hash_hex(&envelope),"envelope":envelope}),
    )
}

#[test]
fn failed_proof_audit_keeps_bounty_open_and_retry_durable() {
    let scratch = Scratch::new();
    let node = Node::boot(&scratch.0);
    let audit = scratch.0.join("bounty-events.ndjson");
    std::fs::create_dir(&audit).expect("simulate unavailable audit target");
    let failed = node.request("POST", "/bounties/gamma-1/proof", Some(&proof()));
    assert_eq!(failed.0, 500, "audit write must fail: {failed:?}");
    let before_retry = node.request("GET", "/bounties/gamma-1", None);
    assert_eq!(
        before_retry.1["bounty"]["status"], "open",
        "failed audit must not publish solved state: {before_retry:?}"
    );
    std::fs::remove_dir(&audit).expect("restore owned audit target");
    let retry = node.request("POST", "/bounties/gamma-1/proof", Some(&proof()));
    assert_eq!(retry.0, 200, "fresh signed retry: {retry:?}");
    assert_eq!(
        retry.1["duplicate"], false,
        "failed audit must not burn proof identity"
    );
    drop(node);
    let recovered = Node::boot(&scratch.0);
    let status = recovered.request("GET", "/bounties/gamma-1", None);
    assert_eq!(
        status.1["bounty"]["status"], "solved",
        "successful retry must survive restart"
    );
}

#[test]
fn failed_status_audit_keeps_prior_status_and_retry_durable() {
    let scratch = Scratch::new();
    let node = Node::boot(&scratch.0);
    let audit = scratch.0.join("bounty-events.ndjson");
    let intent = || {
        signed(
            json!({"schema":"boole.bounty.status.v1","id":"gamma-1","newStatus":"withdrawn","ts":1800000000001u64}),
        )
    };
    std::fs::create_dir(&audit).expect("simulate unavailable audit target");
    let failed = node.request("POST", "/bounties/gamma-1/status", Some(&intent()));
    assert_eq!(failed.0, 500, "audit write must fail: {failed:?}");
    let before_retry = node.request("GET", "/bounties/gamma-1", None);
    assert_eq!(
        before_retry.1["bounty"]["status"], "open",
        "failed audit must not publish terminal status"
    );
    std::fs::remove_dir(&audit).expect("restore owned audit target");
    let retry = node.request("POST", "/bounties/gamma-1/status", Some(&intent()));
    assert_eq!(retry.0, 200, "fresh signed retry: {retry:?}");
    drop(node);
    let recovered = Node::boot(&scratch.0);
    assert_eq!(
        recovered.request("GET", "/bounties/gamma-1", None).1["bounty"]["status"],
        "withdrawn"
    );
}

#[test]
fn failed_create_audit_keeps_bounty_absent_and_retry_durable() {
    let scratch = Scratch::new();
    let node = Node::boot(&scratch.0);
    let audit = scratch.0.join("bounty-events.ndjson");
    let intent = || {
        signed(json!({
            "schema":"boole.bounty.announce.v1","id":"new-bounty","domain":"test.mock-accept",
            "problemHash":"1".repeat(64),"verifier":{"kind":"mock-accept","metadata":{}},
            "reward":"100","deadline":1900000000000u64,"ts":1800000000000u64
        }))
    };
    std::fs::create_dir(&audit).expect("simulate unavailable audit target");
    let failed = node.request("POST", "/bounties", Some(&intent()));
    assert_eq!(failed.0, 500, "audit write must fail: {failed:?}");
    assert_eq!(
        node.request("GET", "/bounties/new-bounty", None).0,
        404,
        "failed audit must not publish bounty"
    );
    std::fs::remove_dir(&audit).expect("restore owned audit target");
    let retry = node.request("POST", "/bounties", Some(&intent()));
    assert_eq!(retry.0, 200, "fresh signed retry: {retry:?}");
    drop(node);
    let recovered = Node::boot(&scratch.0);
    assert_eq!(
        recovered.request("GET", "/bounties/new-bounty", None).1["bounty"]["status"],
        "open"
    );
}
