//! S21 — Hard Guard regression. The bounty consensus track must not
//! alter base-lane economic params (`T_share`/`T_block`/`MinShareScore`/
//! `K_max`) until a `FamilyManifest` is signed and its `activation_height`
//! is reached. This slice ships only the side-pool — activation gating
//! lands in S22 — so right now the invariant is straightforward: bounty
//! proof accept paths must not mutate `SharePool` and must not change
//! `build_block_selection` output.
//!
//! The regression exercises both halves:
//!   * The boot loader registers `*.json` files from `--family-manifests`
//!     into the runtime registry, and `/status` exposes the count.
//!   * Submitting bounty proofs (accept + reject + dedup) leaves the
//!     base-lane status fields (`height`, `c`, `sharePoolSize`,
//!     `replayMatchesRuntime`) byte-equal to the no-bounty baseline.
//!     Only `bountySidePoolTotal` ticks up.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{
    parse_family_manifest, Bounty, BountyProofVerifier, FamilyManifestParseResult, SigningKeyV2,
};
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

fn fresh_nonce() -> String {
    format!("nonce-{}", rand_suffix())
}

const PROOF_HASH_A: &str = "aaaa000000000000000000000000000000000000000000000000000000000000";
const PROOF_HASH_B: &str = "bbbb000000000000000000000000000000000000000000000000000000000000";
const PROOF_HASH_C: &str = "cccc000000000000000000000000000000000000000000000000000000000000";

fn test_prover_key() -> SigningKeyV2 {
    SigningKeyV2::from_dev_id("hard-guard-test-prover")
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

fn write_family_manifest(dir: &Path, name: &str, family_id: &str) {
    let v = json!({
        "version": "1",
        "familyId": family_id,
        "generatorHash": "abababababababababababababababababababababababababababababababab",
        "verifierHash": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
        "canonicalizerHash": "efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef",
        "promptSpecHash": "0101010101010101010101010101010101010101010101010101010101010101",
        "calibrationReportHash": "2323232323232323232323232323232323232323232323232323232323232323",
        "testVectorsHash": "4545454545454545454545454545454545454545454545454545454545454545",
        "resourceLimits": { "maxProofBytes": 16384, "verifyTimeoutMs": 30000, "maxDecls": 1024 },
        "rewardPolicy": { "mode": "no_protocol_reward", "maxBlockRewardShareBps": 0 },
        "activationHeight": u64::MAX,
        "status": "experimental"
    });
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap()).expect("write manifest");
}

/// Write a manifest signed by `signing_key` so it passes both
/// `signature.is_some()` and `verify_family_manifest_signature`. The body
/// is parsed into a `FamilyManifest`, serialized canonically (omitting
/// `signature`), signed, and the resulting hex64 dropped back into the
/// raw JSON before disk write — mirrors the operator workflow.
fn write_signed_family_manifest(
    dir: &Path,
    name: &str,
    family_id: &str,
    activation_height: u64,
    max_shares_per_block: u64,
    max_reward_credit_per_block: &str,
    signing_key: &SigningKeyV2,
) {
    let body = json!({
        "version": "1",
        "familyId": family_id,
        "generatorHash": "abababababababababababababababababababababababababababababababab",
        "verifierHash": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
        "canonicalizerHash": "efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef",
        "promptSpecHash": "0101010101010101010101010101010101010101010101010101010101010101",
        "calibrationReportHash": "2323232323232323232323232323232323232323232323232323232323232323",
        "testVectorsHash": "4545454545454545454545454545454545454545454545454545454545454545",
        "resourceLimits": { "maxProofBytes": 16384, "verifyTimeoutMs": 30000, "maxDecls": 1024 },
        "rewardPolicy": { "mode": "no_protocol_reward", "maxBlockRewardShareBps": 0 },
        "activationHeight": activation_height,
        "status": "experimental",
        "caps": {
            "maxSharesPerBlock": max_shares_per_block,
            "maxScoreMultiplierBps": 10000,
            "maxRewardCreditPerBlock": max_reward_credit_per_block
        }
    });
    let parsed = match parse_family_manifest(&body) {
        FamilyManifestParseResult::Ok(m) => *m,
        FamilyManifestParseResult::Err(e) => panic!("parse signed manifest: {e}"),
    };
    let envelope = signing_key
        .sign(&serde_json::to_value(&parsed).expect("manifest serialize"))
        .expect("manifest sign");
    let mut signed = body.clone();
    signed
        .as_object_mut()
        .unwrap()
        .insert("signature".to_string(), Value::String(envelope.signature));
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_string_pretty(&signed).unwrap())
        .expect("write signed manifest");
}

