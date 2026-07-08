//! Golden vectors for the block-hash preimage v2 (ADR-0014 (a) / N5-pre.1).
//!
//! v1's fixture was a cross-language parity contract against the legacy
//! TypeScript `blockHash`; the v2 preimage is defined by the Rust
//! implementation alone (the deliberate protocol-change carve-out
//! `docs/next-slice-golden-fixtures.md` anticipated), so these vectors are
//! self-golden: they pin the preimage's byte layout against accidental
//! drift — any change to the committed field set or encoding shows up as a
//! hash mismatch here before it silently forks a chain.

use boole_core::{block_hash, PersistedBlock};
use serde::Deserialize;

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
fn block_hash_matches_v2_golden_fixture() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/block-hash/v2.json"
    ))
    .expect("fixture parses");
    assert_eq!(fixture.domain, "block.v2");

    for case in fixture.cases {
        let mut block_value = case.block;
        block_value["c"] = serde_json::json!("");
        let block: PersistedBlock =
            serde_json::from_value(block_value).expect("fixture block parses");
        let got = block_hash(&block).to_hex();
        assert_eq!(got, case.expected_c, "case {}", case.name);
    }
}
