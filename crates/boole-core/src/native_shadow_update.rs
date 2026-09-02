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

use crate::guest_boot::GuestBootArtifactRole;
use crate::release_contract_util::{self, ContractJsonError, RequiredPreviousManifestSha256};
use crate::{verify_signature_with_network, Hex32, SIGNED_ENVELOPE_SCHEMA};

pub const GUEST_UPDATE_MANIFEST_SCHEMA: &str = "boole.native-shadow.guest-update-manifest.v1";
pub const NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT: &str = "boole-native-shadow-guest-update-v1";
pub const GUEST_UPDATE_MANIFEST_SCHEMA_V2: &str = "boole.native-shadow.guest-update-manifest.v2";
pub const NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V2: &str = "boole-native-shadow-guest-update-v2";
pub const MAX_GUEST_UPDATE_ARTIFACT_BYTES: u64 = 2_147_483_648;
const MAX_MANIFEST_BYTES: usize = 1_048_576;
const MAX_DETACHED_SIGNATURE_BYTES: usize = 4_096;

const GUEST_ARTIFACT_ROLES_V1: [GuestArtifactRole; 10] = [
    GuestArtifactRole::GuestRootfs,
    GuestArtifactRole::RootfsContentManifest,
    GuestArtifactRole::Registry,
    GuestArtifactRole::ExecutionPolicy,
    GuestArtifactRole::ToolchainIdentity,
    GuestArtifactRole::CheckerReleaseManifest,
    GuestArtifactRole::RegistryOverlay,
    GuestArtifactRole::ClosedLocalReplayGrant,
    GuestArtifactRole::LocalExecutionAuthority,
    GuestArtifactRole::ClosedLocalReplayExecutionAuthority,
];

const GUEST_ARTIFACT_ROLES_V2: [GuestArtifactRole; 12] = [
    GuestArtifactRole::GuestKernel,
    GuestArtifactRole::GuestInitrd,
    GuestArtifactRole::GuestRootDisk,
    GuestArtifactRole::RootfsContentManifest,
    GuestArtifactRole::Registry,
    GuestArtifactRole::ExecutionPolicy,
    GuestArtifactRole::ToolchainIdentity,
    GuestArtifactRole::CheckerReleaseManifest,
    GuestArtifactRole::RegistryOverlay,
    GuestArtifactRole::ClosedLocalReplayGrant,
    GuestArtifactRole::LocalExecutionAuthority,
    GuestArtifactRole::ClosedLocalReplayExecutionAuthority,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GuestArtifactRole {
    GuestRootfs,
    GuestKernel,
    GuestInitrd,
    GuestRootDisk,
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
    /// Frozen v1 runtime-authority set.  Keep this exact-ten list unchanged:
    /// deployed/KAT v1 material uses one OCI-style `guest-rootfs` artifact.
    pub const ALL: [Self; 10] = GUEST_ARTIFACT_ROLES_V1;

    /// Bootable v2 runtime-authority set.  The legacy OCI `guest-rootfs` is
    /// replaced by the three host-side files required by VZLinuxBootLoader.
    pub const BOOTABLE_ALL: [Self; 12] = GUEST_ARTIFACT_ROLES_V2;

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GuestRootfs => "guest-rootfs",
            Self::GuestKernel => GuestBootArtifactRole::GuestKernel.as_str(),
            Self::GuestInitrd => GuestBootArtifactRole::GuestInitrd.as_str(),
            Self::GuestRootDisk => GuestBootArtifactRole::GuestRootDisk.as_str(),
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
    #[serde(default)]
    boot_format_version: Option<u64>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuestUpdateContract {
    FrozenV1,
    BootableV2,
}

#[derive(Clone, Copy)]
enum UpdateVersionExpectation<'a> {
    Successor(&'a NativeShadowUpdateFloor),
    ExactActive { release_sequence: u64 },
}

impl GuestUpdateContract {
    const fn manifest_schema(self) -> &'static str {
        match self {
            Self::FrozenV1 => GUEST_UPDATE_MANIFEST_SCHEMA,
            Self::BootableV2 => GUEST_UPDATE_MANIFEST_SCHEMA_V2,
        }
    }

    const fn signing_context(self) -> &'static str {
        match self {
            Self::FrozenV1 => NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
            Self::BootableV2 => NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V2,
        }
    }

    const fn boot_format_version(self) -> Option<u64> {
        match self {
            Self::FrozenV1 => None,
            Self::BootableV2 => Some(1),
        }
    }

    const fn required_roles(self) -> &'static [GuestArtifactRole] {
        match self {
            Self::FrozenV1 => &GUEST_ARTIFACT_ROLES_V1,
            Self::BootableV2 => &GUEST_ARTIFACT_ROLES_V2,
        }
    }
}

#[derive(Debug)]
pub struct AuthenticatedStagedNativeShadowUpdate {
    manifest_schema: &'static str,
    boot_format_version: Option<u64>,
    required_roles: &'static [GuestArtifactRole],
    release_sequence: u64,
    release_version: String,
    target_arch: String,
    manifest_sha256: String,
    descriptors: BTreeMap<GuestArtifactRole, GuestArtifactDescriptor>,
    verified_roles: BTreeSet<GuestArtifactRole>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedStagedNativeShadowUpdate {
    manifest_schema: &'static str,
    boot_format_version: Option<u64>,
    release_sequence: u64,
    release_version: String,
    target_arch: String,
    manifest_sha256: String,
    descriptors: BTreeMap<GuestArtifactRole, GuestArtifactDescriptor>,
}

impl VerifiedStagedNativeShadowUpdate {
    pub fn manifest_schema(&self) -> &str {
        self.manifest_schema
    }

