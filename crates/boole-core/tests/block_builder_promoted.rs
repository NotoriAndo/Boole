//! S22a — `build_block_selection` extended with `promoted_bounty_shares`.
//!
//! Hard-Guard invariant: when the promoted slice is empty, every output
//! field is byte-identical to the pre-S22 baseline. When non-empty, the
//! base-lane fields (`selected`, `proposer_index`, `truncated_by_kmax`,
//! `dropped_below_min_score`, `dropped_kernel_reject`, `kernel_checked_tags`,
//! `kernel_accepted`) are byte-identical to the same call with empty
//! promoted; only the new `promoted_bounty_shares` field carries the
//! kernel-accepted entries forward.
//!
//! Per-family caps + activation gating + signature verification live in
//! the *caller* (`select_promoted_bounty_shares` helper, S22b). The block
//! builder treats `promoted_bounty_shares` as already-vetted input.

use std::collections::BTreeSet;

use boole_core::{
    build_block_selection, BlockBuilderConfig, BuildSelectionResult, CandidateShare,
    PromotedBountyShare,
};
use num_bigint::BigUint;
use num_traits::Zero;

const CHAIN: &str = "deadbeef00000000000000000000000000000000000000000000000000000000";

/// `t_block = 0x18`. Share hashes are derived from `pk_byte`, so shares
/// with `pk_byte < 0x18` qualify as proposers — the test fixtures pin
/// the first share at `0x10` (qualifies) and others at `0x20, 0x30`
/// (don't), guaranteeing exactly one proposer regardless of count.
fn permissive_cfg() -> BlockBuilderConfig {
    BlockBuilderConfig {
        t_block: format!("0x{:064x}", 0x18u8),
        t_share: format!("0x{:064x}", u128::MAX),
        min_share_score: BigUint::zero(),
        min_share_score_multiplier_nanos: 1_000_000_000,
        k_max: 4,
        difficulty_epoch: 0,
        difficulty_weight: "1".to_string(),
    }
}

fn make_share(pk_byte: u8, score: u64) -> CandidateShare {
    CandidateShare {
        label: format!("share-{pk_byte:02x}"),
        pk: format!("{:064x}", pk_byte as u128),
        reward_pk: String::new(),
        n: "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
        j: format!("{:08x}", pk_byte),
        c: CHAIN.to_string(),
        share_hash: format!("{:064x}", pk_byte as u128),
        score: score.to_string(),
        canon_tag: 1,
        canon_hash: String::new(),
        proof_package: String::new(),
        seed_hex: String::new(),
    }
}

fn make_promoted(idx: u8) -> PromotedBountyShare {
    PromotedBountyShare {
        family_id: "test.fam".to_string(),
        bounty_id: format!("b-{idx}"),
        proof_hash: format!("{:064x}", idx as u128 * 0x11),
        prover: format!("{:064x}", idx as u128 * 0x101),
        reward: "0".to_string(),
    }
}

#[test]
fn empty_promoted_yields_empty_promoted_bounty_shares_field() {
    let shares = vec![make_share(0x10, 1000), make_share(0x20, 500)];
    let cfg = permissive_cfg();
    let mut accepted = BTreeSet::new();
    accepted.insert(1u8);

    let result =
        build_block_selection(CHAIN, &shares, &cfg, &accepted, &BTreeSet::new(), &[]).unwrap();
    let BuildSelectionResult::Ok(view) = result else {
        panic!("expected Ok proposer");
    };
    assert!(
        view.promoted_bounty_shares.is_empty(),
        "empty promoted input must produce empty promoted output"
    );
    assert_eq!(view.selected.len(), 2);
}

#[test]
fn non_empty_promoted_passes_through_to_result_unchanged() {
    let shares = vec![make_share(0x10, 1000)];
    let cfg = permissive_cfg();
    let mut accepted = BTreeSet::new();
    accepted.insert(1u8);

    let promoted = vec![make_promoted(1), make_promoted(2)];
    let result =
        build_block_selection(CHAIN, &shares, &cfg, &accepted, &BTreeSet::new(), &promoted)
            .unwrap();
    let BuildSelectionResult::Ok(view) = result else {
        panic!("expected Ok proposer");
    };
    // Builder does NOT filter promoted by base-lane kernel-acceptance;
    // bounty proofs are pre-vetted by their family's verifier (a
    // different namespace from the canonicalizer's `canon_tag`).
    assert_eq!(view.promoted_bounty_shares.len(), 2);
    assert_eq!(view.promoted_bounty_shares[0].bounty_id, "b-1");
    assert_eq!(view.promoted_bounty_shares[1].bounty_id, "b-2");
}

#[test]
fn promoted_does_not_alter_base_lane_fields() {
    let shares = vec![
        make_share(0x10, 1000),
        make_share(0x20, 500),
        make_share(0x30, 200),
    ];
    let cfg = permissive_cfg();
    let mut accepted = BTreeSet::new();
    accepted.insert(1u8);

    let baseline = match build_block_selection(
        CHAIN,
        &shares,
        &cfg,
        &accepted,
        &BTreeSet::new(),
        &[],
    )
    .unwrap()
    {
        BuildSelectionResult::Ok(v) => v,
        _ => panic!("baseline expected Ok"),
    };
    let with_promoted = match build_block_selection(
        CHAIN,
        &shares,
        &cfg,
        &accepted,
        &BTreeSet::new(),
        &[make_promoted(1), make_promoted(2), make_promoted(3)],
    )
    .unwrap()
    {
        BuildSelectionResult::Ok(v) => v,
        _ => panic!("with_promoted expected Ok"),
    };

    assert_eq!(baseline.selected, with_promoted.selected);
    assert_eq!(baseline.proposer_index, with_promoted.proposer_index);
    assert_eq!(
        baseline.dropped_below_min_score,
        with_promoted.dropped_below_min_score
    );
    assert_eq!(
        baseline.dropped_kernel_reject,
        with_promoted.dropped_kernel_reject
    );
    assert_eq!(baseline.truncated_by_kmax, with_promoted.truncated_by_kmax);
    assert_eq!(
        baseline.kernel_checked_tags,
        with_promoted.kernel_checked_tags
    );
    assert_eq!(baseline.kernel_accepted, with_promoted.kernel_accepted);
}

#[test]
fn promoted_does_not_alter_block_builder_config_values() {
    let shares = vec![make_share(0x10, 1000)];
    let cfg = permissive_cfg();
    let cfg_snapshot = cfg.clone();
    let mut accepted = BTreeSet::new();
    accepted.insert(1u8);

    let _ = build_block_selection(
        CHAIN,
        &shares,
        &cfg,
        &accepted,
        &BTreeSet::new(),
        &[make_promoted(1), make_promoted(2)],
    )
    .unwrap();

    assert_eq!(cfg.t_block, cfg_snapshot.t_block);
    assert_eq!(cfg.t_share, cfg_snapshot.t_share);
    assert_eq!(cfg.min_share_score, cfg_snapshot.min_share_score);
    assert_eq!(cfg.k_max, cfg_snapshot.k_max);
}
