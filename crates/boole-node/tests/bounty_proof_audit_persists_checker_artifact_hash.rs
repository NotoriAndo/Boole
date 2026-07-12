//! P1.4 — second sub-slice. Audit ledger event for an accepted
//! Lean-verified proof must also carry the `checkerArtifactHash`
//! emitted by the verifier so `boole state verify --deep` (master
//! plan line 110-141) can confirm not just *which* Lean checker
//! configuration was named (slice 19 pinned `verifierHash` from the
//! bounty), but *which physical checker artifact* (compiled Lean
//! package) actually adjudicated the proof.
//!
//! Without this, an operator who later upgrades or rebuilds the
//! checker package and re-runs `--deep` cannot prove the historical
//! verdict still reproduces; the deep-verify story leaks the implicit
//! assumption that the checker artifact is bit-identical across time.
//!
//! Contract this test pins:
//!   * Phase 2 of the proof flow calls
//!     `BountyProofVerifier::verify_with_evidence`, not `verify`, so
//!     a verifier can surface side-band evidence (a JSON map) that
//!     bypasses the boolean return surface used by 4xx-equivalent
//!     rejection paths.
//!   * For `bounty.verifier.kind == "lean"`, every key the verifier
//!     places in the evidence map is merged verbatim into the audit
//!     ledger event JSON. In particular, `checkerArtifactHash` must
//!     reach the durable ledger.
//!   * Non-evidence event fields (`schemaVersion`, `kind`, `workId`,
//!     `verifierKind`, `proofHash`, `solverPk`, `accepted`, `reward`,
//!     `credit`, plus the slice-19 `leanSource`/`verifierHash` pulled
//!     from envelope/bounty.metadata) stay byte-identical so existing
//!     consumers do not regress.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{
    canonical_payload_hash_hex, Bounty, BountyProofVerifier, SigningKeyV2, VerifyOutcome,
};
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Map, Value};

fn fresh_nonce() -> String {
    format!("nonce-{}", rand_suffix())
}

const LEAN_VERIFIER_HASH: &str = "abcd000000000000000000000000000000000000000000000000000000000000";

fn signed_proof_body(key: &SigningKeyV2, bounty_id: &str, envelope: Value) -> Value {
    // §SC W1.b — the node re-derives the proof hash from the envelope's
    // canonical JSON and rejects mismatches, so the test computes it the
    // same way instead of claiming a dummy value.
    let proof_hash = canonical_payload_hash_hex(&envelope);
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
const LEAN_PROBLEM_HASH: &str = "9999000000000000000000000000000000000000000000000000000000000000";
const LEAN_SOURCE: &str = "theorem boole_lean_checker_artifact : 1 + 1 = 2 := by\n  decide\n";
const CHECKER_ARTIFACT_HASH: &str =
    "fedc000000000000000000000000000000000000000000000000000000000000";

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

/// Mock that proves the *plumbing*: verifier returns evidence containing
/// `checkerArtifactHash` via the new `verify_with_evidence` method. The
/// production `LeanBountyVerifier` will fill the same key from
/// `LeanCheckResult.evidence.checker_artifact_hash`, but here we pin the
/// audit-write contract without needing the real Lean toolchain.
struct LeanMockWithEvidence;
impl BountyProofVerifier for LeanMockWithEvidence {
    fn verify(&self, _bounty: &Bounty, _envelope: &Value) -> Result<bool, String> {
        Ok(true)
    }
    fn verify_with_evidence(
        &self,
        _bounty: &Bounty,
        _envelope: &Value,
    ) -> Result<VerifyOutcome, String> {
        let mut evidence: Map<String, Value> = Map::new();
        evidence.insert(
            "checkerArtifactHash".to_string(),
            Value::String(CHECKER_ARTIFACT_HASH.to_string()),
        );
        Ok(VerifyOutcome {
            accepted: true,
            evidence,
        })
    }
}

fn write_lean_bounty_fixture(path: &PathBuf) {
    let fixture = json!({
        "version": 1,
        "source": "p1.4 checker-artifact-hash test fixture",
        "generatedAt": "2026-05-20T00:00:00Z",
        "bounties": [
            {
                "id": "lean-1",
                "domain": "test.lean",
                "problemHash": LEAN_PROBLEM_HASH,
                "verifier": {
                    "kind": "lean",
                    "metadata": {
                        "verifierHash": LEAN_VERIFIER_HASH,
                        "profile": "stub"
                    }
                },
                "reward": "100",
                "deadline": 1900000000000u64,
                "status": "open",
                "createdAt": 1800000000000u64,
                "updatedAt": 1800000000000u64
            }
        ]
    });
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&fixture).expect("fixture json"),
    )
    .expect("write fixture");
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
    let body_text = raw.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
    let parsed: Value = serde_json::from_str(body_text).unwrap_or(Value::Null);
    (status, parsed)
}

