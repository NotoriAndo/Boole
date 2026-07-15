//! SC.10-ii-c — chain-level Lean re-verify over a peer's FULL competing
//! chain, the gate reorg runs before adopting the candidate on a named
//! (checker-pinned) network.
//!
//! `reverify_candidate_chain_selected_shares` folds the block-level gate
//! (`reverify_block_selected_shares`, SC.10-ii-b) over every block of the
//! candidate under the SAME committed base-lane budget and pinned checker.
//! The per-block outcomes fold with the same precedence as the per-share
//! fold:
//!   * every block Verified ⇒ `Verified`;
//!   * any block a deterministic reject ⇒ `DeterministicReject` — the whole
//!     chain is a consensus reject and must not be adopted;
//!   * otherwise the first `RetryableUnavailable` ⇒ `RetryableUnavailable` —
//!     the whole chain is deferred, never rejected and never fail-open
//!     accepted (ADR-0016 (a-3)).
//!
//! These focused tests pin the host-independent chain folds: a good first
//! block followed by a bad second block proves the fold scans past a clean
//! block, and the retryable fold is forced by pointing the checker at a
//! non-existent directory (a launch failure the runner reports as
//! `RetryableUnavailable`). The Lean-`Accepted`/`DeterministicReject` folds
//! over a live checker are exercised end-to-end by the reorg smoke where
//! lake is available.

use std::path::PathBuf;

use boole_core::{
    family_v1_lenbound, lean_bound_canon_package, lean_bound_verifier_hash, PersistedBlock,
    SelectedShareEvidence, BASE_LANE_MAX_HEARTBEATS, BASE_LANE_MAX_REC_DEPTH,
};
use boole_node::{reverify_candidate_chain_selected_shares, BlockReverifyOutcome};
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

/// A minimal `PersistedBlock` at `height` carrying `shares` as its base-lane
/// evidence. Only `height`, `c` and `selected_share_evidence` distinguish the
/// blocks of a candidate; the rest are zeroed since the re-verify fold never
/// touches them.
fn block_at(height: u64, shares: Vec<SelectedShareEvidence>) -> PersistedBlock {
    let c = format!("{height:064x}");
    PersistedBlock {
        height,
        prev_c: PREV_C.to_string(),
        c,
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
fn reverify_chain_verified_when_all_blocks_not_lean_bound() {
    // A two-block candidate whose every share is not Lean-bound: the fold
    // never launches Lean and the chain clears the gate as `Verified`.
    let candidate = vec![
        block_at(0, vec![lean_bound_share(false, "")]),
        block_at(
            1,
            vec![lean_bound_share(false, ""), lean_bound_share(false, "")],
        ),
    ];
    let outcome = reverify_candidate_chain_selected_shares(
        &candidate,
        canonical_checker_dir().as_path(),
        &checker_hash(),
        &lean_bound_verifier_hash(PROFILE),
        BASE_LANE_MAX_HEARTBEATS,
        BASE_LANE_MAX_REC_DEPTH,
    );
    assert!(
        matches!(outcome, BlockReverifyOutcome::Verified),
        "an all-not-lean-bound candidate must fold to Verified, got {outcome:?}"
    );
}

#[test]
fn reverify_chain_deterministic_reject_on_tampered_later_block() {
    // A clean first block followed by a block carrying one tampered-canon
    // share: the fold must scan past the clean block and reject the chain on
    // the later deterministic failure (no Lean process needed).
    let candidate = vec![
        block_at(0, vec![lean_bound_share(false, "")]),
        block_at(1, vec![lean_bound_share(true, SEED_HEX)]),
    ];
    let outcome = reverify_candidate_chain_selected_shares(
        &candidate,
        canonical_checker_dir().as_path(),
        &checker_hash(),
        &lean_bound_verifier_hash(PROFILE),
        BASE_LANE_MAX_HEARTBEATS,
        BASE_LANE_MAX_REC_DEPTH,
    );
    assert!(
        matches!(outcome, BlockReverifyOutcome::DeterministicReject { .. }),
        "a tampered-canon share in a later block must fold to DeterministicReject, got {outcome:?}"
    );
}

#[test]
fn reverify_chain_retryable_when_checker_dir_missing() {
    // A clean first block followed by a block with a well-formed canon share
    // that WOULD run Lean, but the checker directory does not exist: the
    // runner launch fails, an availability failure the chain fold must
    // surface as `RetryableUnavailable` (a defer), never a consensus reject
    // and never a fail-open accept.
    let candidate = vec![
        block_at(0, vec![lean_bound_share(false, "")]),
        block_at(1, vec![lean_bound_share(false, SEED_HEX)]),
    ];
    let missing_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lean/checker-does-not-exist-sc10iic");
    let outcome = reverify_candidate_chain_selected_shares(
        &candidate,
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
