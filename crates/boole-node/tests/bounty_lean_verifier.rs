//! S12 — `LeanBountyVerifier` adapter wires the existing `LeanRunner`
//! into the `BountyVerifier` trait. Tests are lake/lean-gated (early
//! return with diagnostic when toolchain is absent), matching the pattern
//! in `submit_lean_admission_nonce.rs`.
//!
//! Envelope shape: `{leanSource: "<utf8 .lean source>"}`. The verifier
//! writes that source to a temp file and shells out to `lake exec
//! boole_check` via `LeanRunner::check_file`.

use std::path::PathBuf;
use std::process::Command;

use boole_core::{Bounty, BountyProofVerifier, BountyVerifier};
use boole_node::LeanBountyVerifier;
use serde_json::{json, Map, Value};

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn lake_and_lean_available() -> bool {
    let lake_ok = Command::new("lake")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    let lean_ok = Command::new("lean")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    lake_ok && lean_ok
}

fn canonical_checker_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../lean/checker")
        .canonicalize()
        .expect("canonical checker dir")
}

fn make_lean_bounty(id: &str, verifier_hash: &str) -> Bounty {
    let mut metadata: Map<String, Value> = Map::new();
    metadata.insert(
        "verifierHash".to_string(),
        Value::String(verifier_hash.to_string()),
    );
    Bounty {
        id: id.to_string(),
        domain: "lean.test".to_string(),
        problem_hash: "9999999999999999999999999999999999999999999999999999999999999999"
            .to_string(),
        verifier: BountyVerifier {
            kind: "lean".to_string(),
            metadata,
        },
        reward: "1".to_string(),
        deadline: 1900000000000,
        status: "open".to_string(),
        created_at: 1800000000000,
        updated_at: 1800000000000,
    }
}

#[test]
fn lean_verifier_accepts_valid_proof_envelope() {
    if !lake_and_lean_available() {
        eprintln!("skipping lean_verifier_accepts: lake/lean unavailable");
        return;
    }
    let checker_dir = canonical_checker_dir();
    let verifier_hash = format!(
        "lean-bounty-verifier-test-accept-{}-{}",
        std::process::id(),
        rand_suffix()
    );
    let verifier = LeanBountyVerifier::new(checker_dir);
    let bounty = make_lean_bounty("alpha-1", &verifier_hash);
    let envelope = json!({
        "leanSource": "theorem boole_lean_bounty_accept : 2 + 2 = 4 := by\n  decide\n"
    });
    let result = verifier
        .verify(&bounty, &envelope)
        .expect("verifier returns Ok on valid lean source");
    assert!(result, "valid proof must accept");
}

#[test]
fn lean_verifier_rejects_invalid_proof_envelope() {
    if !lake_and_lean_available() {
        eprintln!("skipping lean_verifier_rejects: lake/lean unavailable");
        return;
    }
    let checker_dir = canonical_checker_dir();
    let verifier_hash = format!(
        "lean-bounty-verifier-test-reject-{}-{}",
        std::process::id(),
        rand_suffix()
    );
    let verifier = LeanBountyVerifier::new(checker_dir);
    let bounty = make_lean_bounty("alpha-1", &verifier_hash);
    let envelope = json!({
        // 1 + 1 = 3 is false; `decide` cannot close it; lake exec exits non-zero.
        "leanSource": "theorem boole_lean_bounty_reject : 1 + 1 = 3 := by\n  decide\n"
    });
    let result = verifier
        .verify(&bounty, &envelope)
        .expect("verifier returns Ok(false), not Err, on invalid lean source");
    assert!(!result, "invalid proof must reject");
}
