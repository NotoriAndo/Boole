//! Pure verification contract for staged Linux/arm64 native-shadow updates.
//!
//! The verifier does not download, persist, adopt, roll back or execute any
//! artifact. A runtime owner injects a public trust root and streams the
//! already-staged bytes through this module. Production private keys never
//! belong in this crate.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

use ed25519_dalek::VerifyingKey;
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::release_contract_util::{self, ContractJsonError, RequiredPreviousManifestSha256};
use crate::{verify_signature_with_network, Hex32, SIGNED_ENVELOPE_SCHEMA};

pub const GUEST_UPDATE_MANIFEST_SCHEMA: &str = "boole.native-shadow.guest-update-manifest.v1";
pub const NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT: &str = "boole-native-shadow-guest-update-v1";
pub const MAX_GUEST_UPDATE_ARTIFACT_BYTES: u64 = 2_147_483_648;
const MAX_MANIFEST_BYTES: usize = 1_048_576;
const MAX_DETACHED_SIGNATURE_BYTES: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GuestArtifactRole {
    GuestRootfs,
    RootfsContentManifest,
    Registry,
    ExecutionPolicy,
    ToolchainIdentity,
    CheckerReleaseManifest,
    RegistryOverlay,
    ClosedLocalReplayGrant,
    LocalExecutionAuthority,
    ClosedLocalReplayExecutionAuthority,
}

