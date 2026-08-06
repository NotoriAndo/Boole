//! Frozen `boole.canonical-json.v1` test vectors (EMITTER-CONTRACT-v1_2-final
//! §2). These pin the NEW spec exactly: key sorting, compact separators,
//! ensure_ascii escaping incl. non-BMP surrogate pairs, integers-only, and the
//! three hard-error cases (duplicate key, NaN, non-integer).

use boole_emitter::canonical_json::{to_canonical_bytes, CanonicalJsonError};

fn canon(input: &str) -> Vec<u8> {
    to_canonical_bytes(input).expect("input should canonicalize")
}

#[test]
fn sorts_object_keys() {
    assert_eq!(canon(r#"{"b":1,"a":2}"#), br#"{"a":2,"b":1}"#.to_vec());
}

#[test]
fn empty_object_and_array() {
    assert_eq!(canon("{}"), b"{}".to_vec());
    assert_eq!(canon("[]"), b"[]".to_vec());
}

#[test]
fn preserves_array_order() {
    assert_eq!(canon("[3,2,1]"), b"[3,2,1]".to_vec());
}

#[test]
fn escapes_bmp_non_ascii_lowercase() {
    // U+00E9 (é) -> the six ASCII bytes \u00e9 (ensure_ascii, lowercase hex).
    assert_eq!(
        canon("{\"k\":\"\u{00e9}\"}"),
        b"{\"k\":\"\\u00e9\"}".to_vec()
    );
}

#[test]
fn escapes_non_bmp_as_surrogate_pair() {
    // U+1F600 (😀) -> UTF-16 surrogate pair \ud83d\ude00 (lowercase hex).
    assert_eq!(
        canon("{\"k\":\"\u{1f600}\"}"),
        b"{\"k\":\"\\ud83d\\ude00\"}".to_vec()
    );
}

#[test]
fn sorts_mixed_literals() {
    assert_eq!(
        canon(r#"{"n":true,"m":false,"z":null}"#),
        br#"{"m":false,"n":true,"z":null}"#.to_vec()
    );
}

#[test]
fn negative_integer() {
    assert_eq!(canon(r#"{"n":-10}"#), br#"{"n":-10}"#.to_vec());
}

#[test]
fn strips_insignificant_whitespace_and_no_trailing_newline() {
    let out = canon("{ \"a\" : 1 , \"b\" : 2 }");
    assert_eq!(out, br#"{"a":1,"b":2}"#.to_vec());
    assert!(!out.ends_with(b"\n"));
}

#[test]
fn rejects_duplicate_key() {
    assert_eq!(
        to_canonical_bytes(r#"{"a":1,"a":2}"#),
        Err(CanonicalJsonError::DuplicateKey("a".to_string()))
    );
}

#[test]
fn rejects_nan() {
    // NaN is not valid strict JSON; allow_nan=false must reject it.
    assert!(to_canonical_bytes(r#"{"x":NaN}"#).is_err());
}

#[test]
fn rejects_non_integer() {
    assert_eq!(
        to_canonical_bytes(r#"{"x":1.5}"#),
        Err(CanonicalJsonError::NonIntegerNumber)
    );
}
