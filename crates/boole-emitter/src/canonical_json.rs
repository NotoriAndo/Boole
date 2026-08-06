//! `boole.canonical-json.v1` — the NEW canonical JSON serialization defined by
//! the emitter contract (EMITTER-CONTRACT-v1_2-final §2). This is NOT a
//! transcription of any library default; it is a fresh specification with:
//!
//! 1. Value domain: objects, arrays, strings, integers, booleans, null.
//!    `allow_nan = false` — a float / NaN / Infinity is a hard error.
//! 2. Object keys MUST be unique — a duplicate key is a hard error.
//! 3. Keys serialized sorted ascending by Unicode scalar value (code point).
//! 4. Separators `,` and `:`, no spaces.
//! 5. `ensure_ascii`-style escaping: every scalar U+0080..U+FFFF as `\uXXXX`
//!    (lowercase); non-BMP (U+10000..U+10FFFF) as a UTF-16 surrogate pair. No
//!    Unicode normalization (no NFC/NFD).
//! 6. Integers base-10, no leading `+`/zeros; no exponent.
//! 7. Output UTF-8, no surrounding whitespace, no trailing newline.

use std::fmt;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::error::Category;
use thiserror::Error;

/// Errors surfaced while producing `boole.canonical-json.v1` bytes.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CanonicalJsonError {
    /// The input is not syntactically valid JSON (includes the extended
    /// `NaN`/`Infinity` literals, which strict JSON rejects).
    #[error("invalid JSON input: {0}")]
    Parse(String),
    /// An object contained the same key twice (duplicate-key rejection).
    #[error("duplicate object key: {0}")]
    DuplicateKey(String),
    /// A number was a float, NaN, Infinity, or an out-of-range integer
    /// (`allow_nan = false`; integers only).
    #[error("non-integer or non-finite number not permitted (allow_nan=false)")]
    NonIntegerNumber,
}

/// The closed value domain of `boole.canonical-json.v1`. Objects retain insertion
/// order and duplicate keys so the duplicate-key check can run as a second pass;
/// integers are held as `i128` so every JSON `i64`/`u64` fits without a float.
enum CanonValue {
    Null,
    Bool(bool),
    Int(i128),
    Str(String),
    Array(Vec<CanonValue>),
    Object(Vec<(String, CanonValue)>),
}

impl<'de> Deserialize<'de> for CanonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct CanonVisitor;

        impl<'de> Visitor<'de> for CanonVisitor {
            type Value = CanonValue;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON value with integer-only numbers")
            }

            fn visit_unit<E>(self) -> Result<CanonValue, E> {
                Ok(CanonValue::Null)
            }

            fn visit_bool<E>(self, v: bool) -> Result<CanonValue, E> {
                Ok(CanonValue::Bool(v))
            }

            fn visit_i64<E>(self, v: i64) -> Result<CanonValue, E> {
                Ok(CanonValue::Int(v as i128))
            }

            fn visit_u64<E>(self, v: u64) -> Result<CanonValue, E> {
                Ok(CanonValue::Int(v as i128))
            }

            fn visit_i128<E>(self, v: i128) -> Result<CanonValue, E> {
                Ok(CanonValue::Int(v))
            }

            fn visit_u128<E>(self, v: u128) -> Result<CanonValue, E>
            where
                E: de::Error,
            {
                i128::try_from(v)
                    .map(CanonValue::Int)
                    .map_err(|_| E::custom("integer out of range (allow_nan=false, integers only)"))
            }

            fn visit_f64<E>(self, _v: f64) -> Result<CanonValue, E>
            where
                E: de::Error,
            {
                // Floats/NaN/Infinity are rejected. The `custom` message keeps
                // this in serde_json's `Data` error category so the caller can
                // map it to `NonIntegerNumber`.
                Err(E::custom(
                    "non-integer number not permitted (allow_nan=false)",
                ))
            }

            fn visit_str<E>(self, v: &str) -> Result<CanonValue, E> {
                Ok(CanonValue::Str(v.to_owned()))
            }

            fn visit_string<E>(self, v: String) -> Result<CanonValue, E> {
                Ok(CanonValue::Str(v))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<CanonValue, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut items = Vec::new();
                while let Some(elem) = seq.next_element::<CanonValue>()? {
                    items.push(elem);
                }
                Ok(CanonValue::Array(items))
            }

            fn visit_map<A>(self, mut map: A) -> Result<CanonValue, A::Error>
            where
                A: MapAccess<'de>,
            {
                // Preserve every (key, value) pair verbatim, including
                // duplicates, so `check_no_duplicate_keys` can flag them.
                let mut entries = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, CanonValue>()? {
                    entries.push((k, v));
                }
                Ok(CanonValue::Object(entries))
            }
        }

        deserializer.deserialize_any(CanonVisitor)
    }
}

/// Serialize `input` (a raw JSON document) to `boole.canonical-json.v1` bytes.
pub fn to_canonical_bytes(input: &str) -> Result<Vec<u8>, CanonicalJsonError> {
    let value: CanonValue = serde_json::from_str(input).map_err(|e| match e.classify() {
        Category::Data => CanonicalJsonError::NonIntegerNumber,
        _ => CanonicalJsonError::Parse(e.to_string()),
    })?;
    check_no_duplicate_keys(&value)?;
    let mut out = String::new();
    write_value(&value, &mut out);
    Ok(out.into_bytes())
}

/// Reject any object that carries the same key twice, at any depth. Returns the
/// first duplicated key encountered in document order.
fn check_no_duplicate_keys(value: &CanonValue) -> Result<(), CanonicalJsonError> {
    match value {
        CanonValue::Object(entries) => {
            let mut seen = std::collections::BTreeSet::new();
            for (k, _) in entries {
                if !seen.insert(k.as_str()) {
                    return Err(CanonicalJsonError::DuplicateKey(k.clone()));
                }
            }
            for (_, v) in entries {
                check_no_duplicate_keys(v)?;
            }
            Ok(())
        }
        CanonValue::Array(items) => {
            for item in items {
                check_no_duplicate_keys(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Append the canonical serialization of `value` to `out`.
fn write_value(value: &CanonValue, out: &mut String) {
    match value {
        CanonValue::Null => out.push_str("null"),
        CanonValue::Bool(true) => out.push_str("true"),
        CanonValue::Bool(false) => out.push_str("false"),
        CanonValue::Int(i) => out.push_str(&i.to_string()),
        CanonValue::Str(s) => write_escaped_string(s, out),
        CanonValue::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        CanonValue::Object(entries) => {
            // Keys sorted ascending by Unicode scalar value. For well-formed
            // UTF-8 `str`, byte-lexicographic order equals code-point order.
            let mut sorted: Vec<&(String, CanonValue)> = entries.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            out.push('{');
            for (i, (k, v)) in sorted.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_escaped_string(k, out);
                out.push(':');
                write_value(v, out);
            }
            out.push('}');
        }
    }
}

/// Write `s` as a JSON string literal using `ensure_ascii` escaping: short
/// escapes for the standard controls, `\u00xx` for other C0 controls, literal
/// bytes for 0x20..=0x7F (except `"`/`\`), `\uxxxx` (lowercase) for
/// U+0080..U+FFFF, and a UTF-16 surrogate pair for non-BMP scalars.
fn write_escaped_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) <= 0x7f => out.push(c),
            c if (c as u32) <= 0xffff => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => {
                let cp = c as u32 - 0x1_0000;
                let high = 0xd800 + (cp >> 10);
                let low = 0xdc00 + (cp & 0x3ff);
                out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
            }
        }
    }
    out.push('"');
}
