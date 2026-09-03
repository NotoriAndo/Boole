//! Offline recovery-authorized trust policy for product and guest releases.
//!
//! This module owns no private key. An out-of-band recovery role authenticates
//! the separate public roots consumed by the existing product and guest
//! release verifiers. Transport location and Apple identity grant no authority.

use std::collections::{BTreeMap, BTreeSet};

use ed25519_dalek::VerifyingKey;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::release_contract_util::{self, ContractJsonError};
use crate::{verify_signature, CurlProductReleaseTrustRoot, Hex32, NativeShadowUpdateTrustRoot};

pub const OPERATIONAL_RELEASE_RECOVERY_ROOT_SCHEMA: &str =
    "boole.operational-release-recovery-root.v1";
pub const OPERATIONAL_RELEASE_TRUST_POLICY_SCHEMA: &str =
    "boole.operational-release-trust-policy.v1";
pub const OPERATIONAL_RELEASE_TRUST_POLICY_SIGNATURES_SCHEMA: &str =
    "boole.operational-release-trust-policy-signatures.v1";
pub const OPERATIONAL_RELEASE_TRUST_POLICY_SIGNING_CONTEXT: &str =
    "boole-operational-release-trust-policy-v1";

pub const MAX_OPERATIONAL_RELEASE_RECOVERY_ROOT_BYTES: usize = 65_536;
pub const MAX_OPERATIONAL_RELEASE_TRUST_POLICY_BYTES: usize = 65_536;
pub const MAX_OPERATIONAL_RELEASE_TRUST_POLICY_SIGNATURES_BYTES: usize = 65_536;
const MAX_RECOVERY_KEYS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PublicKeyIdentity {
    key_id: String,
    public_key: Hex32,
}

impl PublicKeyIdentity {
    fn from_wire(value: PublicKeyWire) -> Result<Self, OperationalReleaseTrustPolicyError> {
        release_contract_util::check_safe_identifier("keyId", &value.key_id)
            .map_err(OperationalReleaseTrustPolicyError::Malformed)?;
        let public_key = Hex32::from_hex(&value.public_key).map_err(|_| {
            OperationalReleaseTrustPolicyError::Malformed(
                "publicKey must be 64 lowercase hexadecimal characters".to_string(),
            )
        })?;
        let verifying_key = VerifyingKey::from_bytes(public_key.as_bytes()).map_err(|_| {
            OperationalReleaseTrustPolicyError::Malformed(
                "publicKey is not a valid Ed25519 point".to_string(),
            )
        })?;
        if verifying_key.is_weak() {
            return Err(OperationalReleaseTrustPolicyError::Malformed(
                "publicKey must not be a weak Ed25519 point".to_string(),
            ));
        }
        Ok(Self {
            key_id: value.key_id,
            public_key,
        })
    }

