//! Stage-1 node-side EVM verifier bridge — ADR-0019, label
//! `EVM-NODE-BRIDGE-QUALIFIED` (live node-path gate pending).
//!
//! This module QUALIFIES the frozen verifier-only compressed-STARK EVM checker
//! (`boole_evm_adapter::zk_verify::CompressedProofVerifier`) against the node's
//! block-verdict outcome type: it maps a [`VerifyOutcome`] onto the SAME
//! [`BlockReverifyOutcome`] the live re-verify funnel consumes, so the checker's
//! accept/reject decision is proven verdict-shaped. That is the whole of Stage 1.
//!
//! What Stage 1 is NOT (operator msg 3540, 2026-08-06):
//!
//! * It is **not** a live-funnel connection. Nothing here is invoked from any
//!   block-verdict path (`reverify_block_selected_shares` and friends do not
//!   call it), and no block carries EVM evidence — `PersistedBlock` /
//!   `SelectedShareEvidence` have no EVM field. Wiring the gate into the funnel,
//!   and adding the block field that transports the proof bytes, is Stage 2
//!   (ADR-0019, un-ratified).
//! * There is deliberately **no** [`BlockReverifyOutcome::RetryableUnavailable`]
//!   path here. The compressed-STARK verify is a pure, in-process, deterministic
//!   computation over bytes already in hand, so it never yields an availability
//!   failure. Fetching the proof bytes — and the `RetryableUnavailable` that a
//!   missing/late transport implies — is Stage-2 transport responsibility.
//! * No consensus / genesis / block-binding / rule-version / reward / Base /
//!   `mineable_now` value is touched, and the EVM verifier stays default-OFF.
//!
//! Operator ratification: msg 3538 / 3540 (2026-08-06), Stage-1 scope only.

use crate::BlockReverifyOutcome;
use boole_evm_adapter::zk_verify::{CompressedProofVerifier, VerifyOutcome, VerifyReject};

/// Render a [`VerifyReject`] into a stable, human-readable detail string for the
/// [`BlockReverifyOutcome::DeterministicReject`] this bridge emits. The text
/// names the gate that fired, mirroring the verifier's own variant taxonomy so a
/// verdict log records *why* the EVM check refused.
fn evm_reject_detail(reject: &VerifyReject) -> String {
    match reject {
        VerifyReject::ProofTooLarge { len, max } => {
            format!("evm proof too large: {len} > {max}")
        }
        VerifyReject::PublicValuesTooShort { len } => {
            format!("evm public values too short: {len}")
        }
        VerifyReject::AdmissionMismatch {
            field,
            expected,
            actual,
        } => {
            format!("evm admission mismatch on {field}: expected {expected}, got {actual}")
        }
        VerifyReject::ProofRejected { detail } => {
            format!("evm proof rejected: {detail}")
        }
    }
}

/// The deterministic accept/reject converter: map a verifier-only
/// compressed-STARK [`VerifyOutcome`] onto the node's block-verdict three-state.
///
/// `Accepted` -> [`BlockReverifyOutcome::Verified`]; every `Rejected` variant ->
/// [`BlockReverifyOutcome::DeterministicReject`] carrying a gate-named detail. The
/// [`BlockReverifyOutcome::RetryableUnavailable`] arm is intentionally
/// unreachable here — see the module docs.
pub fn evm_verify_outcome_to_reverify(outcome: VerifyOutcome) -> BlockReverifyOutcome {
    match outcome {
        VerifyOutcome::Accepted => BlockReverifyOutcome::Verified,
        VerifyOutcome::Rejected(reject) => BlockReverifyOutcome::DeterministicReject {
            detail: evm_reject_detail(&reject),
        },
    }
}

