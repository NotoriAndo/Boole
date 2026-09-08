//! Separate development-only authorization. No signing key, default trust root,
//! issuance permission, or conversion to a frozen replay grant exists here.
use std::collections::BTreeMap;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{sha256_hex, validate_strict_json, TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES};

pub const SIGNING_DOMAIN: &[u8] = b"BOOLE-DEVELOPMENT-FRESH-ANSWER-CANARY-V1\0";
pub const REDELIVERY_SIGNING_DOMAIN: &[u8] = b"BOOLE-DEVELOPMENT-CANARY-REDELIVERY-V1\0";
const SCHEMA: &str = "boole.development.fresh-answer-canary.v1";

#[derive(Debug, Error)]
pub enum CanaryError {
    #[error("canary authorization rejected: {0}")]
    Rejected(&'static str),
    #[error("canary encoding: {0}")]
    Encoding(#[from] serde_json::Error),
    #[error("canary execution request: {0}")]
    Wire(#[from] crate::WireError),
    #[error("canary private state I/O: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(unix)]
mod budget;
#[cfg(unix)]
pub use budget::{CanaryBudget, CanaryBudgetRole, VerifiedCanaryExecutionAuthorization};

/// Public material identity, not authority. Exact comparison at verification
/// rejects additional fields as well as changed task/checker/policy values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CanaryBindings(BTreeMap<String, String>);

impl CanaryBindings {
    pub fn installed() -> Result<Self, CanaryError> {
        let replay: serde_json::Value =
            serde_json::from_slice(TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES)?;
        let mut values = BTreeMap::new();
        for (name, section, field) in [
            ("familyVersion", "task", "familyVersion"),
            ("templateId", "task", "templateId"),
            ("challengeSha256", "task", "challengeSha256"),
            ("taskDigest", "task", "sha256"),
            ("intakeVersion", "task", "intakeVersion"),
            ("anchorDigest", "anchor", "sha256"),
            ("registryVersion", "registry", "version"),
            ("registryDigest", "registry", "sha256"),
            ("checkerDigest", "checker", "sha256"),
            ("checkerArtifactHash", "checker", "artifactHash"),
            ("checkerPolicyDigest", "checker", "policySha256"),
            ("checkerReleaseDigest", "checker", "releaseManifestSha256"),
            ("executionPolicyDigest", "executionPolicy", "sha256"),
            ("toolchainDigest", "toolchainIdentity", "sha256"),
        ] {
            values.insert(
                name.to_string(),
                replay[section][field]
                    .as_str()
                    .ok_or(CanaryError::Rejected("compiled material binding"))?
                    .to_string(),
            );
        }
        Ok(Self(values))
    }

    pub fn get(&self, name: &str) -> &str {
        self.0.get(name).map(String::as_str).unwrap_or("")
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanaryGrant {
    schema: String,
    run_id: String,
    journal_id: String,
    epoch: u64,
    bindings: CanaryBindings,
    max_checker_executions: u8,
    max_redeliveries: u8,
    non_issuable: bool,
    activation_allowed: bool,
}

impl CanaryGrant {
    pub fn one_task(
        run_id: String,
        journal_id: String,
        epoch: u64,
        bindings: CanaryBindings,
    ) -> Self {
        Self {
            schema: SCHEMA.to_string(),
            run_id,
            journal_id,
            epoch,
            bindings,
            max_checker_executions: 1,
            max_redeliveries: 1,
            non_issuable: true,
            activation_allowed: false,
        }
    }
}

/// The caller must provision this key out of band, independently of the grant
/// and model request. No production/release trust-root type is accepted.
pub struct CanaryTrustRoot(VerifyingKey);

impl CanaryTrustRoot {
    pub fn new(public_key: [u8; 32]) -> Result<Self, CanaryError> {
        let key = VerifyingKey::from_bytes(&public_key)
            .map_err(|_| CanaryError::Rejected("public key"))?;
        if key.is_weak() {
            return Err(CanaryError::Rejected("weak public key"));
        }
        Ok(Self(key))
    }
}

#[derive(Debug)]
pub struct VerifiedCanaryGrant {
    grant: CanaryGrant,
    digest: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CanaryRedelivery {
    schema: String,
    grant_digest: String,
    candidate_digest: String,
    submission_digest: String,
}

impl CanaryRedelivery {
    pub fn for_candidate(grant: &VerifiedCanaryGrant, candidate: &str, submission: &str) -> Self {
        Self {
            schema: "boole.development.canary-redelivery.v1".to_string(),
            grant_digest: grant.digest().to_string(),
            candidate_digest: candidate.to_string(),
            submission_digest: submission.to_string(),
        }
    }
}

pub struct VerifiedCanaryRedelivery {
    recovery: CanaryRedelivery,
}

pub fn verify_redelivery(
    bytes: &[u8],
    signature: &[u8],
    root: &CanaryTrustRoot,
    grant: &VerifiedCanaryGrant,
) -> Result<VerifiedCanaryRedelivery, CanaryError> {
    if bytes.len() > 2048 {
        return Err(CanaryError::Rejected("redelivery size"));
    }
    validate_strict_json(bytes).map_err(|_| CanaryError::Rejected("redelivery strict JSON"))?;
    let signature =
        Signature::from_slice(signature).map_err(|_| CanaryError::Rejected("signature length"))?;
    root.0
        .verify_strict(&[REDELIVERY_SIGNING_DOMAIN, bytes].concat(), &signature)
        .map_err(|_| CanaryError::Rejected("redelivery operator signature"))?;
    let recovery: CanaryRedelivery = serde_json::from_slice(bytes)?;
    if recovery.schema != "boole.development.canary-redelivery.v1"
        || recovery.grant_digest != grant.digest()
        || !identifier(&recovery.candidate_digest)
        || !identifier(&recovery.submission_digest)
    {
        return Err(CanaryError::Rejected("redelivery binding"));
    }
    Ok(VerifiedCanaryRedelivery { recovery })
}

fn identifier(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        && value.bytes().any(|c| c != b'0')
}

pub fn verify_grant(
    bytes: &[u8],
    signature: &[u8],
    root: &CanaryTrustRoot,
    expected: &CanaryBindings,
) -> Result<VerifiedCanaryGrant, CanaryError> {
    if bytes.len() > 16_384 {
        return Err(CanaryError::Rejected("grant size"));
    }
    validate_strict_json(bytes).map_err(|_| CanaryError::Rejected("strict JSON"))?;
    let signature =
        Signature::from_slice(signature).map_err(|_| CanaryError::Rejected("signature length"))?;
    root.0
        .verify_strict(&[SIGNING_DOMAIN, bytes].concat(), &signature)
        .map_err(|_| CanaryError::Rejected("operator signature"))?;
    let grant: CanaryGrant = serde_json::from_slice(bytes)?;
    if grant.schema != SCHEMA
        || !identifier(&grant.run_id)
        || !identifier(&grant.journal_id)
        || grant.epoch < 4
        || grant.max_checker_executions != 1
        || grant.max_redeliveries != 1
        || !grant.non_issuable
        || grant.activation_allowed
        || &grant.bindings != expected
        || expected != &CanaryBindings::installed()?
    {
        return Err(CanaryError::Rejected("scope or installed material binding"));
    }
    Ok(VerifiedCanaryGrant {
        grant,
        // Bind the out-of-band signer as well as the exact signed bytes. Key
        // rotation cannot silently adopt an already provisioned run journal.
        digest: sha256_hex(&[SIGNING_DOMAIN, root.0.as_bytes(), bytes].concat()),
    })
}

impl VerifiedCanaryGrant {
    pub fn run_id(&self) -> &str {
        &self.grant.run_id
    }
    pub fn journal_id(&self) -> &str {
        &self.grant.journal_id
    }
    pub fn epoch(&self) -> u64 {
        self.grant.epoch
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn bindings(&self) -> &CanaryBindings {
        &self.grant.bindings
    }

    pub fn operation_id(&self) -> String {
        sha256_hex(
            &[
                SIGNING_DOMAIN,
                self.digest.as_bytes(),
                self.run_id().as_bytes(),
                self.journal_id().as_bytes(),
            ]
            .concat(),
        )
    }

    pub fn matches_task(&self, family: &str, template: &str, challenge: &str, epoch: u64) -> bool {
        let b = self.bindings();
        family == b.get("familyVersion")
            && template == b.get("templateId")
            && challenge == b.get("challengeSha256")
            && epoch == self.epoch()
    }

    /// This constructs data only. Execution requires the durable one-shot
    /// budget in addition to this verified signed grant.
    pub fn prepare_request(
        &self,
        nonce: &str,
        raw: &[u8],
        source: &[u8],
    ) -> Result<crate::ExecutionRequest, CanaryError> {
        let extracted = crate::closed_local_replay_grant::extract_replay_source(raw)
            .map_err(|_| CanaryError::Rejected("proof intake"))?;
        if extracted != Some(source) || source.is_empty() || source.contains(&0) {
            return Err(CanaryError::Rejected("source/raw-answer binding"));
        }
        let b = self.bindings();
        Ok(crate::ExecutionRequest::try_new(
            crate::ExecutionRequestFields {
                nonce_hex: nonce.to_string(),
                operation_id_hex: self.operation_id(),
                family_version: b.get("familyVersion").to_string(),
                template_id: b.get("templateId").to_string(),
                challenge_sha256: b.get("challengeSha256").to_string(),
                epoch: self.epoch(),
                raw_answer_base64: BASE64.encode(raw),
                submission_source_base64: BASE64.encode(source),
                submission_source_digest_hex: sha256_hex(source),
                candidate_digest_hex: sha256_hex(raw),
                submission_digest_hex: crate::submission_digest_hex(
                    b.get("familyVersion"),
                    b.get("templateId"),
                    b.get("challengeSha256"),
                    self.epoch(),
                    raw,
                )?,
                registry_version: b.get("registryVersion").to_string(),
                registry_digest_hex: b.get("registryDigest").to_string(),
                anchor_digest_hex: b.get("anchorDigest").to_string(),
                task_digest_hex: b.get("taskDigest").to_string(),
                checker_artifact_hash_hex: b.get("checkerArtifactHash").to_string(),
                checker_policy_digest_hex: b.get("checkerPolicyDigest").to_string(),
                checker_release_manifest_digest_hex: b.get("checkerReleaseDigest").to_string(),
                toolchain_identity_digest_hex: b.get("toolchainDigest").to_string(),
                execution_policy_digest_hex: b.get("executionPolicyDigest").to_string(),
                intake_version: b.get("intakeVersion").to_string(),
            },
        )?)
    }

    pub fn validate_request(&self, request: &crate::ExecutionRequest) -> Result<(), CanaryError> {
        let expected = self.prepare_request(
            request.nonce_hex(),
            &request.raw_answer()?,
            &request.submission_source()?,
        )?;
        if serde_json::to_vec(&expected)? != serde_json::to_vec(request)? {
            return Err(CanaryError::Rejected("request authority binding"));
        }
        Ok(())
    }
}
