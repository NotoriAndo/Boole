//! P1.3b — multi-store atomic per-block commit: crash-mid-commit heal.
//!
//! The per-block commit writes the block store, then the reward ledger (then
//! the bounty-event ledger, then the submit receipt). A crash between the
//! block append and the reward append leaves the reward ledger trailing the
//! block store by one event. The block store is the source of truth — each
//! block fully determines its reward event — so boot re-derives and appends
//! the missing trailing event instead of refusing to come up, then re-runs
//! `verify_ledger_matches_replay` so a GENUINE balance tamper (a wrong amount
//! in an existing event, which does not change the count) still bails.
//!
//! (Master plan §6.5 P1.3 selects this re-derive-on-mismatch closure over a
//! `staging/commit-<height>.json` intent file: the block store already makes
//! every other store re-derivable, so a write-ahead intent file would add a
//! second source of truth that could only diverge. See ADR note in the
//! production-readiness master plan.)

use std::collections::BTreeSet;
use std::path::Path;

use boole_core::AdmissionDecision;
use boole_node::{FileBlockStore, RuntimeAdmissionState, RuntimeConfig};
use boole_testkit::rand_suffix;
use serde::Deserialize;
use serde_json::{Map, Value};

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

/// Commit two valid blocks to a fresh block store and return the runtime
/// config + the block-store path + the prover pk. Mirrors the proven
/// two-block commit dance in `runtime_policy_boot.rs`.
fn commit_two_blocks(dir: &Path) -> (RuntimeConfig, std::path::PathBuf, String) {
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
    let pk = fixture.constants.pk.clone();

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
        .commit_next_block_for_current_c(&block_path, 1_800_000_000_123, &accepted_tags)
        .expect("height0 block committed");

    let mut body1 = body0.clone();
    body1.insert("c".to_string(), Value::String(committed0.block.c.clone()));
    // N4-pre.1 — consensus proof dedup: the height-1 share must carry a
    // DISTINCT proof or the builder (correctly) refuses to credit the same
    // canon_hash twice. Bump the POFP v1 package's second expr payload.
    let bytes0 = body0
        .get("bytes")
        .and_then(Value::as_str)
        .expect("fixture body carries bytes");
    let bytes1 = format!("{}{}{}", &bytes0[..44], "02000000", &bytes0[52..]);
    assert_ne!(bytes0, bytes1, "second block must carry a distinct proof");
    body1.insert("bytes".to_string(), Value::String(bytes1));
    runtime
        .observe_ticket_from_body(&body1)
        .expect("observe height1 ticket");
    assert!(matches!(
        runtime.admit_body_with_canon_tag(1_800_000_061_000, "198.51.100.42", &body1, 0),
        AdmissionDecision::Accepted { .. }
    ));
    runtime
        .commit_next_block_for_current_c(&block_path, 1_800_000_061_123, &accepted_tags)
        .expect("height1 block committed");

    let recovered = FileBlockStore::recover(&block_path).expect("recover two-block store");
    assert_eq!(recovered.size(), 2, "setup must produce a two-block store");
    (config, block_path, pk)
}

fn tmp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-p1-3b-heal-{tag}-{}-{}",
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

#[test]
fn boot_heals_reward_ledger_trailing_block_store_by_one() {
    let dir = tmp_dir("trailing");
    let (config, block_path, pk) = commit_two_blocks(&dir);
    let reward_path = dir.join("rewards.ndjson");

    // First boot with the reward path absent re-derives a consistent ledger
    // (one event per block) — the "before crash" on-disk state.
    let booted = RuntimeAdmissionState::boot_from_store_with_bounty_ledger(
        config.clone(),
        &block_path,
        Some(reward_path.clone()),
        None,
    )
    .expect("first boot re-derives the absent ledger");
    let full_balance = booted.balance_for(&pk);
    assert!(full_balance >= 2, "two blocks must credit the prover");
    assert_eq!(
        line_count(&reward_path),
        2,
        "consistent ledger has 2 events"
    );

    // Simulate a crash between the block append and the reward append on the
    // SECOND block: the reward ledger trails the block store by one event.
    let kept: String = {
        let content = std::fs::read_to_string(&reward_path).expect("read ledger");
        let mut first = content.lines().take(1).collect::<Vec<_>>().join("\n");
        first.push('\n');
        first
    };
    std::fs::write(&reward_path, kept).expect("truncate ledger to one event");
    assert_eq!(line_count(&reward_path), 1, "ledger now trails by one");

    // Re-boot: the heal re-derives the missing trailing event from the block
    // store instead of refusing to boot, and the balance is fully restored.
    let healed = RuntimeAdmissionState::boot_from_store_with_bounty_ledger(
        config,
        &block_path,
        Some(reward_path.clone()),
        None,
    )
    .expect("boot must HEAL a reward ledger trailing the block store, not bail");
    assert_eq!(
        line_count(&reward_path),
        2,
        "heal must re-append the missing trailing event"
    );
    assert_eq!(
        healed.balance_for(&pk),
        full_balance,
        "healed balance must match the pre-crash balance"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn boot_still_bails_when_an_existing_reward_event_is_tampered() {
    let dir = tmp_dir("tamper");
    let (config, block_path, _pk) = commit_two_blocks(&dir);
    let reward_path = dir.join("rewards.ndjson");

    RuntimeAdmissionState::boot_from_store_with_bounty_ledger(
        config.clone(),
        &block_path,
        Some(reward_path.clone()),
        None,
    )
    .expect("first boot re-derives the absent ledger");

    // Tamper the FIRST (existing) event's credit amounts. The event COUNT
    // still equals the block count, so no trailing heal fires; the re-run of
    // verify_ledger_matches_replay must catch the wrong balance and bail.
    let content = std::fs::read_to_string(&reward_path).expect("read ledger");
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    assert_eq!(lines.len(), 2, "two events expected before tamper");
    let mut first: Value = serde_json::from_str(&lines[0]).expect("event json");
    if let Some(credits) = first.get_mut("credits").and_then(Value::as_array_mut) {
        for credit in credits.iter_mut() {
            credit["amount"] = Value::String("999999".to_string());
        }
    }
    lines[0] = serde_json::to_string(&first).expect("reserialize");
    std::fs::write(&reward_path, format!("{}\n", lines.join("\n"))).expect("rewrite tampered");

    let result = RuntimeAdmissionState::boot_from_store_with_bounty_ledger(
        config,
        &block_path,
        Some(reward_path),
        None,
    );
    let err = match result {
        Ok(_) => panic!("a tampered existing event must still bail boot, not be healed"),
        Err(e) => e,
    };
    assert!(
        err.to_string().contains("reward ledger divergence"),
        "tamper must surface the typed divergence error, got: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