struct Boot {
    addr: SocketAddr,
    handle: thread::JoinHandle<anyhow::Result<()>>,
    dir: PathBuf,
}

fn boot(family_dir: Option<PathBuf>, max_requests: usize) -> Boot {
    boot_full(family_dir, vec![], false, max_requests)
}

fn boot_with_operator_pks(
    family_dir: Option<PathBuf>,
    operator_signer_pks: Vec<String>,
    max_requests: usize,
) -> Boot {
    boot_full(family_dir, operator_signer_pks, false, max_requests)
}

fn boot_with_reward_ledger_and_operator(
    family_dir: Option<PathBuf>,
    operator_signer_pks: Vec<String>,
    max_requests: usize,
) -> Boot {
    boot_full(family_dir, operator_signer_pks, true, max_requests)
}

fn boot_full(
    family_dir: Option<PathBuf>,
    operator_signer_pks: Vec<String>,
    with_reward_ledger: bool,
    max_requests: usize,
) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-s21-hard-guard-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    boot_full_at_dir(
        dir,
        family_dir,
        operator_signer_pks,
        with_reward_ledger,
        max_requests,
    )
}

/// P1.5b — boot helper variant that reuses a caller-supplied state
/// directory. Lets a test simulate a node restart by booting twice
/// against the same `bounty-events.ndjson` / `blocks.ndjson` files.
fn boot_full_at_dir(
    dir: PathBuf,
    family_dir: Option<PathBuf>,
    operator_signer_pks: Vec<String>,
    with_reward_ledger: bool,
    max_requests: usize,
) -> Boot {
    let block_path = dir.join("blocks.ndjson");
    let bounty_event_path = dir.join("bounty-events.ndjson");
    let reward_ledger_path = if with_reward_ledger {
        Some(dir.join("rewards.ndjson"))
    } else {
        None
    };

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path_for_thread = block_path.clone();
    let reward_for_thread = reward_ledger_path.clone();
    let bounties_path = mock_bounty_fixture_path();
    let verifiers = default_mock_verifiers();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path: block_path_for_thread,
                reward_ledger_path: reward_for_thread,
                work_manifests_path: None,
                bounties_path: Some(bounties_path),
                bounty_event_ledger_path: Some(bounty_event_path),
                bounty_verifiers: Some(verifiers),
                family_manifests_dir: family_dir,
                max_requests: Some(max_requests),
                operator_signer_pks,
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
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Boot { addr, handle, dir }
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

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
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
    let (_, body_text) = raw.split_once("\r\n\r\n").unwrap_or(("", ""));
    let parsed: Value = serde_json::from_str(body_text).unwrap_or(Value::Null);
    (status, parsed)
}

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

/// `/status` fields whose values must be byte-identical between baseline
/// and bounty-active. Excludes `bountySidePoolTotal` (the side-pool count
/// is the one field that legitimately changes) and clock-derived fields.
const HARD_GUARD_STATUS_FIELDS: &[&str] = &["height", "c", "sharePoolSize", "replayMatchesRuntime"];

fn extract_hard_guard_view(status: &Value) -> Value {
    let mut view = serde_json::Map::new();
    for field in HARD_GUARD_STATUS_FIELDS {
        if let Some(v) = status.get(*field) {
            view.insert((*field).to_string(), v.clone());
        }
    }
    Value::Object(view)
}

