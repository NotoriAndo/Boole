//! N3-pre.1 (L1 fitness review #5) — replay must not degrade to "block c
//! hash matches, done" when a block carries no `selectedShareEvidence`.
//! Before this slice, `verify_selected_share_evidence` returned `Ok(())`
//! immediately for empty evidence, so a peer could not be forced to
//! independently re-derive the selected share identity. Replay now
//! rejects empty evidence by default; a caller that must replay a
//! pre-evidence legacy chain (existing golden fixtures, hand-built test
//! chains) has to opt in explicitly via `LegacyEvidenceOptIn`, which has
//! no path into `replay_blocks` (the entry point future p2p ingest will
//! use) — see `replay_blocks_allow_legacy_evidence_less`.

use boole_core::{
    block_hash, replay_blocks, replay_blocks_allow_legacy_evidence_less, LegacyEvidenceOptIn,
    PersistedBlock,
};

const PREV_C: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const SHARE_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const PK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn evidence_less_block() -> PersistedBlock {
    let mut block = PersistedBlock {
        height: 0,
        prev_c: PREV_C.to_string(),
        c: String::new(),
        proposer_pk: PK.to_string(),
        selected_share_hashes: vec![SHARE_HASH.to_string()],
        selected_share_pks: vec![PK.to_string()],
        selected_share_reward_pks: vec![],
        proposer_reward_pk: String::new(),
        selected_share_evidence: vec![],
        min_share_score: "1".to_string(),
        min_share_score_multiplier_nanos: 0,
        kmax_applied: 1,
        difficulty_epoch: 0,
        t_block: "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        t_share: "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        difficulty_weight: "1".to_string(),
        dropped_below_min_score: 0,
        dropped_kernel_reject: 0,
        truncated_by_kmax: 0,
        ts: 1_700_000_000_000,
        promoted_bounty_credits: vec![],
        promoted_bounty_shares: vec![],
    };
    block.c = block_hash(&block).to_hex();
    block
}

#[test]
fn replay_rejects_block_with_empty_selected_share_evidence() {
    let block = evidence_less_block();

    let err = replay_blocks(&[block])
        .expect_err("replay must reject a block with empty selectedShareEvidence by default");
    assert!(
        err.to_string().to_lowercase().contains("evidence"),
        "error should name the missing selected-share evidence: {err}"
    );
}

#[test]
fn legacy_evidence_less_block_requires_explicit_opt_in() {
    let block = evidence_less_block();

    // Default (strict) replay rejects it — same guarantee as above.
    assert!(
        replay_blocks(std::slice::from_ref(&block)).is_err(),
        "strict replay must reject the evidence-less block"
    );

    // Only the explicit legacy opt-in accepts a pre-evidence chain.
    let replay = replay_blocks_allow_legacy_evidence_less(
        &[block],
        LegacyEvidenceOptIn::for_legacy_replay_only(),
    )
    .expect("legacy opt-in must accept the pre-evidence block");
    assert_eq!(replay.height, 1);
}
