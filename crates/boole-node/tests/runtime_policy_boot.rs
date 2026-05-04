use boole_core::{
    admit_submission_json, replay_blocks, AdmissionDecision, AdmissionError, AdmissionStatus,
    BuildSelectionResult, CalibrationReport, RateLimitRejectReason, RejectionReason,
    SharePoolRejectReason,
};
use boole_node::block_store::FileBlockStore;
use boole_node::runtime::{RuntimeAdmissionState, RuntimeConfig};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    constants: Constants,
    cfg: CalibrationReport,
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
    #[serde(default)]
    observe_ticket: bool,
    expect: Value,
}

#[test]
fn runtime_boots_policy_first_admission_state_from_report() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    let config = RuntimeConfig::from_calibration_report(fixture.cfg, 60_000)
        .expect("runtime config boots from report");
    assert_eq!(config.policy.share_cap_per_pk_block, 2);
    assert_eq!(config.policy.per_ip_rate_limit_per_60s, 1);

    let mut runtime = RuntimeAdmissionState::new(config);
    runtime.set_current_c(fixture.constants.c.clone());

    let valid_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .expect("valid op");
    let body = body_for(&fixture.constants, &valid_op.body_patch);
    if valid_op.observe_ticket {
        runtime
            .observe_ticket_from_body(&body)
            .expect("observe ticket");
    }

    let decision = runtime.admit_body(1_800_000_000_000, &fixture.constants.ip, &body);
    assert!(matches!(decision, AdmissionDecision::Accepted { .. }));
    assert_eq!(admit_submission_json(&decision), valid_op.expect);
}

#[test]
fn runtime_admission_state_preserves_pool_and_rate_limit_state() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    let config = RuntimeConfig::from_calibration_report(fixture.cfg, 60_000)
        .expect("runtime config boots from report");
    let mut runtime = RuntimeAdmissionState::new(config);
    runtime.set_current_c(fixture.constants.c.clone());

    let first_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .expect("valid op");
    let first_body = body_for(&fixture.constants, &first_op.body_patch);
    runtime
        .observe_ticket_from_body(&first_body)
        .expect("observe first ticket");
    let first = runtime.admit_body(1_800_000_000_000, &fixture.constants.ip, &first_body);
    assert!(matches!(first, AdmissionDecision::Accepted { .. }));
    assert_eq!(runtime.pool_size(), 1);
    assert_eq!(runtime.shares_for_current_c().len(), 1);

    let second_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "second_valid_rate_limited")
        .expect("second valid op");
    let second_body = body_for(&fixture.constants, &second_op.body_patch);
    runtime
        .observe_ticket_from_body(&second_body)
        .expect("observe second ticket");
    let second = runtime.admit_body(1_800_000_000_001, &fixture.constants.ip, &second_body);
    assert_eq!(
        second,
        AdmissionDecision::Rejected {
            status: AdmissionStatus::RateLimited,
            error: AdmissionError::RateLimited {
                reason: RateLimitRejectReason::IpQuota,
            },
            rejection: RejectionReason::RateLimit {
                quota: RateLimitRejectReason::IpQuota,
            },
        }
    );
    assert_eq!(admit_submission_json(&second), second_op.expect);
    assert_eq!(runtime.pool_size(), 1);
    assert_eq!(runtime.shares_for_current_c().len(), 1);
}

#[test]
fn runtime_builds_block_selection_from_admitted_candidates() {
    let mut fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    fixture.cfg.T_share =
        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    fixture.cfg.T_block =
        "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();
    fixture.cfg.MinShareScoreMultiplier = 1.0;
    fixture.cfg.K_max = 4;
    fixture.cfg.perIpRateLimitPer60s = 10;

    let config = RuntimeConfig::from_calibration_report(fixture.cfg, 60_000)
        .expect("runtime config boots from report");
    let mut runtime = RuntimeAdmissionState::new(config);
    runtime.set_current_c(fixture.constants.c.clone());

    let valid_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .expect("valid op");
    let body = body_for(&fixture.constants, &valid_op.body_patch);
    runtime
        .observe_ticket_from_body(&body)
        .expect("observe ticket");
    let decision =
        runtime.admit_body_with_canon_tag(1_800_000_000_000, &fixture.constants.ip, &body, 0);
    assert!(matches!(decision, AdmissionDecision::Accepted { .. }));

    let accepted_tags = BTreeSet::from([0]);
    let selection = runtime
        .build_block_selection_for_current_c(&accepted_tags)
        .expect("block selection runs");
    let BuildSelectionResult::Ok(selection) = selection else {
        panic!("expected admitted candidate to be selected");
    };
    assert_eq!(selection.selected.len(), 1);
    assert_eq!(selection.selected[0].pk, fixture.constants.pk);
    assert_eq!(selection.selected[0].canon_tag, 0);
    assert_eq!(selection.proposer_index, 0);
}

