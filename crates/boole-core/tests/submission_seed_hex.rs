//! N0.4b — the submission body carries an OPTIONAL `seedHex` so the node can
//! persist it on the block and later re-derive the share's canonical Lean
//! source (deep_verify_block / Path 2). It is not part of admission: a body
//! without `seedHex` still parses (backward compatibility with pre-N0.4b
//! miners and the existing submit-lean/bounty flows).

use boole_core::parse_submission_body;
use serde_json::{Map, Value};

fn base_body() -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("c".to_string(), Value::String("11".repeat(32)));
    body.insert("pk".to_string(), Value::String("22".repeat(32)));
    body.insert("n".to_string(), Value::String("33".repeat(32)));
    body.insert("j".to_string(), Value::String("44".repeat(32)));
    body.insert("nonceS".to_string(), Value::String("55".repeat(32)));
    body.insert("bytes".to_string(), Value::String("00".to_string()));
    body
}

#[test]
fn parse_carries_seed_hex_when_present() {
    let mut body = base_body();
    let seed = "66".repeat(32);
    body.insert("seedHex".to_string(), Value::String(seed.clone()));
    let parsed = parse_submission_body(&body).expect("valid body with seedHex parses");
    assert_eq!(parsed.seed_hex, seed);
}

#[test]
fn parse_defaults_seed_hex_empty_when_absent() {
    let body = base_body();
    let parsed = parse_submission_body(&body).expect("body without seedHex still parses");
    assert_eq!(
        parsed.seed_hex, "",
        "seedHex is optional — absence must not reject and defaults to empty"
    );
}
