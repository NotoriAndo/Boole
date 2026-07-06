//! N3-pre.6 (external review A-g1, critical) — `build_block_selection`
//! must not halt forever when two shares co-qualify as proposer in the
//! same build cycle (both satisfy `share_hash < T_block`). Before this
//! slice, `proposer_count > 1` returned `AmbiguousProposer` and no block
//! was produced; because pool pruning only happens on a successful
//! commit (`runtime.rs::apply_block_unchecked`), the qualifying set never
//! shrinks on its own — this was a permanent liveness stall, not a
//! transient one.
//!
//! The fix: pick the qualifying share with the lowest `compare_canonical`
//! order deterministically instead of refusing to build. This must be
//! the exact same shared comparator N3-pre.2 introduced for
//! `replay_evidence::verify_canonical_selection`, so a builder that picks
//! a winner here and a replayer re-deriving the winner from the
//! persisted block agree on exactly the same share — never two
//! comparators that could drift apart.

use std::collections::BTreeSet;

use boole_core::{build_block_selection, BlockBuilderConfig, BuildSelectionResult, CandidateShare};
use num_bigint::BigUint;
use num_traits::Zero;

const CHAIN: &str = "deadbeef00000000000000000000000000000000000000000000000000000000";

/// `t_block = 0x20`. Both shares below qualify (`0x10 < 0x20` and
/// `0x18 < 0x20`), so the same build cycle sees two shares satisfy
/// T_block at once.
fn permissive_cfg() -> BlockBuilderConfig {
    BlockBuilderConfig {
        t_block: format!("0x{:064x}", 0x20u8),
        t_share: format!("0x{:064x}", u128::MAX),
        min_share_score: BigUint::zero(),
        min_share_score_multiplier_nanos: 1_000_000_000,
        k_max: 4,
        difficulty_epoch: 0,
        difficulty_weight: "1".to_string(),
    }
}

/// `pk` and `share_hash` both derive from `byte`, so the fixture never
/// needs to pull apart canonical order (`pk`, `n`, `j`) from share-hash
/// order — the lower `byte` is lower on both axes, which is exactly what
/// `compare_canonical` sorts by first (`pk` is its primary key).
fn make_share(byte: u8, label: &str) -> CandidateShare {
    CandidateShare {
        label: label.to_string(),
        pk: format!("{:064x}", byte as u128),
        reward_pk: String::new(),
        n: "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
        j: format!("{:08x}", byte),
        c: CHAIN.to_string(),
        share_hash: format!("{:064x}", byte as u128),
        score: 1_000u64.to_string(),
        canon_tag: 1,
        canon_hash: String::new(),
        proof_package: String::new(),
        seed_hex: String::new(),
    }
}

#[test]
fn two_co_qualifying_shares_still_commit_a_block() {
    let shares = vec![make_share(0x10, "share-a"), make_share(0x18, "share-b")];
    let cfg = permissive_cfg();
    let accepted = BTreeSet::from([1u8]);

    let result = build_block_selection(CHAIN, &shares, &cfg, &accepted, &BTreeSet::new(), &[], &[])
        .expect("build_block_selection must not error on a co-qualifying pair");

    let selection = match result {
        BuildSelectionResult::Ok(selection) => selection,
        other => panic!(
            "two shares both satisfying T_block must still commit a block via a \
             deterministic tie-break instead of halting: {other:?}"
        ),
    };
    assert_eq!(
        selection.selected.len(),
        2,
        "both co-qualifying shares should still be selected into the block"
    );
}

#[test]
fn proposer_tie_breaks_by_lowest_share_hash() {
    // Insertion order deliberately reversed (share-b before share-a) —
    // the tie-break must not depend on input order, only on the
    // canonical comparator.
    let shares = vec![make_share(0x18, "share-b"), make_share(0x10, "share-a")];
    let cfg = permissive_cfg();
    let accepted = BTreeSet::from([1u8]);

    let result = build_block_selection(CHAIN, &shares, &cfg, &accepted, &BTreeSet::new(), &[], &[])
        .expect("build_block_selection must not error on a co-qualifying pair");

    let selection = match result {
        BuildSelectionResult::Ok(selection) => selection,
        other => panic!("expected a deterministic tie-break to still produce a block: {other:?}"),
    };
    let proposer = &selection.selected[selection.proposer_index];
    assert_eq!(
        proposer.label, "share-a",
        "the lowest-ranked co-qualifying share (share-a, share_hash 0x10) must win the \
         tie deterministically, not share-b (0x18): got proposer {proposer:?}"
    );
}
