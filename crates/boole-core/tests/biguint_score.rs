use boole_core::{
    build_block_selection, calibration_policy, parse_biguint_hex, BlockBuilderConfig,
    BuildSelectionResult, CalibrationReport, CandidateShare, PersistedBlock,
};

fn report_with_t_share(t_share: &str) -> CalibrationReport {
    serde_json::from_value(serde_json::json!({
        "T_submit": "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "T_share": t_share,
        "T_block": "0x1",
        "T_ticket": "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "MinShareScoreMultiplier": 1,
        "K_max": 2,
        "ShareCapPerPK_Block": 16,
        "L": 65536,
        "D_max": 64,
        "EMAWindow": 300,
        "M": 32,
        "perIpRateLimitPer60s": 60,
        "provenance": "biguint-score-regression"
    }))
    .expect("calibration report parses")
}

#[test]
fn block_builder_config_preserves_min_share_score_above_u128() {
    let policy = calibration_policy(&report_with_t_share("0x2")).expect("policy parses");

    let cfg =
        BlockBuilderConfig::from_policy(&policy).expect("config supports 256-bit score space");

    assert!(cfg.min_share_score > parse_biguint_hex("0xffffffffffffffffffffffffffffffff").unwrap());
    assert_eq!(
        cfg.min_share_score.to_string(),
        "57896044618658097711785492504343953926634992332820282019728792003956564819968"
    );
}

#[test]
fn block_selection_orders_scores_above_u128_without_truncation() {
    let policy = calibration_policy(&report_with_t_share("0x100000000000000000000000000000000"))
        .expect("policy parses");
    let mut cfg =
        BlockBuilderConfig::from_policy(&policy).expect("config supports 256-bit score space");
    cfg.k_max = 1;
    let chain_head = "c0";
    let shares = vec![
        CandidateShare {
            label: "larger-than-u128".to_string(),
            pk: "20".to_string(),
            n: "00".to_string(),
            j: "00".to_string(),
            c: chain_head.to_string(),
            share_hash: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            score: "680564733841876926926749214863536422913".to_string(),
            canon_tag: 1,
            canon_hash: "00".repeat(32),
            proof_package: String::new(),
        },
        CandidateShare {
            label: "also-larger-than-u128-but-smaller".to_string(),
            pk: "10".to_string(),
            n: "00".to_string(),
            j: "00".to_string(),
            c: chain_head.to_string(),
            share_hash: "ff00000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            score: "340282366920938463463374607431768211457".to_string(),
            canon_tag: 1,
            canon_hash: "00".repeat(32),
            proof_package: String::new(),
        },
    ];
    let accepted = [1].into_iter().collect();

    let result = build_block_selection(chain_head, &shares, &cfg, &accepted)
        .expect("selection supports scores above u128");

    let BuildSelectionResult::Ok(selection) = result else {
        panic!("expected proposer selection");
    };
    assert_eq!(selection.selected[0].label, "larger-than-u128");
}

#[test]
fn persisted_block_shape_accepts_large_decimal_min_share_score() {
    let block = PersistedBlock {
        height: 1,
        prev_c: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        c: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
        proposer_pk: "2222222222222222222222222222222222222222222222222222222222222222".to_string(),
        selected_share_hashes: vec![
            "3333333333333333333333333333333333333333333333333333333333333333".to_string(),
        ],
        selected_share_pks: vec![
            "2222222222222222222222222222222222222222222222222222222222222222".to_string(),
        ],
        selected_share_evidence: vec![],
        min_share_score: "340282366920938463463374607431768211456".to_string(),
        kmax_applied: 1,
        difficulty_epoch: 0,
        t_block: "0x000fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        t_share: "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        difficulty_weight: "4096".to_string(),
        dropped_below_min_score: 0,
        dropped_kernel_reject: 0,
        truncated_by_kmax: 0,
        ts: 1,
    };

    block
        .validate_shape()
        .expect("large decimal minShareScore is valid shape");
}
