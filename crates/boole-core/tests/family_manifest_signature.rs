//! S20 — `FamilyManifest` schema hardening: optional `caps` block and
//! optional `signature` field, plus the `verify_family_manifest_signature`
//! helper that S22's promotion check consumes.
//!
//! Back-compat invariant: a manifest without `caps` or `signature` continues
//! to parse byte-equal to the pre-S20 shape (the existing
//! `fixtures/protocol/manifests/v1.json` regression in `manifest_fixtures.rs`
//! is the byte-for-byte witness for that).

use boole_core::{
    parse_family_manifest, verify_family_manifest_signature, FamilyManifestParseResult,
    SigningKeyV2,
};
use serde_json::{json, Value};

fn base_manifest_value() -> Value {
    json!({
        "version": "1",
        "familyId": "smart-contract-invariant-v01",
        "generatorHash": "abababababababababababababababababababababababababababababababab",
        "verifierHash": "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
        "canonicalizerHash": "efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef",
        "promptSpecHash": "0101010101010101010101010101010101010101010101010101010101010101",
        "calibrationReportHash": "2323232323232323232323232323232323232323232323232323232323232323",
        "testVectorsHash": "4545454545454545454545454545454545454545454545454545454545454545",
        "resourceLimits": { "maxProofBytes": 16384, "verifyTimeoutMs": 30000, "maxDecls": 1024 },
        "rewardPolicy": { "mode": "capped_bonus", "maxBlockRewardShareBps": 500 },
        "activationHeight": 123000,
        "status": "experimental"
    })
}

fn caps_value(max_shares: u64, mult_bps: u64, reward: &str) -> Value {
    json!({
        "maxSharesPerBlock": max_shares,
        "maxScoreMultiplierBps": mult_bps,
        "maxRewardCreditPerBlock": reward,
    })
}

#[test]
fn parses_manifest_without_caps_or_signature() {
    let v = base_manifest_value();
    let FamilyManifestParseResult::Ok(manifest) = parse_family_manifest(&v) else {
        panic!("expected ok");
    };
    assert!(manifest.caps.is_none());
    assert!(manifest.signature.is_none());
}

#[test]
fn parses_manifest_with_caps() {
    let mut v = base_manifest_value();
    v["caps"] = caps_value(10, 5000, "1000000");
    let FamilyManifestParseResult::Ok(manifest) = parse_family_manifest(&v) else {
        panic!("expected ok");
    };
    let caps = manifest.caps.expect("caps");
    assert_eq!(caps.max_shares_per_block, 10);
    assert_eq!(caps.max_score_multiplier_bps, 5000);
    assert_eq!(caps.max_reward_credit_per_block, "1000000");
}

#[test]
fn rejects_caps_with_non_u128_reward() {
    let mut v = base_manifest_value();
    v["caps"] = caps_value(10, 5000, "not-a-number");
    let FamilyManifestParseResult::Err(reason) = parse_family_manifest(&v) else {
        panic!("expected err");
    };
    assert_eq!(reason, "bad_caps:maxRewardCreditPerBlock");
}

#[test]
fn rejects_caps_with_oversized_score_multiplier() {
    let mut v = base_manifest_value();
    v["caps"] = caps_value(10, 100_001, "0");
    let FamilyManifestParseResult::Err(reason) = parse_family_manifest(&v) else {
        panic!("expected err");
    };
    assert_eq!(reason, "bad_caps:maxScoreMultiplierBps");
}

#[test]
fn rejects_caps_with_missing_field() {
    let mut v = base_manifest_value();
    v["caps"] = json!({"maxSharesPerBlock": 10, "maxScoreMultiplierBps": 5000});
    let FamilyManifestParseResult::Err(reason) = parse_family_manifest(&v) else {
        panic!("expected err");
    };
    assert_eq!(reason, "bad_caps:maxRewardCreditPerBlock");
}

