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
fn rate_limiter_from_calibration_report_is_fallible() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/rate-limiter/v1.json"
    ))
    .expect("fixture parses");
    let mut cfg = fixture.cfg.clone();
    cfg.perIpRateLimitPer60s = 0;

    let err = RateLimiter::from_calibration_report(&cfg, fixture.window_ms)
        .expect_err("invalid policy should not construct limiter");
    assert_eq!(err, "perIpRateLimitPer60s must be > 0");
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
fn releasing_a_semantic_reservation_refunds_only_its_exact_rate_charge() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/rate-limiter/v1.json"
    ))
    .expect("fixture parses");
    let mut limiter = RateLimiter::new(fixture.cfg.clone(), fixture.window_ms);
    let now = 1_800_000_000_000;

    assert!(limiter.observe_ticket(
        &fixture.constants.pk,
        &fixture.constants.c,
        Some(&fixture.constants.n1)
    ));
    assert_eq!(
        limiter.check(
            now,
            &fixture.constants.ip,
            &fixture.constants.pk,
            &fixture.constants.c,
        ),
        RateLimitResult::Allowed,
    );
    assert_eq!(
        limiter.check(
            now + 1,
            &fixture.constants.ip,
            &fixture.constants.pk,
            &fixture.constants.c,
        ),
        RateLimitResult::Allowed,
    );

    assert!(
        !limiter.release_committed(
            now,
            &fixture.constants.ip,
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
            &fixture.constants.c,
        ),
        "a different public key must not consume the matching IP timestamp"
    );
    assert_eq!(
        limiter.peek(
            now + 2,
            &fixture.constants.ip,
            &fixture.constants.pk,
            &fixture.constants.c,
        ),
        RateLimitResult::Rejected {
            reason: RateLimitRejectReason::IpQuota,
        },
        "a failed release must leave both accounting dimensions charged"
    );

    assert!(limiter.release_committed(
        now,
        &fixture.constants.ip,
        &fixture.constants.pk,
        &fixture.constants.c,
    ));
    assert_eq!(
        limiter.peek(
            now + 2,
            &fixture.constants.ip,
            &fixture.constants.pk,
            &fixture.constants.c,
        ),
        RateLimitResult::Allowed,
        "one exact release must refund both the IP and per-key charge"
    );
    assert!(
        !limiter.release_committed(
            now,
            &fixture.constants.ip,
            &fixture.constants.pk,
            &fixture.constants.c,
        ),
        "the same reservation cannot refund twice"
    );
}

#[test]
fn rate_identity_state_is_bounded_and_scoped_to_the_current_head() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/rate-limiter/v1.json"
    ))
    .expect("fixture parses");
    let mut limiter = RateLimiter::new(fixture.cfg.clone(), fixture.window_ms);
    limiter.set_current_c(fixture.constants.c.clone());

    // More structurally observable identities than the semantic-reject LRU
    // may remember must not make any per-(pk,c) accounting map unbounded.
    for index in 0..4_097_u64 {
        let pk = format!("{index:064x}");
        let nonce = format!("{:064x}", index + 1);
        assert!(limiter.observe_ticket(&pk, &fixture.constants.c, Some(&nonce)));
    }
    assert!(
        limiter.tracked_identity_len() <= 4_096,
        "current-head rate identity state must have a hard memory bound"
    );

    let next_c = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    limiter.set_current_c(next_c);
    assert_eq!(
        limiter.tracked_identity_len(),
        0,
        "a canonical-head transition must prune old per-head identity state"
    );
    assert!(
        !limiter.has_observed_ticket(
            &fixture.constants.pk,
            &fixture.constants.c,
            &fixture.constants.n1,
        ),
        "old-head ticket authority must not survive a head transition"
    );
}

