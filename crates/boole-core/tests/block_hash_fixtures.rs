use boole_core::{block_hash, Hex32};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    prev_c: String,
    share_hashes: Vec<String>,
    expected_c: String,
}

#[test]
fn block_hash_matches_typescript_golden_fixture() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/block-hash/v1.json"
    ))
    .expect("fixture parses");

    for case in fixture.cases {
        let prev_c = Hex32::from_hex(&case.prev_c).expect("prevC hex32");
        let share_hashes = case
            .share_hashes
            .iter()
            .map(|h| Hex32::from_hex(h).expect("share hash hex32"))
            .collect::<Vec<_>>();
        let got = block_hash(&prev_c, &share_hashes).to_hex();
        assert_eq!(got, case.expected_c, "case {}", case.name);
    }
}
