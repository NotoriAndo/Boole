//! §SC reset window (ADR-0015 (a)) — block_hash preimage v3.
//!
//! v2 committed every replay-consumed field including declared bounty
//! credit rows; v3 removes the declared rows from the schema entirely and
//! commits the promoted bounty SHARE rows (with their announced `reward`)
//! instead — replay derives the credit amounts from those committed
//! settlement inputs via `derive_bounty_settlement`, so the hash pins the
//! inputs, never a declared outcome. These tests pin that tampering any
//! committed field changes the hash, and that the side-band fields stay
//! excluded.

use boole_core::{block_hash, PersistedBlock, PromotedBountyShare, ShareWorkAuthorization};

const PREV_C: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const SHARE_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const PK_A: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const PK_B: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const PROOF_HASH: &str = "4444444444444444444444444444444444444444444444444444444444444444";

fn base_block() -> PersistedBlock {
    let t_block = format!("0x{}", "f".repeat(64));
    let t_block_big = boole_core::parse_biguint_hex(&t_block).expect("t_block");
    let weight = boole_core::difficulty_weight(&t_block_big)
        .expect("weight")
        .to_string();
    PersistedBlock {
        height: 0,
        prev_c: PREV_C.to_string(),
        c: String::new(),
        proposer_pk: PK_A.to_string(),
        selected_share_hashes: vec![SHARE_HASH.to_string()],
        selected_share_pks: vec![PK_A.to_string()],
        selected_share_reward_pks: Vec::new(),
        proposer_reward_pk: String::new(),
        selected_share_evidence: Vec::new(),
        min_share_score: "0".to_string(),
        min_share_score_multiplier_nanos: 0,
        kmax_applied: 1,
        difficulty_epoch: 0,
        t_block: t_block.clone(),
        t_share: t_block,
        difficulty_weight: weight,
        dropped_below_min_score: 0,
        dropped_kernel_reject: 0,
        truncated_by_kmax: 0,
        ts: 1_700_000_000_000,
        promoted_bounty_shares: Vec::new(),
    }
}

fn promoted_share(reward: &str) -> PromotedBountyShare {
    PromotedBountyShare {
        family_id: "family.v1".to_string(),
        bounty_id: "bounty-1".to_string(),
        proof_hash: PROOF_HASH.to_string(),
        prover: PK_B.to_string(),
        reward: reward.to_string(),
    }
}

fn hash_of(block: &PersistedBlock) -> String {
    block_hash(block).to_hex()
}

#[test]
fn block_hash_v3_commits_reward_routing_fields() {
    let block = base_block();
    let baseline = hash_of(&block);

    let mut routed = block.clone();
    routed.proposer_reward_pk = PK_B.to_string();
    assert_ne!(
        hash_of(&routed),
        baseline,
        "re-routing the proposer reward must change the block hash"
    );

    let mut share_routed = block.clone();
    share_routed.selected_share_reward_pks = vec![PK_B.to_string()];
    assert_ne!(
        hash_of(&share_routed),
        baseline,
        "re-routing a share reward must change the block hash"
    );

    let mut proposer_swapped = block.clone();
    proposer_swapped.proposer_pk = PK_B.to_string();
    assert_ne!(
        hash_of(&proposer_swapped),
        baseline,
        "swapping the proposer must change the block hash"
    );

    let mut owner_swapped = block;
    owner_swapped.selected_share_pks = vec![PK_B.to_string()];
    assert_ne!(
        hash_of(&owner_swapped),
        baseline,
        "swapping a share owner (reward fallback) must change the block hash"
    );
}

#[test]
fn block_hash_v3_commits_promoted_bounty_shares_and_rewards() {
    let mut block = base_block();
    block.promoted_bounty_shares = vec![promoted_share("5")];
    let baseline = hash_of(&block);

    let mut inflated = block.clone();
    inflated.promoted_bounty_shares[0].reward = "500".to_string();
    assert_ne!(
        hash_of(&inflated),
        baseline,
        "inflating a promoted share's reward must change the block hash"
    );

    let mut rerouted = block.clone();
    rerouted.promoted_bounty_shares[0].prover = PK_A.to_string();
    assert_ne!(
        hash_of(&rerouted),
        baseline,
        "re-routing a promoted share's prover must change the block hash"
    );

    let mut swapped_proof = block.clone();
    swapped_proof.promoted_bounty_shares[0].proof_hash =
        "5555555555555555555555555555555555555555555555555555555555555555".to_string();
    assert_ne!(
        hash_of(&swapped_proof),
        baseline,
        "swapping a promoted share's proof must change the block hash"
    );

    let mut dropped = block;
    dropped.promoted_bounty_shares.clear();
    assert_ne!(
        hash_of(&dropped),
        baseline,
        "dropping a promoted share row must change the block hash"
    );
}

#[test]
fn block_hash_v3_commits_ts_and_difficulty_inputs() {
    let block = base_block();
    let baseline = hash_of(&block);

    let mut shifted_ts = block.clone();
    shifted_ts.ts += 1;
    assert_ne!(
        hash_of(&shifted_ts),
        baseline,
        "shifting ts must change the block hash"
    );

    let mut shifted_epoch = block.clone();
    shifted_epoch.difficulty_epoch += 1;
    assert_ne!(
        hash_of(&shifted_epoch),
        baseline,
        "shifting the difficulty epoch must change the block hash"
    );

    let mut eased = block;
    eased.t_block = format!("0x{}", "e".repeat(64));
    assert_ne!(
        hash_of(&eased),
        baseline,
        "changing the t_block target must change the block hash"
    );
}

#[test]
fn block_hash_v3_ignores_side_band_and_telemetry_fields() {
    let block = base_block();
    let baseline = hash_of(&block);

    // selected_share_evidence stays a schema-versioned side-band
    // (ADR-0007 (d)) — including the evidence v2 `signedWork` slot.
    let mut with_evidence = block.clone();
    with_evidence.selected_share_evidence = vec![boole_core::SelectedShareEvidence {
        pk: PK_A.to_string(),
        n: PK_B.to_string(),
        j: PK_B.to_string(),
        c: PREV_C.to_string(),
        canon_hash: PROOF_HASH.to_string(),
        proof_package: String::new(),
        seed_hex: String::new(),
        signed_work: Some(ShareWorkAuthorization {
            schema: "boole.signed.v1".to_string(),
            payload: serde_json::json!({"schema": "boole.signer.work.v2"}),
            pk: PK_A.to_string(),
            signature: "aa".repeat(64),
            network_id: None,
        }),
    }];
    assert_eq!(
        hash_of(&with_evidence),
        baseline,
        "selected_share_evidence (incl. signedWork) must stay outside the preimage"
    );

    // Telemetry counters are diagnostics, not consensus inputs.
    let mut noisy = block.clone();
    noisy.dropped_below_min_score = 9;
    noisy.dropped_kernel_reject = 9;
    noisy.truncated_by_kmax = 9;
    assert_eq!(
        hash_of(&noisy),
        baseline,
        "telemetry counters must stay outside the preimage"
    );

    // The stored c itself is never an input to its own hash.
    let mut with_c = block;
    with_c.c = PREV_C.to_string();
    assert_eq!(
        hash_of(&with_c),
        baseline,
        "the stored c must not feed its own preimage"
    );
}
