//! S12 / TB.2 — `LeanBountyVerifier` adapter wires the existing
//! `LeanRunner` shell-out into the `BountyVerifier` trait, and binds every
//! accepted proof to the bounty's own commissioned statement. Tests are
//! lake/lean-gated (early return with diagnostic when toolchain is absent)
//! only where an actual Lean elaboration is exercised, matching the
//! pattern in `submit_lean_admission_nonce.rs`.
//!
//! Envelope shape: `{leanSource: "<utf8 .lean source>"}`. TB.2 changed what
//! the verifier does with it: only the proof term *after* the submitter's
//! own `:=` is kept; everything before it (the submitter's self-authored
//! theorem name/statement) is discarded. The proof term is spliced into a
//! fixed template around the bounty's *own* commissioned statement (from
//! `verifier.metadata.statement`), and `problem_hash` must equal that
//! statement's SHA-256 content hash before any Lean subprocess runs.
//! Before this, `problem_hash` was never checked at all, so any proof of
//! any statement could claim any open bounty.

use std::path::PathBuf;
use std::process::Command;

use boole_core::{Bounty, BountyProofVerifier, BountyVerifier};
use boole_node::LeanBountyVerifier;
use boole_testkit::rand_suffix;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

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

fn content_hash_hex(statement: &str) -> String {
    hex::encode(Sha256::digest(statement.as_bytes()))
}

/// Build a bounty whose `problem_hash` is the given (possibly mismatched)
/// hex hash, with `statement` recorded in `verifier.metadata.statement`.
fn make_lean_bounty_with_problem_hash(
    id: &str,
    verifier_hash: &str,
    statement: &str,
    problem_hash: &str,
) -> Bounty {
    let mut metadata: Map<String, Value> = Map::new();
    metadata.insert(
        "verifierHash".to_string(),
        Value::String(verifier_hash.to_string()),
    );
    metadata.insert(
        "statement".to_string(),
        Value::String(statement.to_string()),
    );
    Bounty {
        id: id.to_string(),
        domain: "lean.test".to_string(),
        problem_hash: problem_hash.to_string(),
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

/// Build a self-consistent bounty: `problem_hash` is `statement`'s own
/// content hash, exactly as a correct commissioner announce would set it.
fn make_lean_bounty(id: &str, verifier_hash: &str, statement: &str) -> Bounty {
    let problem_hash = content_hash_hex(statement);
    make_lean_bounty_with_problem_hash(id, verifier_hash, statement, &problem_hash)
}

#[test]
fn lean_verifier_accepts_valid_proof_envelope() {
    // TB.2 — inverted from "accepts any proof regardless of problem_hash"
    // to "accepts only a proof of the bounty's own matching statement".
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
    let bounty = make_lean_bounty("alpha-1", &verifier_hash, "2 + 2 = 4");
    let envelope = json!({
        "leanSource": "theorem boole_lean_bounty_accept : 2 + 2 = 4 := by\n  decide\n"
    });
    let result = verifier
        .verify(&bounty, &envelope)
        .expect("verifier returns Ok on a proof matching the commissioned statement");
    assert!(result, "a proof of the bounty's own statement must accept");
}

#[test]
fn lean_verifier_rejects_proof_of_unrelated_statement() {
    // TB.2 — a proof (`2 + 2 = 4`) submitted against a bounty whose
    // `problem_hash` does not hash the recorded `statement` ("1 + 1 = 2")
    // must reject before any Lean subprocess is spawned — closes "any
    // proof claims any bounty" (previously `problem_hash` was never
    // checked at all). No lake/lean availability gate needed: the
    // mismatch is caught purely at the Rust layer.
    let checker_dir = canonical_checker_dir();
    let verifier_hash = format!(
        "lean-bounty-verifier-test-unrelated-{}-{}",
        std::process::id(),
        rand_suffix()
    );
    let verifier = LeanBountyVerifier::new(checker_dir);
    let bounty = make_lean_bounty_with_problem_hash(
        "alpha-1",
        &verifier_hash,
        "1 + 1 = 2",
        "9999999999999999999999999999999999999999999999999999999999999999",
    );
    let envelope = json!({
        "leanSource": "theorem boole_lean_bounty_unrelated : 2 + 2 = 4 := by\n  decide\n"
    });
    let result = verifier
        .verify(&bounty, &envelope)
        .expect("verifier returns Ok(false) on an unrelated/unbound proof, not Err");
    assert!(
        !result,
        "a proof of a statement unrelated to the bounty's problem_hash must reject"
    );
}

#[test]
fn lean_verifier_rejects_sorry_proof_before_spawning_lake() {
    // E#2 — the forbidden-token scan fires inside `LeanRunner::check_file`
    // BEFORE any lake/lean subprocess is spawned, so this end-to-end
    // rejection must hold even on hosts without the Lean toolchain (no
    // availability gate on purpose).
    let checker_dir = canonical_checker_dir();
    let verifier_hash = format!(
        "lean-bounty-verifier-test-sorry-{}-{}",
        std::process::id(),
        rand_suffix()
    );
    let verifier = LeanBountyVerifier::new(checker_dir);
    let bounty = make_lean_bounty("alpha-1", &verifier_hash, "False");
    let envelope = json!({
        "leanSource": "theorem sneaky : False := by\n  sorry\n"
    });
    let err = verifier
        .verify_with_evidence(&bounty, &envelope)
        .expect_err("sorry-carrying proof must surface as Err, never Ok(accepted)");
    assert!(
        err.contains("sorry"),
        "rejection must name the forbidden token, got: {err}"
    );
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
    // 1 + 1 = 3 is false and is the bounty's own commissioned (matching)
    // statement; `decide` cannot close it; lake exec exits non-zero.
    let bounty = make_lean_bounty("alpha-1", &verifier_hash, "1 + 1 = 3");
    let envelope = json!({
        "leanSource": "theorem boole_lean_bounty_reject : 1 + 1 = 3 := by\n  decide\n"
    });
    let result = verifier
        .verify(&bounty, &envelope)
        .expect("verifier returns Ok(false), not Err, on invalid lean source");
    assert!(!result, "invalid proof must reject");
}