    pub fn boot_format_version(&self) -> Option<u64> {
        self.boot_format_version
    }

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
        let missing: Vec<_> = self
            .required_roles
            .iter()
            .copied()
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
            manifest_schema: self.manifest_schema,
            boot_format_version: self.boot_format_version,
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
    authenticate_staged_native_shadow_update_for_contract(
        manifest_raw,
        detached_signature_raw,
        trust_root,
        UpdateVersionExpectation::Successor(floor),
        GuestUpdateContract::FrozenV1,
    )
}

/// Authenticate the successor contract used by a directly bootable
/// Linux/arm64 guest.  It is deliberately a separate entrypoint: callers
/// cannot accidentally reinterpret the frozen v1 exact-ten manifest as a
/// bootable release.
pub fn authenticate_staged_bootable_native_shadow_update(
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    trust_root: &NativeShadowUpdateTrustRoot,
    floor: &NativeShadowUpdateFloor,
) -> Result<AuthenticatedStagedNativeShadowUpdate, NativeShadowUpdateVerifyError> {
    authenticate_staged_native_shadow_update_for_contract(
        manifest_raw,
        detached_signature_raw,
        trust_root,
        UpdateVersionExpectation::Successor(floor),
        GuestUpdateContract::BootableV2,
    )
}

pub(crate) fn authenticate_active_staged_bootable_native_shadow_update(
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    trust_root: &NativeShadowUpdateTrustRoot,
    expected_release_sequence: u64,
    expected_manifest_sha256: &str,
) -> Result<AuthenticatedStagedNativeShadowUpdate, NativeShadowUpdateVerifyError> {
    if expected_release_sequence == 0 {
        return Err(NativeShadowUpdateVerifyError::VersionChain(
            "the active guest release sequence must be non-zero".to_string(),
        ));
    }
    require_sha256("active manifest digest", expected_manifest_sha256)?;
    if hex::encode(Sha256::digest(manifest_raw)) != expected_manifest_sha256 {
        return Err(NativeShadowUpdateVerifyError::ManifestDigestMismatch);
    }
    authenticate_staged_native_shadow_update_for_contract(
        manifest_raw,
        detached_signature_raw,
        trust_root,
        UpdateVersionExpectation::ExactActive {
            release_sequence: expected_release_sequence,
        },
        GuestUpdateContract::BootableV2,
    )
}

fn authenticate_staged_native_shadow_update_for_contract(
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    trust_root: &NativeShadowUpdateTrustRoot,
    version_expectation: UpdateVersionExpectation<'_>,
    contract: GuestUpdateContract,
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
    if signature.network_id.as_deref() != Some(contract.signing_context()) {
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
    validate_manifest(&manifest, version_expectation, contract)?;
    let descriptors = manifest
        .artifacts
        .into_iter()
        .map(|descriptor| (descriptor.role, descriptor))
        .collect();
    Ok(AuthenticatedStagedNativeShadowUpdate {
        manifest_schema: contract.manifest_schema(),
        boot_format_version: contract.boot_format_version(),
        required_roles: contract.required_roles(),
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
    version_expectation: UpdateVersionExpectation<'_>,
    contract: GuestUpdateContract,
) -> Result<(), NativeShadowUpdateVerifyError> {
    if manifest.schema != contract.manifest_schema() {
        return Err(NativeShadowUpdateVerifyError::Malformed(
            "unexpected manifest schema".to_string(),
        ));
    }
    if manifest.boot_format_version != contract.boot_format_version() {
        return Err(NativeShadowUpdateVerifyError::WrongTarget(match contract {
            GuestUpdateContract::FrozenV1 => {
                "bootFormatVersion is not part of the frozen v1 contract".to_string()
            }
            GuestUpdateContract::BootableV2 => {
                "bootFormatVersion must be 1 for the bootable v2 contract".to_string()
            }
        }));
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

    match version_expectation {
        UpdateVersionExpectation::Successor(floor) => match (
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
        },
        UpdateVersionExpectation::ExactActive { release_sequence } => {
            if manifest.release_sequence != release_sequence {
                return Err(NativeShadowUpdateVerifyError::VersionChain(
                    "releaseSequence differs from the active guest state".to_string(),
                ));
            }
            if let Some(previous) = &manifest.previous_manifest_sha256.0 {
                require_sha256("previousManifestSha256", previous)?;
            }
        }
    }

    let required_roles = contract.required_roles();
    if manifest.artifacts.len() != required_roles.len() {
        return Err(NativeShadowUpdateVerifyError::ArtifactSet(format!(
            "expected {} artifact descriptors",
            required_roles.len()
        )));
    }
    let mut names = BTreeSet::new();
    let mut total_bytes = 0_u64;
    for (descriptor, expected_role) in manifest.artifacts.iter().zip(required_roles) {
        if descriptor.role != *expected_role {
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
