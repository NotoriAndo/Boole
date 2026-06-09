//! P1.3b — bounty-event ledger crash-mid-commit heal.
//!
//! The per-block commit writes the block store, then the reward ledger, then
//! the bounty-event ledger (`credit` + `share_promoted` rows), then the submit
//! receipt. A crash AFTER the reward append but BEFORE the bounty-event appends
//! leaves the bounty-event ledger short of the last block's rows — and because
//! `verify_ledger_matches_replay` checks the bounty-event ledger's per-family
//! `credit` totals against the replayed block store, the node refuses to boot
//! with `--bounty-events` (an unbootable node, not a silent divergence).
//!
//! Closure mirrors the reward-ledger heal: each block now persists BOTH
//! `promoted_bounty_credits` AND `promoted_bounty_shares` (the latter carrying
//! the `proofHash` that is otherwise unrecoverable once the in-memory selection
//! drops), so boot re-derives and appends the missing trailing rows from the
//! block store. The re-derive is consensus-safe: `promoted_bounty_shares` is
//! NOT part of `block_hash` (asserted below), so recording it changes no block
//! identity. Healing BOTH rows (not just `credit`) is what prevents a
//! double-promotion: a missing `share_promoted` row would let
//! `rebuild_bounty_side_pool` treat the already-committed share as still
//! pending and re-promote it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use boole_core::{
    AdmissionDecision, Hex32, PersistedBlock, PromotedBountyCredit, PromotedBountyShare,
};
use boole_node::{FileBlockStore, RuntimeAdmissionState, RuntimeConfig};
use boole_testkit::rand_suffix;
use serde::Deserialize;
use serde_json::{Map, Value};

const FAMILY: &str = "fam-heal";
const BOUNTY: &str = "bounty-1";
const PROOF_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const PROOF_HASH_2: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const PROVER: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const AMOUNT: &str = "5";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    constants: Constants,
    cfg: boole_core::CalibrationReport,
    operations: Vec<Operation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Constants {
    c: String,
    pk: String,
    n: String,
    j: String,
    nonce_s: String,
    ip: String,
    valid_bytes_hex: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Operation {
    name: String,
    #[serde(default)]
    body_patch: Map<String, Value>,
}

fn body_for(constants: &Constants, patch: &Map<String, Value>) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("c".to_string(), Value::String(constants.c.clone()));
    body.insert("pk".to_string(), Value::String(constants.pk.clone()));
    body.insert("n".to_string(), Value::String(constants.n.clone()));
    body.insert("j".to_string(), Value::String(constants.j.clone()));
    body.insert(
        "nonceS".to_string(),
        Value::String(constants.nonce_s.clone()),
    );
    body.insert(
        "bytes".to_string(),
        Value::String(constants.valid_bytes_hex.clone()),
    );
    for (key, value) in patch {
        if value.is_null() {
            body.remove(key);
        } else {
            body.insert(key.clone(), value.clone());
        }
    }
    body
}

fn promoted_shares() -> Vec<PromotedBountyShare> {
    vec![PromotedBountyShare {
        family_id: FAMILY.to_string(),
        bounty_id: BOUNTY.to_string(),
        proof_hash: PROOF_HASH.to_string(),
        prover: PROVER.to_string(),
    }]
}

fn promoted_credits() -> Vec<PromotedBountyCredit> {
    vec![PromotedBountyCredit {
        family_id: FAMILY.to_string(),
        bounty_id: BOUNTY.to_string(),
        prover: PROVER.to_string(),
        amount: AMOUNT.to_string(),
    }]
}

/// Load and configure the shared admission fixture (max thresholds so the lone
/// valid op is always block-eligible).
fn configured_fixture() -> Fixture {
    let mut fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    fixture.constants.c =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    fixture.cfg.T_share =
        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    fixture.cfg.T_block =
        "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();
    fixture.cfg.MinShareScoreMultiplier = serde_json::Number::from(1);
    fixture.cfg.K_max = 4;
    fixture
}

