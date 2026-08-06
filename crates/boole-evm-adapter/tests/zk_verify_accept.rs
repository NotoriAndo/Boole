//! Real-frozen-proof ACCEPT + the reject matrix for the verifier-only
//! compressed-STARK path (ADR-0018, operator msg 3522/3526).
//!
//! Unlike `zk_verify_bounds.rs` (synthetic), these consume the committed frozen
//! fixtures under `fixtures/evm-zkvm/` so that CI — not just a local run —
//! checks the ACCEPT path and every REJECT gate:
//!
//! * ACCEPT — the real transcoded proof + real public values verify against the
//!   compiled-in FIXED vk.
//! * crypto REJECT — a tampered proof, or public values tampered in a field the
//!   admission gate does NOT read, fail the cryptographic compressed-STARK check.
//! * admission REJECT — public values whose frozen `task_contract` / `fork` /
//!   `author_oracle` header digest is wrong are refused by the external
//!   admission filter, each naming its own field.
//!
//! The proof's byte digest is pinned only by the fixture `SHA256SUMS` (integrity);
//! it is NOT a production accept condition — a different valid proof of the same
//! frozen task would also ACCEPT.

use boole_evm_adapter::zk_verify::{CompressedProofVerifier, VerifyOutcome, VerifyReject};

const FROZEN_PROOF: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/evm-zkvm/proof-compressed.bin"
));
const FROZEN_PV: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/evm-zkvm/public-values.bin"
));

/// ACCEPT: the real frozen proof + real public values verify against the FIXED
/// compiled-in vk. This is the guarantee that PR #109 (reject-only) lacked.
#[test]
fn real_frozen_proof_is_accepted() {
    let verifier = CompressedProofVerifier::frozen();
    assert_eq!(
        verifier.verify(FROZEN_PROOF, FROZEN_PV),
        VerifyOutcome::Accepted,
        "the real frozen proof must ACCEPT against the frozen vk"
    );
}

/// crypto REJECT: flip the last byte of the proof; the compressed STARK no
/// longer verifies.
#[test]
fn tampered_proof_is_rejected() {
    let mut proof = FROZEN_PROOF.to_vec();
    let last = proof.len() - 1;
    proof[last] ^= 0x01;
    match CompressedProofVerifier::frozen().verify(&proof, FROZEN_PV) {
        VerifyOutcome::Rejected(VerifyReject::ProofRejected { .. }) => {}
        other => panic!("expected ProofRejected for tampered proof, got {other:?}"),
    }
}

/// crypto REJECT: tamper the public values in a NON-admission header field
/// (`case_or_batch_root`, offset 32..64). The admission gate passes, but the
/// STARK binding to the exact pv bytes makes the crypto gate reject — proving
/// the cryptographic public-values binding is live, not just the admission
/// policy.
#[test]
fn public_values_tampered_in_bound_field_is_rejected_by_crypto() {
    let mut pv = FROZEN_PV.to_vec();
    pv[40] ^= 0x01; // inside case_or_batch_root (32..64), not read by admission
    match CompressedProofVerifier::frozen().verify(FROZEN_PROOF, &pv) {
        VerifyOutcome::Rejected(VerifyReject::ProofRejected { .. }) => {}
        other => panic!("expected crypto ProofRejected for tampered pv, got {other:?}"),
    }
}

/// admission REJECT: a wrong `task_contract` header digest (offset 0..32) is
/// refused by the external admission filter, before the crypto gate.
#[test]
fn wrong_task_contract_is_admission_rejected() {
    let mut pv = FROZEN_PV.to_vec();
    pv[0] ^= 0x01;
    match CompressedProofVerifier::frozen().verify(FROZEN_PROOF, &pv) {
        VerifyOutcome::Rejected(VerifyReject::AdmissionMismatch { field, .. }) => {
            assert_eq!(field, "task_contract");
        }
        other => panic!("expected AdmissionMismatch(task_contract), got {other:?}"),
    }
}

/// admission REJECT: a wrong `fork` header digest (offset 64..96) is refused.
#[test]
fn wrong_fork_is_admission_rejected() {
    let mut pv = FROZEN_PV.to_vec();
    pv[64] ^= 0x01;
    match CompressedProofVerifier::frozen().verify(FROZEN_PROOF, &pv) {
        VerifyOutcome::Rejected(VerifyReject::AdmissionMismatch { field, .. }) => {
            assert_eq!(field, "fork");
        }
        other => panic!("expected AdmissionMismatch(fork), got {other:?}"),
    }
}

/// admission REJECT: a wrong `author_oracle` header digest (offset 128..160) is
/// refused.
#[test]
fn wrong_author_oracle_is_admission_rejected() {
    let mut pv = FROZEN_PV.to_vec();
    pv[128] ^= 0x01;
    match CompressedProofVerifier::frozen().verify(FROZEN_PROOF, &pv) {
        VerifyOutcome::Rejected(VerifyReject::AdmissionMismatch { field, .. }) => {
            assert_eq!(field, "author_oracle");
        }
        other => panic!("expected AdmissionMismatch(author_oracle), got {other:?}"),
    }
}

/// admission REJECT: public values shorter than the 192-byte header cannot be
/// admitted.
#[test]
fn short_public_values_are_rejected() {
    let short = &FROZEN_PV[..100];
    match CompressedProofVerifier::frozen().verify(FROZEN_PROOF, short) {
        VerifyOutcome::Rejected(VerifyReject::PublicValuesTooShort { len }) => {
            assert_eq!(len, 100);
        }
        other => panic!("expected PublicValuesTooShort, got {other:?}"),
    }
}
