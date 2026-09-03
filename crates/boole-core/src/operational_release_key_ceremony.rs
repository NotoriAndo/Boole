//! Public evidence for an operational release-key ceremony rehearsal.
//!
//! The verifier accepts canonical public documents and detached proofs of
//! possession only. It never creates, reads, stores, or exports a private key.
//! This first contract deliberately accepts only a non-production KAT
//! environment; enabling an operational ceremony remains a separate decision.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::operational_release_trust_policy::{
    verify_initial_operational_release_trust_policy, OperationalReleaseRecoveryRoot,
    OperationalReleaseTrustPolicyError,
};
use crate::release_contract_util::{self, ContractJsonError};
use crate::verify_signature;

pub const OPERATIONAL_RELEASE_KEY_CEREMONY_SCHEMA: &str =
    "boole.operational-release-key-ceremony.v1";
pub const OPERATIONAL_RELEASE_KEY_CEREMONY_SIGNATURES_SCHEMA: &str =
    "boole.operational-release-key-ceremony-signatures.v1";
pub const OPERATIONAL_RELEASE_KEY_CEREMONY_SIGNING_CONTEXT: &str =
    "boole-operational-release-key-ceremony-v1";
pub const OPERATIONAL_RELEASE_KEY_CEREMONY_ENVIRONMENT: &str = "non-production-kat";
pub const MAX_OPERATIONAL_RELEASE_KEY_CEREMONY_BYTES: usize = 65_536;
pub const MAX_OPERATIONAL_RELEASE_KEY_CEREMONY_SIGNATURES_BYTES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedOperationalReleaseKeyCeremony {
    ceremony_id: String,
    ceremony_sha256: String,
    recovery_root_sha256: String,
    trust_policy_sha256: String,
    signer_count: usize,
}

impl VerifiedOperationalReleaseKeyCeremony {
    pub fn ceremony_id(&self) -> &str {
        &self.ceremony_id
    }

    pub fn ceremony_sha256(&self) -> &str {
        &self.ceremony_sha256
    }

    pub fn recovery_root_sha256(&self) -> &str {
        &self.recovery_root_sha256
    }

    pub fn trust_policy_sha256(&self) -> &str {
        &self.trust_policy_sha256
    }