#[test]
fn runtime_produces_persists_and_replays_selected_block() {
    let mut fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    fixture.constants.c =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    fixture.cfg.T_share =
        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    fixture.cfg.T_block =
        "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();
    fixture.cfg.MinShareScoreMultiplier = 1.0;
    fixture.cfg.K_max = 4;
    fixture.cfg.perIpRateLimitPer60s = 10;

    let config = RuntimeConfig::from_calibration_report(fixture.cfg, 60_000)
        .expect("runtime config boots from report");
    let mut runtime = RuntimeAdmissionState::new(config);
    runtime.set_current_c(fixture.constants.c.clone());

    let valid_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .expect("valid op");
    let body = body_for(&fixture.constants, &valid_op.body_patch);
    runtime
        .observe_ticket_from_body(&body)
        .expect("observe ticket");
    let decision =
        runtime.admit_body_with_canon_tag(1_800_000_000_000, &fixture.constants.ip, &body, 0);
    assert!(matches!(decision, AdmissionDecision::Accepted { .. }));

    let accepted_tags = BTreeSet::from([0]);
    let block = runtime
        .produce_block_for_current_c(0, 1_800_000_000_123, &accepted_tags)
        .expect("block is produced");
    assert_eq!(block.height, 0);
    assert_eq!(block.prev_c, fixture.constants.c);
    assert_eq!(block.selected_share_hashes.len(), 1);
    assert_eq!(block.selected_share_pks, vec![fixture.constants.pk.clone()]);
    block.validate_shape().expect("block shape is valid");

    let dir = std::env::temp_dir().join(format!(
        "boole-runtime-block-production-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blockstore.ndjson");

    FileBlockStore::append(&block_path, &block).expect("append block");
    let recovered = FileBlockStore::recover(&block_path).expect("recover block store");
    assert_eq!(recovered.size(), 1);
    assert_eq!(recovered.latest().expect("latest block"), &block);

    let replay = replay_blocks(recovered.blocks()).expect("replay produced block");
    assert_eq!(replay.height, 1);
    assert_eq!(replay.latest_c, block.c);
    assert_eq!(replay.balances.get(&fixture.constants.pk).copied(), Some(2));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runtime_applies_block_head_and_prunes_stale_shares() {
    let mut fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    fixture.constants.c =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    fixture.cfg.T_share =
        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    fixture.cfg.T_block =
        "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();
    fixture.cfg.MinShareScoreMultiplier = 1.0;
    fixture.cfg.K_max = 4;

    let config = RuntimeConfig::from_calibration_report(fixture.cfg, 60_000)
        .expect("runtime config boots from report");
    let mut runtime = RuntimeAdmissionState::new(config);
    runtime.set_current_c(fixture.constants.c.clone());

    let valid_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .expect("valid op");
    let old_body = body_for(&fixture.constants, &valid_op.body_patch);
    runtime
        .observe_ticket_from_body(&old_body)
        .expect("observe old ticket");
    assert!(matches!(
        runtime.admit_body_with_canon_tag(1_800_000_000_000, &fixture.constants.ip, &old_body, 0),
        AdmissionDecision::Accepted { .. }
    ));
    assert_eq!(runtime.pool_size(), 1);

    let accepted_tags = BTreeSet::from([0]);
    let block = runtime
        .produce_block_for_current_c(0, 1_800_000_000_123, &accepted_tags)
        .expect("block is produced");
    let dropped = runtime.apply_produced_block(&block).expect("apply block");
    assert_eq!(dropped, 1);
    assert_eq!(runtime.current_c(), Some(block.c.as_str()));
    assert_eq!(runtime.pool_size(), 0);
    assert_eq!(runtime.shares_for_current_c().len(), 0);
    assert_eq!(runtime.candidate_shares_for_current_c().len(), 0);

    let mut stale_body = old_body.clone();
    stale_body.insert(
        "pk".to_string(),
        Value::String(
            "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
        ),
    );
    runtime
        .observe_ticket_from_body(&stale_body)
        .expect("observe stale ticket");
    let stale = runtime.admit_body(1_800_000_000_001, "198.51.100.99", &stale_body);
    assert_eq!(
        stale,
        AdmissionDecision::Rejected {
            status: AdmissionStatus::UnprocessableEntity,
            error: AdmissionError::SharePool {
                reason: SharePoolRejectReason::StaleC,
            },
            rejection: RejectionReason::SharePool {
                detail: SharePoolRejectReason::StaleC,
            },
        }
    );
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
