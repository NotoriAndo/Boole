//! BF.1 — useful-work task contract and pure state machine (B1 + C1 + C2).
//!
//! Two-identity split: `TaskSpecIdentity` is the problem's identity, fixed
//! before the epoch seed is revealed; `SubmissionIdentity` is the miner's
//! result identity. A build-new task cannot know its result digests before
//! mining, so `TaskKind` is a tagged enum: `BuildNew` carries no digests,
//! `AuditExisting` requires both. Reward-once dedup keys on `task_id`;
//! submission dedup keys on `artifact_root`/`submission_id`.
//!
//! Pure data + pure transitions only: no file IO, no HTTP, no verifier
//! processes (BF.1 non-goals). Deliberately separate from `WorkManifest`
//! (bounty lane) — converters come later, extension would break that API.
//! Nothing in the runtime reads these types yet; deleting this module
//! restores pre-BF behavior (BF.1 rollback contract).

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::hash::{h_protocol, Hex32};

/// Domain tags — task and submission identities must never collide even
/// on identical canonical bytes (C2 domain separation).
const TASK_ID_DOMAIN: &[u8] = b"boole.useful-work.task-id.v0";
const SUBMISSION_ID_DOMAIN: &[u8] = b"boole.useful-work.submission-id.v0";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UsefulWorkError {
    #[error("useful-work json malformed: {0}")]
    MalformedJson(String),
    #[error("unknown task kind: {0}")]
    UnknownTaskKind(String),
    #[error("buildNew tasks must not carry artifact digests")]
    BuildNewCarriesDigest,
    #[error("auditExisting tasks require both artifact digests")]
    AuditExistingMissingDigest,
    #[error("invalid digest in field {field}")]
    InvalidDigest { field: &'static str },
    #[error("empty identity field {field}")]
    EmptyField { field: &'static str },
}

impl UsefulWorkError {
    /// Stable machine-readable label — part of the fixture contract.
    pub fn label(&self) -> &'static str {
        match self {
            UsefulWorkError::MalformedJson(_) => "malformed-json",
            UsefulWorkError::UnknownTaskKind(_) => "unknown-task-kind",
            UsefulWorkError::BuildNewCarriesDigest => "build-new-carries-digest",
            UsefulWorkError::AuditExistingMissingDigest => "audit-existing-missing-digest",
            UsefulWorkError::InvalidDigest { .. } => "invalid-digest",
            UsefulWorkError::EmptyField { .. } => "empty-field",
        }
    }
}

/// C1 — tagged task kind. Optional-field combinations cannot express an
/// ambiguous identity: the tag decides which digests must exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskKind {
    BuildNew,
    AuditExisting {
        input_artifact_digest: Hex32,
        target_release_digest: Hex32,
    },
}

/// B1 — the problem's identity, frozen before the epoch seed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpecIdentity {
    pub spec_id: String,
    pub variant_id: String,
    pub component_id: String,
    pub property_id: String,
    pub spec_version: u32,
    pub task_kind: TaskKind,
}

