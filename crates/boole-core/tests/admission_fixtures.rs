use boole_core::{
    admit_submission_json, admit_submission_typed, AdmissionDecision, AdmissionDeps,
    AdmissionError, AdmissionStatus, CalibrationReport, DecodeDetail, RateLimiter, RejectionReason,
    SharePool, SharePoolRejectReason, ValidationReason,
};
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
fn admission_pipeline_matches_improved_fixture() {
    let fixture = load_fixture();
    let mut rate_limiter = RateLimiter::new(fixture.cfg.clone(), 60_000);
    let mut pool = SharePool::new(fixture.cfg.ShareCapPerPK_Block as usize);
    pool.set_current_c(fixture.constants.c.clone());

    for (idx, op) in fixture.operations.iter().enumerate() {
        let body = body_for(&fixture.constants, &op.body_patch);
        if op.observe_ticket {
            observe_from_body(&mut rate_limiter, &body);
        }
        let decision = admit_submission_typed(AdmissionDeps {
            cfg: &fixture.cfg,
            rate_limiter: &mut rate_limiter,
            pool: &mut pool,
            now: 1_800_000_000_000 + idx as i64,
            ip: &fixture.constants.ip,
            body: &body,
        });
        assert_eq!(admit_submission_json(&decision), op.expect, "{}", op.name);
    }
}

#[test]
fn admission_pool_rejection_does_not_consume_rate_quota() {
    let fixture = load_fixture();
    let mut cfg = fixture.cfg.clone();
    cfg.perIpRateLimitPer60s = 10;
    cfg.M = 1;
    let mut rate_limiter = RateLimiter::new(cfg.clone(), 60_000);
    let mut pool = SharePool::new(cfg.ShareCapPerPK_Block as usize);
    pool.set_current_c(fixture.constants.c.clone());

    // Legacy ticket observations preserve TypeScript compatibility: with no exact
    // tickets observed, any n for this (pk, c) is considered observed. Two tickets
    // make exactly two valid admission quota slots available for this pk/c.
    assert!(rate_limiter.observe_ticket(&fixture.constants.pk, &fixture.constants.c, None));
    assert!(rate_limiter.observe_ticket(&fixture.constants.pk, &fixture.constants.c, None));

    let first_body = body_for(&fixture.constants, &Map::new());
    let first = admit_submission_typed(AdmissionDeps {
        cfg: &cfg,
        rate_limiter: &mut rate_limiter,
        pool: &mut pool,
        now: 1_800_000_000_000,
        ip: &fixture.constants.ip,
        body: &first_body,
    });
    assert!(matches!(first, AdmissionDecision::Accepted { .. }));

    let duplicate = admit_submission_typed(AdmissionDeps {
        cfg: &cfg,
        rate_limiter: &mut rate_limiter,
        pool: &mut pool,
        now: 1_800_000_000_001,
        ip: &fixture.constants.ip,
        body: &first_body,
    });
    assert_eq!(
        duplicate,
        AdmissionDecision::Rejected {
            status: AdmissionStatus::UnprocessableEntity,
            error: AdmissionError::SharePool {
                reason: SharePoolRejectReason::Duplicate
            },
            rejection: RejectionReason::SharePool {
                detail: SharePoolRejectReason::Duplicate
            },
        }
    );

    let mut fresh_patch = Map::new();
    fresh_patch.insert(
        "n".to_string(),
        Value::String(
            "3434343434343434343434343434343434343434343434343434343434343434".to_string(),
        ),
    );
    fresh_patch.insert(
        "j".to_string(),
        Value::String(
            "1212121212121212121212121212121212121212121212121212121212121212".to_string(),
        ),
    );
    let fresh_body = body_for(&fixture.constants, &fresh_patch);
    let fresh = admit_submission_typed(AdmissionDeps {
        cfg: &cfg,
        rate_limiter: &mut rate_limiter,
        pool: &mut pool,
        now: 1_800_000_000_002,
        ip: &fixture.constants.ip,
        body: &fresh_body,
    });
    assert!(
        matches!(fresh, AdmissionDecision::Accepted { .. }),
        "share-pool duplicate rejection must not debit the second valid quota slot; got {fresh:?}"
    );
}

