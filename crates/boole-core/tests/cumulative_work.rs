//! N4.1 — cumulative chain work. Fork-choice orders competing chains by total
//! proof-of-work, not by length: a chain's weight is the sum of each block's
//! `difficulty_weight` (≈ 2^256 / t_block, so a harder block contributes more).
//! This pins that `cumulative_difficulty_weight` folds the persisted per-block
//! weights so two chains can be compared by total work — and that it parses the
//! stored weight as the DECIMAL string the builder writes, not as hex.

use boole_core::{
    cumulative_difficulty_weight, difficulty_weight, parse_biguint_hex, PersistedBlock,
};

// Max target → weight floor(2^256 / (2^256 - 1)) == 1 (an "easy" block).
const T_EASY: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
// Tiny target → weight 2^256 / 2 == 2^255 (a very "hard" block).
const T_HARD: &str = "0x0000000000000000000000000000000000000000000000000000000000000002";

/// A minimal-but-realistic block whose `difficulty_weight` is derived from
/// `t_block` exactly as `block_builder` persists it: the DECIMAL rendering of
/// `difficulty_weight(t_block)`.
fn block(height: u64, t_block: &str) -> PersistedBlock {
    PersistedBlock {
        height,
        prev_c: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        c: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        proposer_pk: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        selected_share_hashes: vec![
            "0101010101010101010101010101010101010101010101010101010101010101".to_string(),
        ],
        selected_share_pks: vec![
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        ],
        selected_share_reward_pks: vec![],
        proposer_reward_pk: String::new(),
        selected_share_evidence: vec![],
        min_share_score: "1".to_string(),
        min_share_score_multiplier_nanos: 0,
        kmax_applied: 1,
        difficulty_epoch: 0,
        t_block: t_block.to_string(),
        t_share: "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        difficulty_weight: difficulty_weight(&parse_biguint_hex(t_block).unwrap())
            .unwrap()
            .to_string(),
        dropped_below_min_score: 0,
        dropped_kernel_reject: 0,
        truncated_by_kmax: 0,
        ts: height * 1_000,
        promoted_bounty_shares: vec![],
    }
}

/// The weight a single `t_block` contributes, computed independently of the
/// summed path so the tests can assert the fold's exact value.
fn weight_of(t_block: &str) -> num_bigint::BigUint {
    difficulty_weight(&parse_biguint_hex(t_block).unwrap()).unwrap()
}

#[test]
fn heavier_chain_has_greater_total_weight() {
    // Base cases: an empty chain has zero work; a one-block chain equals that
    // block's weight.
    assert_eq!(
        cumulative_difficulty_weight(&[]).unwrap(),
        num_bigint::BigUint::from(0u32),
        "an empty chain must have zero cumulative work"
    );
    assert_eq!(
        cumulative_difficulty_weight(&[block(0, T_HARD)]).unwrap(),
        weight_of(T_HARD),
        "a single-block chain must equal that block's weight"
    );

    let heavy = [block(0, T_HARD), block(1, T_HARD)];
    let light = [block(0, T_EASY)];

    // Exact fold: the two hard blocks sum to twice a hard block's weight.
    assert_eq!(
        cumulative_difficulty_weight(&heavy).unwrap(),
        weight_of(T_HARD) + weight_of(T_HARD),
        "cumulative work must be the exact sum of per-block weights"
    );
    assert!(
        cumulative_difficulty_weight(&heavy).unwrap()
            > cumulative_difficulty_weight(&light).unwrap(),
        "the chain with more total work must outweigh the lighter one"
    );
}

#[test]
fn equal_length_different_weight_orders_correctly() {
    // Both chains are two blocks long, so length cannot be the tiebreaker —
    // only total work can. Chain A carries one hard + one easy block; chain B
    // carries two easy blocks.
    let chain_a = [block(0, T_HARD), block(1, T_EASY)];
    let chain_b = [block(0, T_EASY), block(1, T_EASY)];
    assert_eq!(chain_a.len(), chain_b.len(), "chains must be equal length");

    let work_a = cumulative_difficulty_weight(&chain_a).unwrap();
    let work_b = cumulative_difficulty_weight(&chain_b).unwrap();

    assert_eq!(
        work_a,
        weight_of(T_HARD) + weight_of(T_EASY),
        "chain A's work must be the exact sum of its blocks' weights"
    );
    assert!(
        work_a > work_b,
        "equal-length chains must be ordered by total work, not length"
    );
}
