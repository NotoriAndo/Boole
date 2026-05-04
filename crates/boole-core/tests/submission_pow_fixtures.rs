use boole_core::{check_submission_pow, CalibrationReport};
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
fn submission_pow_matches_typescript_golden_fixture() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/submission-pow/v1.json"
    ))
    .expect("fixture parses");

    for case in &fixture.cases {
        let got = check_submission_pow(
            &hex::decode(&case.input.c).expect("c hex"),
            &hex::decode(&case.input.pk).expect("pk hex"),
            &hex::decode(&case.input.nonce_s).expect("nonce hex"),
            &hex::decode(&case.input.canon_hash).expect("canon hash hex"),
            &case.cfg,
        );
        assert_eq!(got, case.expected, "{}", case.name);
    }
}