/// B1/C2 — the miner's result identity. `source_root` names the packet's
/// source tree; `artifact_root` is the packet root computed WITHOUT any
/// identity fields (C2 anti-self-reference), so `submission_id` can hash
/// over it without circularity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmissionIdentity {
    pub task_id: Hex32,
    pub source_root: Hex32,
    pub implementation_digest: Hex32,
    pub release_digest: Hex32,
    pub artifact_root: Hex32,
    pub reward_pk: Hex32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TaskKindRaw {
    kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    input_artifact_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target_release_digest: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct TaskSpecIdentityRaw {
    spec_id: String,
    variant_id: String,
    component_id: String,
    property_id: String,
    spec_version: u32,
    task_kind: TaskKindRaw,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SubmissionIdentityRaw {
    task_id: String,
    source_root: String,
    implementation_digest: String,
    release_digest: String,
    artifact_root: String,
    reward_pk: String,
}

fn non_empty(value: String, field: &'static str) -> Result<String, UsefulWorkError> {
    if value.is_empty() {
        return Err(UsefulWorkError::EmptyField { field });
    }
    Ok(value)
}

fn digest(value: &str, field: &'static str) -> Result<Hex32, UsefulWorkError> {
    Hex32::from_hex(value).map_err(|_| UsefulWorkError::InvalidDigest { field })
}

/// Length-prefixed field framing: no two distinct field sequences share
/// canonical bytes (prevents concatenation-boundary collisions).
fn push_field(out: &mut Vec<u8>, field: &[u8]) {
    out.extend_from_slice(&(field.len() as u64).to_le_bytes());
    out.extend_from_slice(field);
}

impl TaskSpecIdentity {
    pub fn from_json_value(value: &serde_json::Value) -> Result<Self, UsefulWorkError> {
        let raw: TaskSpecIdentityRaw = serde_json::from_value(value.clone())
            .map_err(|err| UsefulWorkError::MalformedJson(err.to_string()))?;
        let task_kind = match raw.task_kind.kind.as_str() {
            "buildNew" => {
                if raw.task_kind.input_artifact_digest.is_some()
                    || raw.task_kind.target_release_digest.is_some()
                {
                    return Err(UsefulWorkError::BuildNewCarriesDigest);
                }
                TaskKind::BuildNew
            }
            "auditExisting" => {
                let (Some(input), Some(target)) = (
                    raw.task_kind.input_artifact_digest,
                    raw.task_kind.target_release_digest,
                ) else {
                    return Err(UsefulWorkError::AuditExistingMissingDigest);
                };
                TaskKind::AuditExisting {
                    input_artifact_digest: digest(&input, "inputArtifactDigest")?,
                    target_release_digest: digest(&target, "targetReleaseDigest")?,
                }
            }
            other => return Err(UsefulWorkError::UnknownTaskKind(other.to_string())),
        };
        Ok(Self {
            spec_id: non_empty(raw.spec_id, "specId")?,
            variant_id: non_empty(raw.variant_id, "variantId")?,
            component_id: non_empty(raw.component_id, "componentId")?,
            property_id: non_empty(raw.property_id, "propertyId")?,
            spec_version: raw.spec_version,
            task_kind,
        })
    }

    pub fn to_json_value(&self) -> serde_json::Value {
        let task_kind = match &self.task_kind {
            TaskKind::BuildNew => TaskKindRaw {
                kind: "buildNew".to_string(),
                input_artifact_digest: None,
                target_release_digest: None,
            },
            TaskKind::AuditExisting {
                input_artifact_digest,
                target_release_digest,
            } => TaskKindRaw {
                kind: "auditExisting".to_string(),
                input_artifact_digest: Some(input_artifact_digest.to_hex()),
                target_release_digest: Some(target_release_digest.to_hex()),
            },
        };
        serde_json::to_value(TaskSpecIdentityRaw {
            spec_id: self.spec_id.clone(),
            variant_id: self.variant_id.clone(),
            component_id: self.component_id.clone(),
            property_id: self.property_id.clone(),
            spec_version: self.spec_version,
            task_kind,
        })
        .expect("task spec identity serializes")
    }

    /// C1 — `task_id` is the domain-separated hash of the canonical
    /// (length-prefixed) identity bytes. Reward-once dedup keys on this.
    pub fn task_id(&self) -> Hex32 {
        let mut bytes = Vec::new();
        push_field(&mut bytes, self.spec_id.as_bytes());
        push_field(&mut bytes, self.variant_id.as_bytes());
        push_field(&mut bytes, self.component_id.as_bytes());
        push_field(&mut bytes, self.property_id.as_bytes());
        push_field(&mut bytes, &self.spec_version.to_le_bytes());
        match &self.task_kind {
            TaskKind::BuildNew => push_field(&mut bytes, b"buildNew"),
            TaskKind::AuditExisting {
                input_artifact_digest,
                target_release_digest,
            } => {
                push_field(&mut bytes, b"auditExisting");
                push_field(&mut bytes, input_artifact_digest.as_bytes());
                push_field(&mut bytes, target_release_digest.as_bytes());
            }
        }
        h_protocol(TASK_ID_DOMAIN, &[&bytes])
    }
}

impl SubmissionIdentity {
    pub fn from_json_value(value: &serde_json::Value) -> Result<Self, UsefulWorkError> {
        let raw: SubmissionIdentityRaw = serde_json::from_value(value.clone())
            .map_err(|err| UsefulWorkError::MalformedJson(err.to_string()))?;
        Ok(Self {
            task_id: digest(&raw.task_id, "taskId")?,
            source_root: digest(&raw.source_root, "sourceRoot")?,
            implementation_digest: digest(&raw.implementation_digest, "implementationDigest")?,
            release_digest: digest(&raw.release_digest, "releaseDigest")?,
            artifact_root: digest(&raw.artifact_root, "artifactRoot")?,
            reward_pk: digest(&raw.reward_pk, "rewardPk")?,
        })
    }

    pub fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(SubmissionIdentityRaw {
            task_id: self.task_id.to_hex(),
            source_root: self.source_root.to_hex(),
            implementation_digest: self.implementation_digest.to_hex(),
            release_digest: self.release_digest.to_hex(),
            artifact_root: self.artifact_root.to_hex(),
            reward_pk: self.reward_pk.to_hex(),
        })
        .expect("submission identity serializes")
    }

    /// C2 — `submission_id = H_submission(task_id || artifact_root ||
    /// implementation_digest || release_digest || reward_pk)`. The
    /// `source_root` is already covered by the packet bytes under
    /// `artifact_root`; identity fields are excluded from that root, so
    /// this derivation has no self-reference.
    pub fn submission_id(&self) -> Hex32 {
        let mut bytes = Vec::new();
        push_field(&mut bytes, self.task_id.as_bytes());
        push_field(&mut bytes, self.artifact_root.as_bytes());
        push_field(&mut bytes, self.implementation_digest.as_bytes());
        push_field(&mut bytes, self.release_digest.as_bytes());
        push_field(&mut bytes, self.reward_pk.as_bytes());
        h_protocol(SUBMISSION_ID_DOMAIN, &[&bytes])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskState {
    Registered,
    Assigned,
    Committed,
    Verified,
    Rewarded,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskEvent {
    /// Forced assignment to a prepaid ticket (BF.2).
    Assign,
    /// Result commitment lands before the reveal.
    Commit,
    /// Reveal accepted and verified by the node.
    Verify,
    /// Reward settled — at most once per task.
    Reward,
    /// Deadline passed before verification.
    Expire,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TaskTransitionError {
    #[error("invalid task transition: {from:?} on {event:?}")]
    InvalidTransition { from: TaskState, event: TaskEvent },
}

/// The single valid path is `Registered -> Assigned -> Committed ->
/// Verified -> Rewarded`; expiry is only reachable before verification.
/// Everything else — skips (`Registered` + `Reward`), duplicates
/// (`Verify` on `Verified`), post-expiry commits — is a typed rejection.
pub fn transition(state: TaskState, event: TaskEvent) -> Result<TaskState, TaskTransitionError> {
    use TaskEvent::*;
    use TaskState::*;
    match (state, event) {
        (Registered, Assign) => Ok(Assigned),
        (Assigned, Commit) => Ok(Committed),
        (Committed, Verify) => Ok(Verified),
        (Verified, Reward) => Ok(Rewarded),
        (Registered, Expire) | (Assigned, Expire) | (Committed, Expire) => Ok(Expired),
        (from, event) => Err(TaskTransitionError::InvalidTransition { from, event }),
    }
}
