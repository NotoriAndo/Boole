use boole_core::{
    calibration_policy, calibration_thresholds, hex_to_biguint, validate_calibration_report,
    CalibrationReport,
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
fn calibration_policy_types_numeric_runtime_fields() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/config/v1.json"))
            .expect("fixture parses");
    let valid = fixture
        .cases
        .iter()
        .find(|case| case.name == "valid")
        .expect("valid config case");

    let policy = calibration_policy(&valid.report).expect("policy parses");
    assert_eq!(
        policy.thresholds,
        calibration_thresholds(&valid.report).unwrap()
    );
    assert_eq!(policy.k_max, valid.report.K_max as usize);
    assert_eq!(policy.global_share_cap, valid.report.K_max as usize);
    assert_eq!(
        policy.share_cap_per_pk_block,
        valid.report.ShareCapPerPK_Block as usize
    );
    assert_eq!(policy.l, valid.report.L as usize);
    assert_eq!(policy.d_max, valid.report.D_max as usize);
    assert_eq!(policy.m, valid.report.M);
    assert_eq!(
        policy.per_ip_rate_limit_per_60s,
        valid.report.perIpRateLimitPer60s as usize
    );
    assert_eq!(policy.min_share_score_multiplier_nanos, 2_000_000_000);
}

#[test]
fn calibration_policy_uses_explicit_share_pool_global_cap_without_changing_k_max() {
    let report: CalibrationReport = serde_json::from_str(
        r#"{
          "T_submit": "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
          "T_share": "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
          "T_block": "0x00ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
          "T_ticket": "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
          "MinShareScoreMultiplier": 1,
          "K_max": 4,
          "ShareCapPerPK_Block": 2,
          "SharePoolGlobalCap": 100,
          "L": 5,
          "D_max": 8,
          "EMAWindow": 16,
          "M": 2,
          "perIpRateLimitPer60s": 10,
          "provenance": "global-cap-test"
        }"#,
    )
    .expect("report parses");

    let policy = calibration_policy(&report).expect("policy parses");

    assert_eq!(policy.k_max, 4);
    assert_eq!(policy.global_share_cap, 100);
}

#[test]
fn calibration_policy_converts_multiplier_to_fixed_point_nanos() {
    let report: CalibrationReport = serde_json::from_str(
        r#"{
          "T_submit": "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
          "T_share": "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
          "T_block": "0x00ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
          "T_ticket": "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
          "MinShareScoreMultiplier": 1.234567891,
          "K_max": 4,
          "ShareCapPerPK_Block": 2,
          "L": 5,
          "D_max": 8,
          "EMAWindow": 16,
          "M": 2,
          "perIpRateLimitPer60s": 10,
          "provenance": "fixed-point-test"
        }"#,
    )
    .expect("report parses");

    let policy = calibration_policy(&report).expect("policy parses");

    assert_eq!(policy.min_share_score_multiplier_nanos, 1_234_567_891);
}

#[test]
fn calibration_report_rejects_multiplier_precision_beyond_nanos() {
    let report: CalibrationReport = serde_json::from_str(
        r#"{
          "T_submit": "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
          "T_share": "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
          "T_block": "0x00ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
          "T_ticket": "0x0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
          "MinShareScoreMultiplier": 1.0000000001,
          "K_max": 4,
          "ShareCapPerPK_Block": 2,
          "L": 5,
          "D_max": 8,
          "EMAWindow": 16,
          "M": 2,
          "perIpRateLimitPer60s": 10,
          "provenance": "fixed-point-test"
        }"#,
    )
    .expect("report parses at JSON boundary");
    let err =
        calibration_policy(&report).expect_err("sub-nano multiplier precision must be rejected");

    assert!(
        err.contains("at most 9 decimal places"),
        "unexpected error: {err}"
    );
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
