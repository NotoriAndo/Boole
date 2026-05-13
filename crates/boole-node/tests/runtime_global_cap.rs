use boole_core::{
    AdmissionDecision, AdmissionError, CalibrationReport, RejectionReason, SharePoolRejectReason,
};
use boole_node::{RuntimeAdmissionState, RuntimeConfig};
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
    valid_bytes_hex: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Operation {
    name: String,
    #[serde(default)]
    body_patch: Map<String, Value>,
}

#[test]
fn runtime_admission_rejects_before_candidate_vec_exceeds_global_cap() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    let mut report = fixture.cfg;
    report.K_max = 2;
    report.ShareCapPerPK_Block = 100;
    report.M = 100;

    let valid_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .expect("valid op");
    let mut runtime = RuntimeAdmissionState::new(
        RuntimeConfig::from_calibration_report(report, 60_000).expect("runtime config boots"),
    );
    runtime.set_current_c(fixture.constants.c.clone());

    for idx in 0..2 {
        let mut body = body_for(&fixture.constants, &valid_op.body_patch);
        body.insert(
            "pk".to_string(),
            Value::String(hex32_from_byte((idx + 1) as u8)),
        );
        body.insert(
            "n".to_string(),
            Value::String(hex32_from_byte((idx + 11) as u8)),
        );
        body.insert(
            "j".to_string(),
            Value::String(hex32_from_byte((idx + 21) as u8)),
        );
        runtime
            .observe_ticket_from_body(&body)
            .expect("observe ticket");
        let decision = runtime.admit_body(
            1_800_000_000_000 + idx as i64,
            &format!("198.51.100.{}", idx + 1),
            &body,
        );
        assert!(
            matches!(decision, AdmissionDecision::Accepted { .. }),
            "expected admission {idx} to be accepted: {decision:?}"
        );
    }

    let mut rejected_body = body_for(&fixture.constants, &valid_op.body_patch);
    rejected_body.insert("pk".to_string(), Value::String(hex32_from_byte(9)));
    rejected_body.insert("n".to_string(), Value::String(hex32_from_byte(19)));
    rejected_body.insert("j".to_string(), Value::String(hex32_from_byte(29)));
    runtime
        .observe_ticket_from_body(&rejected_body)
        .expect("observe rejected ticket");
    let rejected = runtime.admit_body(1_800_000_000_010, "198.51.100.3", &rejected_body);
    assert!(
        matches!(
            rejected,
            AdmissionDecision::Rejected {
                error: AdmissionError::SharePool {
                    reason: SharePoolRejectReason::GlobalCapExceeded
                },
                rejection: RejectionReason::SharePool {
                    detail: SharePoolRejectReason::GlobalCapExceeded
                },
                ..
            }
        ),
        "global cap admission should reject with global cap: {rejected:?}"
    );
    assert_eq!(runtime.pool_size(), 2);
    assert_eq!(runtime.candidate_shares_for_current_c().len(), 2);
}

#[test]
fn runtime_admission_uses_explicit_global_share_cap_separate_from_k_max() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    let mut report = fixture.cfg;
    report.K_max = 2;
    report.ShareCapPerPK_Block = 100;
    report.SharePoolGlobalCap = Some(100);
    report.M = 100;

    let valid_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .expect("valid op");
    let mut runtime = RuntimeAdmissionState::new(
        RuntimeConfig::from_calibration_report(report, 60_000).expect("runtime config boots"),
    );
    runtime.set_current_c(fixture.constants.c.clone());

    for idx in 0..3 {
        let mut body = body_for(&fixture.constants, &valid_op.body_patch);
        body.insert(
            "pk".to_string(),
            Value::String(hex32_from_byte((idx + 1) as u8)),
        );
        body.insert(
            "n".to_string(),
            Value::String(hex32_from_byte((idx + 11) as u8)),
        );
        body.insert(
            "j".to_string(),
            Value::String(hex32_from_byte((idx + 21) as u8)),
        );
        runtime
            .observe_ticket_from_body(&body)
            .expect("observe ticket");
        let decision = runtime.admit_body(
            1_800_000_000_000 + idx as i64,
            &format!("198.51.100.{}", idx + 1),
            &body,
        );
        assert!(
            matches!(decision, AdmissionDecision::Accepted { .. }),
            "explicit global cap should let admission {idx} exceed K_max: {decision:?}"
        );
    }
    assert_eq!(runtime.pool_size(), 3);
    assert_eq!(runtime.candidate_shares_for_current_c().len(), 3);
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

fn hex32_from_byte(byte: u8) -> String {
    format!("{:064x}", byte)
}
