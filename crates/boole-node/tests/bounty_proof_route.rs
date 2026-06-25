//! S12 — `POST /bounties/{id}/proof` route, 8 branches matching pof parity.
//!
//! Boots a node with `LocalNodeConfig.bounties_path` set to the mock fixture
//! and `LocalNodeConfig.bounty_verifiers` injecting `mock-accept` / `mock-reject`
//! kinds. Asserts the validation order, response shapes, and side-effect
//! contracts (status flip, dedup, ledger append) are byte-frozen against the
//! pof reference (`projects/pof/dispatcher/src/httpServer.ts:337-388`).
//!
//! P1.6d — the route requires a `boole.signed.v1` outer envelope around a
//! `boole.bounty.proof.v1` payload. Operator/static gates (`bounty_not_found`)
//! run before the envelope check; envelope/sig errors precede inner payload
//! errors; the envelope `pk` must equal `payload.prover`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{Bounty, BountyProofVerifier, SigningKeyV2};
// BountyProofVerifier trait lives in boole-core; the existing struct
// `BountyVerifier { kind, metadata }` keeps its name in the bounty schema.
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

fn fresh_nonce() -> String {
    format!("nonce-{}", rand_suffix())
}

const PROOF_HASH_A: &str = "aaaa000000000000000000000000000000000000000000000000000000000000";
const PROOF_HASH_B: &str = "bbbb000000000000000000000000000000000000000000000000000000000000";

fn prover_key() -> SigningKeyV2 {
    SigningKeyV2::from_dev_id("bounty-proof-test-prover")
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

struct MockReject;
impl BountyProofVerifier for MockReject {
    fn verify(&self, _bounty: &Bounty, _envelope: &Value) -> Result<bool, String> {
        Ok(false)
    }
}

fn default_mock_verifiers() -> HashMap<String, Arc<dyn BountyProofVerifier>> {
    let mut m: HashMap<String, Arc<dyn BountyProofVerifier>> = HashMap::new();
    m.insert("mock-accept".to_string(), Arc::new(MockAccept));
    m.insert("mock-reject".to_string(), Arc::new(MockReject));
    m
}

fn boot_with_mock_verifiers(
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    boot_with_signed_nonce_ledger(max_requests, None)
}

fn boot_with_signed_nonce_ledger(
    max_requests: usize,
    signed_nonce_ledger_filename: Option<&str>,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-s12-bounty-proof-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let bounty_event_path = dir.join("bounty-events.ndjson");
    let signed_nonce_path = signed_nonce_ledger_filename.map(|name| dir.join(name));

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path_for_thread = block_path.clone();
    let bounties_path = mock_bounty_fixture_path();
    let verifiers = default_mock_verifiers();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                proof_dedup_ledger_path: None,
                scenario_path: scenario,
                block_path: block_path_for_thread,
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
                signed_nonce_ledger_path: signed_nonce_path,
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
    (addr, handle, dir)
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
    let status_line = raw.lines().next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let (_, body_text) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("no body break in: {raw}"));
    let parsed: Value =
        serde_json::from_str(body_text).unwrap_or_else(|_| panic!("body not json: {body_text}"));
    (status, parsed)
}

/// Build a `boole.signed.v1` envelope around a
/// `boole.bounty.proof.v1` payload signed by `key`. `prover` is filled
/// from the key's pk_hex so the inner prover-vs-pk check passes.
/// `validBefore` is stamped 60s into the future so the freshness (P1.6a)
/// gate admits the request and the future cap (D#3) is not tripped.
fn signed_proof_body(
    key: &SigningKeyV2,
    bounty_id: &str,
    proof_hash: &str,
    envelope: Value,
) -> Value {
    proof_envelope_with_payload(
        key,
        json!({
            "schema": "boole.bounty.proof.v1",
            "bountyId": bounty_id,
            "proofHash": proof_hash,
            "prover": key.pk_hex(),
            "envelope": envelope,
            "validBefore": valid_before_fresh(),
            "nonce": fresh_nonce(),
        }),
    )
}

/// Sign an arbitrary inner payload (lets tests construct deliberately
/// broken payloads while still producing a valid outer envelope).
fn proof_envelope_with_payload(key: &SigningKeyV2, payload: Value) -> Value {
    let signed = key.sign(&payload).expect("sign proof payload");
    json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    })
}

