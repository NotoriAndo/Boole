use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::canonicalize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReceiptCommitmentInput {
    pub agent_pk: String,
    pub family_id: String,
    pub verifier_id: String,
    pub verifier_hash_version: String,
    pub artifact_hash: String,
    pub request_hash: String,
    pub result: String,
    pub fee_charged: String,
    pub reward_recipient: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReceiptCommitment {
    pub receipt_id: String,
    pub agent_pk: String,
    pub family_id: String,
    pub verifier_id: String,
    pub verifier_hash_version: String,
    pub artifact_hash: String,
    pub request_hash: String,
    pub result: String,
    pub fee_charged: String,
    pub reward_recipient: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x402_version: Option<String>,
}

impl ReceiptCommitment {
    pub fn new(input: ReceiptCommitmentInput) -> anyhow::Result<Self> {
        validate_hex32("agentPk", &input.agent_pk)?;
        validate_non_empty("familyId", &input.family_id)?;
        validate_non_empty("verifierId", &input.verifier_id)?;
        validate_non_empty("verifierHashVersion", &input.verifier_hash_version)?;
        validate_hex32("artifactHash", &input.artifact_hash)?;
        validate_hex32("requestHash", &input.request_hash)?;
        validate_non_empty("result", &input.result)?;
        validate_non_empty("feeCharged", &input.fee_charged)?;
        validate_hex32("rewardRecipient", &input.reward_recipient)?;

        let mut commitment = Self {
            receipt_id: String::new(),
            agent_pk: input.agent_pk,
            family_id: input.family_id,
            verifier_id: input.verifier_id,
            verifier_hash_version: input.verifier_hash_version,
            artifact_hash: input.artifact_hash,
            request_hash: input.request_hash,
            result: input.result,
            fee_charged: input.fee_charged,
            reward_recipient: input.reward_recipient,
            x402_version: None,
        };
        commitment.receipt_id = commitment.compute_id();
        Ok(commitment)
    }

    pub fn compute_id(&self) -> String {
        let mut preimage = Map::new();
        preimage.insert("agentPk".to_string(), json!(self.agent_pk));
        preimage.insert("familyId".to_string(), json!(self.family_id));
        preimage.insert("verifierId".to_string(), json!(self.verifier_id));
        preimage.insert(
            "verifierHashVersion".to_string(),
            json!(self.verifier_hash_version),
        );
        preimage.insert("artifactHash".to_string(), json!(self.artifact_hash));
        preimage.insert("requestHash".to_string(), json!(self.request_hash));
        preimage.insert("result".to_string(), json!(self.result));
        preimage.insert("feeCharged".to_string(), json!(self.fee_charged));
        preimage.insert("rewardRecipient".to_string(), json!(self.reward_recipient));
        if let Some(version) = self.x402_version.as_ref() {
            preimage.insert("x402Version".to_string(), json!(version));
        }
        hex::encode(Sha256::digest(canonicalize(&Value::Object(preimage))))
    }

    #[cfg(test)]
    pub fn test_fixture() -> Self {
        Self::new(ReceiptCommitmentInput {
            agent_pk: "1111111111111111111111111111111111111111111111111111111111111111"
                .to_string(),
            family_id: "v1-lenbound".to_string(),
            verifier_id: "lean-runner-v01".to_string(),
            verifier_hash_version: "v0".to_string(),
            artifact_hash: "2222222222222222222222222222222222222222222222222222222222222222"
                .to_string(),
            request_hash: "3333333333333333333333333333333333333333333333333333333333333333"
                .to_string(),
            result: "accepted".to_string(),
            fee_charged: "1".to_string(),
            reward_recipient: "4444444444444444444444444444444444444444444444444444444444444444"
                .to_string(),
        })
        .expect("test fixture is valid")
    }
}

fn validate_non_empty(field: &str, value: &str) -> anyhow::Result<()> {
    if value.is_empty() {
        anyhow::bail!("{field} must be non-empty");
    }
    Ok(())
}

fn validate_hex32(field: &str, value: &str) -> anyhow::Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        anyhow::bail!("{field} must be hex32 lowercase hex");
    }
    Ok(())
}
