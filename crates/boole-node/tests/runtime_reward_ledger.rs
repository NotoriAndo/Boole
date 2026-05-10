//! S9 — RuntimeAdmissionState reward ledger integration.
//!
//! Drives the new `boot_from_store(config, block_path, reward_ledger_path)`
//! signature against three boot scenarios (file present + matches, file
//! absent → re-derive, file present + tampered → bail) and confirms the
//! ledger advances on every committed block.

use std::path::{Path, PathBuf};

use boole_core::CalibrationReport;
use boole_core::{PersistedBlock, PersistedRewardEvent};
use boole_node::block_store::FileBlockStore;
use boole_node::reward_store::FileRewardLedger;
use boole_node::runtime::{RuntimeAdmissionState, RuntimeConfig};
use serde::Deserialize;

const PK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PK_1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const PK_2: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const PK_3: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const PK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[derive(Debug, Deserialize)]
struct ReplayFixture {
    blocks: Vec<PersistedBlock>,
    #[serde(rename = "rewardEvents")]
    reward_events: Vec<PersistedRewardEvent>,
}

fn replay_fixture() -> ReplayFixture {
    serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
        .expect("fixture parses")
}

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn fresh_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-s9-runtime-{}-{}-{}",
        tag,
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir
}

fn seed_blocks(block_path: &Path, blocks: &[PersistedBlock]) {
    for block in blocks {
        FileBlockStore::append(block_path, block).expect("append block");
    }
}

fn seed_ledger(reward_path: &Path, events: &[PersistedRewardEvent]) {
    for event in events {
        FileRewardLedger::append(reward_path, event).expect("append reward");
    }
}

fn smoke_calibration_report() -> CalibrationReport {
    let scenario: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/runtime-smoke/v1.json"
    ))
    .expect("scenario json");
    serde_json::from_value(scenario["cfg"].clone()).expect("calibration report")
}

fn boot_runtime(
    block_path: &Path,
    reward_ledger_path: Option<&Path>,
) -> anyhow::Result<RuntimeAdmissionState> {
    let report = smoke_calibration_report();
    let config = RuntimeConfig::from_calibration_report(report, 60_000)
        .expect("runtime config from smoke calibration");
    RuntimeAdmissionState::boot_from_store(
        config,
        block_path,
        reward_ledger_path.map(Path::to_path_buf),
    )
}

#[test]
fn boot_recovers_existing_ledger_and_passes_verification() {
    let fix = replay_fixture();
    let dir = fresh_dir("recover-ok");
    let block_path = dir.join("blocks.ndjson");
    let reward_path = dir.join("rewards.ndjson");
    seed_blocks(&block_path, &fix.blocks);
    seed_ledger(&reward_path, &fix.reward_events);

    let runtime = boot_runtime(&block_path, Some(&reward_path)).expect("boot succeeds");
    assert_eq!(runtime.balance_for(PK_A), 1, "proposer block 0");
    assert_eq!(runtime.balance_for(PK_1), 1, "share owner block 0");
    assert_eq!(runtime.balance_for(PK_2), 3, "shared in both blocks");
    assert_eq!(runtime.balance_for(PK_3), 1, "share owner block 1");
    assert_eq!(runtime.balance_for(PK_B), 1, "proposer block 1");

    let head = runtime.ledger_head().expect("ledger head present");
    assert_eq!(head.0, 1, "asOfHeight is the latest event height");
    assert_eq!(
        head.1,
        fix.blocks.last().expect("last block").c,
        "asOfC matches latest block c"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn boot_re_derives_ledger_when_file_absent() {
    let fix = replay_fixture();
    let dir = fresh_dir("rederive");
    let block_path = dir.join("blocks.ndjson");
    let reward_path = dir.join("rewards.ndjson");
    seed_blocks(&block_path, &fix.blocks);
    // No ledger seeded — boot must re-derive from blocks.

    let runtime = boot_runtime(&block_path, Some(&reward_path)).expect("boot re-derives");
    assert_eq!(runtime.balance_for(PK_A), 1);
    assert_eq!(runtime.balance_for(PK_2), 3);

    assert!(
        reward_path.exists(),
        "boot must materialize rewards.ndjson on re-derive"
    );
    let recovered = FileRewardLedger::recover(&reward_path).expect("recover written ledger");
    assert_eq!(
        recovered.size(),
        fix.blocks.len(),
        "one event per block after re-derive"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn boot_bails_on_ledger_divergence() {
    let fix = replay_fixture();
    let dir = fresh_dir("divergence");
    let block_path = dir.join("blocks.ndjson");
    let reward_path = dir.join("rewards.ndjson");
    seed_blocks(&block_path, &fix.blocks);

    // Tamper: increase block-0 share owner credit so the ledger over-counts PK_1.
    let mut tampered = fix.reward_events.clone();
    if let Some(event) = tampered.first_mut() {
        if let Some(credit) = event.credits.iter_mut().find(|c| c.pk == PK_1) {
            credit.amount = "99".to_string();
        }
    }
    seed_ledger(&reward_path, &tampered);

    let result = boot_runtime(&block_path, Some(&reward_path));
    let err = match result {
        Ok(_) => panic!("boot must bail on tampered ledger"),
        Err(err) => err,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("reward ledger divergence"),
        "typed prefix expected, got: {msg}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn boot_with_no_reward_path_keeps_legacy_behavior() {
    let fix = replay_fixture();
    let dir = fresh_dir("legacy");
    let block_path = dir.join("blocks.ndjson");
    seed_blocks(&block_path, &fix.blocks);

    let runtime = boot_runtime(&block_path, None).expect("legacy boot succeeds");
    assert_eq!(
        runtime.balance_for(PK_A),
        0,
        "no ledger configured → balance_for returns 0 for any pk"
    );
    assert!(
        runtime.ledger_head().is_none(),
        "no ledger configured → no head"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
