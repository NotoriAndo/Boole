//! BF.1a — closed protocol-owned useful-task registry admission (B3).
//!
//! Supply inlet for the useful-work lane. Scaffold-phase rules:
//! - Only three task sources are admissible: protocol-owned K-LADDER,
//!   a pinned real ZK release (https origin + immutable commit + source
//!   hash + license), or a strict-ready adapter card reference. The
//!   adapter reference is a digest only — no dependency on the BF.5-pre
//!   fixture import (anti-circularity, BF.1a conflict note).
//! - Only the single protocol authority can register (permissionless
//!   registration, bonds, challenges, governance = future ADR, post-BF.8).
//! - At epoch cutoff `freeze()` fixes the eligible list (sorted by
//!   task_id ascending — the published sort rule) and the registry root;
//!   late registration and re-freezing are typed rejections. The frozen
//!   list is BF.2's forced-assignment input (B2 precondition).
//!
//! Deliberately separate from the open bounty inbox — different type,
//! different path. Pure data only: no file IO, no HTTP.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::hash::{h_protocol, Hex32};
use crate::useful_work::{TaskKind, TaskSpecIdentity, UsefulWorkError};

const REGISTRY_ROOT_DOMAIN: &[u8] = b"boole.useful-work.registry-root.v0";
const VARIANT_FREE_KEY_DOMAIN: &[u8] = b"boole.useful-work.variant-free-key.v0";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RegistryError {
    #[error("registrar is not the protocol authority")]
    NotProtocolAuthority,
    #[error("registration is closed: the registry is frozen")]
    RegistrationClosed,
    #[error("registry is already frozen")]
    AlreadyFrozen,
    #[error("registry is not frozen yet")]
    RegistryNotFrozen,
    #[error("duplicate task identity")]
    DuplicateTask,
    #[error("variant re-registration binds the same source: no new supply")]
    MeaninglessVariant,
    #[error("unknown task source kind: {0}")]
    UnknownTaskSource(String),
    #[error("commit must be an immutable 40-hex sha")]
    InvalidCommit,
    #[error("invalid digest in field {field}")]
    InvalidDigest { field: &'static str },
    #[error("repo url must be an https origin")]
    InvalidRepoUrl,
    #[error("empty field {field}")]
    EmptyField { field: &'static str },
    #[error("eligible entries require strict-audited spec fidelity")]
    EligibleRequiresStrictAudit,
    #[error("registry entry json malformed: {0}")]
    MalformedJson(String),
    #[error(transparent)]
    Task(#[from] UsefulWorkError),
}

impl RegistryError {
    /// Stable machine-readable label — part of the fixture contract.
    pub fn label(&self) -> &'static str {
        match self {
            RegistryError::NotProtocolAuthority => "not-protocol-authority",
            RegistryError::RegistrationClosed => "registration-closed",
            RegistryError::AlreadyFrozen => "already-frozen",
            RegistryError::RegistryNotFrozen => "registry-not-frozen",
            RegistryError::DuplicateTask => "duplicate-task",
            RegistryError::MeaninglessVariant => "meaningless-variant",
            RegistryError::UnknownTaskSource(_) => "unknown-task-source",
            RegistryError::InvalidCommit => "invalid-commit",
            RegistryError::InvalidDigest { .. } => "invalid-digest",
            RegistryError::InvalidRepoUrl => "invalid-repo-url",
            RegistryError::EmptyField { .. } => "empty-field",
            RegistryError::EligibleRequiresStrictAudit => "eligible-requires-strict-audit",
            RegistryError::MalformedJson(_) => "malformed-json",
            RegistryError::Task(inner) => inner.label(),
        }
    }
}

/// The three admissible scaffold-phase task sources — a closed enum, so
/// "some other source" is structurally inexpressible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskSource {
    ProtocolKLadder {
        ladder_id: String,
        step: u32,
    },
    PinnedZkRelease {
        repo_url: String,
        commit: String,
        source_hash: Hex32,
        license: String,
    },
    StrictReadyAdapter {
        adapter_card_id: String,
        golden_digest: Hex32,
    },
}

impl TaskSource {
    /// The digest that identifies the underlying source bytes — used for
    /// the meaningless-variant check: a re-registration that differs only
    /// in `variant_id` but binds the same source adds no supply.
    fn binding_digest(&self) -> Vec<u8> {
        match self {
            TaskSource::ProtocolKLadder { ladder_id, step } => {
                let mut bytes = b"kladder:".to_vec();
                bytes.extend_from_slice(ladder_id.as_bytes());
                bytes.extend_from_slice(&step.to_le_bytes());
                bytes
            }
            TaskSource::PinnedZkRelease {
                commit,
                source_hash,
                ..
            } => {
                let mut bytes = b"release:".to_vec();
                bytes.extend_from_slice(commit.as_bytes());
                bytes.extend_from_slice(source_hash.as_bytes());
                bytes
            }
            TaskSource::StrictReadyAdapter { golden_digest, .. } => {
                let mut bytes = b"adapter:".to_vec();
                bytes.extend_from_slice(golden_digest.as_bytes());
                bytes
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SpecFidelity {
    StrictAudited,
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    pub task: TaskSpecIdentity,
    pub source: TaskSource,
    pub spec_fidelity: SpecFidelity,
    pub eligible: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RegistryEntryRaw {
    task: serde_json::Value,
    source: serde_json::Value,
    spec_fidelity: SpecFidelity,
    eligible: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct KLadderSourceRaw {
    #[allow(dead_code)]
    kind: String,
    ladder_id: String,
    step: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PinnedReleaseSourceRaw {
    #[allow(dead_code)]
    kind: String,
    repo_url: String,
    commit: String,
    source_hash: String,
    license: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AdapterSourceRaw {
    #[allow(dead_code)]
    kind: String,
    adapter_card_id: String,
    golden_digest: String,
}

fn non_empty(value: String, field: &'static str) -> Result<String, RegistryError> {
    if value.is_empty() {
        return Err(RegistryError::EmptyField { field });
    }
    Ok(value)
}

fn digest(value: &str, field: &'static str) -> Result<Hex32, RegistryError> {
    Hex32::from_hex(value).map_err(|_| RegistryError::InvalidDigest { field })
}

fn immutable_commit(value: &str) -> Result<String, RegistryError> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(RegistryError::InvalidCommit);
    }
    Ok(value.to_string())
}

fn parse_source(value: &serde_json::Value) -> Result<TaskSource, RegistryError> {
    let kind = value
        .get("kind")
        .and_then(|k| k.as_str())
        .ok_or_else(|| RegistryError::MalformedJson("source.kind missing".to_string()))?;
    match kind {
        "protocolKLadder" => {
            let raw: KLadderSourceRaw = serde_json::from_value(value.clone())
                .map_err(|err| RegistryError::MalformedJson(err.to_string()))?;
            Ok(TaskSource::ProtocolKLadder {
                ladder_id: non_empty(raw.ladder_id, "ladderId")?,
                step: raw.step,
            })
        }
        "pinnedZkRelease" => {
            let raw: PinnedReleaseSourceRaw = serde_json::from_value(value.clone())
                .map_err(|err| RegistryError::MalformedJson(err.to_string()))?;
            let repo_url = non_empty(raw.repo_url, "repoUrl")?;
            if !repo_url.starts_with("https://") {
                return Err(RegistryError::InvalidRepoUrl);
            }
            Ok(TaskSource::PinnedZkRelease {
                repo_url,
                commit: immutable_commit(&raw.commit)?,
                source_hash: digest(&raw.source_hash, "sourceHash")?,
                license: non_empty(raw.license, "license")?,
            })
        }
        "strictReadyAdapter" => {
            let raw: AdapterSourceRaw = serde_json::from_value(value.clone())
                .map_err(|err| RegistryError::MalformedJson(err.to_string()))?;
            Ok(TaskSource::StrictReadyAdapter {
                adapter_card_id: non_empty(raw.adapter_card_id, "adapterCardId")?,
                golden_digest: digest(&raw.golden_digest, "goldenDigest")?,
            })
        }
        other => Err(RegistryError::UnknownTaskSource(other.to_string())),
    }
}

impl RegistryEntry {
    pub fn from_json_value(value: &serde_json::Value) -> Result<Self, RegistryError> {
        let raw: RegistryEntryRaw = serde_json::from_value(value.clone())
            .map_err(|err| RegistryError::MalformedJson(err.to_string()))?;
        let entry = Self {
            task: TaskSpecIdentity::from_json_value(&raw.task)?,
            source: parse_source(&raw.source)?,
            spec_fidelity: raw.spec_fidelity,
            eligible: raw.eligible,
        };
        entry.validate()?;
        Ok(entry)
    }

    fn validate(&self) -> Result<(), RegistryError> {
        if self.eligible && self.spec_fidelity != SpecFidelity::StrictAudited {
            return Err(RegistryError::EligibleRequiresStrictAudit);
        }
        Ok(())
    }

    /// Variant-free identity: every task field except `variant_id`, plus
    /// the source binding digest. Two entries sharing this key differ only
    /// in the variant label — the meaningless-variant rejection.
    fn variant_free_key(&self) -> Hex32 {
        let mut bytes = Vec::new();
        push_field(&mut bytes, self.task.spec_id.as_bytes());
        push_field(&mut bytes, self.task.component_id.as_bytes());
        push_field(&mut bytes, self.task.property_id.as_bytes());
        push_field(&mut bytes, &self.task.spec_version.to_le_bytes());
        match &self.task.task_kind {
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
        push_field(&mut bytes, &self.source.binding_digest());
        h_protocol(VARIANT_FREE_KEY_DOMAIN, &[&bytes])
    }
}

fn push_field(out: &mut Vec<u8>, field: &[u8]) {
    out.extend_from_slice(&(field.len() as u64).to_le_bytes());
    out.extend_from_slice(field);
}

/// Closed protocol-owned registry with a freeze-at-cutoff lifecycle.
#[derive(Debug, Clone)]
pub struct UsefulTaskRegistry {
    authority: Hex32,
    entries: BTreeMap<Hex32, RegistryEntry>,
    variant_free_keys: BTreeMap<Hex32, Hex32>,
    frozen_root: Option<Hex32>,
}

impl UsefulTaskRegistry {
    pub fn new(authority: Hex32) -> Self {
        Self {
            authority,
            entries: BTreeMap::new(),
            variant_free_keys: BTreeMap::new(),
            frozen_root: None,
        }
    }

    pub fn register(
        &mut self,
        entry: RegistryEntry,
        registrar: Hex32,
    ) -> Result<(), RegistryError> {
        if registrar != self.authority {
            return Err(RegistryError::NotProtocolAuthority);
        }
        if self.frozen_root.is_some() {
            return Err(RegistryError::RegistrationClosed);
        }
        entry.validate()?;
        let task_id = entry.task.task_id();
        if self.entries.contains_key(&task_id) {
            return Err(RegistryError::DuplicateTask);
        }
        let variant_free = entry.variant_free_key();
        if self.variant_free_keys.contains_key(&variant_free) {
            return Err(RegistryError::MeaninglessVariant);
        }
        self.variant_free_keys.insert(variant_free, task_id);
        self.entries.insert(task_id, entry);
        Ok(())
    }

    /// Epoch cutoff: freeze the eligible list and its root. The sort rule
    /// is task_id ascending (the `BTreeMap` iteration order), so every
    /// node derives the identical list and root from the same entries.
    pub fn freeze(&mut self) -> Result<Hex32, RegistryError> {
        if self.frozen_root.is_some() {
            return Err(RegistryError::AlreadyFrozen);
        }
        let root = self.compute_root();
        self.frozen_root = Some(root);
        Ok(root)
    }

    /// BF.2's forced-assignment input: only available after the cutoff
    /// freeze, sorted by task_id ascending.
    pub fn eligible_tasks(&self) -> Result<Vec<TaskSpecIdentity>, RegistryError> {
        if self.frozen_root.is_none() {
            return Err(RegistryError::RegistryNotFrozen);
        }
        Ok(self
            .entries
            .values()
            .filter(|entry| entry.eligible)
            .map(|entry| entry.task.clone())
            .collect())
    }

    pub fn registry_root(&self) -> Result<Hex32, RegistryError> {
        self.frozen_root.ok_or(RegistryError::RegistryNotFrozen)
    }

    fn compute_root(&self) -> Hex32 {
        let mut bytes = Vec::new();
        for (task_id, entry) in &self.entries {
            if entry.eligible {
                push_field(&mut bytes, task_id.as_bytes());
            }
        }
        h_protocol(REGISTRY_ROOT_DOMAIN, &[&bytes])
    }
}
