use boole_core::{
    admit_submission_json, AdmissionDecision, AdmissionError, AdmissionStatus, CalibrationReport,
    RateLimitRejectReason, RejectionReason,
};
use boole_node::runtime::{RuntimeAdmissionState, RuntimeConfig};
use serde::Deserialize;
use serde_json::{Map, Value};

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