#[test]
fn source_ip_state_is_bounded_and_stale_keys_are_pruned() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/rate-limiter/v1.json"
    ))
    .expect("fixture parses");
    let mut limiter = RateLimiter::new(fixture.cfg.clone(), fixture.window_ms);
    limiter.set_current_c(fixture.constants.c.clone());
    let now = 1_800_000_000_000;

    for index in 0..4_096_u64 {
        let pk = format!("{index:064x}");
        let nonce = format!("{:064x}", index + 1);
        let ip = format!("2001:db8::{index:x}");
        assert!(limiter.observe_ticket(&pk, &fixture.constants.c, Some(&nonce)));
        assert_eq!(
            limiter.check(now, &ip, &pk, &fixture.constants.c),
            RateLimitResult::Allowed,
        );
    }
    let overflow_pk = format!("{:064x}", 4_095_u64);
    assert!(limiter.observe_ticket(
        &overflow_pk,
        &fixture.constants.c,
        Some("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"),
    ));
    assert_eq!(
        limiter.check(
            now,
            "2001:db8::overflow",
            &overflow_pk,
            &fixture.constants.c,
        ),
        RateLimitResult::Rejected {
            reason: RateLimitRejectReason::IpQuota,
        },
        "a fresh source must fail closed once the active source-key bound is full"
    );
    assert_eq!(limiter.tracked_source_ip_len(), 4_096);

    let later = now + fixture.window_ms + 1;
    assert_eq!(
        limiter.check(
            later,
            "2001:db8::after-window",
            &overflow_pk,
            &fixture.constants.c,
        ),
        RateLimitResult::Allowed,
        "expired source keys must not permanently close admission"
    );
    assert_eq!(
        limiter.tracked_source_ip_len(),
        1,
        "the first post-window commit must prune all stale source keys"
    );
}

#[test]
fn identity_lru_never_evicts_an_in_flight_rate_charge() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/rate-limiter/v1.json"
    ))
    .expect("fixture parses");
    let mut limiter = RateLimiter::new(fixture.cfg.clone(), fixture.window_ms);
    limiter.set_current_c(fixture.constants.c.clone());
    let now = 1_800_000_000_000;
    assert!(limiter.observe_ticket(
        &fixture.constants.pk,
        &fixture.constants.c,
        Some(&fixture.constants.n1),
    ));
    assert_eq!(
        limiter.check(
            now,
            &fixture.constants.ip,
            &fixture.constants.pk,
            &fixture.constants.c,
        ),
        RateLimitResult::Allowed,
    );

    // `/ticket` observation does not own the semantic-verifier permit. It may
    // race an in-flight checker, but must only evict ticket-only identities.
    for index in 0..4_096_u64 {
        let pk = format!("{index:064x}");
        let nonce = format!("{:064x}", index + 10);
        assert!(limiter.observe_ticket(&pk, &fixture.constants.c, Some(&nonce)));
    }
    assert_eq!(
        limiter.peek(
            now + 1,
            "198.51.100.250",
            &fixture.constants.pk,
            &fixture.constants.c,
        ),
        RateLimitResult::Allowed,
        "ticket-only LRU churn must preserve the charged identity until its owner finalizes"
    );
}

#[test]
fn an_evicted_ticket_only_identity_can_recover_from_its_exact_seen_ticket() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/rate-limiter/v1.json"
    ))
    .expect("fixture parses");
    let mut limiter = RateLimiter::new(fixture.cfg.clone(), fixture.window_ms);
    limiter.set_current_c(fixture.constants.c.clone());
    assert!(limiter.observe_ticket(
        &fixture.constants.pk,
        &fixture.constants.c,
        Some(&fixture.constants.n1),
    ));

    for index in 0..4_096_u64 {
        let pk = format!("{index:064x}");
        let nonce = format!("{:064x}", index + 20);
        assert!(limiter.observe_ticket(&pk, &fixture.constants.c, Some(&nonce)));
    }
    assert!(
        !limiter.has_observed_ticket(
            &fixture.constants.pk,
            &fixture.constants.c,
            &fixture.constants.n1,
        ),
        "fixture proves the ticket-only victim was evicted"
    );

    assert!(
        !limiter.observe_ticket(
            &fixture.constants.pk,
            &fixture.constants.c,
            Some(&fixture.constants.n1),
        ),
        "the global exact-ticket memory still identifies a duplicate observation"
    );
    assert!(
        limiter.has_observed_ticket(
            &fixture.constants.pk,
            &fixture.constants.c,
            &fixture.constants.n1,
        ),
        "re-observing exact evidence must restore bounded identity counters instead of poisoning the ticket"
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
