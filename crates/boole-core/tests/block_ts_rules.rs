//! N3-pre.3 (review #3) — deterministic, wall-clock-free block timestamp
//! rule for retarget safety: replay must reject a block whose `ts` does
//! not strictly exceed the median-time-past of the previous
//! `min(MEDIAN_TIME_PAST_WINDOW, height)` blocks. Without this, a
//! self-reported `ts` can steer `actual_span_ms` in
//! `expected_retarget_difficulty_for_height` (`difficulty.rs`) even while
//! every other field in the chain is internally self-consistent. The
//! wall-clock future-drift bound is a separate, node-boundary-only guard
//! (`boole-node::local_node`) and is out of scope for this replay-layer
//! test.

use boole_core::{
    block_hash, difficulty_weight, parse_biguint_hex, replay_blocks, Hex32, PersistedBlock,
};

const ZERO: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const T_BLOCK: &str = "0x000fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

/// Builds a single self-consistent (non-retarget) block linking to
/// `prev_c` with the given `ts`. Mirrors the `valid_chain` helper in
/// `replay_retarget.rs` minus the retarget-policy plumbing this test
/// does not need.
fn block_with_ts(height: u64, prev_c: &str, ts: u64) -> PersistedBlock {
    let share_hash = Hex32::from_bytes([height as u8 + 1; 32]);
    let t_block = parse_biguint_hex(T_BLOCK).unwrap();
    let mut block = PersistedBlock {
        height,
        prev_c: prev_c.to_string(),
        c: String::new(),
        proposer_pk: "11".repeat(32),
        selected_share_hashes: vec![share_hash.to_hex()],
        selected_share_pks: vec!["11".repeat(32)],
        selected_share_reward_pks: vec![],
        proposer_reward_pk: String::new(),
        selected_share_evidence: vec![],
        min_share_score: "1".to_string(),
        min_share_score_multiplier_nanos: 1_000_000_000,
        kmax_applied: 1,
        difficulty_epoch: 0,
        t_block: T_BLOCK.to_string(),
        t_share: "0x00000ffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        difficulty_weight: difficulty_weight(&t_block).unwrap().to_string(),
        dropped_below_min_score: 0,
        dropped_kernel_reject: 0,
        truncated_by_kmax: 0,
        ts,
        promoted_bounty_credits: vec![],
        promoted_bounty_shares: vec![],
    };
    block.c = block_hash(&block).to_hex();
    block
}

#[test]
fn replay_rejects_block_ts_not_after_median_of_recent() {
    let mut blocks = Vec::new();
    let mut prev_c = ZERO.to_string();

    // height 0 — genesis, exempt from the rule (no predecessors).
    let b0 = block_with_ts(0, &prev_c, 1_000);
    prev_c = b0.c.clone();
    blocks.push(b0);

    // height 1 — window = [1000], median = 1000; ts=2000 > 1000 is valid.
    let b1 = block_with_ts(1, &prev_c, 2_000);
    prev_c = b1.c.clone();
    blocks.push(b1);

    // height 2 — window = [1000, 2000], median (sorted middle) = 2000.
    // A proposer stamping ts=1500 rewinds past the median-time-past of the
    // last two blocks (even though 1500 > genesis' 1000) in order to
    // shrink the next retarget's `actual_span_ms`. Replay must reject it.
    let bad = block_with_ts(2, &prev_c, 1_500);
    blocks.push(bad);

    let err = replay_blocks(&blocks)
        .expect_err("block ts rewound past the recent median-time-past must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("ts") || msg.contains("median"),
        "error should name the timestamp/median-time-past rule, got: {msg}"
    );
}