#[test]
fn accept_path_flips_status_to_solved() {
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let key = prover_key();
    let body = signed_proof_body(&key, "gamma-1", PROOF_HASH_A, json!({}));
    let (status, resp) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 200, "expected 200, got {status}: {resp}");
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["accepted"], true);
    assert_eq!(resp["duplicate"], false);
    assert_eq!(resp["bounty"]["status"], "solved");
    assert_eq!(resp["bounty"]["id"], "gamma-1");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reject_path_keeps_status_open() {
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let key = prover_key();
    let body = signed_proof_body(&key, "delta-1", PROOF_HASH_A, json!({}));
    let (status, resp) = http_post(addr, "/bounties/delta-1/proof", &body);
    assert_eq!(status, 200, "expected 200, got {status}: {resp}");
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["accepted"], false);
    assert_eq!(resp["duplicate"], false);
    assert_eq!(resp["bounty"]["status"], "open");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dedup_returns_cached_outcome_without_revisiting_verifier() {
    // Two POSTs of the SAME proof against gamma-1 (mock-accept).
    // Second call must short-circuit at dedup: duplicate=true, no second
    // verifier call, no second ledger event. Also exercises the terminal +
    // dedup interaction (after first call bounty is "solved", but dedup
    // wins over the terminal guard).
    let (addr, handle, dir) = boot_with_mock_verifiers(2);
    let key = prover_key();
    let body = signed_proof_body(&key, "gamma-1", PROOF_HASH_A, json!({}));
    let (s1, r1) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(s1, 200);
    assert_eq!(r1["accepted"], true);
    assert_eq!(r1["duplicate"], false);

    let (s2, r2) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(s2, 200, "second POST must still be 200, got {s2}: {r2}");
    assert_eq!(r2["accepted"], true);
    assert_eq!(
        r2["duplicate"], true,
        "second post must be marked duplicate: {r2}"
    );
    assert_eq!(r2["bounty"]["status"], "solved");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_bounty_returns_404_typed() {
    // bounty_not_found is a public catalog miss; it runs BEFORE envelope
    // parsing so a caller without a signing key can still discover that
    // the route is unusable.
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let key = prover_key();
    let body = signed_proof_body(&key, "no-such", PROOF_HASH_A, json!({}));
    let (status, resp) = http_post(addr, "/bounties/no-such/proof", &body);
    assert_eq!(status, 404, "expected 404, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bounty_not_found");
    assert_eq!(resp["id"], "no-such");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_proof_hash_returns_400_typed() {
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let key = prover_key();
    // proofHash too short. Envelope verifies but inner payload fails the
    // hex32 shape check.
    let body = signed_proof_body(&key, "gamma-1", "deadbeef", json!({}));
    let (status, resp) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 400, "expected 400, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bad_proof_hash");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_prover_returns_400_typed() {
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let key = prover_key();
    // Payload prover is not hex32. Envelope still signs cleanly because
    // signing is over payload bytes, not the prover field's contents.
    // Handler hits the prover hex32 check before the prover==pk check.
    let payload = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": "gamma-1",
        "proofHash": PROOF_HASH_A,
        "prover": "not-a-hex32",
        "envelope": {},
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let body = proof_envelope_with_payload(&key, payload);
    let (status, resp) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 400, "expected 400, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bad_prover");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn terminal_bounty_returns_409_when_not_dedup_hit() {
    // epsilon-1 is `withdrawn` in the fixture. Submitting a fresh proof
    // (not previously seen) must hit the terminal guard, not dedup.
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let key = prover_key();
    let body = signed_proof_body(&key, "epsilon-1", PROOF_HASH_B, json!({}));
    let (status, resp) = http_post(addr, "/bounties/epsilon-1/proof", &body);
    assert_eq!(status, 409, "expected 409, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bounty_terminal");
    assert_eq!(resp["status"], "withdrawn");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_verifier_kind_returns_501_typed() {
    // zeta-1 has verifier.kind = "wholly-unknown-kind".
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let key = prover_key();
    let body = signed_proof_body(&key, "zeta-1", PROOF_HASH_A, json!({}));
    let (status, resp) = http_post(addr, "/bounties/zeta-1/proof", &body);
    assert_eq!(status, 501, "expected 501, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "no_verifier");
    assert_eq!(resp["kind"], "wholly-unknown-kind");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tampered_signature_returns_401_signature_invalid() {
    // Envelope is well-formed (pk + 128-hex signature) but the signature
    // does not validate against the payload bytes.
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let key = prover_key();
    let mut body = signed_proof_body(&key, "gamma-1", PROOF_HASH_A, json!({}));
    // Flip the first hex char of the signature: '0'→'1' or otherwise '0'.
    let sig = body["signature"].as_str().expect("sig str").to_string();
    let mut chars: Vec<char> = sig.chars().collect();
    chars[0] = if chars[0] == '0' { '1' } else { '0' };
    body["signature"] = Value::String(chars.into_iter().collect());
    let (status, resp) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 401, "expected 401, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "signature_invalid");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bad_envelope_schema_returns_400_bad_envelope() {
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let key = prover_key();
    let mut body = signed_proof_body(&key, "gamma-1", PROOF_HASH_A, json!({}));
    body["schema"] = Value::String("boole.signed.WRONG".to_string());
    let (status, resp) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 400, "expected 400, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bad_envelope");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bounty_id_mismatch_returns_400_bad_payload() {
    // Envelope signs cleanly; URL says gamma-1 but payload bountyId
    // claims delta-1 → 400 bad_payload (anti-replay on URL).
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let key = prover_key();
    let body = signed_proof_body(&key, "delta-1", PROOF_HASH_A, json!({}));
    let (status, resp) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 400, "expected 400, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bad_payload");
    assert_eq!(resp["field"], "bountyId");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prover_pk_mismatch_returns_400_bad_payload() {
    // Envelope signed by key A; payload prover = key B's pk. Both are
    // well-formed hex32 so prover-shape passes; the prover==pk check
    // catches the substitution.
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let signer = prover_key();
    let other = SigningKeyV2::from_dev_id("bounty-proof-test-other-prover");
    let payload = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": "gamma-1",
        "proofHash": PROOF_HASH_A,
        "prover": other.pk_hex(),
        "envelope": {},
        "validBefore": valid_before_fresh(),
        "nonce": fresh_nonce(),
    });
    let body = proof_envelope_with_payload(&signer, payload);
    let (status, resp) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 400, "expected 400, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bad_payload");
    assert_eq!(resp["field"], "prover");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

// P1.6a — every signed inner payload must carry `validBefore` (u64 Unix
// seconds) so a replay of a previously-leaked signed envelope cannot be
// posted to a fresh node hours/days later. Missing → 400 bad_payload;
// expired (beyond the configured leeway) → 401 envelope_expired. The
// check runs after the inner schema gate so wire-shape errors keep
// taking precedence over freshness.
#[test]
fn missing_valid_before_returns_400_bad_payload_field_valid_before() {
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let signer = prover_key();
    let payload = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": "gamma-1",
        "proofHash": PROOF_HASH_A,
        "prover": signer.pk_hex(),
        "envelope": {},
    });
    let body = proof_envelope_with_payload(&signer, payload);
    let (status, resp) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 400, "expected 400, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bad_payload");
    assert_eq!(resp["field"], "validBefore");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn expired_valid_before_returns_401_envelope_expired() {
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let signer = prover_key();
    let payload = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": "gamma-1",
        "proofHash": PROOF_HASH_A,
        "prover": signer.pk_hex(),
        "envelope": {},
        // validBefore well in the past (Unix second 1).
        "validBefore": 1_u64,
        "nonce": fresh_nonce(),
    });
    let body = proof_envelope_with_payload(&signer, payload);
    let (status, resp) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 401, "expected 401, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "envelope_expired");
    assert_eq!(resp["validBefore"], 1);
    assert!(
        resp["now"].as_u64().is_some(),
        "envelope_expired must carry `now` so callers see the server clock"
    );

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn far_future_valid_before_returns_400_bad_payload() {
    // D#3 — validBefore must have an upper bound. Without a future cap an
    // envelope stamped years ahead stays replayable until it "expires",
    // defeating the freshness gate entirely.
    let (addr, handle, dir) = boot_with_mock_verifiers(1);
    let signer = prover_key();
    let ten_years_ahead = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 10 * 365 * 24 * 3600)
        .expect("clock after epoch");
    let payload = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": "gamma-1",
        "proofHash": PROOF_HASH_A,
        "prover": signer.pk_hex(),
        "envelope": {},
        "validBefore": ten_years_ahead,
        "nonce": fresh_nonce(),
    });
    let body = proof_envelope_with_payload(&signer, payload);
    let (status, resp) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(status, 400, "expected 400, got {status}: {resp}");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["reason"], "bad_payload");
    assert_eq!(resp["field"], "validBefore");

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn replayed_nonce_with_distinct_proof_hash_returns_409_nonce_replayed() {
    // P1.6b — once a signer burns a nonce on the per-signer signed-envelope
    // ledger, a second envelope from the same signer reusing that nonce is
    // rejected even if the proofHash differs (dedup peek misses). This is
    // the "stolen nonce, fabricated envelope" attack path that dedup alone
    // does not cover.
    let (addr, handle, dir) = boot_with_signed_nonce_ledger(2, Some("signed-nonces.ndjson"));
    let key = prover_key();
    let pk_hex = key.pk_hex();
    let reused_nonce = "deadbeefdeadbeefdeadbeefdeadbeef";

    let payload_a = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": "gamma-1",
        "proofHash": PROOF_HASH_A,
        "prover": pk_hex,
        "envelope": json!({}),
        "validBefore": valid_before_fresh(),
        "nonce": reused_nonce,
    });
    let env_a = proof_envelope_with_payload(&key, payload_a);
    let (s1, r1) = http_post(addr, "/bounties/gamma-1/proof", &env_a);
    assert_eq!(s1, 200, "first proof must succeed: {r1}");
    assert_eq!(r1["accepted"], true);

    let payload_b = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": "gamma-1",
        "proofHash": PROOF_HASH_B,
        "prover": pk_hex,
        "envelope": json!({"different": "envelope"}),
        "validBefore": valid_before_fresh(),
        "nonce": reused_nonce,
    });
    let env_b = proof_envelope_with_payload(&key, payload_b);
    let (s2, r2) = http_post(addr, "/bounties/gamma-1/proof", &env_b);
    assert_eq!(
        s2, 409,
        "different proof envelope reusing the same nonce → 409: {r2}"
    );
    assert_eq!(r2["reason"], "nonce_replayed");
    assert_eq!(r2["signerPk"], pk_hex);
    assert_eq!(r2["nonce"], reused_nonce);

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rejected_proof_burns_nonce_preventing_replay() {
    // Invariant A — the (signer_pk, nonce) burn is unconditional: a proof
    // the verifier REJECTS still consumes its nonce. A rejected envelope
    // must not be replayable (same signer, same nonce, fresh proofHash),
    // otherwise an attacker could grind rejected submissions against a
    // burned-nonce window. delta-1 is wired to mock-reject.
    let (addr, handle, dir) = boot_with_signed_nonce_ledger(2, Some("signed-nonces.ndjson"));
    let key = prover_key();
    let pk_hex = key.pk_hex();
    let reused_nonce = "feedfacefeedfacefeedfacefeedface";

    let payload_a = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": "delta-1",
        "proofHash": PROOF_HASH_A,
        "prover": pk_hex,
        "envelope": json!({}),
        "validBefore": valid_before_fresh(),
        "nonce": reused_nonce,
    });
    let env_a = proof_envelope_with_payload(&key, payload_a);
    let (s1, r1) = http_post(addr, "/bounties/delta-1/proof", &env_a);
    assert_eq!(s1, 200, "rejected proof still returns 200: {r1}");
    assert_eq!(r1["accepted"], false);
    assert_eq!(r1["duplicate"], false);

    let payload_b = json!({
        "schema": "boole.bounty.proof.v1",
        "bountyId": "delta-1",
        "proofHash": PROOF_HASH_B,
        "prover": pk_hex,
        "envelope": json!({"different": "envelope"}),
        "validBefore": valid_before_fresh(),
        "nonce": reused_nonce,
    });
    let env_b = proof_envelope_with_payload(&key, payload_b);
    let (s2, r2) = http_post(addr, "/bounties/delta-1/proof", &env_b);
    assert_eq!(
        s2, 409,
        "nonce burned by the rejected proof must block replay: {r2}"
    );
    assert_eq!(r2["reason"], "nonce_replayed");
    assert_eq!(r2["signerPk"], pk_hex);
    assert_eq!(r2["nonce"], reused_nonce);

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn identical_proof_envelope_retry_returns_200_duplicate_not_nonce_replayed() {
    // HTTP idempotency invariant: the SAME envelope (same proofHash + same
    // nonce) re-POSTed must hit dedup, not the nonce-replay gate. Network
    // retries on a 200 should still return 200 duplicate=true.
    let (addr, handle, dir) = boot_with_signed_nonce_ledger(2, Some("signed-nonces.ndjson"));
    let key = prover_key();
    let body = signed_proof_body(&key, "gamma-1", PROOF_HASH_A, json!({}));
    let (s1, r1) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(s1, 200, "first proof must succeed: {r1}");
    assert_eq!(r1["duplicate"], false);

    let (s2, r2) = http_post(addr, "/bounties/gamma-1/proof", &body);
    assert_eq!(
        s2, 200,
        "identical envelope retry must be 200 duplicate, not 409 replay: {r2}"
    );
    assert_eq!(r2["accepted"], true);
    assert_eq!(r2["duplicate"], true);

    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

fn valid_before_fresh() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() + 60)
        .unwrap_or(u64::MAX / 2)
}
