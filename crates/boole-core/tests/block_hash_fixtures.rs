//! Golden vectors for the block-hash preimage v3 (ADR-0015 (a) / §SC reset
//! window).
//!
//! v1's fixture was a cross-language parity contract against the legacy
//! TypeScript `blockHash`; since v2 the preimage is defined by the Rust
//! implementation alone (the deliberate protocol-change carve-out
//! `docs/next-slice-golden-fixtures.md` anticipated), so these vectors are
//! self-golden: they pin the preimage's byte layout against accidental
//! drift — any change to the committed field set or encoding shows up as a
//! hash mismatch here before it silently forks a chain.
//!
//! To regenerate after a deliberate preimage change (a reset window):
//! `cargo test -p boole-core --test block_hash_fixtures -- --ignored`
//! rewrites the fixture's `expectedC` values in place; review the diff.

use boole_core::{block_hash, PersistedBlock};
use serde::Deserialize;

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/protocol/block-hash/v3.json"
);

#[derive(Debug, Deserialize)]
struct Fixture {
    domain: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    block: serde_json::Value,
    expected_c: String,
}

#[test]
fn block_hash_matches_v3_golden_fixture() {
    let fixture: Fixture =
        serde_json::from_str(&std::fs::read_to_string(FIXTURE_PATH).expect("fixture reads"))
            .expect("fixture parses");
    assert_eq!(fixture.domain, "block.v3");

    for case in fixture.cases {
        let mut block_value = case.block;
        block_value["c"] = serde_json::json!("");
        let block: PersistedBlock =
            serde_json::from_value(block_value).expect("fixture block parses");
        let got = block_hash(&block).to_hex();
        assert_eq!(got, case.expected_c, "case {}", case.name);
    }
}

/// Deliberate-regen helper (NOT part of the suite): recomputes every case's
/// `expectedC` from the current `block_hash` and rewrites the fixture file.
/// Run only when a reset window intentionally changes the preimage.
#[test]
#[ignore = "regen helper — rewrites the golden fixture from the current preimage"]
fn regen_block_hash_golden_fixture() {
    let raw = std::fs::read_to_string(FIXTURE_PATH).expect("fixture reads");
    let mut value: serde_json::Value = serde_json::from_str(&raw).expect("fixture parses");
    let cases = value["cases"].as_array_mut().expect("cases array");
    for case in cases {
        let mut block_value = case["block"].clone();
        block_value["c"] = serde_json::json!("");
        let block: PersistedBlock =
            serde_json::from_value(block_value).expect("fixture block parses");
        case["expectedC"] = serde_json::json!(block_hash(&block).to_hex());
    }
    let mut out = serde_json::to_string_pretty(&value).expect("serialize");
    out.push('\n');
    std::fs::write(FIXTURE_PATH, out).expect("fixture writes");
}
