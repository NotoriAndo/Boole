use boole_core::{
    calibration_policy, rate_limit_result_json, CalibrationReport, RateLimitRejectReason,
    RateLimitResult, RateLimiter,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    cfg: CalibrationReport,
    window_ms: i64,
    constants: Constants,
    operations: Vec<Operation>,
}

#[derive(Debug, Deserialize)]
struct Constants {
    pk: String,
    c: String,
    n1: String,
    n2: String,
    ip: String,
}

#[derive(Debug, Deserialize)]
struct Operation {
    name: String,
    now: i64,
    ok: bool,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

#[test]
fn rate_limiter_from_policy_uses_policy_quotas() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/rate-limiter/v1.json"
    ))
    .expect("fixture parses");
    let policy = calibration_policy(&fixture.cfg).expect("policy parses");
    let mut limiter = RateLimiter::from_policy(&policy, fixture.window_ms);

    assert_eq!(
        limiter.check(
            1_800_000_000_000,
            &fixture.constants.ip,
            &fixture.constants.pk,
            &fixture.constants.c
        ),
        RateLimitResult::Rejected {
            reason: RateLimitRejectReason::PkQuota,
        }
    );
    assert!(limiter.observe_ticket(
        &fixture.constants.pk,
        &fixture.constants.c,
        Some(&fixture.constants.n1)
    ));
    assert_eq!(
        limiter.check(
            1_800_000_000_001,
            &fixture.constants.ip,
            &fixture.constants.pk,
            &fixture.constants.c
        ),
        RateLimitResult::Allowed,
    );
}

#[test]
fn rate_limiter_returns_typed_result_with_json_adapter() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/rate-limiter/v1.json"
    ))
    .expect("fixture parses");
    let mut limiter = RateLimiter::new(fixture.cfg.clone(), fixture.window_ms);

    assert_eq!(
        limiter.peek(
            1_800_000_000_000,
            &fixture.constants.ip,
            &fixture.constants.pk,
            &fixture.constants.c
        ),
        RateLimitResult::Rejected {
            reason: RateLimitRejectReason::PkQuota,
        }
    );
    assert_eq!(
        rate_limit_result_json(&limiter.peek(
            1_800_000_000_000,
            &fixture.constants.ip,
            &fixture.constants.pk,
            &fixture.constants.c,
        )),
        serde_json::json!({ "allowed": false, "reason": "pk_quota" })
    );

    assert!(limiter.observe_ticket(
        &fixture.constants.pk,
        &fixture.constants.c,
        Some(&fixture.constants.n1)
    ));
    assert_eq!(
        limiter.peek(
            1_800_000_000_001,
            &fixture.constants.ip,
            &fixture.constants.pk,
            &fixture.constants.c
        ),
        RateLimitResult::Allowed,
    );
}

#[test]
fn rate_limiter_matches_typescript_golden_fixture() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/rate-limiter/v1.json"
    ))
    .expect("fixture parses");

    let mut limiter = RateLimiter::new(fixture.cfg.clone(), fixture.window_ms);

    for op in &fixture.operations {
        let got = apply_op(&mut limiter, &fixture.constants, op);
        match (op.ok, got) {
            (true, Ok(value)) => assert_eq!(
                value,
                op.result.clone().unwrap_or(Value::Null),
                "{}",
                op.name
            ),
            (false, Err(error)) => assert_eq!(Some(error), op.error.clone(), "{}", op.name),
            (true, Err(error)) => panic!("{} unexpectedly failed: {}", op.name, error),
            (false, Ok(value)) => panic!("{} unexpectedly succeeded: {}", op.name, value),
        }
    }
}

fn apply_op(
    limiter: &mut RateLimiter,
    constants: &Constants,
    op: &Operation,
) -> Result<Value, String> {
    let value = match op.name.as_str() {
        "check_no_ticket_pk_quota" => {
            limiter.check_json(op.now, &constants.ip, &constants.pk, &constants.c)
        }
        "observe_exact_n1" => {
            Value::Bool(limiter.observe_ticket(&constants.pk, &constants.c, Some(&constants.n1)))
        }
        "observe_exact_n1_replay" => {
            Value::Bool(limiter.observe_ticket(&constants.pk, &constants.c, Some(&constants.n1)))
        }
        "has_observed_n1" => {
            Value::Bool(limiter.has_observed_ticket(&constants.pk, &constants.c, &constants.n1))
        }
        "has_observed_n2_before" => {
            Value::Bool(limiter.has_observed_ticket(&constants.pk, &constants.c, &constants.n2))
        }
        "check_allowed_1" => limiter.check_json(op.now, &constants.ip, &constants.pk, &constants.c),
        "check_allowed_2" => limiter.check_json(op.now, &constants.ip, &constants.pk, &constants.c),
        "check_ip_quota_before_pk_quota" => {
            limiter.check_json(op.now, &constants.ip, &constants.pk, &constants.c)
        }
        "check_window_boundary_still_ip_quota" => {
            limiter.check_json(op.now, &constants.ip, &constants.pk, &constants.c)
        }
        "check_pk_quota_after_window" => {
            limiter.check_json(op.now, &constants.ip, &constants.pk, &constants.c)
        }
        "observe_exact_n2" => {
            Value::Bool(limiter.observe_ticket(&constants.pk, &constants.c, Some(&constants.n2)))
        }
        "has_observed_n2_after" => {
            Value::Bool(limiter.has_observed_ticket(&constants.pk, &constants.c, &constants.n2))
        }
        "check_allowed_after_second_ticket" => {
            limiter.check_json(op.now, &constants.ip, &constants.pk, &constants.c)
        }
        "reset" => {
            limiter.reset();
            Value::Null
        }
        "check_after_reset_no_ticket" => {
            limiter.check_json(op.now, &constants.ip, &constants.pk, &constants.c)
        }
        "observe_legacy" => Value::Bool(limiter.observe_ticket(&constants.pk, &constants.c, None)),
        "legacy_has_any_nonce" => {
            Value::Bool(limiter.has_observed_ticket(&constants.pk, &constants.c, &"ff".repeat(32)))
        }
        "legacy_check_allowed_1" => {
            limiter.check_json(op.now, &constants.ip, &constants.pk, &constants.c)
        }
        "legacy_check_allowed_2" => {
            limiter.check_json(op.now, "203.0.113.8", &constants.pk, &constants.c)
        }
        "legacy_check_pk_quota" => {
            limiter.check_json(op.now, "203.0.113.9", &constants.pk, &constants.c)
        }
        other => panic!("unknown op {other}"),
    };
    Ok(value)
}
