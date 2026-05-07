use boole_core::{
    expected_retarget_difficulty_for_height, parse_biguint_hex, retarget_t_block,
    validate_retargeted_difficulty, DifficultyRetargetPolicy, PersistedBlock,
};

#[test]
fn retarget_t_block_halves_when_blocks_are_twice_as_fast() {
    let current =
        parse_biguint_hex("0x8000000000000000000000000000000000000000000000000000000000000000")
            .expect("current target");
    let policy = DifficultyRetargetPolicy {
        target_block_ms: 1_000,
        retarget_every_blocks: 2,
        max_adjustment_factor: 4,
    };

    let next = retarget_t_block(&current, 1_000, 2_000, &policy).expect("retarget computes");

    assert_eq!(
        format!("0x{next:064x}"),
        "0x4000000000000000000000000000000000000000000000000000000000000000"
    );
}

#[test]
fn retarget_t_block_clamps_large_slowdown() {
    let current =
        parse_biguint_hex("0x1000000000000000000000000000000000000000000000000000000000000000")
            .expect("current target");
    let policy = DifficultyRetargetPolicy {
        target_block_ms: 1_000,
        retarget_every_blocks: 2,
        max_adjustment_factor: 4,
    };

    let next = retarget_t_block(&current, 20_000, 2_000, &policy).expect("retarget computes");

    assert_eq!(
        format!("0x{next:064x}"),
        "0x4000000000000000000000000000000000000000000000000000000000000000"
    );
}

#[test]
fn expected_retarget_difficulty_changes_epoch_at_boundary() {
    let policy = DifficultyRetargetPolicy {
        target_block_ms: 1_000,
        retarget_every_blocks: 2,
        max_adjustment_factor: 4,
    };
    let initial = "0x8000000000000000000000000000000000000000000000000000000000000000";
    let blocks = vec![block(0, 1_000, initial, 0), block(1, 1_500, initial, 0)];

    let next =
        expected_retarget_difficulty_for_height(&blocks, initial, &policy).expect("difficulty");

    assert_eq!(next.difficulty_epoch, 1);
    assert_eq!(
        next.t_block,
        "0x4000000000000000000000000000000000000000000000000000000000000000"
    );
    assert_eq!(next.retarget, "enabled");
}

#[test]
fn validate_retargeted_difficulty_rejects_wrong_epoch_target() {
    let policy = DifficultyRetargetPolicy {
        target_block_ms: 1_000,
        retarget_every_blocks: 2,
        max_adjustment_factor: 4,
    };
    let initial = "0x8000000000000000000000000000000000000000000000000000000000000000";
    let blocks = vec![
        block(0, 1_000, initial, 0),
        block(1, 1_500, initial, 0),
        block(2, 2_000, initial, 1),
    ];

    let err = validate_retargeted_difficulty(&blocks, initial, &policy)
        .expect_err("wrong retargeted target is rejected");

    assert!(
        err.to_string().contains("tBlock mismatch"),
        "unexpected error: {err}"
    );
}

fn block(height: u64, ts: u64, t_block: &str, difficulty_epoch: u64) -> PersistedBlock {
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
        selected_share_evidence: vec![],
        min_share_score: "1".to_string(),
        kmax_applied: 1,
        difficulty_epoch,
        t_block: t_block.to_string(),
        t_share: "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        difficulty_weight: boole_core::difficulty_weight(&parse_biguint_hex(t_block).unwrap())
            .unwrap()
            .to_string(),
        dropped_below_min_score: 0,
        dropped_kernel_reject: 0,
        truncated_by_kmax: 0,
        ts,
    }
}
