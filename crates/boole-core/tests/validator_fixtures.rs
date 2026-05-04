use boole_core::{validate_proof_package, CalibrationReport};
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
    bytes_hex: String,
    cfg: CalibrationReport,
    expected: Value,
}

#[test]
fn validator_matches_typescript_golden_fixture() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/validator/v1.json"))
            .expect("fixture parses");

    for case in &fixture.cases {
        let bytes = hex::decode(&case.bytes_hex).expect("case bytes hex");
        let got = validate_proof_package(&bytes, &case.cfg);
        assert_eq!(got, case.expected, "{}", case.name);
    }
}
