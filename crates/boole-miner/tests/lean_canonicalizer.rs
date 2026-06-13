//! N0.2 — `LeanBoundCanonicalizer` produces a POFP-v2-shaped package built
//! from the family's rendered canonical proof plus injected checker
//! evidence, accepted by the core validator, and distinct from the
//! structural BPPK placeholder. The live-loop switch is N0.3.

use boole_core::{calibration_policy, CalibrationPolicy, CalibrationReport, ValidationResult};
use boole_miner::{
    bppk_canon_hash, Canonicalizer, LeanBoundCanonicalizer, StructuralCanonicalizer, Target,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    cfg: CalibrationReport,
}

fn policy() -> CalibrationPolicy {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/rate-limiter/v1.json"
    ))
    .expect("fixture parses");
    calibration_policy(&fixture.cfg).expect("policy parses")
}

fn v1_target() -> Target {
    Target {
        seed_hex: "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20".to_string(),
        d: 1,
        profile: "v1-lenbound".to_string(),
        n: 1,
        render: "render text".to_string(),
    }
}

#[test]
fn lean_bound_canon_bytes_validate_via_core_policy() {
    let cz = LeanBoundCanonicalizer::new("verifier-hash-x", "checker-artifact-hash-y");
    let canon = cz
        .canonicalize("model answer is not the canon source", &v1_target())
        .expect("lean-bound canonicalize succeeds for a valid v1 seed");
    let result = boole_core::validate_proof_package_with_policy(&canon, &policy());
    assert!(
        matches!(result, ValidationResult::Ok { .. }),
        "core validator must accept the lean-bound package, got {result:?}"
    );
}

#[test]
fn lean_bound_canon_differs_from_bppk_placeholder() {
    let target = v1_target();
    let proof_source = "by trivial";

    let lean_bound = LeanBoundCanonicalizer::new("verifier-hash-x", "checker-artifact-hash-y")
        .canonicalize(proof_source, &target)
        .expect("lean-bound canonicalize succeeds");
    let placeholder = StructuralCanonicalizer
        .canonicalize(proof_source, &target)
        .expect("structural canonicalize succeeds");

    assert_ne!(
        bppk_canon_hash(&lean_bound),
        bppk_canon_hash(&placeholder),
        "lean-bound canon hash must differ from the bppk placeholder hash"
    );
}