#[test]
fn boot_loader_registers_family_manifests_from_dir() {
    let manifest_dir = std::env::temp_dir().join(format!(
        "boole-s21-fmr-boot-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&manifest_dir);
    std::fs::create_dir_all(&manifest_dir).expect("tmp manifest dir");
    write_family_manifest(&manifest_dir, "alpha.json", "test.mock-accept");
    write_family_manifest(&manifest_dir, "beta.json", "test.mock-reject");

    let boot = boot(Some(manifest_dir.clone()), 1);
    let (status, body) = http_get(boot.addr, "/status");
    assert_eq!(status, 200, "status code: {body}");
    assert_eq!(
        body["familyManifestCount"], 2,
        "expected familyManifestCount=2 from boot loader: {body}"
    );

    boot.handle.join().expect("server thread").expect("ok");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&manifest_dir);
}

#[test]
fn bounty_proofs_do_not_mutate_base_lane_status() {
    // Boot the node WITH a family manifest registered for the same
    // family_id ("test.mock-accept") that `gamma-1` lives in (its
    // `domain` field). Submitting an accepted proof must route the
    // share into the bounty side-pool, NOT into the base SharePool —
    // and the base-lane fields surfaced via /status must stay byte-
    // identical to the no-bounty baseline.
    let manifest_dir = std::env::temp_dir().join(format!(
        "boole-s21-fmr-isolate-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&manifest_dir);
    std::fs::create_dir_all(&manifest_dir).expect("tmp manifest dir");
    write_family_manifest(&manifest_dir, "accept.json", "test.mock-accept");

    let boot = boot(Some(manifest_dir.clone()), 7);

    // 1) Capture baseline /status before any bounty traffic.
    let (status_code, baseline) = http_get(boot.addr, "/status");
    assert_eq!(status_code, 200);
    let baseline_view = extract_hard_guard_view(&baseline);
    assert_eq!(
        baseline["bountySidePoolTotal"], 0,
        "baseline must report empty side-pool: {baseline}"
    );

    // 2) Drive bounty traffic: 1 accept (gamma-1, mock-accept) → side-pool +1.
    //    1 reject (delta-1, mock-reject) → side-pool unchanged. 1 dedup of
    //    the accept → side-pool unchanged.
    let prover = test_prover_key();
    let (s_a, r_a) = http_post(
        boot.addr,
        "/bounties/gamma-1/proof",
        &signed_proof_body(&prover, "gamma-1", PROOF_HASH_A, json!({})),
    );
    assert_eq!(s_a, 200, "gamma-1 accept: {r_a}");
    assert_eq!(r_a["accepted"], true);

    let (s_b, r_b) = http_post(
        boot.addr,
        "/bounties/delta-1/proof",
        &signed_proof_body(&prover, "delta-1", PROOF_HASH_B, json!({})),
    );
    assert_eq!(s_b, 200, "delta-1 reject: {r_b}");
    assert_eq!(r_b["accepted"], false);

    let (s_c, r_c) = http_post(
        boot.addr,
        "/bounties/gamma-1/proof",
        &signed_proof_body(&prover, "gamma-1", PROOF_HASH_A, json!({})),
    );
    assert_eq!(s_c, 200, "gamma-1 dedup: {r_c}");
    assert_eq!(r_c["duplicate"], true);

    // 3) Capture post-traffic /status. Hard-Guard view must be byte-equal.
    let (_, after) = http_get(boot.addr, "/status");
    let after_view = extract_hard_guard_view(&after);
    assert_eq!(
        baseline_view, after_view,
        "Hard Guard violated — base-lane status fields drifted:\nbefore={baseline_view}\nafter={after_view}"
    );

    // 4) Side-pool legitimately advanced by 1 (the one accept).
    assert_eq!(
        after["bountySidePoolTotal"], 1,
        "expected side-pool to record the one accepted proof: {after}"
    );

    // 5) Reject of a proof against an unregistered family must not crash.
    //    `epsilon-1` (`test.expired-bounty` domain) has no matching
    //    manifest in our dir but is also `withdrawn`, so it returns 409
    //    before reaching the side-pool. This pins that the side-pool
    //    insert path is only reached AFTER the verifier accepts.
    let (s_d, r_d) = http_post(
        boot.addr,
        "/bounties/epsilon-1/proof",
        &signed_proof_body(&prover, "epsilon-1", PROOF_HASH_C, json!({})),
    );
    assert_eq!(s_d, 409, "epsilon-1 must hit terminal: {r_d}");
    let (_, final_status) = http_get(boot.addr, "/status");
    assert_eq!(
        final_status["bountySidePoolTotal"], 1,
        "rejected/terminal path must not advance side-pool: {final_status}"
    );

    boot.handle.join().expect("server thread").expect("ok");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&manifest_dir);
}

#[test]
fn promotion_active_does_not_alter_base_lane_status() {
    // S22c — Hard Guard under active promotion. With a SIGNED family
    // manifest whose `activation_height=0` and an operator pk that
    // matches the signature, `select_promoted_bounty_shares` should pull
    // the accepted bounty share into the promoted slice. Critically,
    // the base-lane `/status` view (`height`, `c`, `sharePoolSize`,
    // `replayMatchesRuntime`) must STAY byte-equal to the pre-traffic
    // baseline — promotion is read-side only.
    let manifest_dir = std::env::temp_dir().join(format!(
        "boole-s22-promo-active-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&manifest_dir);
    std::fs::create_dir_all(&manifest_dir).expect("tmp manifest dir");
    let signing_key = SigningKeyV2::from_dev_id("op-s22");
    write_signed_family_manifest(
        &manifest_dir,
        "promo.json",
        "test.mock-accept",
        0,
        5,
        "0",
        &signing_key,
    );

    let boot = boot_with_operator_pks(Some(manifest_dir.clone()), vec![signing_key.pk_hex()], 3);

    // 1) Baseline before any bounty traffic — promoted slice must be
    //    empty (side-pool is empty).
    let (s_b, baseline) = http_get(boot.addr, "/status");
    assert_eq!(s_b, 200, "baseline status: {baseline}");
    assert_eq!(
        baseline["promotedBountySharesCount"], 0,
        "baseline promoted slice must be empty: {baseline}"
    );
    let baseline_view = extract_hard_guard_view(&baseline);

    // 2) Submit one accepted proof for `gamma-1` (domain
    //    `test.mock-accept`, matching the signed manifest's family_id).
    let prover = test_prover_key();
    let (s_a, r_a) = http_post(
        boot.addr,
        "/bounties/gamma-1/proof",
        &signed_proof_body(&prover, "gamma-1", PROOF_HASH_A, json!({})),
    );
    assert_eq!(s_a, 200, "gamma-1 accept: {r_a}");
    assert_eq!(r_a["accepted"], true);

    // 3) After promotion is active, the promoted slice MUST surface the
    //    accepted share. Hard Guard view stays byte-identical.
    let (_, after) = http_get(boot.addr, "/status");
    assert_eq!(
        after["promotedBountySharesCount"], 1,
        "promotion gating: signed manifest + matching operator pk must \
         expose the accepted share: {after}"
    );
    let after_view = extract_hard_guard_view(&after);
    assert_eq!(
        baseline_view, after_view,
        "Hard Guard violated under active promotion — base-lane status \
         drifted:\nbefore={baseline_view}\nafter={after_view}"
    );

    boot.handle.join().expect("server thread").expect("ok");
    let _ = std::fs::remove_dir_all(&boot.dir);
    let _ = std::fs::remove_dir_all(&manifest_dir);
}

/// runtime-smoke step 0 body — the genesis-c share that drives a deterministic
/// block 0 commit. Both halves of the S23 regression test commit this same
/// share so the resulting `(height, c, sharePoolSize)` state is identical.
fn smoke_step0_submit_payload() -> Value {
    let scenario_text = std::fs::read_to_string(scenario_path()).expect("read scenario");
    let scenario: Value = serde_json::from_str(&scenario_text).expect("scenario json");
    let body = scenario["steps"][0]["body"].clone();
    json!({"body": body, "canonTag": 0})
}

#[test]
fn promoted_credit_lands_in_balance_and_preserves_hard_guard() {
    // S23 — when a signed family manifest is active and an accepted bounty
    // proof has entered the side-pool, a base-lane block commit must:
    //   * fold the promoted credit into the persisted reward ledger so
    //     `/account/{prover_pk}/balance` surfaces it,
    //   * emit a parallel "credit" event into the bounty event ledger,
    //   * leave base-lane Hard-Guard view (`height`, `c`, `sharePoolSize`,
    //     `replayMatchesRuntime`) byte-identical to a no-bounty baseline
    //     that committed the same share.
    //
    // We boot twice. The two runs differ ONLY in bounty traffic — the
    // base-lane share submission is byte-identical. If promotion bleeds
    // into the base lane (mutates the SharePool, alters proposer/score
    // rules, etc.) the two committed blocks would diverge. They must not.

    // ---------- Boot A: baseline, no manifest, no bounty traffic ----------
    let baseline = boot_with_reward_ledger_and_operator(None, vec![], 2);
    let (s_sub_a, r_sub_a) = http_post(baseline.addr, "/submit", &smoke_step0_submit_payload());
    assert_eq!(s_sub_a, 200, "baseline submit: {r_sub_a}");
    assert_eq!(
        r_sub_a["accepted"], true,
        "baseline submit must accept: {r_sub_a}"
    );
    let (_, baseline_status) = http_get(baseline.addr, "/status");
    let baseline_view = extract_hard_guard_view(&baseline_status);
    baseline
        .handle
        .join()
        .expect("baseline thread")
        .expect("ok");
    let _ = std::fs::remove_dir_all(&baseline.dir);

    // ---------- Boot B: promoted, signed manifest with non-zero credit cap
    let manifest_dir = std::env::temp_dir().join(format!(
        "boole-s23-promo-credit-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&manifest_dir);
    std::fs::create_dir_all(&manifest_dir).expect("tmp manifest dir");
    let signing_key = SigningKeyV2::from_dev_id("op-s23");
    write_signed_family_manifest(
        &manifest_dir,
        "promo.json",
        "test.mock-accept",
        0,
        5,
        "100",
        &signing_key,
    );

    let promoted = boot_with_reward_ledger_and_operator(
        Some(manifest_dir.clone()),
        vec![signing_key.pk_hex()],
        4,
    );

    // 1) Accepted bounty proof → side-pool entry with reward=100.
    let prover = test_prover_key();
    let prover_pk = prover.pk_hex();
    let (s_proof, r_proof) = http_post(
        promoted.addr,
        "/bounties/gamma-1/proof",
        &signed_proof_body(&prover, "gamma-1", PROOF_HASH_A, json!({})),
    );
    assert_eq!(s_proof, 200, "gamma-1 accept: {r_proof}");
    assert_eq!(r_proof["accepted"], true);

    // 2) Base-lane share commit. The promoted selection runs at this point
    //    and folds gamma-1's credit into the same reward event.
    let (s_sub_b, r_sub_b) = http_post(promoted.addr, "/submit", &smoke_step0_submit_payload());
    assert_eq!(s_sub_b, 200, "promoted submit: {r_sub_b}");
    assert_eq!(
        r_sub_b["accepted"], true,
        "promoted submit must accept: {r_sub_b}"
    );

    // 3) Balance route surfaces the credit (budget 100, share reward 100,
    //    min(100,100) = 100).
    let (s_bal, balance) = http_get(promoted.addr, &format!("/account/{prover_pk}/balance"));
    assert_eq!(s_bal, 200, "balance status: {balance}");
    assert_eq!(
        balance["balance"], "100",
        "promoted credit must land in reward ledger and surface via balance route: {balance}"
    );

    // 4) Hard-Guard view across the two committed states must be byte-equal.
    let (_, promoted_status) = http_get(promoted.addr, "/status");
    let promoted_view = extract_hard_guard_view(&promoted_status);
    assert_eq!(
        baseline_view, promoted_view,
        "Hard Guard violated — promotion altered base-lane state:\n\
         baseline={baseline_view}\npromoted={promoted_view}"
    );

    // 5) P1.5a — once a share has been promoted into a committed block,
    //    the side-pool MUST drop it so the next block commit does not
    //    re-promote the same proof and double-credit the prover.
    assert_eq!(
        promoted_status["bountySidePoolTotal"], 0,
        "promoted share must be drained from side-pool after block commit \
         to prevent re-promotion / double credit: {promoted_status}"
    );

    promoted
        .handle
        .join()
        .expect("promoted thread")
        .expect("ok");
    let _ = std::fs::remove_dir_all(&promoted.dir);
    let _ = std::fs::remove_dir_all(&manifest_dir);
}

// P1.5b — `BountySidePool` is in-memory only. On boot, the runtime
// rebuilds the registry from the durable bounty event ledger but
// silently drops every accepted share that had not yet been promoted
// into a committed block. After a restart those shares would never
// pay out, even though the verifier already accepted them and the
// audit log records them as accepted. A node restart must not silently
// erase pending bounty credit.
//
// This test is the post-commit twin of P1.5a: P1.5a pinned that
// promoted shares are drained; P1.5b pins that NON-promoted shares
// survive a restart by being rebuilt from the durable audit log.

#[test]
fn boot_restores_unpromoted_bounty_shares_from_durable_audit_log() {
    let manifest_dir = std::env::temp_dir().join(format!(
        "boole-p15b-fmr-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&manifest_dir);
    std::fs::create_dir_all(&manifest_dir).expect("tmp manifest dir");
    write_family_manifest(&manifest_dir, "accept.json", "test.mock-accept");

    let state_dir = std::env::temp_dir().join(format!(
        "boole-p15b-state-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&state_dir);
    std::fs::create_dir_all(&state_dir).expect("tmp state dir");

    // ---------- First boot: accept one bounty proof, then shut down.
    let first = boot_full_at_dir(
        state_dir.clone(),
        Some(manifest_dir.clone()),
        vec![],
        false,
        2,
    );
    let prover = test_prover_key();
    let (s_a, r_a) = http_post(
        first.addr,
        "/bounties/gamma-1/proof",
        &signed_proof_body(&prover, "gamma-1", PROOF_HASH_A, json!({})),
    );
    assert_eq!(s_a, 200, "first-boot accept must succeed: {r_a}");
    assert_eq!(r_a["accepted"], true, "first-boot accept: {r_a}");
    let (_, first_status) = http_get(first.addr, "/status");
    assert_eq!(
        first_status["bountySidePoolTotal"], 1,
        "first boot must record the accepted share in the side-pool: {first_status}"
    );
    first.handle.join().expect("first thread").expect("ok");

    // ---------- Second boot: same state dir, no traffic that touches
    // the side-pool. The accepted share has not been committed into
    // any block (no /submit was issued), so it must be rebuilt from
    // the durable bounty event ledger.
    let second = boot_full_at_dir(
        state_dir.clone(),
        Some(manifest_dir.clone()),
        vec![],
        false,
        1,
    );
    let (s_status, restored) = http_get(second.addr, "/status");
    assert_eq!(s_status, 200, "second-boot status: {restored}");
    assert_eq!(
        restored["bountySidePoolTotal"], 1,
        "second boot must rebuild the unpromoted share from the durable \
         audit log; an empty side-pool would silently drop accepted bounty \
         credit on every restart: {restored}"
    );

    second.handle.join().expect("second thread").expect("ok");
    let _ = std::fs::remove_dir_all(&state_dir);
    let _ = std::fs::remove_dir_all(&manifest_dir);
}
