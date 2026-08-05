//! RED->GREEN bounds for the verifier-only compressed-STARK path (ADR-0018,
//! operator msg 3518).
//!
//! These are SYNTHETIC and CI-runnable: they never touch the frozen sandbox
//! proof (which lives in a git-ignored tree). They pin the two guarantees the
//! operator required for the bincode-backed deserializer (RUSTSEC-2025-0141,
//! `bincode` unmaintained): an explicit proof-size cap enforced BEFORE
//! deserialization, and rejection of BOTH oversized and unparseable input.
//! They also pin that this crate's DIRECT sp1 dependency is verifier-only
//! (no `sp1-sdk` / `sp1-prover`), matching ADR-0018 decision (f).

use boole_evm_adapter::zk_verify::{
    CompressedProofVerifier, VerifyOutcome, VerifyReject, MAX_COMPRESSED_PROOF_BYTES,
};

/// A dummy preloaded vkey hash. The size/parse gates fire before the vkey is
/// ever consulted, so its exact value is irrelevant to these tests.
fn dummy_vkey_hash() -> Vec<u8> {
    vec![0u8; 32]
}

/// Oversized input is rejected by the size gate BEFORE any deserialization —
/// the distinct `ProofTooLarge` variant (not a parse error) proves the cap
/// fired first, so a hostile blob never reaches the bincode allocator.
#[test]
fn oversized_proof_is_rejected_before_deserialization() {
    let verifier = CompressedProofVerifier::new(dummy_vkey_hash());
    let proof = vec![0u8; MAX_COMPRESSED_PROOF_BYTES + 1];
    match verifier.verify(&proof, &[]) {
        VerifyOutcome::Rejected(VerifyReject::ProofTooLarge { len, max }) => {
            assert_eq!(len, MAX_COMPRESSED_PROOF_BYTES + 1);
            assert_eq!(max, MAX_COMPRESSED_PROOF_BYTES);
        }
        other => panic!("expected ProofTooLarge, got {other:?}"),
    }
}

/// A blob EXACTLY at the cap passes the (inclusive) size gate and proceeds to
/// deserialization, which fails on non-proof bytes -> `ProofRejected`. This
/// pins the boundary AND that a parse failure is a reject, not an accept.
#[test]
fn at_cap_nonproof_passes_size_gate_then_parse_rejects() {
    let verifier = CompressedProofVerifier::new(dummy_vkey_hash());
    // 0xFF prefix forces an immediate invalid-enum-tag deserialize error, so
    // the check is fast and deterministic despite the full-size buffer.
    let proof = vec![0xffu8; MAX_COMPRESSED_PROOF_BYTES];
    match verifier.verify(&proof, &[]) {
        VerifyOutcome::Rejected(VerifyReject::ProofRejected { .. }) => {}
        other => panic!("expected ProofRejected (parse), got {other:?}"),
    }
}

/// Small, clearly-invalid bytes fail to deserialize as a compressed proof and
/// are rejected (never accepted).
#[test]
fn small_garbage_proof_is_rejected() {
    let verifier = CompressedProofVerifier::new(dummy_vkey_hash());
    match verifier.verify(&[0xff, 0xff, 0xff, 0xff, 0x00, 0x01], &[]) {
        VerifyOutcome::Rejected(VerifyReject::ProofRejected { .. }) => {}
        other => panic!("expected ProofRejected, got {other:?}"),
    }
}

/// Empty input is not a valid proof and is rejected.
#[test]
fn empty_proof_is_rejected() {
    let verifier = CompressedProofVerifier::new(dummy_vkey_hash());
    match verifier.verify(&[], &[]) {
        VerifyOutcome::Rejected(VerifyReject::ProofRejected { .. }) => {}
        other => panic!("expected ProofRejected, got {other:?}"),
    }
}

/// This crate's DIRECT sp1 dependency is verifier-only: `sp1-verifier` with
/// `default-features = false`, and neither `sp1-sdk` nor `sp1-prover` as a
/// direct dependency (ADR-0018 (f): "Boole's CALLS are verifier-only"). The
/// transitive `slop-basefold-prover` BUILD dep is out of scope for this check
/// by design — the ADR does not claim the whole GRAPH is verifier-only.
#[test]
fn direct_sp1_dependency_is_verifier_only() {
    let manifest = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
    let mut has_verifier_no_default = false;
    for raw in manifest.lines() {
        // Strip line comments so the "NO sp1-sdk / NO sp1-prover" note in the
        // dependency comment cannot trip the negative checks below.
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let name = line.split([' ', '=', '.']).next().unwrap_or("");
        assert_ne!(name, "sp1-sdk", "sp1-sdk must not be a direct dependency");
        assert_ne!(
            name, "sp1-prover",
            "sp1-prover must not be a direct dependency"
        );
        if name == "sp1-verifier" {
            assert!(
                line.contains("default-features = false"),
                "sp1-verifier must be default-features = false: {line}"
            );
            has_verifier_no_default = true;
        }
    }
    assert!(
        has_verifier_no_default,
        "sp1-verifier (verify-only) must be a direct dependency"
    );
}
