//! Canonical JSON serialization for signing primitives.
//!
//! Rules (RFC 8785-lite):
//! - Object keys are sorted lexicographically by raw bytes.
//! - Arrays preserve insertion order.
//! - Numbers and strings serialize via serde_json's default emit.
//! - No whitespace, no trailing newline.
//!
//! Full RFC 8785 (number normalization, escape minimization) is a future
//! parity hardening when on-chain commitment lands; today's scheme is
//! sufficient to defeat "object keys re-ordered by another implementation".

use std::collections::BTreeMap;

use serde_json::{Map, Value};

/// Returns the canonical-JSON byte serialization of `value`. Idempotent:
/// `canonicalize(canonicalize(x).parse()) == canonicalize(x)`.
pub fn canonicalize(value: &Value) -> Vec<u8> {
    let canonical = sort_keys(value);
    serde_json::to_vec(&canonical).expect("serde_json cannot fail on owned Value")
}

fn sort_keys(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            // BTreeMap sorts by raw byte ordering of the key string, which
            // matches the lexicographic-bytes contract.
            let sorted: BTreeMap<&str, Value> = map
                .iter()
                .map(|(k, v)| (k.as_str(), sort_keys(v)))
                .collect();
            let mut out = Map::with_capacity(sorted.len());
            for (k, v) in sorted {
                out.insert(k.to_string(), v);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(sort_keys).collect()),
        other => other.clone(),
    }
}
