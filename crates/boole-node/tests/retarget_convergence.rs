//! N1.4 (G5) — difficulty-retarget convergence evidence. Over a controlled
//! span history, the retarget at an epoch boundary moves difficulty toward
//! the target: a FAST chain (blocks closer together than `target_block_ms`)
//! makes `t_block` HARDER (smaller), a SLOW chain makes it EASIER (larger),
//! and the moved value matches `expected_retarget_difficulty_for_height`.
//!
//! The span history is hand-built (not committed through the runtime) so the
//! evidence is deterministic and independent of share proposer-qualification;
//! the integrated commit path is covered by N1.2
//! (runtime_difficulty_consistency) and the replay path by N1.3
//! (replay_retarget). Closed local smoke only — no public mining claim.

use boole_core::{
    block_hash, expected_retarget_difficulty_for_height, parse_biguint_hex,
    DifficultyRetargetPolicy, Hex32, PersistedBlock,
};

// Mid-range initial difficulty (2^254): room to move both harder and easier
// without clamping at the 2^256-1 ceiling, and well above any share hash.
const INITIAL_T_BLOCK: &str = "0x4000000000000000000000000000000000000000000000000000000000000000";
const TARGET_BLOCK_MS: u64 = 60_000;
const RETARGET_EVERY: u64 = 4;

fn policy() -> DifficultyRetargetPolicy {
    DifficultyRetargetPolicy {
        target_block_ms: TARGET_BLOCK_MS,
        retarget_every_blocks: RETARGET_EVERY,
        max_adjustment_factor: 4,
    }
}

/// Pre-boundary span history: `RETARGET_EVERY` blocks (heights 0..N-1) all at
/// the initial difficulty (retarget engages only AT height N), spaced
/// `span_ms` apart. Returned so the caller can compute the height-N retarget.
fn span_history(span_ms: u64) -> Vec<PersistedBlock> {
    let p = policy();
    let mut blocks: Vec<PersistedBlock> = Vec::new();
    let mut prev_c = "00".repeat(32);
    let base_ts = 1_800_000_000_000u64;
    for h in 0..RETARGET_EVERY {
        let ev = expected_retarget_difficulty_for_height(&blocks, INITIAL_T_BLOCK, &p)
            .expect("expected difficulty");
        let share_hash = Hex32::from_bytes([h as u8 + 1; 32]);
        let mut block = PersistedBlock {
            height: h,
            prev_c: prev_c.clone(),
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
            difficulty_epoch: ev.difficulty_epoch,
            t_block: ev.t_block,
            t_share: "0x0000ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                .to_string(),
            difficulty_weight: ev.difficulty_weight,
            dropped_below_min_score: 0,
            dropped_kernel_reject: 0,
            truncated_by_kmax: 0,
            ts: base_ts + h * span_ms,
            promoted_bounty_credits: vec![],
            promoted_bounty_shares: vec![],
        };
        block.c = block_hash(&block).to_hex();
        prev_c = block.c.clone();
        blocks.push(block);
    }
    blocks
}

/// The retargeted `t_block` (hex) at the epoch boundary for a given span.
fn boundary_t_block_hex(span_ms: u64) -> String {
    let history = span_history(span_ms);
    let initial = format!("0x{:064x}", parse_biguint_hex(INITIAL_T_BLOCK).unwrap());
    assert!(
        history.iter().all(|b| b.t_block == initial),
        "pre-boundary blocks stay at the initial difficulty"
    );
    expected_retarget_difficulty_for_height(&history, INITIAL_T_BLOCK, &policy())
        .expect("retarget at boundary")
        .t_block
}

#[test]
fn retarget_fast_chain_converges_toward_target() {
    // 20s/block vs 60s target → 3× too fast → retarget makes it HARDER.
    let initial = parse_biguint_hex(INITIAL_T_BLOCK).unwrap();
    let next = parse_biguint_hex(&boundary_t_block_hex(20_000)).unwrap();
    assert!(
        next < initial,
        "a fast chain must lower t_block (harder): next={next:x} initial={initial:x}"
    );
}

#[test]
fn retarget_slow_chain_converges_toward_target() {
    // 180s/block vs 60s target → 3× too slow → retarget makes it EASIER.
    let initial = parse_biguint_hex(INITIAL_T_BLOCK).unwrap();
    let next = parse_biguint_hex(&boundary_t_block_hex(180_000)).unwrap();
    assert!(
        next > initial,
        "a slow chain must raise t_block (easier): next={next:x} initial={initial:x}"
    );
}
