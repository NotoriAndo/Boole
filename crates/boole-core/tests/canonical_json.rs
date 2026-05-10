//! S13a — Canonical JSON serialization for signing primitives.
//!
//! Canonicalization rules (RFC 8785-lite):
//! - Object keys are sorted lexicographically by raw bytes.
//! - Arrays preserve insertion order.
//! - Numbers / strings serialize via serde_json's default emit (no number
//!   normalization, no minimal-escape pass — full JCS is a future hardening
//!   when on-chain commitment lands).
//! - No whitespace, no trailing newline.

use boole_core::canonicalize;
use serde_json::json;

#[test]
fn object_keys_sort_alphabetically_regardless_of_input_order() {
    let unsorted = json!({"b": 2, "a": 1, "c": 3});
    let bytes = canonicalize(&unsorted);
    let text = std::str::from_utf8(&bytes).expect("utf8");
    assert_eq!(text, r#"{"a":1,"b":2,"c":3}"#);
}

#[test]
fn nested_objects_sort_recursively() {
    let nested = json!({
        "outer_b": {"inner_z": 1, "inner_a": 2},
        "outer_a": {"y": "yes", "x": "exit"},
    });
    let bytes = canonicalize(&nested);
    let text = std::str::from_utf8(&bytes).expect("utf8");
    assert_eq!(
        text,
        r#"{"outer_a":{"x":"exit","y":"yes"},"outer_b":{"inner_a":2,"inner_z":1}}"#,
    );
}

#[test]
fn arrays_preserve_insertion_order_not_sorted() {
    let value = json!({"items": [3, 1, 2, "first", "z"]});
    let bytes = canonicalize(&value);
    let text = std::str::from_utf8(&bytes).expect("utf8");
    assert_eq!(text, r#"{"items":[3,1,2,"first","z"]}"#);
}

#[test]
fn canonicalize_is_idempotent_across_round_trips() {
    let value = json!({
        "z": [{"b": 1, "a": 2}, {"d": 4, "c": 3}],
        "a": "leading",
        "m": null,
    });
    let first = canonicalize(&value);
    let reparsed: serde_json::Value =
        serde_json::from_slice(&first).expect("canonical bytes parse");
    let second = canonicalize(&reparsed);
    assert_eq!(
        first, second,
        "canonicalize(canonicalize(x)) must equal canonicalize(x)",
    );
}
