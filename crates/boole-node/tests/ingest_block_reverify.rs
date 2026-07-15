//! SC.10-ii-b — block-level Lean re-verify over a peer block's base-lane
//! `selectedShareEvidence`, the gate ingest runs before adopting a block on
//! a named (checker-pinned) network.
//!
//! `reverify_block_selected_shares` is the block-level fan-out over the
//! single share verifier entry (`verify_lean_bound_share_evidence`). It runs
//! the pinned checker under the committed base-lane budget and folds the
//! per-share verdicts into ONE block outcome:
//!   * every share accepts / is not-Lean-bound / is skipped ⇒ `Verified`;
//!   * any deterministic failure (source re-derive, canon mismatch, Lean
//!     `DeterministicReject`) ⇒ `DeterministicReject` — the block is a
//!     consensus reject;
//!   * any `RetryableUnavailable` (containment / availability) ⇒
//!     `RetryableUnavailable` — the block is deferred, never rejected and
//!     never fail-open accepted (ADR-0016 (a-3)).
//!
//! These focused tests pin the host-independent folds: the empty-seed
//! Verified fold and the canon-mismatch deterministic reject need no Lean
//! process, and the retryable fold is forced by pointing the checker at a
//! non-existent directory (a launch failure the runner reports as
//! `RetryableUnavailable`). The Lean-`Accepted`/`DeterministicReject` folds
//! over a live checker are exercised end-to-end by the ingest smoke
//! (SC.10-iv) where lake is available.

use std::path::PathBuf;

use boole_core::{
    family_v1_lenbound, lean_bound_canon_package, lean_bound_verifier_hash, PersistedBlock,
    SelectedShareEvidence, BASE_LANE_MAX_HEARTBEATS, BASE_LANE_MAX_REC_DEPTH,
};
use boole_node::{reverify_block_selected_shares, BlockReverifyOutcome};
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
const BLOCK_C: &str = "1212121212121212121212121212121212121212121212121212121212121212";
const PK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const N: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const J: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn checker_hash() -> String {
    boole_lean_runner::checker_artifact_hash(&canonical_checker_dir())
        .expect("checker artifact hash")
}

/// One base-lane Lean-bound share whose `proofPackage` is the canon a live
/// miner would have ground for `SEED_HEX` against the canonical checker.
/// `tamper_package` flips a canon byte so canon recompute mismatches; an
/// empty `seed_hex` yields a non-Lean-bound placeholder share.
fn lean_bound_share(tamper_package: bool, seed_hex: &str) -> SelectedShareEvidence {
    let checker_hash = checker_hash();
    let verifier_hash = lean_bound_verifier_hash(PROFILE);
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
    SelectedShareEvidence {
        pk: PK.to_string(),
        n: N.to_string(),
        j: J.to_string(),
        c: PREV_C.to_string(),
        canon_hash,
        proof_package,
        seed_hex: seed_hex.to_string(),
        signed_work: None,
    }
}

/// A minimal `PersistedBlock` carrying `shares` as its base-lane evidence.
/// Only `c` and `selected_share_evidence` matter to the re-verify fold; the
/// rest are zeroed since the fold never touches them.
fn block_with_shares(shares: Vec<SelectedShareEvidence>) -> PersistedBlock {
    PersistedBlock {
        height: 1,
        prev_c: PREV_C.to_string(),
        c: BLOCK_C.to_string(),
        proposer_pk: PK.to_string(),
        selected_share_hashes: Vec::new(),
        selected_share_pks: Vec::new(),
        selected_share_reward_pks: Vec::new(),
        proposer_reward_pk: String::new(),
        selected_share_evidence: shares,
        min_share_score: "0".to_string(),
        min_share_score_multiplier_nanos: 0,
        kmax_applied: 0,
        difficulty_epoch: 0,
        t_block: "0".to_string(),
        t_share: "0".to_string(),
        difficulty_weight: "0".to_string(),
        dropped_below_min_score: 0,
        dropped_kernel_reject: 0,
        truncated_by_kmax: 0,
        ts: 0,
        promoted_bounty_shares: Vec::new(),
    }
}

#[test]
fn reverify_block_verified_when_all_shares_not_lean_bound() {
    // Empty-seed shares are not Lean-bound, so the fold never launches Lean
    // and the block is `Verified` with no deterministic or retryable failure.
    let block = block_with_shares(vec![
        lean_bound_share(false, ""),
        lean_bound_share(false, ""),
    ]);
    let outcome = reverify_block_selected_shares(
        &block,
        canonical_checker_dir().as_path(),
        &checker_hash(),
        &lean_bound_verifier_hash(PROFILE),
        BASE_LANE_MAX_HEARTBEATS,
        BASE_LANE_MAX_REC_DEPTH,
    );
    assert!(
        matches!(outcome, BlockReverifyOutcome::Verified),
        "all-not-lean-bound shares must fold to Verified, got {outcome:?}"
    );
}

#[test]
fn reverify_block_deterministic_reject_on_canon_mismatch() {
    // One tampered-canon share is a pure file-hash binding failure — a
    // deterministic reject that needs no Lean process, so the block folds to
    // `DeterministicReject` regardless of checker availability.
    let block = block_with_shares(vec![lean_bound_share(true, SEED_HEX)]);
    let outcome = reverify_block_selected_shares(
        &block,
        canonical_checker_dir().as_path(),
        &checker_hash(),
        &lean_bound_verifier_hash(PROFILE),
        BASE_LANE_MAX_HEARTBEATS,
        BASE_LANE_MAX_REC_DEPTH,
    );
    assert!(
        matches!(outcome, BlockReverifyOutcome::DeterministicReject { .. }),
        "a tampered-canon share must fold to DeterministicReject, got {outcome:?}"
    );
}

#[test]
fn reverify_block_retryable_when_checker_dir_missing() {
    // A well-formed canon share that WOULD run Lean, but the checker
    // directory does not exist: the runner launch fails, which is an
    // availability failure the fold must surface as `RetryableUnavailable`
    // (a defer), never a consensus reject and never a fail-open accept.
    let block = block_with_shares(vec![lean_bound_share(false, SEED_HEX)]);
    let missing_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lean/checker-does-not-exist-sc10iib");
    let outcome = reverify_block_selected_shares(
        &block,
        missing_dir.as_path(),
        &checker_hash(),
        &lean_bound_verifier_hash(PROFILE),
        BASE_LANE_MAX_HEARTBEATS,
        BASE_LANE_MAX_REC_DEPTH,
    );
    assert!(
        matches!(outcome, BlockReverifyOutcome::RetryableUnavailable { .. }),
        "a missing checker dir must fold to RetryableUnavailable, got {outcome:?}"
    );
}
