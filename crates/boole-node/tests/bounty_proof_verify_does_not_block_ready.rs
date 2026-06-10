//! P1.7 — verifier must not hold the write lock.
//!
//! `bounty_proof_handler` mutates the in-memory `LocalNodeState`, so it
//! takes `state.inner.write().await` for the whole request. Today the
//! handler also runs `BountyProofVerifier::verify` while holding that
//! write lock. Verify is a sync trait call that can take arbitrarily
//! long (Lean child invocation, network-bound mock-prover, etc.), and
//! every other HTTP handler that touches `state.inner` — `/ready`,
//! `/status`, `/config`, the bounty status routes — queues behind it.
//!
//! This test proves the harm with `/ready`, the orchestrator readiness
//! probe we just hardened in P2.6:
//!
//!   1. A worker thread POSTs `/bounties/{id}/proof` against a bounty
//!      whose verifier (`mock-slow`) sleeps 500ms inside `verify`.
//!   2. The main thread waits ~50ms — enough for the POST to enter the
//!      handler and reach the verifier call site, while ~450ms of
//!      verifier sleep still lies ahead.
//!   3. The main thread issues `GET /ready` and measures wall time.
//!
//! With the write lock held across `verify`, `/ready`'s `read().await`
//! parks behind the writer and the response takes ~450ms instead of the
//! ~1ms a healthy readiness probe deserves. The assertion bound is set
//! at 200ms, well above scheduler noise (typical /ready latency is <
//! 5ms) but well below the verifier's 500ms sleep, so the only way to
//! pass is to drop the write lock before calling `verify`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use boole_core::{Bounty, BountyProofVerifier, SigningKeyV2};
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

fn fresh_nonce() -> String {
    format!("nonce-{}", rand_suffix())
}

const PROOF_HASH_A: &str = "aaaa000000000000000000000000000000000000000000000000000000000000";

fn signed_proof_body(
    key: &SigningKeyV2,
    bounty_id: &str,
    proof_hash: &str,
    envelope: Value,
) -> Value {
    let payload = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": bounty_id,
        "proofHash": proof_hash,
        "prover": key.pk_hex(),
        "envelope": envelope,
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let signed = key.sign(&payload).expect("sign proof payload");
    json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    })
}

fn valid_before_fresh() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 60)
        .unwrap_or(u64::MAX / 2)
}

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

struct MockSlow {
    sleep: Duration,
}

impl BountyProofVerifier for MockSlow {
    fn verify(&self, _bounty: &Bounty, _envelope: &Value) -> Result<bool, String> {
        std::thread::sleep(self.sleep);
        Ok(true)
    }
}

fn write_slow_bounty_fixture(path: &PathBuf) {
    let v = json!({
        "version": 1,
        "source": "P1.7 verify-outside-write-lock test fixture",
        "generatedAt": "2026-05-19T00:00:00Z",
        "bounties": [
            {
                "id": "slow-1",
                "domain": "test.mock-slow",
                "problemHash": "1111111111111111111111111111111111111111111111111111111111111111",
                "verifier": {
                    "kind": "mock-slow",
                    "metadata": {
                        "verifierHash": "2222222222222222222222222222222222222222222222222222222222222222",
                        "profile": "stub"
                    }
                },
                "reward": "100",
                "deadline": 1900000000000i64,
                "status": "open",
                "createdAt": 1800000000000i64,
                "updatedAt": 1800000000000i64
            }
        ]
    });
    std::fs::write(path, serde_json::to_string_pretty(&v).unwrap()).expect("write fixture");
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
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf).to_string();
    let status: u16 = raw
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let parsed: Value = raw
        .split_once("\r\n\r\n")
        .and_then(|(_, b)| serde_json::from_str(b).ok())
        .unwrap_or(Value::Null);
    (status, parsed)
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
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf).to_string();
    let status: u16 = raw
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = raw
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .unwrap_or_default();
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    (status, parsed)
}

#[test]
fn bounty_proof_verify_does_not_block_ready_probe() {
    let dir = std::env::temp_dir().join(format!(
        "boole-verify-lock-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let bounty_event_path = dir.join("bounty-events.ndjson");
    let bounties_path = dir.join("bounties.json");
    write_slow_bounty_fixture(&bounties_path);

    // 500ms verifier sleep is long enough to dominate scheduler noise
    // (<5ms) while staying well under the 10s socket timeout.
    let mut verifiers: HashMap<String, Arc<dyn BountyProofVerifier>> = HashMap::new();
    verifiers.insert(
        "mock-slow".to_string(),
        Arc::new(MockSlow {
            sleep: Duration::from_millis(500),
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (ready_tx, ready_rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_for_thread = block_path.clone();
    let bounties_for_thread = bounties_path.clone();
    let bounty_event_for_thread = bounty_event_path.clone();

    // `max_requests = 2` matches the two `Connection: close` HTTP
    // requests this test issues: the slow POST and the /ready probe.
    let handle = thread::spawn(move || {
        ready_tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path: block_for_thread,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: Some(bounties_for_thread),
                bounty_event_ledger_path: Some(bounty_event_for_thread),
                bounty_verifiers: Some(verifiers),
                family_manifests_dir: None,
                max_requests: Some(2),
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
            },
        )
    });
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));

    let (proof_tx, proof_rx) = mpsc::channel::<(u16, Value)>();
    let worker = thread::spawn(move || {
        let key = SigningKeyV2::from_dev_id("bounty-verify-block-ready-test");
        let body = signed_proof_body(&key, "slow-1", PROOF_HASH_A, json!({}));
        let (status, value) = http_post(addr, "/bounties/slow-1/proof", &body);
        proof_tx.send((status, value)).expect("proof channel");
    });

    // Let the worker's POST enter the handler and reach the verifier
    // sleep before we issue /ready. 50ms is comfortably more than the
    // TCP+axum dispatch cost on a loopback connection.
    thread::sleep(Duration::from_millis(50));

    let start = Instant::now();
    let (ready_status, ready_body) = http_get(addr, "/ready");
    let elapsed = start.elapsed();

    assert_eq!(
        ready_status, 200,
        "/ready must return 200 while a verifier call is in flight; \
         body: {ready_body}"
    );
    assert!(
        elapsed < Duration::from_millis(200),
        "/ready must respond in well under the verifier's 500ms sleep \
         so a stuck verify path cannot mask a healthy node as unready. \
         Observed {elapsed:?}. If this assertion fires, the bounty \
         proof handler is still holding the write lock during \
         verifier.verify — drop the lock first."
    );

    let (proof_status, proof_body) = proof_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("proof worker completes");
    assert_eq!(
        proof_status, 200,
        "POST /bounties/slow-1/proof must still succeed after the \
         refactor; body: {proof_body}"
    );
    assert_eq!(
        proof_body.get("accepted"),
        Some(&Value::Bool(true)),
        "mock-slow verifier returns Ok(true); body: {proof_body}"
    );

    worker.join().expect("worker joined");
    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    let _ = std::fs::remove_dir_all(&dir);
}
