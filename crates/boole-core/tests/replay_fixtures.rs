use std::collections::BTreeMap;

use boole_core::{
    block_hash, compute_block_reward_credits, min_share_score, parse_biguint_hex, replay_blocks,
    replay_blocks_allow_legacy_evidence_less, share_hash, FamilyManifestRegistry, Hex32,
    LegacyEvidenceOptIn, PersistedBlock, PersistedRewardEvent, SelectedShareEvidence,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const V1_FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/protocol/replay/v1.json"
);
const V2_FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/protocol/replay/v2.json"
);

fn read_fixture(path: &str) -> Fixture {
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixture reads"))
        .expect("fixture parses")
}

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
    // v1.json is the legacy TypeScript-derived fixture and predates
    // `selectedShareEvidence` (see docs/replay-consensus.md); it replays
    // via the explicit N3-pre.1 legacy opt-in, not the strict-by-default
    // `replay_blocks` used by every other test in this file.
    let fixture = read_fixture(V1_FIXTURE_PATH);
    assert_legacy_replay_fixture(fixture);
}

#[test]
fn replay_matches_evidence_backed_v2_golden_fixture() {
    let fixture = read_fixture(V2_FIXTURE_PATH);
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

fn assert_replay_fixture(fixture: Fixture) {
    assert_reward_events_match(&fixture);
    let replay = replay_blocks(&fixture.blocks).expect("replay passes");
    assert_replay_result_matches(&fixture, replay);
}

/// N3-pre.1 — for the legacy (pre-`selectedShareEvidence`) v1 fixture only;
/// see the doc comment on `replay_matches_typescript_golden_fixture`.
fn assert_legacy_replay_fixture(fixture: Fixture) {
    assert_reward_events_match(&fixture);
    let replay = replay_blocks_allow_legacy_evidence_less(
        &fixture.blocks,
        LegacyEvidenceOptIn::for_legacy_replay_only(),
        &FamilyManifestRegistry::new(),
    )
    .expect("legacy replay passes");
    assert_replay_result_matches(&fixture, replay);
}

fn assert_reward_events_match(fixture: &Fixture) {
    for (block, event) in fixture.blocks.iter().zip(fixture.reward_events.iter()) {
        let credits = compute_block_reward_credits(block).expect("credits compute");
        assert_eq!(
            credits, event.credits,
            "reward event height {}",
            event.height
        );
        assert_eq!(block.height, event.height);
        assert_eq!(block.c, event.c);
    }
}

fn assert_replay_result_matches(fixture: &Fixture, replay: boole_core::ReplayResult) {
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
fn replay_credits_reward_override_fields_not_mining_identity_fields() {
    let mut block = evidence_backed_block();
    let reward_pk = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    block.selected_share_reward_pks = vec![reward_pk.to_string()];
    block.proposer_reward_pk = reward_pk.to_string();
    // The preimage commits the reward routing (ADR-0014 (a), v3 since
    // the §SC reset window) — a block proposed WITH overrides hashes over
    // them, so re-derive `c` after setting them (a post-hoc mutation
    // without rehashing is now a c-mismatch reject;
    // `block_hash_v3_commits_reward_routing_fields` pins that side).
    block.c = boole_core::block_hash(&block).to_hex();

    let credits = compute_block_reward_credits(&block).expect("reward override credits compute");
    assert_eq!(credits.len(), 1);
    assert_eq!(credits[0].pk, reward_pk);
    assert_eq!(credits[0].amount, "2");

    let replay = replay_blocks(&[block]).expect("reward override block replays");
    assert_eq!(replay.balances.get(reward_pk).copied(), Some(2));
    assert_eq!(
        replay
            .balances
            .get("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .copied(),
        None,
        "proposer identity must not receive reward when proposerRewardPk is set"
    );
    assert_eq!(
        replay
            .balances
            .get("1111111111111111111111111111111111111111111111111111111111111111")
            .copied(),
        None,
        "share mining identity must not receive reward when selectedShareRewardPks is set"
    );
}

#[test]
fn replay_rejects_block_with_bad_difficulty_weight() {
    let fixture = read_fixture(V1_FIXTURE_PATH);
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

    assert_replay_error_contains(block, "minShareScoreMultiplierNanos");
}

// §SC W1.a (masterplan audit 2026-07-11, item 5) — the multiplier's
// consensus home is the rule set (ADR-0014 Tier-2 constant), not the
// block itself. A block that self-declares a different multiplier with
// an arithmetically CONSISTENT minShareScore passes the pre-W1.a
// consistency check while unilaterally moving the share-score floor;
// replay must bind the declared value to the rule constant, not merely
// to the block's own arithmetic.
#[test]
fn replay_rejects_block_authored_score_multiplier() {
    let mut block = evidence_backed_block();
    let authored_multiplier_nanos = 2_000_000_000;
    block.min_share_score_multiplier_nanos = authored_multiplier_nanos;
    block.min_share_score = min_share_score(
        &parse_biguint_hex(&block.t_share).expect("t_share parses"),
        authored_multiplier_nanos,
    )
    .expect("min share score computes")
    .to_string();

    assert_replay_error_contains(block, "consensus rule constant");
}

// SC.7 (masterplan audit item 1, Critical) — replay must re-derive every
// selected share's ACTUAL score and reject the block when any share falls
// below the committed minimum. Before this slice replay only checked the
// declared minimum's arithmetic (t_share x multiplier); no code path ever
// executed `share_score(rederived_share_hash) >= expected_min_share_score`,
// so a block could commit shares below its own floor.
#[test]
fn replay_rejects_selected_share_below_committed_min_score() {
    let mut block = evidence_backed_block();
    // A tiny t_share makes the committed floor astronomically high
    // (min = 2^256 / t_share), so the block's real share (score derived
    // from its hash) is guaranteed below it. The declared minimum stays
    // arithmetically consistent — only the per-share re-check can reject.
    block.t_share = format!("0x{:064x}", 1);
    block.min_share_score = min_share_score(
        &parse_biguint_hex(&block.t_share).expect("t_share parses"),
        block.min_share_score_multiplier_nanos,
    )
    .expect("min share score computes")
    .to_string();

    assert_replay_error_contains(block, "below the committed minimum share score");
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
    let mut block = PersistedBlock {
        height: 0,
        prev_c: prev_c.to_string(),
        c: String::new(),
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
            seed_hex: String::new(),
            signed_work: None,
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
        promoted_bounty_shares: vec![],
    };
    block.c = block_hash(&block).to_hex();
    block
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

/// Deliberate-regen helper (NOT part of the suite): after a preimage
/// change (reset window), recompute each fixture's block `c` chain, the
/// reward events' `c`, and `expected.latestC`, then rewrite the files.
/// Balances and credits are content-derived and unchanged by a preimage
/// bump. Run: `cargo test -p boole-core --test replay_fixtures -- --ignored`
#[test]
#[ignore = "regen helper — rewrites the golden fixtures from the current preimage"]
fn regen_replay_golden_fixtures() {
    for path in [V1_FIXTURE_PATH, V2_FIXTURE_PATH] {
        let raw = std::fs::read_to_string(path).expect("fixture reads");
        let mut value: serde_json::Value = serde_json::from_str(&raw).expect("fixture parses");
        let blocks_value = value["blocks"].clone();
        let mut blocks: Vec<PersistedBlock> =
            serde_json::from_value(blocks_value).expect("blocks parse");
        let mut prev_c = blocks
            .first()
            .map(|b| b.prev_c.clone())
            .unwrap_or_else(|| "0".repeat(64));
        let mut new_cs = Vec::with_capacity(blocks.len());
        for block in &mut blocks {
            block.prev_c = prev_c.clone();
            block.c = String::new();
            block.c = block_hash(block).to_hex();
            prev_c = block.c.clone();
            new_cs.push(block.c.clone());
        }
        value["blocks"] = serde_json::to_value(&blocks).expect("blocks serialize");
        if let Some(events) = value["rewardEvents"].as_array_mut() {
            for (event, c) in events.iter_mut().zip(new_cs.iter()) {
                event["c"] = serde_json::json!(c);
            }
        }
        if let Some(latest) = new_cs.last() {
            value["expected"]["latestC"] = serde_json::json!(latest);
        }
        let mut out = serde_json::to_string_pretty(&value).expect("serialize");
        out.push('\n');
        std::fs::write(path, out).expect("fixture writes");
    }
}
