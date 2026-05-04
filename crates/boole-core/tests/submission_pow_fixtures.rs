use boole_core::{
    calibration_policy, check_submission_pow, check_submission_pow_json,
    check_submission_pow_with_policy, CalibrationReport, SubmissionPowRejectReason,
    SubmissionPowResult,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    input: SubmissionPowInputFixture,
    cfg: CalibrationReport,
    expected: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmissionPowInputFixture {
    c: String,
    pk: String,
    nonce_s: String,
    canon_hash: String,
}

#[test]
fn submission_pow_returns_typed_result_with_json_adapter() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/submission-pow/v1.json"
    ))
    .expect("fixture parses");

    let accept = fixture
        .cases
        .iter()
        .find(|case| case.name == "accept_high_threshold")
        .expect("accept case");
    let accepted = check_submission_pow_case(accept);
    match &accepted {
        SubmissionPowResult::Ok { hash_int } => assert_eq!(
            hash_int.to_string(),
            "32673207698953422362934606298455525198702981768023058197793330300196327404279"
        ),
        other => panic!("expected submit pow accepted, got {other:?}"),
    }
    assert_eq!(check_submission_pow_json(&accepted), accept.expected);

    let reject = fixture
        .cases
        .iter()
        .find(|case| case.name == "reject_equal_hash")
        .expect("reject case");
    let rejected = check_submission_pow_case(reject);
    match &rejected {
        SubmissionPowResult::Err { reason, hash_int } => {
            assert_eq!(*reason, SubmissionPowRejectReason::AboveTSubmit);
            assert_eq!(
                hash_int.to_string(),
                "32673207698953422362934606298455525198702981768023058197793330300196327404279"
            );
        }
        other => panic!("expected submit pow rejected, got {other:?}"),
    }
    assert_eq!(check_submission_pow_json(&rejected), reject.expected);
}

#[test]
fn submission_pow_with_policy_uses_policy_thresholds() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/submission-pow/v1.json"
    ))
    .expect("fixture parses");
    let accept = fixture
        .cases
        .iter()
        .find(|case| case.name == "accept_high_threshold")
        .expect("accept case");
    let policy = calibration_policy(&accept.cfg).expect("policy parses");

    let got = check_submission_pow_with_policy(
        &hex::decode(&accept.input.c).expect("c hex"),
        &hex::decode(&accept.input.pk).expect("pk hex"),
        &hex::decode(&accept.input.nonce_s).expect("nonce hex"),
        &hex::decode(&accept.input.canon_hash).expect("canon hash hex"),
        &policy,
    );
    assert_eq!(check_submission_pow_json(&got), accept.expected);
}

#[test]
fn submission_pow_matches_typescript_golden_fixture() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/submission-pow/v1.json"
    ))
    .expect("fixture parses");

    for case in &fixture.cases {
        let got = check_submission_pow_json(&check_submission_pow_case(case));
        assert_eq!(got, case.expected, "{}", case.name);
    }
}

fn check_submission_pow_case(case: &Case) -> SubmissionPowResult {
    check_submission_pow(
        &hex::decode(&case.input.c).expect("c hex"),
        &hex::decode(&case.input.pk).expect("pk hex"),
        &hex::decode(&case.input.nonce_s).expect("nonce hex"),
        &hex::decode(&case.input.canon_hash).expect("canon hash hex"),
        &case.cfg,
    )
}
