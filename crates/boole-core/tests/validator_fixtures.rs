use boole_core::{
    validate_proof_package, validate_proof_package_json, CalibrationReport, DecodeDetail,
    ValidationReason, ValidationResult,
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
    bytes_hex: String,
    cfg: CalibrationReport,
    expected: Value,
}

#[test]
fn validator_returns_typed_result_with_json_adapter() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/validator/v1.json"))
            .expect("fixture parses");

    let valid = fixture
        .cases
        .iter()
        .find(|case| case.name == "valid_empty")
        .expect("valid case");
    let valid_bytes = hex::decode(&valid.bytes_hex).expect("valid bytes hex");
    let valid_result = validate_proof_package(&valid_bytes, &valid.cfg);
    assert_eq!(
        valid_result,
        ValidationResult::Ok {
            decl_count: 0,
            size: 30,
            universe_arity: 0,
        }
    );
    assert_eq!(validate_proof_package_json(&valid_result), valid.expected);

    let bad_magic = fixture
        .cases
        .iter()
        .find(|case| case.name == "decode_bad_magic")
        .expect("bad magic case");
    let bad_magic_bytes = hex::decode(&bad_magic.bytes_hex).expect("bad magic bytes hex");
    let bad_magic_result = validate_proof_package(&bad_magic_bytes, &bad_magic.cfg);
    assert_eq!(
        bad_magic_result,
        ValidationResult::Err {
            reason: ValidationReason::Decode {
                detail: DecodeDetail::BadMagic,
            },
        }
    );
    assert_eq!(
        validate_proof_package_json(&bad_magic_result),
        bad_magic.expected
    );
}

#[test]
fn validator_matches_typescript_golden_fixture() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/validator/v1.json"))
            .expect("fixture parses");

    for case in &fixture.cases {
        let bytes = hex::decode(&case.bytes_hex).expect("case bytes hex");
        let got = validate_proof_package_json(&validate_proof_package(&bytes, &case.cfg));
        assert_eq!(got, case.expected, "{}", case.name);
    }
}
