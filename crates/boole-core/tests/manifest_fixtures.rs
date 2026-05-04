use boole_core::{
    bounty_to_work_manifest, parse_family_manifest, BountyFixture, FamilyManifestParseResult,
    WorkManifest,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    family_cases: Vec<FamilyCase>,
    work_case: WorkCase,
}

#[derive(Debug, Deserialize)]
struct FamilyCase {
    input: Value,
    result: Value,
}

#[derive(Debug, Deserialize)]
struct WorkCase {
    bounty: BountyFixture,
    expected: WorkManifest,
}

#[test]
fn manifests_match_typescript_golden_fixture() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/manifests/v1.json"))
            .expect("fixture parses");

    for case in fixture.family_cases {
        let expected_ok = case
            .result
            .get("ok")
            .and_then(Value::as_bool)
            .expect("ok field");
        let got = parse_family_manifest(&case.input);
        match (expected_ok, got) {
            (true, FamilyManifestParseResult::Ok(manifest)) => {
                let expected_manifest = case.result.get("manifest").expect("manifest");
                assert_eq!(
                    serde_json::to_value(manifest).expect("manifest json"),
                    *expected_manifest
                );
            }
            (false, FamilyManifestParseResult::Err(reason)) => {
                let expected_reason = case
                    .result
                    .get("reason")
                    .and_then(Value::as_str)
                    .expect("reason");
                assert_eq!(reason, expected_reason);
            }
            (true, FamilyManifestParseResult::Err(reason)) => panic!("expected ok got {reason}"),
            (false, FamilyManifestParseResult::Ok(manifest)) => {
                panic!("expected err got {manifest:?}")
            }
        }
    }

    let work = bounty_to_work_manifest(&fixture.work_case.bounty);
    assert_eq!(work, fixture.work_case.expected);
}
