//! SC.10-ii-a — the single Lean-bound share verifier entry.
//!
//! `verify_lean_bound_share_evidence` is the one classification point that
//! the offline `--deep` audit AND (SC.10-ii-b/c/d) the admission / ingest /
//! reorg consensus paths converge on, so every path reaches the same
//! accept / reject / unavailable decision from the same bytes, committed
//! budget and pinned checker (ADR-0016 (c-2)). These tests pin the
//! host-independent classifications (source re-derivation, canon binding,
//! the `run_lean=false` audit skip); the actual Lean three-state verdict is
//! exercised by the deep-verify round-trip and the ingest slice where lake
//! is available.

use std::path::PathBuf;

use boole_core::{
    family_v1_lenbound, lean_bound_canon_package, lean_bound_verifier_hash, SelectedShareEvidence,
};
use boole_node::{verify_lean_bound_share_evidence, ShareEvidenceVerdict};
use sha2::{Digest, Sha256};

fn canonical_checker_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../lean/checker")
        .canonicalize()
        .expect("canonical checker dir")
}

const PROFILE: &str = "v1-lenbound";
const SEED_HEX: &str = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
const PREV_C: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const PK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const N: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const J: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// Build one Lean-bound share whose `proofPackage` is the canon a live
/// miner would have ground for `SEED_HEX` against the canonical checker.
/// Returns `(share, checker_hash, verifier_hash)` so the test can call the
/// entry with exactly the committed identity the share was bound under.
fn lean_bound_share(
    tamper_package: bool,
    seed_hex: &str,
) -> (SelectedShareEvidence, String, String) {
    let checker_hash = boole_lean_runner::checker_artifact_hash(&canonical_checker_dir())
        .expect("checker artifact hash");
    let verifier_hash = lean_bound_verifier_hash(PROFILE);
    // A valid seed still lets us build a canon; an invalid seed cannot, so
    // for the invalid-seed case we bind an arbitrary package.
    let (canon, proof_package) = match family_v1_lenbound::generate_from_hex(seed_hex) {
        Ok(instance) => {
            let lean_source = family_v1_lenbound::render_canonical_proof(&instance);
            let canon = lean_bound_canon_package(&verifier_hash, &checker_hash, &lean_source);
            let mut pkg = hex::encode(&canon);
            if tamper_package {
                let mut bytes = canon.clone();
                bytes[50] ^= 0xff;
                pkg = hex::encode(bytes);
            }
            (canon, pkg)
        }
        Err(_) => (Vec::new(), "00".repeat(64)),
    };
    let canon_hash = if canon.is_empty() {
        "00".repeat(32)
    } else {
        hex::encode(Sha256::digest(&canon))
    };
    let share = SelectedShareEvidence {
        pk: PK.to_string(),
        n: N.to_string(),
        j: J.to_string(),
        c: PREV_C.to_string(),
        canon_hash,
        proof_package,
        seed_hex: seed_hex.to_string(),
        signed_work: None,
    };
    (share, checker_hash, verifier_hash)
}

#[test]
fn entry_skips_lean_when_run_lean_false_on_matching_canon() {
    let (share, checker_hash, verifier_hash) = lean_bound_share(false, SEED_HEX);
    let verdict = verify_lean_bound_share_evidence(
        PREV_C,
        &share,
        canonical_checker_dir().as_path(),
        &checker_hash,
        &verifier_hash,
        false, // audit path with lake unavailable: canon checked, Lean not run
    );
    assert!(
        matches!(verdict, ShareEvidenceVerdict::LeanSkipped),
        "matching canon + run_lean=false must classify as LeanSkipped, got {verdict:?}"
    );
}

#[test]
fn entry_reports_canon_mismatch_on_tampered_package() {
    let (share, checker_hash, verifier_hash) = lean_bound_share(true, SEED_HEX);
    let verdict = verify_lean_bound_share_evidence(
        PREV_C,
        &share,
        canonical_checker_dir().as_path(),
        &checker_hash,
        &verifier_hash,
        true,
    );
    assert!(
        matches!(verdict, ShareEvidenceVerdict::CanonMismatch { .. }),
        "a tampered proofPackage must classify as CanonMismatch, got {verdict:?}"
    );
}

#[test]
fn entry_reports_not_lean_bound_on_empty_seed() {
    let (mut share, checker_hash, verifier_hash) = lean_bound_share(false, SEED_HEX);
    share.seed_hex = String::new();
    let verdict = verify_lean_bound_share_evidence(
        PREV_C,
        &share,
        canonical_checker_dir().as_path(),
        &checker_hash,
        &verifier_hash,
        true,
    );
    assert!(
        matches!(verdict, ShareEvidenceVerdict::NotLeanBound),
        "an empty seedHex must classify as NotLeanBound, got {verdict:?}"
    );
}

#[test]
fn entry_reports_source_rederive_failed_on_bad_seed() {
    // A non-hex seedHex cannot be decoded into a family seed, so the
    // source the canon claims to bind cannot be re-derived.
    let bad_seed = "not-a-hex-seed";
    let (share, checker_hash, verifier_hash) = lean_bound_share(false, bad_seed);
    let verdict = verify_lean_bound_share_evidence(
        PREV_C,
        &share,
        canonical_checker_dir().as_path(),
        &checker_hash,
        &verifier_hash,
        true,
    );
    assert!(
        matches!(verdict, ShareEvidenceVerdict::SourceRederiveFailed { .. }),
        "an unparseable seedHex must classify as SourceRederiveFailed, got {verdict:?}"
    );
}