/// Commit a single valid block carrying one promoted bounty credit + share to a
/// fresh block store. Mirrors the proven admission dance in
/// `reward_ledger_crash_heal.rs` but uses the `_with_promoted` commit path.
fn commit_block_with_promoted(dir: &Path) -> (RuntimeConfig, PathBuf, PersistedBlock) {
    let fixture = configured_fixture();

    let config = RuntimeConfig::from_calibration_report(fixture.cfg, 60_000)
        .expect("runtime config boots from report");
    let mut runtime = RuntimeAdmissionState::new(config.clone());
    runtime.set_current_c(fixture.constants.c.clone());

    let valid_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .expect("valid op");
    let block_path = dir.join("blockstore.ndjson");
    let accepted_tags = BTreeSet::from([0]);

    let body0 = body_for(&fixture.constants, &valid_op.body_patch);
    runtime
        .observe_ticket_from_body(&body0)
        .expect("observe height0 ticket");
    assert!(matches!(
        runtime.admit_body_with_canon_tag(1_800_000_000_000, &fixture.constants.ip, &body0, 0),
        AdmissionDecision::Accepted { .. }
    ));
    let committed = runtime
        .commit_next_block_for_current_c_with_promoted(
            &block_path,
            1_800_000_000_123,
            &accepted_tags,
            &promoted_shares(),
            &promoted_credits(),
        )
        .expect("block with promoted bounty data committed");

    // The persist fix records BOTH promoted rows on the block — the heal relies
    // on re-deriving them from the block store, so guard the source here.
    assert_eq!(
        committed.block.promoted_bounty_credits.len(),
        1,
        "block must persist the promoted credit"
    );
    assert_eq!(
        committed.block.promoted_bounty_shares.len(),
        1,
        "block must persist the promoted share (proofHash carrier)"
    );

    let recovered = FileBlockStore::recover(&block_path).expect("recover one-block store");
    assert_eq!(recovered.size(), 1, "setup must produce a one-block store");
    (config, block_path, committed.block)
}

/// Commit TWO valid blocks, each carrying a promoted bounty credit + share in
/// the SAME family (distinct proofHashes), to a fresh block store. Proves the
/// heal rebuilds the rows for EVERY block (the deleted-ledger upgrade path),
/// not only the last.
fn commit_two_blocks_with_promoted(dir: &Path) -> (RuntimeConfig, PathBuf) {
    let fixture = configured_fixture();
    let config = RuntimeConfig::from_calibration_report(fixture.cfg, 60_000)
        .expect("runtime config boots from report");
    let mut runtime = RuntimeAdmissionState::new(config.clone());
    runtime.set_current_c(fixture.constants.c.clone());

    let valid_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .expect("valid op");
    let block_path = dir.join("blockstore.ndjson");
    let accepted_tags = BTreeSet::from([0]);

    let body0 = body_for(&fixture.constants, &valid_op.body_patch);
    runtime
        .observe_ticket_from_body(&body0)
        .expect("observe height0 ticket");
    assert!(matches!(
        runtime.admit_body_with_canon_tag(1_800_000_000_000, &fixture.constants.ip, &body0, 0),
        AdmissionDecision::Accepted { .. }
    ));
    let committed0 = runtime
        .commit_next_block_for_current_c_with_promoted(
            &block_path,
            1_800_000_000_123,
            &accepted_tags,
            &promoted_shares(),
            &promoted_credits(),
        )
        .expect("height0 block committed");

    let mut body1 = body0.clone();
    body1.insert("c".to_string(), Value::String(committed0.block.c.clone()));
    runtime
        .observe_ticket_from_body(&body1)
        .expect("observe height1 ticket");
    assert!(matches!(
        runtime.admit_body_with_canon_tag(1_800_000_061_000, "198.51.100.42", &body1, 0),
        AdmissionDecision::Accepted { .. }
    ));
    let shares1 = vec![PromotedBountyShare {
        family_id: FAMILY.to_string(),
        bounty_id: BOUNTY.to_string(),
        proof_hash: PROOF_HASH_2.to_string(),
        prover: PROVER.to_string(),
    }];
    runtime
        .commit_next_block_for_current_c_with_promoted(
            &block_path,
            1_800_000_061_123,
            &accepted_tags,
            &shares1,
            &promoted_credits(),
        )
        .expect("height1 block committed");

    let recovered = FileBlockStore::recover(&block_path).expect("recover two-block store");
    assert_eq!(recovered.size(), 2, "setup must produce a two-block store");
    (config, block_path)
}

