use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkManifest {
    pub version: String,
    pub work_id: String,
    pub source: String,
    pub family_id: String,
    pub problem_hash: String,
    pub verifier: WorkVerifier,
    pub reward: String,
    pub deadline: u64,
    pub status: String,
    pub retryable: bool,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkVerifier {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BountyFixture {
    pub id: String,
    pub domain: String,
    pub problem_hash: String,
    pub verifier: WorkVerifier,
    pub reward: String,
    pub deadline: u64,
    pub status: String,
    pub created_at: u64,
    pub updated_at: u64,
}

pub fn bounty_to_work_manifest(b: &BountyFixture) -> WorkManifest {
    WorkManifest {
        version: "1".to_string(),
        work_id: b.id.clone(),
        source: "bounty".to_string(),
        family_id: b.domain.clone(),
        problem_hash: b.problem_hash.clone(),
        verifier: b.verifier.clone(),
        reward: b.reward.clone(),
        deadline: b.deadline,
        status: b.status.clone(),
        retryable: b.status == "open",
        created_at: b.created_at,
        updated_at: b.updated_at,
    }
}
