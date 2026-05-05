use serde::{Deserialize, Serialize};
use serde_json::Value;

const HEX32_FIELDS: &[&str] = &[
    "generatorHash",
    "verifierHash",
    "canonicalizerHash",
    "promptSpecHash",
    "calibrationReportHash",
    "testVectorsHash",
];

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilyResourceLimits {
    pub max_proof_bytes: u64,
    pub verify_timeout_ms: u64,
    pub max_decls: u64,
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
        if !is_lower_hex32(s) {
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
        },
        reward_policy: FamilyRewardPolicy {
            mode: mode.to_string(),
            max_block_reward_share_bps,
        },
        activation_height,
        status: status.to_string(),
    }))
}

fn is_lower_hex32(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn positive_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64).filter(|v| *v > 0)
}