#[test]
fn rejects_caps_when_not_an_object() {
    let mut v = base_manifest_value();
    v["caps"] = json!("oops");
    let FamilyManifestParseResult::Err(reason) = parse_family_manifest(&v) else {
        panic!("expected err");
    };
    assert_eq!(reason, "bad_caps");
}

#[test]
fn parses_manifest_with_signature_field() {
    let mut v = base_manifest_value();
    v["signature"] = json!(
        "1111111111111111111111111111111111111111111111111111111111111111\
         1111111111111111111111111111111111111111111111111111111111111111"
    );
    let FamilyManifestParseResult::Ok(manifest) = parse_family_manifest(&v) else {
        panic!("expected ok");
    };
    assert_eq!(manifest.signature.as_deref().map(str::len), Some(128));
}

#[test]
fn rejects_signature_with_bad_hex_length() {
    let mut v = base_manifest_value();
    v["signature"] = json!("deadbeef");
    let FamilyManifestParseResult::Err(reason) = parse_family_manifest(&v) else {
        panic!("expected err");
    };
    assert_eq!(reason, "bad_signature");
}

#[test]
fn manifest_serialization_skips_unset_optionals() {
    let v = base_manifest_value();
    let FamilyManifestParseResult::Ok(manifest) = parse_family_manifest(&v) else {
        panic!("parse");
    };
    let json_value = serde_json::to_value(&manifest).expect("serialize");
    let obj = json_value.as_object().expect("object");
    assert!(
        !obj.contains_key("caps"),
        "caps must not appear when unset (back-compat with manifests/v1.json)"
    );
    assert!(
        !obj.contains_key("signature"),
        "signature must not appear when unset (back-compat with manifests/v1.json)"
    );
}

#[test]
fn verify_returns_true_for_well_signed_manifest() {
    let key = SigningKeyV2::from_dev_id("s20-test-key");
    let mut manifest_value = base_manifest_value();
    manifest_value["caps"] = caps_value(10, 10_000, "100");
    let env = key.sign(&manifest_value).expect("sign");
    manifest_value["signature"] = json!(env.signature);

    let FamilyManifestParseResult::Ok(manifest) = parse_family_manifest(&manifest_value) else {
        panic!("parse");
    };
    let verified = verify_family_manifest_signature(&key.pk_hex(), &manifest).expect("verify ran");
    assert!(verified, "signature should verify against signing key");
}

#[test]
fn verify_returns_false_for_tampered_payload() {
    let key = SigningKeyV2::from_dev_id("s20-test-key-2");
    let mut manifest_value = base_manifest_value();
    manifest_value["caps"] = caps_value(10, 10_000, "100");
    let env = key.sign(&manifest_value).expect("sign");
    manifest_value["signature"] = json!(env.signature);
    manifest_value["caps"]["maxSharesPerBlock"] = json!(999);

    let FamilyManifestParseResult::Ok(manifest) = parse_family_manifest(&manifest_value) else {
        panic!("parse");
    };
    let verified = verify_family_manifest_signature(&key.pk_hex(), &manifest).expect("verify ran");
    assert!(!verified, "tampered manifest should fail verification");
}

#[test]
fn verify_errors_on_bad_pk_hex() {
    let key = SigningKeyV2::from_dev_id("s20-test-key-3");
    let mut manifest_value = base_manifest_value();
    let env = key.sign(&manifest_value).expect("sign");
    manifest_value["signature"] = json!(env.signature);
    let FamilyManifestParseResult::Ok(manifest) = parse_family_manifest(&manifest_value) else {
        panic!("parse");
    };
    let err = verify_family_manifest_signature("not-hex", &manifest).expect_err("bad pk");
    assert!(err.starts_with("bad_pk"), "got: {err}");
}

#[test]
fn verify_errors_when_unsigned() {
    let manifest_value = base_manifest_value();
    let FamilyManifestParseResult::Ok(manifest) = parse_family_manifest(&manifest_value) else {
        panic!("parse");
    };
    let err = verify_family_manifest_signature(&"0".repeat(64), &manifest).expect_err("unsigned");
    assert_eq!(err, "manifest_unsigned");
}