#[test]
fn admission_validator_rejection_uses_typed_validation_reason() {
    let fixture = load_fixture();
    let mut rate_limiter = RateLimiter::new(fixture.cfg.clone(), 60_000);
    let mut pool = SharePool::new(fixture.cfg.ShareCapPerPK_Block as usize);
    pool.set_current_c(fixture.constants.c.clone());

    let mut patch = Map::new();
    patch.insert("bytes".to_string(), Value::String("00".to_string()));
    let body = body_for(&fixture.constants, &patch);
    observe_from_body(&mut rate_limiter, &body);

    let decision = admit_submission_typed(AdmissionDeps {
        cfg: &fixture.cfg,
        rate_limiter: &mut rate_limiter,
        pool: &mut pool,
        now: 1_800_000_000_000,
        ip: &fixture.constants.ip,
        body: &body,
    });

    assert_eq!(
        decision,
        AdmissionDecision::Rejected {
            status: AdmissionStatus::UnprocessableEntity,
            error: AdmissionError::Validator {
                reason: ValidationReason::Decode {
                    detail: DecodeDetail::UnexpectedEof,
                },
            },
            rejection: RejectionReason::Validator {
                reason: ValidationReason::Decode {
                    detail: DecodeDetail::UnexpectedEof,
                },
            },
        }
    );
}

#[test]
fn admission_core_returns_typed_rejection_and_success() {
    let fixture = load_fixture();
    let mut rate_limiter = RateLimiter::new(fixture.cfg.clone(), 60_000);
    let mut pool = SharePool::new(fixture.cfg.ShareCapPerPK_Block as usize);
    pool.set_current_c(fixture.constants.c.clone());

    let missing_pk = body_for(&fixture.constants, &fixture.operations[0].body_patch);
    let decision = admit_submission_typed(AdmissionDeps {
        cfg: &fixture.cfg,
        rate_limiter: &mut rate_limiter,
        pool: &mut pool,
        now: 1_800_000_000_000,
        ip: &fixture.constants.ip,
        body: &missing_pk,
    });
    assert_eq!(
        decision,
        AdmissionDecision::Rejected {
            status: AdmissionStatus::BadRequest,
            error: AdmissionError::MissingField {
                field: "pk".to_string()
            },
            rejection: RejectionReason::BadRequest {
                field: "pk".to_string()
            },
        }
    );

    let valid_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .expect("valid op");
    let valid_body = body_for(&fixture.constants, &valid_op.body_patch);
    observe_from_body(&mut rate_limiter, &valid_body);
    let decision = admit_submission_typed(AdmissionDeps {
        cfg: &fixture.cfg,
        rate_limiter: &mut rate_limiter,
        pool: &mut pool,
        now: 1_800_000_000_002,
        ip: &fixture.constants.ip,
        body: &valid_body,
    });
    match decision {
        AdmissionDecision::Accepted { share_hash } => {
            assert_eq!(
                share_hash.to_hex(),
                "f6be734b86f7d5892da61a18add1a53f850cf642b21cecac0c85997f53f6377c"
            );
        }
        other => panic!("expected accepted, got {other:?}"),
    }
}

fn load_fixture() -> Fixture {
    serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
        .expect("fixture parses")
}

fn observe_from_body(rate_limiter: &mut RateLimiter, body: &Map<String, Value>) {
    assert!(rate_limiter.observe_ticket(
        body.get("pk").and_then(Value::as_str).expect("pk"),
        body.get("c").and_then(Value::as_str).expect("c"),
        body.get("n").and_then(Value::as_str),
    ));
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