#[test]
fn accepted_lean_proof_audit_event_records_checker_artifact_hash_from_verifier_evidence() {
    let dir = std::env::temp_dir().join(format!(
        "boole-lean-checker-artifact-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let bounties_path = dir.join("bounties.json");
    let bounty_event_path = dir.join("bounty-events.ndjson");
    let block_path = dir.join("blocks.ndjson");
    write_lean_bounty_fixture(&bounties_path);

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (ready_tx, ready_rx) = mpsc::channel();
    let scenario = scenario_path();
    let bounties_path_for_thread = bounties_path.clone();
    let bounty_event_for_thread = bounty_event_path.clone();
    let block_for_thread = block_path.clone();

    let mut verifiers: HashMap<String, Arc<dyn BountyProofVerifier>> = HashMap::new();
    verifiers.insert("lean".to_string(), Arc::new(LeanMockWithEvidence));

    let handle = thread::spawn(move || {
        ready_tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                proof_dedup_ledger_path: None,
                scenario_path: scenario,
                block_path: block_for_thread,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: Some(bounties_path_for_thread),
                bounty_event_ledger_path: Some(bounty_event_for_thread),
                bounty_verifiers: Some(verifiers),
                family_manifests_dir: None,
                max_requests: Some(1),
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
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));

    let key = SigningKeyV2::from_dev_id("bounty-checker-artifact-hash-test");
    let body = signed_proof_body(&key, "lean-1", json!({ "leanSource": LEAN_SOURCE }));
    let (status, resp) = http_post(addr, "/bounties/lean-1/proof", &body);
    assert_eq!(status, 200, "expected 200, got {status}: {resp}");
    assert_eq!(resp["accepted"], true, "mock must accept: {resp}");

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    let raw = std::fs::read_to_string(&bounty_event_path)
        .expect("audit ledger file present after accept");
    let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "exactly one audit-ledger line for one accepted proof; got {} lines: {raw:?}",
        lines.len()
    );
    let event: Value = serde_json::from_str(lines[0]).expect("audit line is valid JSON");

    // Slice 19 fields stay byte-stable so consumers reading the older
    // schema do not regress.
    assert_eq!(event["verifierKind"], "lean");
    assert_eq!(event["accepted"], true);
    assert_eq!(
        event["leanSource"].as_str(),
        Some(LEAN_SOURCE),
        "leanSource pinned by slice 19 must still land in the event: {event}"
    );
    assert_eq!(
        event["verifierHash"].as_str(),
        Some(LEAN_VERIFIER_HASH),
        "verifierHash pinned by slice 19 must still land in the event: {event}"
    );

    // P1.4 slice-20 contract: checkerArtifactHash from the verifier's
    // evidence map must reach the durable ledger so deep-verify can
    // cross-check that the *physical* compiled checker matches the one
    // that adjudicated the proof.
    assert_eq!(
        event["checkerArtifactHash"].as_str(),
        Some(CHECKER_ARTIFACT_HASH),
        "P1.4 — audit line must persist verifier evidence \
         checkerArtifactHash for deep re-verification. event: {event}"
    );
}
