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

use boole_core::{
    build_block_selection, build_block_selection_for_network, canonical_payload_hash_hex,
    BlockBuilderConfig, BuildSelectionResult, CandidateShare, ShareWorkAuthorization, SigningKeyV2,
};
use num_bigint::BigUint;
use num_traits::Zero;
use serde_json::json;

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
        signed_work: None,
    }
}

fn make_network_authorized_share(
    byte: u8,
    label: &str,
    score: u64,
    network_id: &str,
) -> CandidateShare {
    let key = SigningKeyV2::from_dev_id(&format!("selection-{label}"));
    let pk = key.pk_hex();
    let n = format!("{:064x}", byte as u128 + 1);
    let j = format!("{:064x}", byte as u128 + 2);
    let proof_package = "00".to_string();
    let body = json!({
        "bytes": proof_package,
        "c": CHAIN,
        "j": j,
        "n": n,
        "nonceS": format!("{:064x}", byte as u128 + 3),
        "pk": pk,
    });
    let payload = json!({
        "schema": "boole.signer.work.v2",
        "route": "/submit",
        "requestHash": canonical_payload_hash_hex(&body),
        "rewardRecipient": pk,
        "workPayload": body,
    });
    let envelope = key
        .sign_for_network(&payload, Some(network_id))
        .expect("test authorization signs");

    CandidateShare {
        label: label.to_string(),
        pk,
        reward_pk: envelope.payload["rewardRecipient"]
            .as_str()
            .expect("reward recipient")
            .to_string(),
        n,
        j,
        c: CHAIN.to_string(),
        share_hash: format!("{:064x}", byte as u128),
        score: score.to_string(),
        canon_tag: 1,
        canon_hash: String::new(),
        proof_package,
        seed_hex: String::new(),
        signed_work: Some(ShareWorkAuthorization {
            schema: envelope.schema.to_string(),
            payload: envelope.payload,
            pk: envelope.pk,
            signature: envelope.signature,
            network_id: envelope.network_id,
        }),
    }
}

#[test]
fn two_co_qualifying_shares_still_commit_a_block() {
    let shares = vec![make_share(0x10, "share-a"), make_share(0x18, "share-b")];
    let cfg = permissive_cfg();
    let accepted = BTreeSet::from([1u8]);

    let result = build_block_selection(CHAIN, &shares, &cfg, &accepted, &BTreeSet::new(), &[])
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

    let result = build_block_selection(CHAIN, &shares, &cfg, &accepted, &BTreeSet::new(), &[])
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

#[test]
fn testnet2_filters_missing_and_foreign_authorization_before_top_k() {
    let mut missing =
        make_network_authorized_share(0x10, "missing-high-score", 3_000, "boole-testnet-2");
    missing.signed_work = None;
    let foreign = make_network_authorized_share(0x11, "foreign-mid-score", 2_000, "boole-dev");
    let mut mismatched =
        make_network_authorized_share(0x13, "mismatched-work", 2_500, "boole-testnet-2");
    mismatched.n = format!("{:064x}", 999u64);
    let matching =
        make_network_authorized_share(0x12, "matching-low-score", 1_000, "boole-testnet-2");
    let mut cfg = permissive_cfg();
    cfg.k_max = 1;
    let accepted = BTreeSet::from([1u8]);

    let result = build_block_selection_for_network(
        CHAIN,
        &[missing, foreign, mismatched, matching],
        &cfg,
        &accepted,
        &BTreeSet::new(),
        &[],
        Some("boole-testnet-2"),
    )
    .expect("authorization filtering must not fail the whole block build");

    let BuildSelectionResult::Ok(selection) = result else {
        panic!("the lower-ranked matching authorization must survive before top-k truncation");
    };
    assert_eq!(selection.selected.len(), 1);
    assert_eq!(selection.selected[0].label, "matching-low-score");
}

#[test]
fn boole_dev_and_legacy_selection_keep_anonymous_candidates() {
    let share = make_share(0x10, "anonymous-dev");
    let cfg = permissive_cfg();
    let accepted = BTreeSet::from([1u8]);

    let BuildSelectionResult::Ok(dev_selection) = build_block_selection_for_network(
        CHAIN,
        std::slice::from_ref(&share),
        &cfg,
        &accepted,
        &BTreeSet::new(),
        &[],
        Some("boole-dev"),
    )
    .expect("explicit boole-dev selection runs") else {
        panic!("boole-dev must preserve anonymous candidate selection");
    };
    assert_eq!(dev_selection.selected[0].label, "anonymous-dev");

    let BuildSelectionResult::Ok(legacy_selection) = build_block_selection(
        CHAIN,
        std::slice::from_ref(&share),
        &cfg,
        &accepted,
        &BTreeSet::new(),
        &[],
    )
    .expect("legacy selection runs") else {
        panic!("legacy selection must preserve anonymous candidate selection");
    };
    assert_eq!(legacy_selection.selected[0].label, "anonymous-dev");
}
