use boole_core::BountyEventLedger;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    valid_events: Vec<Value>,
    expected: Expected,
    invalid_cases: Vec<InvalidCase>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    all: Vec<Value>,
    by_work_id_alpha: Vec<Value>,
    by_solver_pk1: Vec<Value>,
    by_verifier_lean: Vec<Value>,
    by_missing: Vec<Value>,
    size: usize,
}

#[derive(Debug, Deserialize)]
struct InvalidCase {
    name: String,
    event: Value,
    result: InvalidResult,
}

#[derive(Debug, Deserialize)]
struct InvalidResult {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

#[test]
fn bounty_ledger_matches_typescript_golden_fixture() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/bounty-ledger/v1.json"
    ))
    .expect("fixture parses");

    let mut ledger = BountyEventLedger::new();
    for event in &fixture.valid_events {
        ledger.append(event.clone()).expect("valid event appends");
    }

    assert_eq!(ledger.size(), fixture.expected.size);
    assert_eq!(ledger.get_all(), fixture.expected.all);
    assert_eq!(
        ledger.get_by_work_id("alpha-1"),
        fixture.expected.by_work_id_alpha
    );
    assert_eq!(
        ledger.get_by_solver_pk(&"11".repeat(32)),
        fixture.expected.by_solver_pk1
    );
    assert_eq!(
        ledger.get_by_verifier_kind("lean"),
        fixture.expected.by_verifier_lean
    );
    assert_eq!(
        ledger.get_by_work_id("missing"),
        fixture.expected.by_missing
    );

    for case in &fixture.invalid_cases {
        let mut ledger = BountyEventLedger::new();
        let got = ledger.append(case.event.clone());
        assert_eq!(got.is_ok(), case.result.ok, "{}", case.name);
        if !case.result.ok {
            assert_eq!(
                got.expect_err("expected invalid event"),
                case.result.error.clone().expect("expected error"),
                "{}",
                case.name
            );
        }
    }
}