/// Qualify one EVM proof against the FIXED frozen vk: run the frozen verifier
/// over `(proof_bytes, public_values)` and convert its outcome to the node's
/// block-verdict three-state via [`evm_verify_outcome_to_reverify`].
///
/// Stage-1 qualification only: no caller on any block-verdict path invokes this;
/// see the module docs.
pub fn qualify_evm_proof(
    verifier: &CompressedProofVerifier,
    proof_bytes: &[u8],
    public_values: &[u8],
) -> BlockReverifyOutcome {
    evm_verify_outcome_to_reverify(verifier.verify(proof_bytes, public_values))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The pinned Stage-1 fixtures (ADR-0018 / ADR-0019): the real frozen
    // compressed-STARK proof, its public values, and — for reference — the
    // frozen vk hash. Loaded relative to this crate's manifest dir.
    const PROOF: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/evm-zkvm/proof-compressed.bin"
    ));
    const PUBLIC_VALUES: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/evm-zkvm/public-values.bin"
    ));

    // ---- pure mapping (no crypto; these are the fast RED/GREEN signal) ----

    #[test]
    fn accepted_maps_to_verified() {
        assert!(matches!(
            evm_verify_outcome_to_reverify(VerifyOutcome::Accepted),
            BlockReverifyOutcome::Verified
        ));
    }

    #[test]
    fn proof_too_large_maps_to_deterministic_reject() {
        let out =
            evm_verify_outcome_to_reverify(VerifyOutcome::Rejected(VerifyReject::ProofTooLarge {
                len: 9,
                max: 4,
            }));
        match out {
            BlockReverifyOutcome::DeterministicReject { detail } => {
                assert!(detail.contains("too large"), "detail = {detail}");
                assert!(
                    detail.contains('9') && detail.contains('4'),
                    "detail = {detail}"
                );
            }
            other => panic!("expected DeterministicReject, got {other:?}"),
        }
    }

    #[test]
    fn public_values_too_short_maps_to_deterministic_reject() {
        let out = evm_verify_outcome_to_reverify(VerifyOutcome::Rejected(
            VerifyReject::PublicValuesTooShort { len: 5 },
        ));
        match out {
            BlockReverifyOutcome::DeterministicReject { detail } => {
                assert!(detail.contains("too short"), "detail = {detail}");
            }
            other => panic!("expected DeterministicReject, got {other:?}"),
        }
    }

    #[test]
    fn admission_mismatch_maps_to_deterministic_reject() {
        let out = evm_verify_outcome_to_reverify(VerifyOutcome::Rejected(
            VerifyReject::AdmissionMismatch {
                field: "task_contract",
                expected: "aa".to_string(),
                actual: "bb".to_string(),
            },
        ));
        match out {
            BlockReverifyOutcome::DeterministicReject { detail } => {
                assert!(
                    detail.contains("admission mismatch on task_contract"),
                    "detail = {detail}"
                );
                assert!(
                    detail.contains("aa") && detail.contains("bb"),
                    "detail = {detail}"
                );
            }
            other => panic!("expected DeterministicReject, got {other:?}"),
        }
    }

    #[test]
    fn proof_rejected_maps_to_deterministic_reject() {
        let out =
            evm_verify_outcome_to_reverify(VerifyOutcome::Rejected(VerifyReject::ProofRejected {
                detail: "bad stark".to_string(),
            }));
        match out {
            BlockReverifyOutcome::DeterministicReject { detail } => {
                assert!(detail.contains("evm proof rejected"), "detail = {detail}");
                assert!(detail.contains("bad stark"), "detail = {detail}");
            }
            other => panic!("expected DeterministicReject, got {other:?}"),
        }
    }

    // ---- fixture ACCEPT + reject matrix (real frozen verifier) ----

    #[test]
    fn fixture_proof_qualifies_as_verified() {
        let verifier = CompressedProofVerifier::frozen();
        let out = qualify_evm_proof(&verifier, PROOF, PUBLIC_VALUES);
        assert!(
            matches!(out, BlockReverifyOutcome::Verified),
            "real fixture proof must qualify Verified, got {out:?}"
        );
    }

    #[test]
    fn oversized_proof_is_deterministic_reject() {
        let verifier = CompressedProofVerifier::frozen();
        // One byte past the size cap; the gate fires before any deserialization.
        let oversized = vec![0u8; verifier.max_proof_bytes() + 1];
        match qualify_evm_proof(&verifier, &oversized, PUBLIC_VALUES) {
            BlockReverifyOutcome::DeterministicReject { detail } => {
                assert!(detail.contains("too large"), "detail = {detail}");
            }
            other => panic!("expected DeterministicReject, got {other:?}"),
        }
    }

    #[test]
    fn short_public_values_is_deterministic_reject() {
        let verifier = CompressedProofVerifier::frozen();
        // Shorter than the 192-byte admission header.
        let short_pv = &PUBLIC_VALUES[..100];
        match qualify_evm_proof(&verifier, PROOF, short_pv) {
            BlockReverifyOutcome::DeterministicReject { detail } => {
                assert!(detail.contains("too short"), "detail = {detail}");
            }
            other => panic!("expected DeterministicReject, got {other:?}"),
        }
    }

    #[test]
    fn tampered_admission_header_is_deterministic_reject() {
        let verifier = CompressedProofVerifier::frozen();
        // Byte 0 lives in the `task_contract` admission range [0, 32); flipping
        // it fails the external admission gate BEFORE the crypto check.
        let mut pv = PUBLIC_VALUES.to_vec();
        pv[0] ^= 0x01;
        match qualify_evm_proof(&verifier, PROOF, &pv) {
            BlockReverifyOutcome::DeterministicReject { detail } => {
                assert!(detail.contains("admission mismatch"), "detail = {detail}");
            }
            other => panic!("expected DeterministicReject, got {other:?}"),
        }
    }

    #[test]
    fn tampered_public_values_body_is_deterministic_reject() {
        let verifier = CompressedProofVerifier::frozen();
        // Byte 40 is inside the header but OUTSIDE every admission digest range,
        // so admission passes and the cryptographic commitment check rejects.
        let mut pv = PUBLIC_VALUES.to_vec();
        pv[40] ^= 0x01;
        match qualify_evm_proof(&verifier, PROOF, &pv) {
            BlockReverifyOutcome::DeterministicReject { detail } => {
                assert!(detail.contains("evm proof rejected"), "detail = {detail}");
            }
            other => panic!("expected DeterministicReject, got {other:?}"),
        }
    }

    #[test]
    fn tampered_proof_bytes_is_deterministic_reject() {
        let verifier = CompressedProofVerifier::frozen();
        // Public values are untouched (admission passes); a flipped proof byte
        // fails the compressed-STARK check.
        let mut proof = PROOF.to_vec();
        let last = proof.len() - 1;
        proof[last] ^= 0x01;
        match qualify_evm_proof(&verifier, &proof, PUBLIC_VALUES) {
            BlockReverifyOutcome::DeterministicReject { detail } => {
                assert!(detail.contains("evm proof rejected"), "detail = {detail}");
            }
            other => panic!("expected DeterministicReject, got {other:?}"),
        }
    }
}