impl GuestArtifactRole {
    pub const ALL: [Self; 10] = [
        Self::GuestRootfs,
        Self::RootfsContentManifest,
        Self::Registry,
        Self::ExecutionPolicy,
        Self::ToolchainIdentity,
        Self::CheckerReleaseManifest,
        Self::RegistryOverlay,
        Self::ClosedLocalReplayGrant,
        Self::LocalExecutionAuthority,
        Self::ClosedLocalReplayExecutionAuthority,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GuestRootfs => "guest-rootfs",
            Self::RootfsContentManifest => "rootfs-content-manifest",
            Self::Registry => "registry",
            Self::ExecutionPolicy => "execution-policy",
            Self::ToolchainIdentity => "toolchain-identity",
            Self::CheckerReleaseManifest => "checker-release-manifest",
            Self::RegistryOverlay => "registry-overlay",
            Self::ClosedLocalReplayGrant => "closed-local-replay-grant",
            Self::LocalExecutionAuthority => "local-execution-authority",
            Self::ClosedLocalReplayExecutionAuthority => "closed-local-replay-execution-authority",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeShadowUpdateTrustRoot {
    key_id: String,
    public_key: Hex32,
}

impl NativeShadowUpdateTrustRoot {
    pub fn new(key_id: &str, public_key_hex: &str) -> Result<Self, NativeShadowUpdateVerifyError> {
        require_safe_identifier("keyId", key_id)?;
        let public_key = Hex32::from_hex(public_key_hex).map_err(|_| {
            NativeShadowUpdateVerifyError::Malformed(
                "public key must be 64 lowercase hexadecimal characters".to_string(),
            )
        })?;
        let verifying_key = VerifyingKey::from_bytes(public_key.as_bytes()).map_err(|_| {
            NativeShadowUpdateVerifyError::Malformed(
                "public key is not a valid Ed25519 point".to_string(),
            )
        })?;
        if verifying_key.is_weak() {
            return Err(NativeShadowUpdateVerifyError::Malformed(
                "public key must not be a weak Ed25519 point".to_string(),
            ));
        }
        Ok(Self {
            key_id: key_id.to_string(),
            public_key,
        })
    }

    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    pub fn public_key_hex(&self) -> String {
        self.public_key.to_hex()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeShadowUpdateFloor {
    highest_accepted_sequence: u64,
    active_manifest_sha256: Option<Hex32>,
    minimum_first_install_sequence: Option<u64>,
}

impl NativeShadowUpdateFloor {
    pub fn first_install(
        minimum_accepted_sequence: u64,
    ) -> Result<Self, NativeShadowUpdateVerifyError> {
        if minimum_accepted_sequence == 0 {
            return Err(NativeShadowUpdateVerifyError::VersionChain(
                "the pinned first-install minimum must be non-zero".to_string(),
            ));
        }
        Ok(Self {
            highest_accepted_sequence: 0,
            active_manifest_sha256: None,
            minimum_first_install_sequence: Some(minimum_accepted_sequence),
        })
    }

    pub fn installed(
        highest_accepted_sequence: u64,
        active_manifest_sha256: &str,
    ) -> Result<Self, NativeShadowUpdateVerifyError> {
        if highest_accepted_sequence == 0 {
            return Err(NativeShadowUpdateVerifyError::VersionChain(
                "an installed floor must have a non-zero sequence".to_string(),
            ));
        }
        let active_manifest_sha256 = Hex32::from_hex(active_manifest_sha256).map_err(|_| {
            NativeShadowUpdateVerifyError::Malformed(
                "active manifest digest must be lowercase SHA-256".to_string(),
            )
        })?;
        Ok(Self {
            highest_accepted_sequence,
            active_manifest_sha256: Some(active_manifest_sha256),
            minimum_first_install_sequence: None,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DetachedUpdateSignature {
    schema: String,
    key_id: String,
    pk: String,
    signature: String,
    network_id: Option<String>,
    manifest_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NativeShadowUpdateManifest {
    schema: String,
    channel: String,
    release_sequence: u64,
    release_version: String,
    target_os: String,
    target_arch: String,
    previous_manifest_sha256: RequiredPreviousManifestSha256,
    artifacts: Vec<GuestArtifactDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct GuestArtifactDescriptor {
    role: GuestArtifactRole,
    file_name: String,
    byte_length: u64,
    sha256: String,
}

#[derive(Debug)]
pub struct AuthenticatedStagedNativeShadowUpdate {
    release_sequence: u64,
    release_version: String,
    target_arch: String,
    manifest_sha256: String,
    descriptors: BTreeMap<GuestArtifactRole, GuestArtifactDescriptor>,
    verified_roles: BTreeSet<GuestArtifactRole>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedStagedNativeShadowUpdate {
    release_sequence: u64,
    release_version: String,
    target_arch: String,
    manifest_sha256: String,
    descriptors: BTreeMap<GuestArtifactRole, GuestArtifactDescriptor>,
}

impl VerifiedStagedNativeShadowUpdate {
    pub fn release_sequence(&self) -> u64 {
        self.release_sequence
    }

    pub fn release_version(&self) -> &str {
        &self.release_version
    }

    pub fn target_arch(&self) -> &str {
        &self.target_arch
    }

    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    pub fn artifact_file_name(&self, role: GuestArtifactRole) -> Option<&str> {
        self.descriptors
            .get(&role)
            .map(|descriptor| descriptor.file_name.as_str())
    }

    pub fn artifact_byte_length(&self, role: GuestArtifactRole) -> Option<u64> {
        self.descriptors
            .get(&role)
            .map(|descriptor| descriptor.byte_length)
    }

    pub fn artifact_sha256(&self, role: GuestArtifactRole) -> Option<&str> {
        self.descriptors
            .get(&role)
            .map(|descriptor| descriptor.sha256.as_str())
    }
}

impl AuthenticatedStagedNativeShadowUpdate {
    pub fn artifact_file_name(&self, role: GuestArtifactRole) -> Option<&str> {
        self.descriptors
            .get(&role)
            .map(|descriptor| descriptor.file_name.as_str())
    }

    pub fn artifact_byte_length(&self, role: GuestArtifactRole) -> Option<u64> {
        self.descriptors
            .get(&role)
            .map(|descriptor| descriptor.byte_length)
    }

    pub fn artifact_sha256(&self, role: GuestArtifactRole) -> Option<&str> {
        self.descriptors
            .get(&role)
            .map(|descriptor| descriptor.sha256.as_str())
    }

    pub fn verify_artifact<R: Read>(
        &mut self,
        role: GuestArtifactRole,
        mut reader: R,
    ) -> Result<(), NativeShadowUpdateVerifyError> {
        if self.verified_roles.contains(&role) {
            return Err(NativeShadowUpdateVerifyError::ArtifactSet(format!(
                "{} was supplied more than once",
                role.as_str()
            )));
        }
        let descriptor = self.descriptors.get(&role).ok_or_else(|| {
            NativeShadowUpdateVerifyError::ArtifactSet(format!("{} is not declared", role.as_str()))
        })?;

        let mut hasher = Sha256::new();
        let mut byte_length = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|error| NativeShadowUpdateVerifyError::Io(error.to_string()))?;
            if read == 0 {
                break;
            }
            byte_length = byte_length
                .checked_add(read as u64)
                .ok_or(NativeShadowUpdateVerifyError::ArtifactTooLarge)?;
            if byte_length > descriptor.byte_length {
                return Err(NativeShadowUpdateVerifyError::ArtifactMismatch(format!(
                    "{} exceeds declared byteLength",
                    role.as_str()
                )));
            }
            hasher.update(&buffer[..read]);
        }
        if byte_length != descriptor.byte_length {
            return Err(NativeShadowUpdateVerifyError::ArtifactMismatch(format!(
                "{} byteLength mismatch",
                role.as_str()
            )));
        }
        if hex::encode(hasher.finalize()) != descriptor.sha256 {
            return Err(NativeShadowUpdateVerifyError::ArtifactMismatch(format!(
                "{} SHA-256 mismatch",
                role.as_str()
            )));
        }
        self.verified_roles.insert(role);
        Ok(())
    }

    pub fn finish(self) -> Result<VerifiedStagedNativeShadowUpdate, NativeShadowUpdateVerifyError> {
        let missing: Vec<_> = GuestArtifactRole::ALL
            .into_iter()
            .filter(|role| !self.verified_roles.contains(role))
            .map(GuestArtifactRole::as_str)
            .collect();
        if !missing.is_empty() {
            return Err(NativeShadowUpdateVerifyError::ArtifactSet(format!(
                "missing staged artifacts: {}",
                missing.join(",")
            )));
        }
        Ok(VerifiedStagedNativeShadowUpdate {
            release_sequence: self.release_sequence,
            release_version: self.release_version,
            target_arch: self.target_arch,
            manifest_sha256: self.manifest_sha256,
            descriptors: self.descriptors,
        })
    }
}

pub fn authenticate_staged_native_shadow_update(
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    trust_root: &NativeShadowUpdateTrustRoot,
    floor: &NativeShadowUpdateFloor,
) -> Result<AuthenticatedStagedNativeShadowUpdate, NativeShadowUpdateVerifyError> {
    let manifest_value = parse_canonical_json("manifest", manifest_raw, MAX_MANIFEST_BYTES)?;
    let signature_value = parse_canonical_json(
        "detached signature",
        detached_signature_raw,
        MAX_DETACHED_SIGNATURE_BYTES,
    )?;
    let signature: DetachedUpdateSignature = serde_json::from_value(signature_value)
        .map_err(|error| NativeShadowUpdateVerifyError::Malformed(error.to_string()))?;
    if signature.schema != SIGNED_ENVELOPE_SCHEMA {
        return Err(NativeShadowUpdateVerifyError::Malformed(
            "unexpected detached signature schema".to_string(),
        ));
    }
    if signature.key_id != trust_root.key_id || signature.pk != trust_root.public_key.to_hex() {
        return Err(NativeShadowUpdateVerifyError::UntrustedKey);
    }
    if signature.network_id.as_deref() != Some(NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT) {
        return Err(NativeShadowUpdateVerifyError::InvalidSignatureContext);
    }
    let manifest_sha256 = hex::encode(Sha256::digest(manifest_raw));
    require_sha256("manifestSha256", &signature.manifest_sha256)?;
    if signature.manifest_sha256 != manifest_sha256 {
        return Err(NativeShadowUpdateVerifyError::ManifestDigestMismatch);
    }
    match verify_signature_with_network(
        &signature.pk,
        &signature.signature,
        &manifest_value,
        signature.network_id.as_deref(),
    ) {
        Ok(true) => {}
        Ok(false) => return Err(NativeShadowUpdateVerifyError::InvalidSignature),
        Err(error) => return Err(NativeShadowUpdateVerifyError::Malformed(error)),
    }

    let manifest: NativeShadowUpdateManifest = serde_json::from_value(manifest_value)
        .map_err(|error| NativeShadowUpdateVerifyError::Malformed(error.to_string()))?;
    validate_manifest(&manifest, floor)?;
    let descriptors = manifest
        .artifacts
        .into_iter()
        .map(|descriptor| (descriptor.role, descriptor))
        .collect();
    Ok(AuthenticatedStagedNativeShadowUpdate {
        release_sequence: manifest.release_sequence,
        release_version: manifest.release_version,
        target_arch: manifest.target_arch,
        manifest_sha256,
        descriptors,
        verified_roles: BTreeSet::new(),
    })
}

fn parse_canonical_json(
    name: &str,
    raw: &[u8],
    max_bytes: usize,
) -> Result<Value, NativeShadowUpdateVerifyError> {
    release_contract_util::parse_canonical_json(name, raw, max_bytes).map_err(|error| match error {
        ContractJsonError::Malformed(reason) => NativeShadowUpdateVerifyError::Malformed(reason),
        ContractJsonError::NonCanonical(name) => {
            NativeShadowUpdateVerifyError::NonCanonicalJson(name)
        }
    })
}

fn validate_manifest(
    manifest: &NativeShadowUpdateManifest,
    floor: &NativeShadowUpdateFloor,
) -> Result<(), NativeShadowUpdateVerifyError> {
    if manifest.schema != GUEST_UPDATE_MANIFEST_SCHEMA {
        return Err(NativeShadowUpdateVerifyError::Malformed(
            "unexpected manifest schema".to_string(),
        ));
    }
    if manifest.channel != "stable" {
        return Err(NativeShadowUpdateVerifyError::WrongTarget(
            "channel must be stable".to_string(),
        ));
    }
    if manifest.target_os != "linux" || manifest.target_arch != "aarch64" {
        return Err(NativeShadowUpdateVerifyError::WrongTarget(
            "target must be linux/aarch64".to_string(),
        ));
    }
    require_safe_identifier("releaseVersion", &manifest.release_version)?;

    match (
        floor.highest_accepted_sequence,
        floor.active_manifest_sha256,
        floor.minimum_first_install_sequence,
        &manifest.previous_manifest_sha256.0,
    ) {
        (0, None, Some(minimum), None) if manifest.release_sequence >= minimum => {}
        (0, None, Some(_), _) => {
            return Err(NativeShadowUpdateVerifyError::VersionChain(
                "candidate is below the pinned first-install minimum (which must be non-zero), or declares a predecessor"
                    .to_string(),
            ));
        }
        (sequence, Some(active), None, Some(previous)) => {
            require_sha256("previousManifestSha256", previous)?;
            if sequence == u64::MAX {
                return Err(NativeShadowUpdateVerifyError::VersionChain(
                    "release sequence space exhausted".to_string(),
                ));
            }
            if manifest.release_sequence <= sequence || previous != &active.to_hex() {
                return Err(NativeShadowUpdateVerifyError::VersionChain(
                    "candidate must advance the sequence and bind the exact active manifest"
                        .to_string(),
                ));
            }
        }
        _ => {
            return Err(NativeShadowUpdateVerifyError::VersionChain(
                "update floor is internally inconsistent".to_string(),
            ));
        }
    }

    if manifest.artifacts.len() != GuestArtifactRole::ALL.len() {
        return Err(NativeShadowUpdateVerifyError::ArtifactSet(format!(
            "expected {} artifact descriptors",
            GuestArtifactRole::ALL.len()
        )));
    }
    let mut names = BTreeSet::new();
    let mut total_bytes = 0_u64;
    for (descriptor, expected_role) in manifest.artifacts.iter().zip(GuestArtifactRole::ALL) {
        if descriptor.role != expected_role {
            return Err(NativeShadowUpdateVerifyError::ArtifactSet(
                "artifact descriptors must use the fixed role order".to_string(),
            ));
        }
        require_safe_file_name(&descriptor.file_name)?;
        if !names.insert(descriptor.file_name.as_str()) {
            return Err(NativeShadowUpdateVerifyError::ArtifactSet(
                "artifact fileName values must be unique".to_string(),
            ));
        }
        if descriptor.byte_length == 0 {
            return Err(NativeShadowUpdateVerifyError::ArtifactSet(format!(
                "{} byteLength must be non-zero",
                descriptor.role.as_str()
            )));
        }
        total_bytes = total_bytes
            .checked_add(descriptor.byte_length)
            .ok_or(NativeShadowUpdateVerifyError::ArtifactTooLarge)?;
        if total_bytes > MAX_GUEST_UPDATE_ARTIFACT_BYTES {
            return Err(NativeShadowUpdateVerifyError::ArtifactTooLarge);
        }
        require_sha256(
            &format!("{}.sha256", descriptor.role.as_str()),
            &descriptor.sha256,
        )?;
    }
    Ok(())
}

fn require_safe_identifier(name: &str, value: &str) -> Result<(), NativeShadowUpdateVerifyError> {
    release_contract_util::check_safe_identifier(name, value)
        .map_err(NativeShadowUpdateVerifyError::Malformed)
}

fn require_safe_file_name(value: &str) -> Result<(), NativeShadowUpdateVerifyError> {
    release_contract_util::check_safe_file_name(value)
        .map_err(NativeShadowUpdateVerifyError::Malformed)
}

fn require_sha256(name: &str, value: &str) -> Result<(), NativeShadowUpdateVerifyError> {
    release_contract_util::check_sha256(name, value)
        .map_err(NativeShadowUpdateVerifyError::Malformed)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NativeShadowUpdateVerifyError {
    #[error("malformed update contract: {0}")]
    Malformed(String),
    #[error("{0} must be canonical JSON")]
    NonCanonicalJson(String),
    #[error("signature key is not the injected trust root")]
    UntrustedKey,
    #[error("signature context is not the native-shadow guest-update domain")]
    InvalidSignatureContext,
    #[error("manifest SHA-256 does not match the detached signature")]
    ManifestDigestMismatch,
    #[error("manifest signature is invalid")]
    InvalidSignature,
    #[error("update target rejected: {0}")]
    WrongTarget(String),
    #[error("version chain rejected: {0}")]
    VersionChain(String),
    #[error("artifact set rejected: {0}")]
    ArtifactSet(String),
    #[error("staged artifact rejected: {0}")]
    ArtifactMismatch(String),
    #[error("staged artifact set exceeds the frozen 2 GiB cap")]
    ArtifactTooLarge,
    #[error("staged artifact read failed: {0}")]
    Io(String),
}
