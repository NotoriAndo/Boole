//! Shared validation helpers for signed release contracts.
//!
//! The native-shadow guest-update verifier and the curl product-release
//! verifier freeze the same low-level rules: canonical JSON envelopes, safe
//! ASCII identifiers, plain non-hidden file names and lowercase SHA-256
//! digests. This module holds the single implementation of those rules; each
//! verifier maps the returned reasons into its own error type without
//! changing any message text.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::canonicalize;

#[derive(Debug)]
pub(crate) enum ContractJsonError {
    Malformed(String),
    NonCanonical(String),
}

pub(crate) fn parse_canonical_json(
    name: &str,
    raw: &[u8],
    max_bytes: usize,
) -> Result<Value, ContractJsonError> {
    if raw.is_empty() || raw.len() > max_bytes {
        return Err(ContractJsonError::Malformed(format!(
            "{name} size is outside its allowed range"
        )));
    }
    let value: Value = serde_json::from_slice(raw)
        .map_err(|error| ContractJsonError::Malformed(error.to_string()))?;
    if canonicalize(&value) != raw {
        return Err(ContractJsonError::NonCanonical(name.to_string()));
    }
    Ok(value)
}

pub(crate) fn check_safe_identifier(name: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'+' | b'-'))
    {
        return Err(format!("{name} must be 1..64 safe ASCII characters"));
    }
    Ok(())
}

pub(crate) fn check_safe_file_name(value: &str) -> Result<(), String> {
    check_safe_identifier("fileName", value)?;
    if value == "." || value == ".." || value.starts_with('.') || value.contains("..") {
        return Err("fileName must be a plain non-hidden release-asset name".to_string());
    }
    Ok(())
}

pub(crate) fn check_sha256(name: &str, value: &str) -> Result<(), String> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(format!(
            "{name} must be 64 lowercase hexadecimal characters"
        ))
    }
}

#[derive(Debug)]
pub(crate) struct RequiredPreviousManifestSha256(pub(crate) Option<String>);

impl<'de> Deserialize<'de> for RequiredPreviousManifestSha256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Value::deserialize(deserializer)? {
            Value::Null => Ok(Self(None)),
            Value::String(value) => Ok(Self(Some(value))),
            _ => Err(D::Error::custom(
                "previousManifestSha256 must be null or a lowercase SHA-256 string",
            )),
        }
    }
}
