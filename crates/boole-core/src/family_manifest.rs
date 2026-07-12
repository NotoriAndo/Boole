use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{signed_envelope::verify_signature, Hex32, Hex64};

const HEX32_FIELDS: &[&str] = &[
    "generatorHash",
    "verifierHash",
    "canonicalizerHash",
    "promptSpecHash",
    "calibrationReportHash",
    "testVectorsHash",
];

/// Per-family economic caps that bound any side-pool contribution to a block.
/// Set on signed manifests only — the parser validates ranges, the block
/// builder applies them, and the replay-divergence sweep audits them.
///
/// `max_score_multiplier_bps` is in basis points where 10_000 = 1.0× and
/// 100_000 = 10.0×; values above 100_000 are rejected to keep the bounty
/// lane from dominating share-score under any operator misconfiguration.
///
/// `max_reward_credit_per_block` is a `u128` decimal string because JSON
/// numbers can't represent the full u128 range — every consumer parses
/// via `parse::<u128>()` and the parser already validates that here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FamilyCaps {
    pub max_shares_per_block: u64,
    pub max_score_multiplier_bps: u64,
    pub max_reward_credit_per_block: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilyManifest {
    pub version: String,
    pub family_id: String,
    pub generator_hash: String,
    pub verifier_hash: String,
    pub canonicalizer_hash: String,
    pub prompt_spec_hash: String,
    pub calibration_report_hash: String,
    pub test_vectors_hash: String,
    pub resource_limits: FamilyResourceLimits,
    pub reward_policy: FamilyRewardPolicy,
    pub activation_height: u64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caps: Option<FamilyCaps>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilyResourceLimits {
    pub max_proof_bytes: u64,
    /// Containment metadata since ADR-0016 (a): wall-clock is an
    /// availability bound, never a verdict input. The verdict-committed
    /// budget is `max_heartbeats`/`max_rec_depth`.
    pub verify_timeout_ms: u64,
    pub max_decls: u64,
    /// ADR-0016 (b): the Lean step budget (`-D maxHeartbeats`) that IS the
    /// verification verdict for this family. Committed to consensus via
    /// the family manifest root (ADR-0015 (c)).
    pub max_heartbeats: u64,
    /// ADR-0016 (b-1): the recursion-depth budget (`-D maxRecDepth`),
    /// committed alongside `max_heartbeats` so both verdict counters have
    /// a consensus home.
    pub max_rec_depth: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilyRewardPolicy {
    pub mode: String,
    pub max_block_reward_share_bps: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FamilyManifestParseResult {
    Ok(Box<FamilyManifest>),
    Err(String),
}

pub fn parse_family_manifest(input: &Value) -> FamilyManifestParseResult {
    let Some(obj) = input.as_object() else {
        return FamilyManifestParseResult::Err("bad_json".to_string());
    };
    match obj.get("version") {
        None => return FamilyManifestParseResult::Err("missing_field:version".to_string()),
        Some(v) if v == "1" => {}
        Some(_) => return FamilyManifestParseResult::Err("bad_version".to_string()),
    }
    let Some(family_id) = obj
        .get("familyId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    else {
        return FamilyManifestParseResult::Err("missing_field:familyId".to_string());
    };
    for field in HEX32_FIELDS {
        let Some(v) = obj.get(*field) else {
            return FamilyManifestParseResult::Err(format!("missing_field:{field}"));
        };
        let Some(s) = v.as_str() else {
            return FamilyManifestParseResult::Err(format!("missing_field:{field}"));
        };
        if Hex32::from_hex(s).is_err() {
            return FamilyManifestParseResult::Err(format!("bad_hex32:{field}"));
        }
    }

    let Some(rl) = obj.get("resourceLimits").and_then(Value::as_object) else {
        return FamilyManifestParseResult::Err("missing_field:resourceLimits".to_string());
    };
    let Some(max_proof_bytes) = positive_u64(rl.get("maxProofBytes")) else {
        return FamilyManifestParseResult::Err("bad_resource_limit:maxProofBytes".to_string());
    };
    let Some(verify_timeout_ms) = positive_u64(rl.get("verifyTimeoutMs")) else {
        return FamilyManifestParseResult::Err("bad_resource_limit:verifyTimeoutMs".to_string());
    };
    let Some(max_decls) = positive_u64(rl.get("maxDecls")) else {
        return FamilyManifestParseResult::Err("bad_resource_limit:maxDecls".to_string());
    };
    let Some(max_heartbeats) = positive_u64(rl.get("maxHeartbeats")) else {
        return FamilyManifestParseResult::Err("bad_resource_limit:maxHeartbeats".to_string());
    };
    let Some(max_rec_depth) = positive_u64(rl.get("maxRecDepth")) else {
        return FamilyManifestParseResult::Err("bad_resource_limit:maxRecDepth".to_string());
    };

    let Some(rp) = obj.get("rewardPolicy").and_then(Value::as_object) else {
        return FamilyManifestParseResult::Err("missing_field:rewardPolicy".to_string());
    };
    let Some(mode) = rp.get("mode").and_then(Value::as_str) else {
        return FamilyManifestParseResult::Err("bad_mode".to_string());
    };
    if !matches!(mode, "no_protocol_reward" | "capped_bonus") {
        return FamilyManifestParseResult::Err("bad_mode".to_string());
    }
    let Some(max_block_reward_share_bps) = rp.get("maxBlockRewardShareBps").and_then(Value::as_u64)
    else {
        return FamilyManifestParseResult::Err("bad_bps".to_string());
    };
    if max_block_reward_share_bps > 10_000 {
        return FamilyManifestParseResult::Err("bad_bps".to_string());
    }
    if mode == "no_protocol_reward" && max_block_reward_share_bps != 0 {
        return FamilyManifestParseResult::Err("bad_bps".to_string());
    }

    let Some(activation_height) = obj.get("activationHeight").and_then(Value::as_u64) else {
        return FamilyManifestParseResult::Err("bad_activation_height".to_string());
    };
    let Some(status) = obj.get("status").and_then(Value::as_str) else {
        return FamilyManifestParseResult::Err("bad_status".to_string());
    };
    if !matches!(
        status,
        "draft" | "bounty-only" | "experimental" | "capped-official" | "official" | "deprecated"
    ) {
        return FamilyManifestParseResult::Err("bad_status".to_string());
    }

    let caps = match parse_caps(obj.get("caps")) {
        Ok(value) => value,
        Err(reason) => return FamilyManifestParseResult::Err(reason),
    };
    let signature = match parse_signature(obj.get("signature")) {
        Ok(value) => value,
        Err(reason) => return FamilyManifestParseResult::Err(reason),
    };

    let get = |field: &str| obj.get(field).and_then(Value::as_str).unwrap().to_string();
    FamilyManifestParseResult::Ok(Box::new(FamilyManifest {
        version: "1".to_string(),
        family_id: family_id.to_string(),
        generator_hash: get("generatorHash"),
        verifier_hash: get("verifierHash"),
        canonicalizer_hash: get("canonicalizerHash"),
        prompt_spec_hash: get("promptSpecHash"),
        calibration_report_hash: get("calibrationReportHash"),
        test_vectors_hash: get("testVectorsHash"),
        resource_limits: FamilyResourceLimits {
            max_proof_bytes,
            verify_timeout_ms,
            max_decls,
            max_heartbeats,
            max_rec_depth,
        },
        reward_policy: FamilyRewardPolicy {
            mode: mode.to_string(),
            max_block_reward_share_bps,
        },
        activation_height,
        status: status.to_string(),
        caps,
        signature,
    }))
}

/// Verify a `FamilyManifest`'s embedded `signature` over the manifest body
/// (the same manifest with `signature` cleared, then canonicalized as JSON).
///
/// Returns `Ok(true|false)` for "verification ran" — a `false` here is the
/// 200-`invalid` case that disqualifies the manifest from promotion. Wire-
/// malformed pk hex returns `Err("bad_pk:…")`. An unsigned manifest returns
/// `Err("manifest_unsigned")` so callers don't accidentally treat absence
/// as a passing verification.
pub fn verify_family_manifest_signature(
    pk_hex: &str,
    manifest: &FamilyManifest,
) -> Result<bool, String> {
    let Some(sig_hex) = manifest.signature.as_deref() else {
        return Err("manifest_unsigned".to_string());
    };
    let mut payload = manifest.clone();
    payload.signature = None;
    let payload_value = serde_json::to_value(&payload)
        .map_err(|err| format!("manifest_serialize_failed: {err}"))?;
    verify_signature(pk_hex, sig_hex, &payload_value)
}

fn parse_caps(value: Option<&Value>) -> Result<Option<FamilyCaps>, String> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let Some(obj) = raw.as_object() else {
        return Err("bad_caps".to_string());
    };
    let max_shares_per_block = obj
        .get("maxSharesPerBlock")
        .and_then(Value::as_u64)
        .ok_or_else(|| "bad_caps:maxSharesPerBlock".to_string())?;
    let max_score_multiplier_bps = obj
        .get("maxScoreMultiplierBps")
        .and_then(Value::as_u64)
        .ok_or_else(|| "bad_caps:maxScoreMultiplierBps".to_string())?;
    if max_score_multiplier_bps > 100_000 {
        return Err("bad_caps:maxScoreMultiplierBps".to_string());
    }
    let max_reward_credit_per_block = obj
        .get("maxRewardCreditPerBlock")
        .and_then(Value::as_str)
        .ok_or_else(|| "bad_caps:maxRewardCreditPerBlock".to_string())?;
    if max_reward_credit_per_block.parse::<u128>().is_err() {
        return Err("bad_caps:maxRewardCreditPerBlock".to_string());
    }
    Ok(Some(FamilyCaps {
        max_shares_per_block,
        max_score_multiplier_bps,
        max_reward_credit_per_block: max_reward_credit_per_block.to_string(),
    }))
}

fn parse_signature(value: Option<&Value>) -> Result<Option<String>, String> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let Some(s) = raw.as_str() else {
        return Err("bad_signature".to_string());
    };
    if Hex64::from_hex(s).is_err() {
        return Err("bad_signature".to_string());
    }
    Ok(Some(s.to_string()))
}

fn positive_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64).filter(|v| *v > 0)
}
