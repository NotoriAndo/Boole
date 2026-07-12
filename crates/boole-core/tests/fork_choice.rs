//! N4.2 — canonical head selection. Among competing candidate chains, the
//! canonical one is the head of the chain with the greatest cumulative work
//! (N4.1); an exact tie is broken deterministically by the lowest block hash so
//! every honest node converges on the same tip without further coordination.
//! Pins `choose_canonical_head`.

use boole_core::{
    block_hash, choose_canonical_head, difficulty_weight, parse_biguint_hex, Hex32, PersistedBlock,
};

// Max target → weight 1 (an "easy" block); tiny target → weight 2^255 (a
// "hard" block). Same values as the N4.1 cumulative-work test.
const T_EASY: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const T_HARD: &str = "0x0000000000000000000000000000000000000000000000000000000000000002";

/// A minimal block whose weight derives from `t_block` and whose consensus hash
/// derives from `prev_c` + the single `share_hash` (so tests can produce blocks
/// of equal weight but distinct block hashes).
fn block(height: u64, t_block: &str, prev_c: &str, share_hash: &str) -> PersistedBlock {
    PersistedBlock {
        height,
        prev_c: prev_c.to_string(),
        c: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        proposer_pk: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        selected_share_hashes: vec![share_hash.to_string()],
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

/// The consensus hash of a head built by `block(_, _, prev_c, share_hash)`,
/// computed independently (preimage v2 hashes the block's committed fields)
/// so tests can assert which head is chosen.
fn head_hash(height: u64, t_block: &str, prev_c: &str, share_hash: &str) -> Hex32 {
    block_hash(&block(height, t_block, prev_c, share_hash))
}

#[test]
fn selects_heaviest_chain() {
    let prev = "00".repeat(32);
    let share_heavy = "01".repeat(32);
    let share_light = "02".repeat(32);

    // Heavy chain: two hard blocks. Light chain: one easy block. Weight, not
    // length, must decide — but here the heavy chain also wins on both.
    let heavy = vec![
        block(0, T_HARD, &prev, &share_heavy),
        block(1, T_HARD, &prev, &share_heavy),
    ];
    let light = vec![block(0, T_EASY, &prev, &share_light)];

    // Order the lighter chain first to prove selection is by work, not position.
    let chosen = choose_canonical_head(&[light, heavy]).unwrap();
    assert_eq!(
        chosen,
        head_hash(1, T_HARD, &prev, &share_heavy),
        "the heaviest chain's head must be chosen"
    );
}

#[test]
fn breaks_exact_tie_by_lowest_block_hash() {
    let prev = "00".repeat(32);
    let share_a = "01".repeat(32);
    let share_b = "02".repeat(32);

    // Two single-block chains of IDENTICAL weight (same t_block) but different
    // heads → the tie must break on the lower block hash, deterministically.
    let hash_a = head_hash(0, T_HARD, &prev, &share_a);
    let hash_b = head_hash(0, T_HARD, &prev, &share_b);
    assert_ne!(hash_a, hash_b, "test needs two distinct head hashes");
    let expected_low = hash_a.min(hash_b);

    let chain_a = vec![block(0, T_HARD, &prev, &share_a)];
    let chain_b = vec![block(0, T_HARD, &prev, &share_b)];

    // Result must be independent of candidate order: try both orderings.
    assert_eq!(
        choose_canonical_head(&[chain_a.clone(), chain_b.clone()]).unwrap(),
        expected_low,
        "an exact tie must resolve to the lowest block hash (a,b order)"
    );
    assert_eq!(
        choose_canonical_head(&[chain_b, chain_a]).unwrap(),
        expected_low,
        "an exact tie must resolve to the lowest block hash (b,a order)"
    );
}