    fn public_key_hex(&self) -> String {
        self.public_key.to_hex()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationalReleaseRecoveryRoot {
    threshold: usize,
    keys: BTreeMap<String, PublicKeyIdentity>,
}

impl OperationalReleaseRecoveryRoot {
    pub fn from_canonical_json(raw: &[u8]) -> Result<Self, OperationalReleaseTrustPolicyError> {
        let value = parse_canonical_json(
            "recovery root",
            raw,
            MAX_OPERATIONAL_RELEASE_RECOVERY_ROOT_BYTES,
        )?;
        let wire: RecoveryRootWire = serde_json::from_value(value)
            .map_err(|error| OperationalReleaseTrustPolicyError::Malformed(error.to_string()))?;
        if wire.schema != OPERATIONAL_RELEASE_RECOVERY_ROOT_SCHEMA {
            return Err(OperationalReleaseTrustPolicyError::Malformed(
                "recovery root schema mismatch".to_string(),
            ));
        }
        recovery_role(wire.threshold, wire.keys)
    }

    pub fn threshold(&self) -> usize {
        self.threshold
    }

    pub fn key_count(&self) -> usize {
        self.keys.len()
    }
}

#[derive(Debug, Clone)]
pub struct VerifiedOperationalReleaseTrustPolicy {
    generation: u64,
    policy_sha256: String,
    product_release: Option<CurlProductReleaseTrustRoot>,
    guest_release: Option<NativeShadowUpdateTrustRoot>,
    product_identity: Option<PublicKeyIdentity>,
    guest_identity: Option<PublicKeyIdentity>,
    recovery: OperationalReleaseRecoveryRoot,
    retired_keys: BTreeSet<RetiredKeyIdentity>,
}

impl VerifiedOperationalReleaseTrustPolicy {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn policy_sha256(&self) -> &str {
        &self.policy_sha256
    }

    pub fn product_release_trust_root(&self) -> Option<&CurlProductReleaseTrustRoot> {
        self.product_release.as_ref()
    }

    pub fn guest_release_trust_root(&self) -> Option<&NativeShadowUpdateTrustRoot> {
        self.guest_release.as_ref()
    }

    pub fn recovery_threshold(&self) -> usize {
        self.recovery.threshold
    }

    pub fn retired_key_count(&self) -> usize {
        self.retired_keys.len()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryRootWire {
    schema: String,
    threshold: usize,
    keys: Vec<PublicKeyWire>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublicKeyWire {
    key_id: String,
    public_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyWire {
    schema: String,
    generation: u64,
    previous_policy_sha256: Option<String>,
    product_release: OnlineRoleWire,
    guest_release: OnlineRoleWire,
    recovery: RecoveryRoleWire,
    retired_keys: Vec<RetiredKeyWire>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
enum OnlineRoleWire {
    Active {
        #[serde(rename = "keyId")]
        key_id: String,
        #[serde(rename = "publicKey")]
        public_key: String,
    },
    Disabled,
}

impl OnlineRoleWire {
    fn into_identity(
        self,
    ) -> Result<Option<PublicKeyIdentity>, OperationalReleaseTrustPolicyError> {
        match self {
            Self::Active { key_id, public_key } => {
                PublicKeyIdentity::from_wire(PublicKeyWire { key_id, public_key }).map(Some)
            }
            Self::Disabled => Ok(None),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryRoleWire {
    threshold: usize,
    keys: Vec<PublicKeyWire>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
enum RetiredKeyRole {
    ProductRelease,
    GuestRelease,
    Recovery,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RetiredKeyWire {
    role: RetiredKeyRole,
    key_id: String,
    public_key: String,
    retired_at_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RetiredKeyIdentity {
    role: RetiredKeyRole,
    identity: PublicKeyIdentity,
    retired_at_generation: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignaturesWire {
    schema: String,
    policy_sha256: String,
    signatures: Vec<SignatureWire>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignatureWire {
    key_id: String,
    public_key: String,
    signature: String,
}

pub fn verify_initial_operational_release_trust_policy(
    policy_raw: &[u8],
    signatures_raw: &[u8],
    trusted_recovery_root: &OperationalReleaseRecoveryRoot,
) -> Result<VerifiedOperationalReleaseTrustPolicy, OperationalReleaseTrustPolicyError> {
    let (policy, policy_sha256) = parse_policy(policy_raw)?;
    if policy.generation != 1 || policy.previous_policy_sha256.is_some() {
        return Err(OperationalReleaseTrustPolicyError::Continuity(
            "initial policy must be generation 1 with a null predecessor".to_string(),
        ));
    }
    if !policy.retired_keys.is_empty() {
        return Err(OperationalReleaseTrustPolicyError::Continuity(
            "initial policy must not contain retired keys".to_string(),
        ));
    }
    let product_identity = policy.product_release.into_identity()?;
    let guest_identity = policy.guest_release.into_identity()?;
    require_distinct_online_roles(product_identity.as_ref(), guest_identity.as_ref())?;
    let next_recovery = recovery_role(policy.recovery.threshold, policy.recovery.keys)?;
    if &next_recovery != trusted_recovery_root {
        return Err(OperationalReleaseTrustPolicyError::Continuity(
            "initial recovery role must exactly match the out-of-band recovery root".to_string(),
        ));
    }
    require_recovery_separation(
        product_identity.as_ref(),
        guest_identity.as_ref(),
        &next_recovery,
    )?;
    let signatures = parse_signatures(signatures_raw, &policy_sha256)?;
    verify_dual_recovery_threshold(
        &signatures,
        &policy_sha256,
        trusted_recovery_root,
        &next_recovery,
    )?;

    let product_release = product_identity
        .as_ref()
        .map(|identity| {
            CurlProductReleaseTrustRoot::new(&identity.key_id, &identity.public_key_hex())
        })
        .transpose()
        .map_err(|error| OperationalReleaseTrustPolicyError::Malformed(error.to_string()))?;
    let guest_release = guest_identity
        .as_ref()
        .map(|identity| {
            NativeShadowUpdateTrustRoot::new(&identity.key_id, &identity.public_key_hex())
        })
        .transpose()
        .map_err(|error| OperationalReleaseTrustPolicyError::Malformed(error.to_string()))?;

    Ok(VerifiedOperationalReleaseTrustPolicy {
        generation: 1,
        policy_sha256,
        product_release,
        guest_release,
        product_identity,
        guest_identity,
        recovery: next_recovery,
        retired_keys: BTreeSet::new(),
    })
}

pub fn verify_operational_release_trust_policy_successor(
    previous: &VerifiedOperationalReleaseTrustPolicy,
    policy_raw: &[u8],
    signatures_raw: &[u8],
) -> Result<VerifiedOperationalReleaseTrustPolicy, OperationalReleaseTrustPolicyError> {
    let (policy, policy_sha256) = parse_policy(policy_raw)?;
    if policy.generation != previous.generation.saturating_add(1) {
        return Err(OperationalReleaseTrustPolicyError::Continuity(
            "successor generation must advance by exactly one".to_string(),
        ));
    }
    if policy.previous_policy_sha256.as_deref() != Some(previous.policy_sha256()) {
        return Err(OperationalReleaseTrustPolicyError::Continuity(
            "successor must bind the exact previous policy digest".to_string(),
        ));
    }

    let product_identity = policy.product_release.into_identity()?;
    let guest_identity = policy.guest_release.into_identity()?;
    require_distinct_online_roles(product_identity.as_ref(), guest_identity.as_ref())?;
    let next_recovery = recovery_role(policy.recovery.threshold, policy.recovery.keys)?;
    require_shared_recovery_ids_unchanged(&previous.recovery, &next_recovery)?;
    require_recovery_separation(
        product_identity.as_ref(),
        guest_identity.as_ref(),
        &next_recovery,
    )?;

    let retired_keys = retired_key_set(policy.retired_keys, policy.generation)?;
    require_exact_retirement_history(
        previous,
        product_identity.as_ref(),
        guest_identity.as_ref(),
        &next_recovery,
        &retired_keys,
        policy.generation,
    )?;
    require_no_retired_key_is_active(
        product_identity.as_ref(),
        guest_identity.as_ref(),
        &next_recovery,
        &retired_keys,
    )?;

    let signatures = parse_signatures(signatures_raw, &policy_sha256)?;
    verify_dual_recovery_threshold(
        &signatures,
        &policy_sha256,
        &previous.recovery,
        &next_recovery,
    )?;

    let product_release = product_identity
        .as_ref()
        .map(|identity| {
            CurlProductReleaseTrustRoot::new(&identity.key_id, &identity.public_key_hex())
        })
        .transpose()
        .map_err(|error| OperationalReleaseTrustPolicyError::Malformed(error.to_string()))?;
    let guest_release = guest_identity
        .as_ref()
        .map(|identity| {
            NativeShadowUpdateTrustRoot::new(&identity.key_id, &identity.public_key_hex())
        })
        .transpose()
        .map_err(|error| OperationalReleaseTrustPolicyError::Malformed(error.to_string()))?;

    Ok(VerifiedOperationalReleaseTrustPolicy {
        generation: policy.generation,
        policy_sha256,
        product_release,
        guest_release,
        product_identity,
        guest_identity,
        recovery: next_recovery,
        retired_keys,
    })
}

fn parse_policy(raw: &[u8]) -> Result<(PolicyWire, String), OperationalReleaseTrustPolicyError> {
    let value = parse_canonical_json(
        "trust policy",
        raw,
        MAX_OPERATIONAL_RELEASE_TRUST_POLICY_BYTES,
    )?;
    let wire: PolicyWire = serde_json::from_value(value)
        .map_err(|error| OperationalReleaseTrustPolicyError::Malformed(error.to_string()))?;
    if wire.schema != OPERATIONAL_RELEASE_TRUST_POLICY_SCHEMA {
        return Err(OperationalReleaseTrustPolicyError::Malformed(
            "trust policy schema mismatch".to_string(),
        ));
    }
    Ok((wire, hex::encode(Sha256::digest(raw))))
}

fn parse_signatures(
    raw: &[u8],
    policy_sha256: &str,
) -> Result<Vec<SignatureWire>, OperationalReleaseTrustPolicyError> {
    let value = parse_canonical_json(
        "trust policy signatures",
        raw,
        MAX_OPERATIONAL_RELEASE_TRUST_POLICY_SIGNATURES_BYTES,
    )?;
    let wire: SignaturesWire = serde_json::from_value(value)
        .map_err(|error| OperationalReleaseTrustPolicyError::Malformed(error.to_string()))?;
    if wire.schema != OPERATIONAL_RELEASE_TRUST_POLICY_SIGNATURES_SCHEMA {
        return Err(OperationalReleaseTrustPolicyError::Malformed(
            "trust policy signatures schema mismatch".to_string(),
        ));
    }
    if wire.policy_sha256 != policy_sha256 {
        return Err(OperationalReleaseTrustPolicyError::Signature(
            "signature set is bound to another policy digest".to_string(),
        ));
    }
    if wire.signatures.is_empty() || wire.signatures.len() > MAX_RECOVERY_KEYS * 2 {
        return Err(OperationalReleaseTrustPolicyError::Malformed(
            "signature count is outside its allowed range".to_string(),
        ));
    }
    let mut previous = None;
    for signature in &wire.signatures {
        release_contract_util::check_safe_identifier("keyId", &signature.key_id)
            .map_err(OperationalReleaseTrustPolicyError::Malformed)?;
        if previous
            .as_ref()
            .is_some_and(|id: &String| id >= &signature.key_id)
        {
            return Err(OperationalReleaseTrustPolicyError::Malformed(
                "signatures must be strictly ordered by keyId".to_string(),
            ));
        }
        previous = Some(signature.key_id.clone());
    }
    Ok(wire.signatures)
}

fn recovery_role(
    threshold: usize,
    keys: Vec<PublicKeyWire>,
) -> Result<OperationalReleaseRecoveryRoot, OperationalReleaseTrustPolicyError> {
    if keys.len() != 3 || threshold != 2 || keys.len() > MAX_RECOVERY_KEYS {
        return Err(OperationalReleaseTrustPolicyError::Malformed(
            "recovery role must contain exactly three keys with threshold two".to_string(),
        ));
    }
    let mut identities = BTreeMap::new();
    let mut public_keys = BTreeSet::new();
    let mut previous = None;
    for wire in keys {
        if previous
            .as_ref()
            .is_some_and(|id: &String| id >= &wire.key_id)
        {
            return Err(OperationalReleaseTrustPolicyError::Malformed(
                "recovery keys must be strictly ordered by keyId".to_string(),
            ));
        }
        let identity = PublicKeyIdentity::from_wire(wire)?;
        if !public_keys.insert(identity.public_key) {
            return Err(OperationalReleaseTrustPolicyError::Malformed(
                "recovery public key is repeated".to_string(),
            ));
        }
        previous = Some(identity.key_id.clone());
        identities.insert(identity.key_id.clone(), identity);
    }
    Ok(OperationalReleaseRecoveryRoot {
        threshold,
        keys: identities,
    })
}

fn retired_key_set(
    values: Vec<RetiredKeyWire>,
    generation: u64,
) -> Result<BTreeSet<RetiredKeyIdentity>, OperationalReleaseTrustPolicyError> {
    let mut out = BTreeSet::new();
    let mut seen_ids = BTreeSet::new();
    let mut seen_public_keys = BTreeSet::new();
    let mut previous = None;
    for value in values {
        if value.retired_at_generation < 2 || value.retired_at_generation > generation {
            return Err(OperationalReleaseTrustPolicyError::Continuity(
                "retiredAtGeneration is outside the policy history".to_string(),
            ));
        }
        let retired = RetiredKeyIdentity {
            role: value.role,
            identity: PublicKeyIdentity::from_wire(PublicKeyWire {
                key_id: value.key_id,
                public_key: value.public_key,
            })?,
            retired_at_generation: value.retired_at_generation,
        };
        if previous.as_ref().is_some_and(|prior| prior >= &retired) {
            return Err(OperationalReleaseTrustPolicyError::Continuity(
                "retiredKeys must be strictly ordered".to_string(),
            ));
        }
        if !seen_ids.insert(retired.identity.key_id.clone())
            || !seen_public_keys.insert(retired.identity.public_key)
        {
            return Err(OperationalReleaseTrustPolicyError::Continuity(
                "retired key identity is repeated".to_string(),
            ));
        }
        previous = Some(retired.clone());
        out.insert(retired);
    }
    Ok(out)
}

fn require_shared_recovery_ids_unchanged(
    previous: &OperationalReleaseRecoveryRoot,
    next: &OperationalReleaseRecoveryRoot,
) -> Result<(), OperationalReleaseTrustPolicyError> {
    for (key_id, previous_key) in &previous.keys {
        if let Some(next_key) = next.keys.get(key_id) {
            if next_key.public_key != previous_key.public_key {
                return Err(OperationalReleaseTrustPolicyError::Continuity(
                    "a recovery keyId cannot change public key".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn require_exact_retirement_history(
    previous: &VerifiedOperationalReleaseTrustPolicy,
    next_product: Option<&PublicKeyIdentity>,
    next_guest: Option<&PublicKeyIdentity>,
    next_recovery: &OperationalReleaseRecoveryRoot,
    next_retired: &BTreeSet<RetiredKeyIdentity>,
    generation: u64,
) -> Result<(), OperationalReleaseTrustPolicyError> {
    let mut expected = previous.retired_keys.clone();
    if previous.product_identity.as_ref() != next_product {
        if let Some(identity) = &previous.product_identity {
            expected.insert(RetiredKeyIdentity {
                role: RetiredKeyRole::ProductRelease,
                identity: identity.clone(),
                retired_at_generation: generation,
            });
        }
    }
    if previous.guest_identity.as_ref() != next_guest {
        if let Some(identity) = &previous.guest_identity {
            expected.insert(RetiredKeyIdentity {
                role: RetiredKeyRole::GuestRelease,
                identity: identity.clone(),
                retired_at_generation: generation,
            });
        }
    }
    for (key_id, identity) in &previous.recovery.keys {
        if next_recovery.keys.get(key_id) != Some(identity) {
            expected.insert(RetiredKeyIdentity {
                role: RetiredKeyRole::Recovery,
                identity: identity.clone(),
                retired_at_generation: generation,
            });
        }
    }
    if &expected != next_retired {
        return Err(OperationalReleaseTrustPolicyError::Continuity(
            "retiredKeys must exactly preserve history and record every removed key".to_string(),
        ));
    }
    Ok(())
}

fn require_no_retired_key_is_active(
    product: Option<&PublicKeyIdentity>,
    guest: Option<&PublicKeyIdentity>,
    recovery: &OperationalReleaseRecoveryRoot,
    retired: &BTreeSet<RetiredKeyIdentity>,
) -> Result<(), OperationalReleaseTrustPolicyError> {
    let retired_ids: BTreeSet<&str> = retired
        .iter()
        .map(|entry| entry.identity.key_id.as_str())
        .collect();
    let retired_public_keys: BTreeSet<Hex32> = retired
        .iter()
        .map(|entry| entry.identity.public_key)
        .collect();
    for active in [product, guest]
        .into_iter()
        .flatten()
        .chain(recovery.keys.values())
    {
        if retired_ids.contains(active.key_id.as_str())
            || retired_public_keys.contains(&active.public_key)
        {
            return Err(OperationalReleaseTrustPolicyError::Continuity(
                "a retired key cannot become active again".to_string(),
            ));
        }
    }
    Ok(())
}

fn require_distinct_online_roles(
    product: Option<&PublicKeyIdentity>,
    guest: Option<&PublicKeyIdentity>,
) -> Result<(), OperationalReleaseTrustPolicyError> {
    if let (Some(product), Some(guest)) = (product, guest) {
        if product.key_id == guest.key_id || product.public_key == guest.public_key {
            return Err(OperationalReleaseTrustPolicyError::RoleSeparation(
                "product and guest release roles must use distinct keys".to_string(),
            ));
        }
    }
    Ok(())
}

fn require_recovery_separation(
    product: Option<&PublicKeyIdentity>,
    guest: Option<&PublicKeyIdentity>,
    recovery: &OperationalReleaseRecoveryRoot,
) -> Result<(), OperationalReleaseTrustPolicyError> {
    for online in [product, guest].into_iter().flatten() {
        if recovery
            .keys
            .values()
            .any(|root| root.key_id == online.key_id || root.public_key == online.public_key)
        {
            return Err(OperationalReleaseTrustPolicyError::RoleSeparation(
                "online release keys must be distinct from recovery keys".to_string(),
            ));
        }
    }
    Ok(())
}

fn verify_dual_recovery_threshold(
    signatures: &[SignatureWire],
    policy_sha256: &str,
    previous: &OperationalReleaseRecoveryRoot,
    next: &OperationalReleaseRecoveryRoot,
) -> Result<(), OperationalReleaseTrustPolicyError> {
    let payload = json!({
        "context": OPERATIONAL_RELEASE_TRUST_POLICY_SIGNING_CONTEXT,
        "policySha256": policy_sha256
    });
    let mut previous_valid = BTreeSet::new();
    let mut next_valid = BTreeSet::new();
    for signature in signatures {
        let previous_key = previous.keys.get(&signature.key_id);
        let next_key = next.keys.get(&signature.key_id);
        if previous_key.is_none() && next_key.is_none() {
            return Err(OperationalReleaseTrustPolicyError::Signature(
                "signature is from a key outside both recovery roles".to_string(),
            ));
        }
        let expected = previous_key.or(next_key).expect("checked recovery key");
        if signature.public_key != expected.public_key_hex() {
            return Err(OperationalReleaseTrustPolicyError::Signature(
                "signature public key does not match its recovery keyId".to_string(),
            ));
        }
        let valid = verify_signature(&signature.public_key, &signature.signature, &payload)
            .map_err(OperationalReleaseTrustPolicyError::Malformed)?;
        if !valid {
            return Err(OperationalReleaseTrustPolicyError::Signature(
                "recovery signature is invalid".to_string(),
            ));
        }
        if previous_key.is_some() {
            previous_valid.insert(signature.key_id.clone());
        }
        if next_key.is_some() {
            next_valid.insert(signature.key_id.clone());
        }
    }
    if previous_valid.len() < previous.threshold {
        return Err(OperationalReleaseTrustPolicyError::Signature(
            "previous recovery threshold is not satisfied".to_string(),
        ));
    }
    if next_valid.len() < next.threshold {
        return Err(OperationalReleaseTrustPolicyError::Signature(
            "next recovery threshold is not satisfied".to_string(),
        ));
    }
    Ok(())
}

fn parse_canonical_json(
    name: &str,
    raw: &[u8],
    max_bytes: usize,
) -> Result<Value, OperationalReleaseTrustPolicyError> {
    release_contract_util::parse_canonical_json(name, raw, max_bytes).map_err(|error| match error {
        ContractJsonError::Malformed(reason) => {
            OperationalReleaseTrustPolicyError::Malformed(reason)
        }
        ContractJsonError::NonCanonical(name) => {
            OperationalReleaseTrustPolicyError::NonCanonical(name)
        }
    })
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OperationalReleaseTrustPolicyError {
    #[error("malformed operational release trust policy: {0}")]
    Malformed(String),
    #[error("{0} must be canonical JSON")]
    NonCanonical(String),
    #[error("release-role separation rejected: {0}")]
    RoleSeparation(String),
    #[error("recovery authorization rejected: {0}")]
    Signature(String),
    #[error("trust-policy continuity rejected: {0}")]
    Continuity(String),
}