    pub fn signer_count(&self) -> usize {
        self.signer_count
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum OperationalReleaseKeyCeremonyError {
    #[error("malformed operational release key ceremony: {0}")]
    Malformed(String),
    #[error("{0} must be canonical JSON")]
    NonCanonical(String),
    #[error("operational release key ceremony rejected: {0}")]
    Rejected(String),
    #[error("initial trust policy rejected: {0}")]
    TrustPolicy(#[from] OperationalReleaseTrustPolicyError),
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PublicKeyWire {
    key_id: String,
    public_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RecoveryRootView {
    schema: String,
    threshold: usize,
    keys: Vec<PublicKeyWire>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ParticipantRole {
    ProductRelease,
    GuestRelease,
    Recovery,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum CustodyClass {
    OnlineSigning,
    OfflineRecovery,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ParticipantWire {
    role: ParticipantRole,
    custody_class: CustodyClass,
    key_id: String,
    public_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CeremonyWire {
    schema: String,
    ceremony_id: String,
    environment: String,
    recovery_root_sha256: String,
    trust_policy_sha256: String,
    participants: Vec<ParticipantWire>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignaturesWire {
    schema: String,
    ceremony_sha256: String,
    signatures: Vec<SignatureWire>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SignatureWire {
    key_id: String,
    public_key: String,
    signature: String,
}

/// Verify a complete, non-production public ceremony transcript.
///
/// The initial policy must already satisfy the two-of-three recovery policy.
/// The ceremony adds proof of possession from every one of the five active
/// keys: product, guest, and all three recovery keys.
pub fn verify_operational_release_key_ceremony(
    recovery_root_raw: &[u8],
    trust_policy_raw: &[u8],
    trust_policy_signatures_raw: &[u8],
    ceremony_raw: &[u8],
    ceremony_signatures_raw: &[u8],
) -> Result<VerifiedOperationalReleaseKeyCeremony, OperationalReleaseKeyCeremonyError> {
    let recovery_value = parse_canonical_json(
        "recovery root",
        recovery_root_raw,
        crate::MAX_OPERATIONAL_RELEASE_RECOVERY_ROOT_BYTES,
    )?;
    let recovery_view: RecoveryRootView = serde_json::from_value(recovery_value)
        .map_err(|error| OperationalReleaseKeyCeremonyError::Malformed(error.to_string()))?;
    let recovery = OperationalReleaseRecoveryRoot::from_canonical_json(recovery_root_raw)?;
    let verified_policy = verify_initial_operational_release_trust_policy(
        trust_policy_raw,
        trust_policy_signatures_raw,
        &recovery,
    )?;
    let product = verified_policy
        .product_release_trust_root()
        .ok_or_else(|| {
            OperationalReleaseKeyCeremonyError::Rejected(
                "the initial product release role must be active".to_string(),
            )
        })?;
    let guest = verified_policy.guest_release_trust_root().ok_or_else(|| {
        OperationalReleaseKeyCeremonyError::Rejected(
            "the initial guest release role must be active".to_string(),
        )
    })?;

    let ceremony_value = parse_canonical_json(
        "key ceremony",
        ceremony_raw,
        MAX_OPERATIONAL_RELEASE_KEY_CEREMONY_BYTES,
    )?;
    let ceremony: CeremonyWire = serde_json::from_value(ceremony_value)
        .map_err(|error| OperationalReleaseKeyCeremonyError::Malformed(error.to_string()))?;
    if ceremony.schema != OPERATIONAL_RELEASE_KEY_CEREMONY_SCHEMA {
        return Err(OperationalReleaseKeyCeremonyError::Malformed(
            "key ceremony schema mismatch".to_string(),
        ));
    }
    release_contract_util::check_safe_identifier("ceremonyId", &ceremony.ceremony_id)
        .map_err(OperationalReleaseKeyCeremonyError::Malformed)?;
    if ceremony.environment != OPERATIONAL_RELEASE_KEY_CEREMONY_ENVIRONMENT {
        return Err(OperationalReleaseKeyCeremonyError::Rejected(
            "only the non-production-kat ceremony environment is enabled".to_string(),
        ));
    }

    let recovery_root_sha256 = sha256(recovery_root_raw);
    let trust_policy_sha256 = sha256(trust_policy_raw);
    for (name, observed, expected) in [
        (
            "recoveryRootSha256",
            ceremony.recovery_root_sha256.as_str(),
            recovery_root_sha256.as_str(),
        ),
        (
            "trustPolicySha256",
            ceremony.trust_policy_sha256.as_str(),
            trust_policy_sha256.as_str(),
        ),
    ] {
        release_contract_util::check_sha256(name, observed)
            .map_err(OperationalReleaseKeyCeremonyError::Malformed)?;
        if observed != expected {
            return Err(OperationalReleaseKeyCeremonyError::Rejected(format!(
                "{name} does not match the supplied public document"
            )));
        }
    }
    if verified_policy.policy_sha256() != trust_policy_sha256 {
        return Err(OperationalReleaseKeyCeremonyError::Rejected(
            "verified trust policy digest differs from the ceremony".to_string(),
        ));
    }

    if recovery_view.schema != crate::OPERATIONAL_RELEASE_RECOVERY_ROOT_SCHEMA
        || recovery_view.threshold != 2
    {
        return Err(OperationalReleaseKeyCeremonyError::Malformed(
            "recovery root view differs from the verified contract".to_string(),
        ));
    }
    let mut expected = vec![
        ParticipantWire {
            role: ParticipantRole::ProductRelease,
            custody_class: CustodyClass::OnlineSigning,
            key_id: product.key_id().to_string(),
            public_key: product.public_key_hex().to_string(),
        },
        ParticipantWire {
            role: ParticipantRole::GuestRelease,
            custody_class: CustodyClass::OnlineSigning,
            key_id: guest.key_id().to_string(),
            public_key: guest.public_key_hex().to_string(),
        },
    ];
    expected.extend(recovery_view.keys.into_iter().map(|key| ParticipantWire {
        role: ParticipantRole::Recovery,
        custody_class: CustodyClass::OfflineRecovery,
        key_id: key.key_id,
        public_key: key.public_key,
    }));
    if ceremony.participants != expected {
        return Err(OperationalReleaseKeyCeremonyError::Rejected(
            "participants must exactly list product, guest, and all three recovery keys in contract order"
                .to_string(),
        ));
    }

    let ceremony_sha256 = sha256(ceremony_raw);
    let signatures_value = parse_canonical_json(
        "key ceremony signatures",
        ceremony_signatures_raw,
        MAX_OPERATIONAL_RELEASE_KEY_CEREMONY_SIGNATURES_BYTES,
    )?;
    let signatures: SignaturesWire = serde_json::from_value(signatures_value)
        .map_err(|error| OperationalReleaseKeyCeremonyError::Malformed(error.to_string()))?;
    if signatures.schema != OPERATIONAL_RELEASE_KEY_CEREMONY_SIGNATURES_SCHEMA {
        return Err(OperationalReleaseKeyCeremonyError::Malformed(
            "key ceremony signatures schema mismatch".to_string(),
        ));
    }
    release_contract_util::check_sha256("ceremonySha256", &signatures.ceremony_sha256)
        .map_err(OperationalReleaseKeyCeremonyError::Malformed)?;
    if signatures.ceremony_sha256 != ceremony_sha256 {
        return Err(OperationalReleaseKeyCeremonyError::Rejected(
            "signature set is bound to another key ceremony".to_string(),
        ));
    }

    let expected_keys = expected
        .iter()
        .map(|participant| (participant.key_id.clone(), participant.public_key.clone()))
        .collect::<BTreeMap<_, _>>();
    if expected_keys.len() != 5 || signatures.signatures.len() != expected_keys.len() {
        return Err(OperationalReleaseKeyCeremonyError::Rejected(
            "every active product, guest, and recovery key must sign exactly once".to_string(),
        ));
    }
    let payload = json!({
        "context": OPERATIONAL_RELEASE_KEY_CEREMONY_SIGNING_CONTEXT,
        "ceremonySha256": ceremony_sha256,
    });
    let mut seen = BTreeSet::new();
    let mut previous_key_id = None;
    for signature in &signatures.signatures {
        release_contract_util::check_safe_identifier("keyId", &signature.key_id)
            .map_err(OperationalReleaseKeyCeremonyError::Malformed)?;
        if previous_key_id
            .as_ref()
            .is_some_and(|previous: &String| previous >= &signature.key_id)
        {
            return Err(OperationalReleaseKeyCeremonyError::Malformed(
                "key ceremony signatures must be strictly ordered by keyId".to_string(),
            ));
        }
        previous_key_id = Some(signature.key_id.clone());
        let expected_public_key = expected_keys.get(&signature.key_id).ok_or_else(|| {
            OperationalReleaseKeyCeremonyError::Rejected(
                "key ceremony signature is from an unknown key".to_string(),
            )
        })?;
        if &signature.public_key != expected_public_key {
            return Err(OperationalReleaseKeyCeremonyError::Rejected(
                "key ceremony signature public key differs from its participant".to_string(),
            ));
        }
        let valid = verify_signature(&signature.public_key, &signature.signature, &payload)
            .map_err(OperationalReleaseKeyCeremonyError::Malformed)?;
        if !valid {
            return Err(OperationalReleaseKeyCeremonyError::Rejected(
                "key ceremony proof-of-possession signature is invalid".to_string(),
            ));
        }
        seen.insert(signature.key_id.clone());
    }
    if seen.len() != expected_keys.len() {
        return Err(OperationalReleaseKeyCeremonyError::Rejected(
            "every active product, guest, and recovery key must sign exactly once".to_string(),
        ));
    }

    Ok(VerifiedOperationalReleaseKeyCeremony {
        ceremony_id: ceremony.ceremony_id,
        ceremony_sha256,
        recovery_root_sha256,
        trust_policy_sha256,
        signer_count: seen.len(),
    })
}

fn parse_canonical_json(
    name: &str,
    raw: &[u8],
    max_bytes: usize,
) -> Result<Value, OperationalReleaseKeyCeremonyError> {
    release_contract_util::parse_canonical_json(name, raw, max_bytes).map_err(|error| match error {
        ContractJsonError::Malformed(reason) => {
            OperationalReleaseKeyCeremonyError::Malformed(reason)
        }
        ContractJsonError::NonCanonical(name) => {
            OperationalReleaseKeyCeremonyError::NonCanonical(name)
        }
    })
}

fn sha256(raw: &[u8]) -> String {
    hex::encode(Sha256::digest(raw))
}
