//! N1.1 (G1/G2) — the runtime exposes the height-effective difficulty that
//! `/head` must emit: a retarget-aware `T_block` plus epoch/mode labels.
//! When retarget is disabled the labels stay `static-calibrated`/epoch 0;
//! when enabled they report the `epoch-retarget-v0` path so a miner can see
//! it is mining the runtime-effective difficulty, not the static report.

use std::collections::BTreeSet;

use boole_core::{AdmissionDecision, CalibrationReport, DifficultyRetargetPolicy};
use boole_node::{RuntimeAdmissionState, RuntimeConfig};
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    constants: Constants,
    cfg: CalibrationReport,
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

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
        .expect("fixture parses")
}

fn runtime_config() -> RuntimeConfig {
    let mut cfg = fixture().cfg;
    // Permissive thresholds so the single fixture share qualifies as a block
    // proposer (mirrors runtime_policy_boot's committing test). The retarget
    // labels under test are independent of the exact threshold values.
    cfg.T_share = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    cfg.T_block = "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();
    cfg.MinShareScoreMultiplier = serde_json::Number::from(1);
    cfg.K_max = 4;
    RuntimeConfig::from_calibration_report(cfg, 60_000).expect("runtime config")
}

fn valid_body(c: &Constants) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("c".to_string(), Value::String(c.c.clone()));
    body.insert("pk".to_string(), Value::String(c.pk.clone()));
    body.insert("n".to_string(), Value::String(c.n.clone()));
    body.insert("j".to_string(), Value::String(c.j.clone()));
    body.insert("nonceS".to_string(), Value::String(c.nonce_s.clone()));
    body.insert(
        "bytes".to_string(),
        Value::String(c.valid_bytes_hex.clone()),
    );
    body
}

#[test]
fn effective_difficulty_for_head_static_when_retarget_disabled() {
    let runtime = RuntimeAdmissionState::new(runtime_config());
    let evidence = runtime
        .effective_difficulty_for_head()
        .expect("effective difficulty");
    assert_eq!(evidence.mode, "static-calibrated");
    assert_eq!(evidence.difficulty_epoch, 0);
    assert_eq!(evidence.retarget, "not-enabled");
}

#[test]
fn effective_difficulty_for_head_labels_epoch_retarget_after_a_block_when_enabled() {
    // At genesis (no blocks) the retarget path still reports static-calibrated
    // — the epoch-retarget-v0 label only engages once a block exists. Commit
    // one block, then /head must report the retarget path.
    let f = fixture();
    let config = runtime_config()
        .with_difficulty_retarget(DifficultyRetargetPolicy {
            target_block_ms: 61_000,
            retarget_every_blocks: 4,
            max_adjustment_factor: 4,
        })
        .expect("retarget policy valid");
    let dir = std::env::temp_dir().join(format!("boole-n11-head-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let block_path = dir.join("blocks.ndjson");

    // Commit one block on the retarget-enabled runtime (permissive thresholds
    // make the single share a proposer), giving height 1 so the retarget
    // label engages.
    let mut runtime = RuntimeAdmissionState::new(config);
    runtime.set_current_c(f.constants.c.clone());
    let body = valid_body(&f.constants);
    runtime
        .observe_ticket_from_body(&body)
        .expect("observe ticket");
    assert!(matches!(
        runtime.admit_body_with_canon_tag(1_800_000_000_000, &f.constants.ip, &body, 0),
        AdmissionDecision::Accepted { .. }
    ));
    runtime
        .commit_next_block_for_current_c(&block_path, 1_800_000_000_123, &BTreeSet::from([0]))
        .expect("commit block 0");

    let evidence = runtime
        .effective_difficulty_for_head()
        .expect("effective difficulty");
    assert_eq!(
        evidence.mode, "epoch-retarget-v0",
        "after a block, retarget-enabled /head must label difficulty epoch-retarget-v0"
    );
    assert_eq!(evidence.retarget, "enabled");

    let _ = std::fs::remove_dir_all(&dir);
}
