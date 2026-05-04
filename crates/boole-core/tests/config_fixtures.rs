use boole_core::{
    calibration_thresholds, hex_to_biguint, validate_calibration_report, CalibrationReport,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    cases: Vec<ConfigCase>,
    hex_cases: Vec<HexCase>,
}

#[derive(Debug, Deserialize)]
struct ConfigCase {
    name: String,
    report: CalibrationReport,
    result: CaseResult,
}

#[derive(Debug, Deserialize)]
struct CaseResult {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HexCase {
    input: String,
    ok: bool,
    #[serde(default)]
    value: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[test]
fn calibration_thresholds_parse_all_thresholds_once() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/config/v1.json"))
            .expect("fixture parses");
    let valid = fixture
        .cases
        .iter()
        .find(|case| case.name == "valid")
        .expect("valid config case");

    let thresholds = calibration_thresholds(&valid.report).expect("thresholds parse");
    assert_eq!(
        thresholds.t_submit,
        hex_to_biguint(&valid.report.T_submit).unwrap()
    );
    assert_eq!(
        thresholds.t_share,
        hex_to_biguint(&valid.report.T_share).unwrap()
    );
    assert_eq!(
        thresholds.t_block,
        hex_to_biguint(&valid.report.T_block).unwrap()
    );
    assert_eq!(
        thresholds.t_ticket,
        hex_to_biguint(&valid.report.T_ticket).unwrap()
    );
}

#[test]
fn config_validation_matches_typescript_golden_fixture() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/config/v1.json"))
            .expect("fixture parses");

    for case in &fixture.cases {
        let got = validate_calibration_report(&case.report);
        assert_eq!(got.is_ok(), case.result.ok, "{}", case.name);
        if !case.result.ok {
            assert_eq!(
                got.expect_err("expected invalid report"),
                case.result.error.clone().expect("expected error"),
                "{}",
                case.name
            );
        }
    }

    for case in &fixture.hex_cases {
        let got = hex_to_biguint(&case.input).map(|v| v.to_string());
        assert_eq!(got.is_ok(), case.ok, "{}", case.input);
        match (got, case.ok) {
            (Ok(value), true) => assert_eq!(Some(value), case.value, "{}", case.input),
            (Err(error), false) => assert_eq!(Some(error), case.error.clone(), "{}", case.input),
            _ => unreachable!(),
        }
    }
}
