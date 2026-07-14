//! SC.9a / ADR-0016 (a-3) — a containment kill (wall-clock timeout, signal
//! death) of the bounty verifier is an AVAILABILITY failure, never a
//! verdict. The `LeanBountyVerifier` maps a `retryable_unavailable` runner
//! result to a verifier `Err` (unit-pinned in `lean_bounty_verifier.rs`);
//! this test pins the route half of the invariant: the error path of
//! `/bounties/:id/proof` must return a retryable 502 WITHOUT recording a
//! bounty event, flipping bounty status, consuming the proof identity, or
//! advancing the chain head — so a slow node can never mint a
//! consensus-visible rejection (or acceptance) for a proof a faster node
//! would have judged on its merits.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{canonical_payload_hash_hex, Bounty, BountyProofVerifier, SigningKeyV2};
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

/// Stub that fails exactly the way `LeanBountyVerifier` now surfaces a
/// containment kill: an `Err` typed `retryable_unavailable`, never a
/// `VerifyOutcome{accepted:false}`.
struct ContainmentKilled;
impl BountyProofVerifier for ContainmentKilled {
    fn verify(&self, _bounty: &Bounty, _envelope: &Value) -> Result<bool, String> {
        Err(
            "retryable_unavailable: verifier availability failure (containment_wall_clock_kill), \
             not a verdict"
                .to_string(),
        )
    }
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

fn boot_with_containment_verifier(
    bounty_event_path: PathBuf,
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path = bounty_event_path
        .parent()
        .expect("parent")
        .join("blocks.ndjson");
    let bounties_path = mock_bounty_fixture_path();
    let mut verifiers: HashMap<String, Arc<dyn BountyProofVerifier>> = HashMap::new();
    verifiers.insert("mock-accept".to_string(), Arc::new(ContainmentKilled));
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                proof_dedup_ledger_path: None,
                scenario_path: scenario,
                block_path,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: Some(bounties_path),
                bounty_event_ledger_path: Some(bounty_event_path),
                bounty_verifiers: Some(verifiers),
                family_manifests_dir: None,
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
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
    (addr, handle)
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
    let (_, body_text) = raw.split_once("\r\n\r\n").expect("body");
    let parsed: Value = serde_json::from_str(body_text).expect("body json");
    (status, parsed)
}

fn http_post(addr: SocketAddr, path: &str, body: &Value) -> (u16, Value) {
    let body_str = serde_json::to_string(body).expect("body json");
    http_request(
        addr,
        format!(
            "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
            body_str.len()
        ),
    )
}

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
    http_request(
        addr,
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"),
    )
}

fn signed_proof_body(key: &SigningKeyV2, bounty_id: &str, envelope: Value) -> Value {
    let proof_hash = canonical_payload_hash_hex(&envelope);
    let valid_before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 60)
        .unwrap_or(u64::MAX / 2);
    let payload = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": bounty_id,
        "proofHash": proof_hash,
        "prover": key.pk_hex(),
        "envelope": envelope,
        "validBefore": valid_before,
        "nonce": format!("nonce-{}", rand_suffix()),
    });
    let signed = key.sign(&payload).expect("sign proof payload");
    json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    })
}

#[test]
fn containment_kill_is_retryable_unavailable_and_does_not_advance_head_or_checkpoint() {
    let dir = std::env::temp_dir().join(format!(
        "boole-sc9-containment-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let bounty_event_path = dir.join("bounty-events.ndjson");
    let key = SigningKeyV2::from_dev_id("sc9-containment-prover");

    let (addr, handle) = boot_with_containment_verifier(bounty_event_path.clone(), 8);

    let (head_status_before, head_before) = http_get(addr, "/block/latest");

    // Submit a well-formed signed proof; the verifier dies by containment.
    let body = signed_proof_body(&key, "gamma-1", json!({}));
    let (status, response) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(
        status, 502,
        "availability failure must be a retryable gateway error, got {status}: {response}"
    );
    assert_eq!(
        response["reason"], "verifier_error",
        "typed kind: {response}"
    );
    assert!(
        response["detail"]
            .as_str()
            .is_some_and(|d| d.contains("retryable_unavailable")),
        "error detail must be typed retryable so clients retry instead of \
         treating it as a judgement: {response}"
    );

    // The bounty must still be open — no status flip, no consumed identity.
    let (bounty_status, bounty) = http_get(addr, "/bounties/gamma-1");
    assert_eq!(bounty_status, 200);
    assert_eq!(
        bounty["bounty"]["status"], "open",
        "containment kill must not resolve or poison the bounty: {bounty}"
    );

    // The chain head must be untouched.
    let (head_status_after, head_after) = http_get(addr, "/block/latest");
    assert_eq!(head_status_before, head_status_after);
    assert_eq!(
        head_before, head_after,
        "containment kill must not advance the head"
    );

    drop(handle);

    // No bounty event may be recorded: the ledger file must not exist (the
    // node creates it lazily on first append) or must be empty.
    let ledger_bytes = std::fs::read(&bounty_event_path).unwrap_or_default();
    assert!(
        ledger_bytes.is_empty(),
        "containment kill must not append any bounty event, found: {}",
        String::from_utf8_lossy(&ledger_bytes)
    );

    let _ = std::fs::remove_dir_all(&dir);
}
