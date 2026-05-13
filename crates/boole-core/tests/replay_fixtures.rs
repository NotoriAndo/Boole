use std::collections::BTreeMap;

use boole_core::{
    block_hash, compute_block_credits, replay_blocks, share_hash, Hex32, PersistedBlock,
    PersistedRewardEvent, SelectedShareEvidence,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
struct Fixture {
    blocks: Vec<PersistedBlock>,
    #[serde(rename = "rewardEvents")]
    reward_events: Vec<PersistedRewardEvent>,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    latest_c: String,
    height: u64,
    balances: BTreeMap<String, String>,
}

#[test]
fn replay_matches_typescript_golden_fixture() {
    assert_replay_fixture_matches(include_str!("../../../fixtures/protocol/replay/v1.json"));
}

#[test]
fn replay_matches_evidence_backed_v2_golden_fixture() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v2.json"))
            .expect("fixture parses");
    assert!(
        fixture
            .blocks
            .iter()
            .any(|block| !block.selected_share_evidence.is_empty()
                && block.min_share_score_multiplier_nanos != 0),
        "v2 fixture must cover selected-share evidence with policy-bound multiplier"
    );

    assert_replay_fixture(fixture);
}

fn assert_replay_fixture_matches(raw: &str) {
    let fixture: Fixture = serde_json::from_str(raw).expect("fixture parses");
    assert_replay_fixture(fixture);
}

fn assert_replay_fixture(fixture: Fixture) {
    for (block, event) in fixture.blocks.iter().zip(fixture.reward_events.iter()) {
        let credits = compute_block_credits(&block.proposer_pk, &block.selected_share_pks)
            .expect("credits compute");
        assert_eq!(
            credits, event.credits,
            "reward event height {}",
            event.height
        );
        assert_eq!(block.height, event.height);
        assert_eq!(block.c, event.c);
    }

    let replay = replay_blocks(&fixture.blocks).expect("replay passes");
    assert_eq!(replay.latest_c, fixture.expected.latest_c);
    assert_eq!(replay.height, fixture.expected.height);
    let got_balances = replay
        .balances
        .into_iter()
        .map(|(pk, amount)| (pk, amount.to_string()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(got_balances, fixture.expected.balances);
}

#[test]
fn replay_rejects_block_with_bad_difficulty_weight() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");
    let mut blocks = fixture.blocks;
    blocks[0].difficulty_weight = "999".to_string();

    let err = replay_blocks(&blocks).expect_err("bad difficulty weight is rejected");
    assert!(
        err.to_string().contains("difficultyWeight mismatch"),
        "unexpected error: {err}"
    );
}

#[test]
fn replay_accepts_valid_selected_share_evidence() {
    let block = evidence_backed_block();

    let replay = replay_blocks(&[block]).expect("valid selected share evidence replays");

    assert_eq!(replay.height, 1);
}

#[test]
fn replay_rejects_selected_share_evidence_count_mismatch() {
    let mut block = evidence_backed_block();
    block
        .selected_share_evidence
        .push(block.selected_share_evidence[0].clone());

    assert_replay_error_contains(block, "selected share evidence count mismatch");
}

#[test]
fn replay_rejects_selected_share_evidence_c_mismatch() {
    let mut block = evidence_backed_block();
    block.selected_share_evidence[0].c =
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();

    assert_replay_error_contains(block, "selected share evidence c mismatch");
}

#[test]
fn replay_rejects_selected_share_evidence_pk_mismatch() {
    let mut block = evidence_backed_block();
    block.selected_share_evidence[0].pk =
        "2222222222222222222222222222222222222222222222222222222222222222".to_string();

    assert_replay_error_contains(block, "selected share evidence pk mismatch");
}

#[test]
fn replay_rejects_selected_share_evidence_invalid_proof_package_hex() {
    let mut block = evidence_backed_block();
    block.selected_share_evidence[0].proof_package = "not-hex".to_string();

    assert_replay_error_contains(block, "selected share evidence proofPackage hex invalid");
}

#[test]
fn replay_rejects_selected_share_evidence_invalid_proof_package_shape() {
    let mut block = evidence_backed_block();
    block.selected_share_evidence[0].proof_package = "00".to_string();

    assert_replay_error_contains(block, "selected share evidence proofPackage invalid");
}

#[test]
fn replay_rejects_selected_share_evidence_canon_hash_mismatch() {
    let mut block = evidence_backed_block();
    block.selected_share_evidence[0].canon_hash =
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string();

    assert_replay_error_contains(block, "selected share evidence canonHash mismatch");
}

#[test]
fn replay_rejects_selected_share_evidence_share_hash_mismatch() {
    let mut block = evidence_backed_block();
    block.selected_share_evidence[0].j =
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string();

    assert_replay_error_contains(block, "selected share evidence shareHash mismatch");
}

#[test]
fn replay_rejects_selected_share_evidence_min_share_score_policy_mismatch() {
    let mut block = evidence_backed_block();
    block.min_share_score = "9".to_string();

    assert_replay_error_contains(block, "selected share evidence minShareScore mismatch");
}

#[test]
fn replay_rejects_selected_share_evidence_without_policy_multiplier() {
    let mut block = evidence_backed_block();
    block.min_share_score_multiplier_nanos = 0;

    assert_replay_error_contains(
        block,
        "selected share evidence requires minShareScoreMultiplierNanos",
    );
}

fn assert_replay_error_contains(block: PersistedBlock, expected: &str) {
    let err = replay_blocks(&[block]).expect_err("tampered selected share evidence is rejected");
    assert!(
        err.to_string().contains(expected),
        "expected error containing {expected:?}, got: {err}"
    );
}

fn evidence_backed_block() -> PersistedBlock {
    let proof_package = valid_pofp_v2_package_hex();
    let proof_package_bytes = hex::decode(&proof_package).expect("valid proof package hex");
    let canon_hash = hex::encode(Sha256::digest(&proof_package_bytes));
    let prev_c = "0000000000000000000000000000000000000000000000000000000000000000";
    let pk = "1111111111111111111111111111111111111111111111111111111111111111";
    let n = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let j = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let share_hash = share_hash(
        &Hex32::from_hex(prev_c).expect("prev c hex32"),
        &Hex32::from_hex(pk).expect("pk hex32"),
        &Hex32::from_hex(n).expect("n hex32"),
        &Hex32::from_hex(j).expect("j hex32"),
        &Hex32::from_hex(&canon_hash).expect("canon hash hex32"),
    )
    .to_hex();
    let c = block_hash(
        &Hex32::from_hex(prev_c).expect("prev c hex32"),
        &[Hex32::from_hex(&share_hash).expect("share hash hex32")],
    )
    .to_hex();

    PersistedBlock {
        height: 0,
        prev_c: prev_c.to_string(),
        c,
        proposer_pk: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        selected_share_hashes: vec![share_hash],
        selected_share_pks: vec![pk.to_string()],
        selected_share_reward_pks: vec![],
        proposer_reward_pk: String::new(),
        selected_share_evidence: vec![SelectedShareEvidence {
            pk: pk.to_string(),
            n: n.to_string(),
            j: j.to_string(),
            c: prev_c.to_string(),
            canon_hash,
            proof_package,
        }],
        min_share_score: "1".to_string(),
        min_share_score_multiplier_nanos: 1_000_000_000,
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
    }
}

fn valid_pofp_v2_package_hex() -> String {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"POFP");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0x19);
    bytes.extend_from_slice(&[0x11; 32]);
    bytes.push(0x19);
    bytes.extend_from_slice(&[0x22; 32]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    hex::encode(bytes)
}
