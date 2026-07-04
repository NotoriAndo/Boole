//! N1.3 (G2) — replay must reject a persisted chain whose `t_block` does not
//! match what the retarget policy computes for that height. `replay_blocks`
//! alone only checks share evidence + linkage (non-retarget-aware);
//! `replay_blocks_with_retarget` folds in `validate_retargeted_difficulty`
//! so a forged epoch-boundary difficulty is caught.
//!
//! N3-pre.1: this file's hand-built chain predates `selectedShareEvidence`
//! (each block carries a bare share hash with no evidence, per the doc
//! comment on `valid_chain` below), so both tests replay via the explicit
//! `LegacyEvidenceOptIn` path (`replay_blocks_with_retarget_allow_legacy_evidence_less`)
//! rather than the strict-by-default `replay_blocks_with_retarget`.

use boole_core::{
    block_hash, expected_retarget_difficulty_for_height,
    replay_blocks_with_retarget_allow_legacy_evidence_less, DifficultyRetargetPolicy, Hex32,
    LegacyEvidenceOptIn, PersistedBlock,
};

const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const INITIAL_T_BLOCK: &str = "0x00000fffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn policy() -> DifficultyRetargetPolicy {
    DifficultyRetargetPolicy {
        target_block_ms: 60_000,
        retarget_every_blocks: 2,
        max_adjustment_factor: 4,
    }
}

/// Build a self-consistent retargeted chain: each block's difficulty fields
/// are exactly what `expected_retarget_difficulty_for_height` computes from
/// the prefix, and `c = block_hash(prev_c, [])`. `span_ms` is the gap
/// between block timestamps (drives the retarget direction).
fn valid_chain(n: u64, span_ms: u64) -> Vec<PersistedBlock> {
    let p = policy();
    let mut blocks: Vec<PersistedBlock> = Vec::new();
    let mut prev_c = ZERO.to_string();
    let base_ts = 1_800_000_000_000u64;
    for h in 0..n {
        let ev = expected_retarget_difficulty_for_height(&blocks, INITIAL_T_BLOCK, &p)
            .expect("expected difficulty");
        // One legacy-style share per block (no evidence) so replay's
        // compute_block_credits has a non-empty owner list; the retarget
        // validation under test is independent of share contents.
        let share_hash = Hex32::from_bytes([h as u8 + 1; 32]);
        let c = block_hash(
            &Hex32::from_hex(&prev_c).unwrap(),
            std::slice::from_ref(&share_hash),
        )
        .to_hex();
        blocks.push(PersistedBlock {
            height: h,
            prev_c: prev_c.clone(),
            c: c.clone(),
            proposer_pk: "11".repeat(32),
            selected_share_hashes: vec![share_hash.to_hex()],
            selected_share_pks: vec!["11".repeat(32)],
            selected_share_reward_pks: vec![],
            proposer_reward_pk: String::new(),
            selected_share_evidence: vec![],
            min_share_score: "1".to_string(),
            min_share_score_multiplier_nanos: 1_000_000_000,
            kmax_applied: 1,
            difficulty_epoch: ev.difficulty_epoch,
            t_block: ev.t_block,
            t_share: "0x00000ffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                .to_string(),
            difficulty_weight: ev.difficulty_weight,
            dropped_below_min_score: 0,
            dropped_kernel_reject: 0,
            truncated_by_kmax: 0,
            ts: base_ts + h * span_ms,
            promoted_bounty_credits: vec![],
            promoted_bounty_shares: vec![],
        });
        prev_c = c;
    }
    blocks
}

#[test]
fn replay_accepts_correct_retargeted_chain() {
    // Fast span (30s vs 60s target) → retarget at height 2 raises difficulty;
    // the chain is self-consistent so replay-with-retarget accepts it.
    let blocks = valid_chain(3, 30_000);
    let result = replay_blocks_with_retarget_allow_legacy_evidence_less(
        &blocks,
        INITIAL_T_BLOCK,
        &policy(),
        LegacyEvidenceOptIn::for_legacy_replay_only(),
    );
    assert!(
        result.is_ok(),
        "valid retargeted chain must replay: {result:?}"
    );
}

#[test]
fn replay_rejects_tampered_t_block_at_epoch_boundary() {
    let mut blocks = valid_chain(3, 30_000);
    // Forge the retarget-boundary block's difficulty back to the (easier)
    // initial value — a miner trying to keep difficulty low past a retarget.
    blocks[2].t_block = INITIAL_T_BLOCK.to_string();
    let err = replay_blocks_with_retarget_allow_legacy_evidence_less(
        &blocks,
        INITIAL_T_BLOCK,
        &policy(),
        LegacyEvidenceOptIn::for_legacy_replay_only(),
    )
    .expect_err("tampered t_block at the epoch boundary must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("tBlock") || msg.contains("difficulty"),
        "error should name the difficulty mismatch, got: {msg}"
    );
}