fn tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-p1-3b-bounty-heal-{tag}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir
}

fn line_count(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

/// Count bounty-event rows of `kind` in the ledger file.
fn kind_count(path: &Path, kind: &str) -> usize {
    std::fs::read_to_string(path)
        .map(|s| {
            s.lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .filter(|e| e.get("kind").and_then(Value::as_str) == Some(kind))
                .count()
        })
        .unwrap_or(0)
}

fn boot(
    config: RuntimeConfig,
    block_path: &Path,
    reward_path: PathBuf,
    bounty_path: PathBuf,
) -> anyhow::Result<RuntimeAdmissionState> {
    RuntimeAdmissionState::boot_from_store_with_bounty_ledger(
        config,
        block_path,
        Some(reward_path),
        Some(bounty_path),
    )
}

/// First boot with both ledger paths absent re-derives a consistent on-disk
/// state: one reward event + (1 credit, 1 share_promoted) bounty events.
fn establish_consistent_state(
    config: &RuntimeConfig,
    block_path: &Path,
    reward_path: &Path,
    bounty_path: &Path,
) {
    boot(
        config.clone(),
        block_path,
        reward_path.to_path_buf(),
        bounty_path.to_path_buf(),
    )
    .expect("first boot re-derives absent ledgers");
    assert_eq!(line_count(reward_path), 1, "consistent reward ledger");
    assert_eq!(
        kind_count(bounty_path, "credit"),
        1,
        "consistent credit row"
    );
    assert_eq!(
        kind_count(bounty_path, "share_promoted"),
        1,
        "consistent share_promoted row"
    );
}

#[test]
fn boot_heals_bounty_event_ledger_trailing_after_reward_written() {
    let dir = tmp_dir("trailing");
    let (config, block_path, _block) = commit_block_with_promoted(&dir);
    let reward_path = dir.join("rewards.ndjson");
    let bounty_path = dir.join("bounty-events.ndjson");
    establish_consistent_state(&config, &block_path, &reward_path, &bounty_path);

    // Simulate a crash AFTER the reward append but BEFORE the bounty-event
    // appends: the reward ledger survived, the bounty-event ledger is empty.
    std::fs::write(&bounty_path, "").expect("truncate bounty-event ledger");
    assert_eq!(line_count(&bounty_path), 0, "bounty ledger now trails");
    assert_eq!(line_count(&reward_path), 1, "reward ledger intact");

    // Re-boot: the heal re-derives the missing credit + share_promoted rows
    // from the block store instead of bailing in verify_ledger_matches_replay.
    boot(config, &block_path, reward_path, bounty_path.clone())
        .expect("boot must HEAL a bounty-event ledger trailing the block store, not bail");
    assert_eq!(
        kind_count(&bounty_path, "credit"),
        1,
        "heal re-appends the credit row"
    );
    assert_eq!(
        kind_count(&bounty_path, "share_promoted"),
        1,
        "heal re-appends the share_promoted row"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn boot_heal_is_idempotent_no_duplicate_events() {
    let dir = tmp_dir("idempotent");
    let (config, block_path, _block) = commit_block_with_promoted(&dir);
    let reward_path = dir.join("rewards.ndjson");
    let bounty_path = dir.join("bounty-events.ndjson");
    establish_consistent_state(&config, &block_path, &reward_path, &bounty_path);

    // Boot again with a CONSISTENT bounty-event ledger: present == expected, so
    // the heal must NOT fire and must NOT duplicate rows.
    boot(config, &block_path, reward_path, bounty_path.clone())
        .expect("re-boot on a consistent ledger must pass");
    assert_eq!(
        kind_count(&bounty_path, "credit"),
        1,
        "no duplicate credit row"
    );
    assert_eq!(
        kind_count(&bounty_path, "share_promoted"),
        1,
        "no duplicate share_promoted row"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn heal_restores_share_promoted_proof_hash_for_side_pool() {
    let dir = tmp_dir("proofhash");
    let (config, block_path, _block) = commit_block_with_promoted(&dir);
    let reward_path = dir.join("rewards.ndjson");
    let bounty_path = dir.join("bounty-events.ndjson");
    establish_consistent_state(&config, &block_path, &reward_path, &bounty_path);

    // Crash drops the bounty-event ledger; heal must restore the share_promoted
    // row WITH its proofHash. rebuild_bounty_side_pool keys promoted shares by
    // (familyId, bountyId, proofHash); a missing row would re-promote the
    // already-committed share (double-promotion). The proofHash is only
    // recoverable from the block's promoted_bounty_shares field.
    std::fs::write(&bounty_path, "").expect("truncate bounty-event ledger");
    boot(config, &block_path, reward_path, bounty_path.clone()).expect("boot must heal");

    let restored = std::fs::read_to_string(&bounty_path)
        .expect("read healed ledger")
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|e| e.get("kind").and_then(Value::as_str) == Some("share_promoted"))
        .expect("healed ledger has a share_promoted row");
    assert_eq!(
        restored.get("proofHash").and_then(Value::as_str),
        Some(PROOF_HASH),
        "healed share_promoted row must carry the original proofHash"
    );
    assert_eq!(
        restored.get("familyId").and_then(Value::as_str),
        Some(FAMILY)
    );
    assert_eq!(
        restored.get("bountyId").and_then(Value::as_str),
        Some(BOUNTY)
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn boot_still_bails_when_an_existing_credit_event_is_tampered() {
    let dir = tmp_dir("tamper");
    let (config, block_path, _block) = commit_block_with_promoted(&dir);
    let reward_path = dir.join("rewards.ndjson");
    let bounty_path = dir.join("bounty-events.ndjson");
    establish_consistent_state(&config, &block_path, &reward_path, &bounty_path);

    // Tamper the EXISTING credit event's amount. The event COUNT still matches
    // the block store, so no trailing heal fires; verify_ledger_matches_replay
    // must catch the wrong per-family total and bail.
    let content = std::fs::read_to_string(&bounty_path).expect("read ledger");
    let tampered: Vec<String> = content
        .lines()
        .map(|line| {
            let mut event: Value = serde_json::from_str(line).expect("event json");
            if event.get("kind").and_then(Value::as_str) == Some("credit") {
                event["amount"] = Value::String("999999".to_string());
            }
            serde_json::to_string(&event).expect("reserialize")
        })
        .collect();
    std::fs::write(&bounty_path, format!("{}\n", tampered.join("\n"))).expect("rewrite tampered");

    let result = boot(config, &block_path, reward_path, bounty_path);
    let err = match result {
        Ok(_) => panic!("a tampered existing credit event must still bail boot, not be healed"),
        Err(e) => e,
    };
    assert!(
        err.to_string()
            .contains("bounty ledger family-credit divergence"),
        "tamper must surface the typed bounty divergence error, got: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn block_hash_is_unchanged_by_promoted_bounty_shares_field() {
    // Consensus safety: block identity is hash(prev_c, selected_share_hashes).
    // Recording promoted_bounty_shares on the block (node-local audit data) must
    // not perturb it. Compute the block hash from a real committed block, then
    // from a clone with the field cleared, and assert equality.
    let dir = tmp_dir("blockhash");
    let (_config, _block_path, block) = commit_block_with_promoted(&dir);
    assert!(
        !block.promoted_bounty_shares.is_empty(),
        "fixture block must carry a promoted share"
    );

    let prev_c = Hex32::from_hex(&block.prev_c).expect("prev_c hex32");
    let share_hashes: Vec<Hex32> = block
        .selected_share_hashes
        .iter()
        .map(|h| Hex32::from_hex(h).expect("share hash hex32"))
        .collect();
    let with_shares = boole_core::block_hash(&prev_c, &share_hashes);

    let mut cleared = block.clone();
    cleared.promoted_bounty_shares.clear();
    let prev_c2 = Hex32::from_hex(&cleared.prev_c).expect("prev_c hex32");
    let share_hashes2: Vec<Hex32> = cleared
        .selected_share_hashes
        .iter()
        .map(|h| Hex32::from_hex(h).expect("share hash hex32"))
        .collect();
    let without_shares = boole_core::block_hash(&prev_c2, &share_hashes2);

    assert_eq!(
        with_shares.to_hex(),
        without_shares.to_hex(),
        "promoted_bounty_shares must not influence block_hash"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn boot_rebuilds_a_deleted_ledger_for_every_block_not_just_the_last() {
    // The documented upgrade-recovery path is "delete the bounty-event ledger
    // and reboot". With a MULTI-block store the heal must re-derive the rows for
    // EVERY block, not only the last — otherwise verify sees a per-family total
    // (last block only) below the replayed total (all blocks) and bails.
    let dir = tmp_dir("rebuild-all");
    let (config, block_path) = commit_two_blocks_with_promoted(&dir);
    let reward_path = dir.join("rewards.ndjson");
    let bounty_path = dir.join("bounty-events.ndjson");

    // First boot re-derives both ledgers from the two-block store.
    boot(
        config.clone(),
        &block_path,
        reward_path.clone(),
        bounty_path.clone(),
    )
    .expect("first boot re-derives absent ledgers for both blocks");
    assert_eq!(kind_count(&bounty_path, "credit"), 2, "two credit rows");
    assert_eq!(
        kind_count(&bounty_path, "share_promoted"),
        2,
        "two share_promoted rows"
    );

    // Operator deletes the whole ledger and reboots.
    std::fs::remove_file(&bounty_path).expect("delete bounty-event ledger");
    boot(config, &block_path, reward_path, bounty_path.clone())
        .expect("reboot must rebuild the deleted ledger for ALL blocks, not bail");
    assert_eq!(
        kind_count(&bounty_path, "credit"),
        2,
        "rebuild restores both blocks' credit rows"
    );
    assert_eq!(
        kind_count(&bounty_path, "share_promoted"),
        2,
        "rebuild restores both blocks' share_promoted rows"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn boot_heals_a_prefix_torn_inside_one_blocks_rows() {
    // Crash can tear the ledger BETWEEN a block's credit row and its
    // share_promoted row (the rows are appended one durable line at a time).
    // The surviving content is still a strict prefix of the expected sequence,
    // so the heal must append exactly the missing suffix — no duplicate credit.
    let dir = tmp_dir("prefix-torn");
    let (config, block_path, _block) = commit_block_with_promoted(&dir);
    let reward_path = dir.join("rewards.ndjson");
    let bounty_path = dir.join("bounty-events.ndjson");
    establish_consistent_state(&config, &block_path, &reward_path, &bounty_path);

    // Keep only the FIRST line (the credit row); drop the share_promoted row.
    let content = std::fs::read_to_string(&bounty_path).expect("read ledger");
    let first = content
        .lines()
        .next()
        .expect("at least one row")
        .to_string();
    std::fs::write(&bounty_path, format!("{first}\n")).expect("truncate to credit row only");
    assert_eq!(kind_count(&bounty_path, "credit"), 1, "credit row kept");
    assert_eq!(
        kind_count(&bounty_path, "share_promoted"),
        0,
        "share row dropped"
    );

    boot(config, &block_path, reward_path, bounty_path.clone())
        .expect("boot must heal the missing share_promoted suffix, not bail");
    assert_eq!(
        kind_count(&bounty_path, "credit"),
        1,
        "no duplicate credit row after heal"
    );
    assert_eq!(
        kind_count(&bounty_path, "share_promoted"),
        1,
        "heal appends the missing share_promoted row"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn boot_heals_block_rows_when_route_events_are_interleaved() {
    // P1.3b regression (adversarial-audit finding): the bounty-event ledger
    // interleaves ROUTE-driven rows (`create`/`status_change`/`proof`, written
    // by the announce/status/proof handlers) with BLOCK-driven
    // `credit`/`share_promoted` rows. The heal must count ONLY the block-driven
    // rows. A raw `present.len()` (all kinds) vs `expected.len()` (credit+share
    // only) comparison is wrong: with more route rows than expected block rows
    // the heal silently never fires, leaving the credit/share rows missing and
    // the `--bounty-events` node UNBOOTABLE (verify bails on the family-credit
    // divergence). This test pins the filtered count.
    let dir = tmp_dir("interleaved");
    let (config, block_path, _block) = commit_block_with_promoted(&dir);
    let reward_path = dir.join("rewards.ndjson");
    let bounty_path = dir.join("bounty-events.ndjson");
    establish_consistent_state(&config, &block_path, &reward_path, &bounty_path);

    // Simulate the crash: the announce/status handlers had written route rows,
    // but the block commit crashed before its `credit` + `share_promoted`
    // appends. The surviving ledger holds ONLY the route rows — and MORE of them
    // (3) than the block's expected block rows (2: one credit + one share) — so
    // the buggy raw-count heal would not fire. The reward ledger is intact.
    // Valid route-driven `create` rows (workId/problemHash/verifierKind/ts are
    // required by `validate_bounty_ledger_event`), matching the shape the
    // announce handler writes. THREE of them — more than the two expected
    // block rows — so the buggy raw-count heal would not fire.
    let ph = "1111111111111111111111111111111111111111111111111111111111111111";
    let route_only = [
        format!(r#"{{"schemaVersion":1,"kind":"create","workId":"b1","problemHash":"{ph}","verifierKind":"lean-runner-v01","ts":1}}"#),
        format!(r#"{{"schemaVersion":1,"kind":"create","workId":"b2","problemHash":"{ph}","verifierKind":"lean-runner-v01","ts":2}}"#),
        format!(r#"{{"schemaVersion":1,"kind":"create","workId":"b3","problemHash":"{ph}","verifierKind":"lean-runner-v01","ts":3}}"#),
    ]
    .join("\n");
    std::fs::write(&bounty_path, format!("{route_only}\n")).expect("write route-only ledger");
    assert_eq!(kind_count(&bounty_path, "credit"), 0, "no block rows yet");
    assert_eq!(kind_count(&bounty_path, "create"), 3, "route rows present");

    // Re-boot: the heal must re-derive the missing credit + share_promoted rows,
    // counting only the block-driven rows already present (zero), NOT the 3
    // route rows. With the bug this boot would bail (unbootable node).
    boot(config, &block_path, reward_path, bounty_path.clone())
        .expect("boot must heal the block rows past the interleaved route rows, not bail");
    assert_eq!(
        kind_count(&bounty_path, "credit"),
        1,
        "credit row re-derived despite interleaved route rows"
    );
    assert_eq!(
        kind_count(&bounty_path, "share_promoted"),
        1,
        "share_promoted row re-derived"
    );
    assert_eq!(
        kind_count(&bounty_path, "create"),
        3,
        "route rows left untouched by the heal"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
