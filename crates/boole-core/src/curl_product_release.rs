//! Pure verification contract for the curl-first macOS arm64 product release.
//!
//! `boole.curl-product-release.v1` freezes which files form one installable
//! product release (four host binaries plus the signed guest-update manifest
//! pair) and what makes that set authentic: an injected Ed25519 trust root in
//! a product-release signing domain. Transport identity (URLs, GitHub
//! Releases, Apple Team ID, Bundle ID, notarization) never appears in the
//! authority fields. The verifier does not download, install, adopt or
//! execute anything; adoption and installation are follow-up gates.
//!
//! The embedded guest-update pair is bound by exact bytes: the product
//! manifest pins the guest manifest digest, and the streamed guest artifacts
//! must match the product's `guestReleaseSequence`/`guestReleaseVersion` and
//! bind each other. The guest signature is not cryptographically re-verified
//! here — the guest trust root is injected separately at guest staging time
//! and the product release pins the exact signature bytes by hash.
//!
//! TOCTOU boundary: each artifact is verified by streaming from an open
//! `std::fs::File`, and the success result retains that exact handle so a
//! consumer can keep using the verified descriptor instead of re-opening a
//! swappable path.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;

use ed25519_dalek::VerifyingKey;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::native_shadow_update::{
    GuestArtifactRole, GUEST_UPDATE_MANIFEST_SCHEMA, GUEST_UPDATE_MANIFEST_SCHEMA_V2,
    GUEST_UPDATE_MANIFEST_SCHEMA_V3, NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
    NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V2, NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V3,
};
use crate::release_contract_util::{self, ContractJsonError, RequiredPreviousManifestSha256};
use crate::{verify_signature_with_network, Hex32, SIGNED_ENVELOPE_SCHEMA};

pub const CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA: &str = "boole.curl-product-release.v1";
pub const CURL_PRODUCT_RELEASE_SIGNING_CONTEXT: &str = "boole-curl-product-release-v1";
pub const CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V2: &str = "boole.curl-product-release.v2";
pub const CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V2: &str = "boole-curl-product-release-v2";
pub const CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V3: &str = "boole.curl-product-release.v3";
pub const CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V3: &str = "boole-curl-product-release-v3";
pub const CURL_PRODUCT_RELEASE_CONTROLLER_PROTOCOL_VERSION: u64 = 1;
pub const MAX_CURL_PRODUCT_HOST_PAYLOAD_BYTES: u64 = 536_870_912;
pub const CURL_PRODUCT_RELEASE_MINIMUM_MACOS: &str = "14.0";
pub const MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES: usize = 1_048_576;
pub const MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES: usize = 4_096;
const MAX_EMBEDDED_GUEST_MANIFEST_BYTES: u64 = 1_048_576;
const MAX_EMBEDDED_GUEST_SIGNATURE_BYTES: u64 = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProductArtifactRole {
    HostCli,
    HostNode,
    HostWalletAgent,
    HostController,
    GuestUpdateManifest,
    GuestUpdateSignature,
}

impl ProductArtifactRole {
    pub const ALL: [Self; 6] = [
        Self::HostCli,
        Self::HostNode,
        Self::HostWalletAgent,
        Self::HostController,
        Self::GuestUpdateManifest,
        Self::GuestUpdateSignature,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HostCli => "host-cli",
            Self::HostNode => "host-node",
            Self::HostWalletAgent => "host-wallet-agent",
            Self::HostController => "host-controller",
            Self::GuestUpdateManifest => "guest-update-manifest",
            Self::GuestUpdateSignature => "guest-update-signature",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlProductReleaseTrustRoot {
    key_id: String,
    public_key: Hex32,
}

impl CurlProductReleaseTrustRoot {
    pub fn new(key_id: &str, public_key_hex: &str) -> Result<Self, CurlProductReleaseVerifyError> {
        require_safe_identifier("keyId", key_id)?;
        let public_key = Hex32::from_hex(public_key_hex).map_err(|_| {
            CurlProductReleaseVerifyError::Malformed(
                "public key must be 64 lowercase hexadecimal characters".to_string(),
            )
        })?;
        let verifying_key = VerifyingKey::from_bytes(public_key.as_bytes()).map_err(|_| {
            CurlProductReleaseVerifyError::Malformed(
                "public key is not a valid Ed25519 point".to_string(),
            )
        })?;
        if verifying_key.is_weak() {
            return Err(CurlProductReleaseVerifyError::Malformed(
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
pub struct CurlProductReleaseFloor {
    highest_accepted_sequence: u64,
    active_manifest_sha256: Option<Hex32>,
    minimum_first_install_sequence: Option<u64>,
}

impl CurlProductReleaseFloor {
    pub fn first_install(
        minimum_accepted_sequence: u64,
    ) -> Result<Self, CurlProductReleaseVerifyError> {
        if minimum_accepted_sequence == 0 {
            return Err(CurlProductReleaseVerifyError::VersionChain(
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
    ) -> Result<Self, CurlProductReleaseVerifyError> {
        if highest_accepted_sequence == 0 {
            return Err(CurlProductReleaseVerifyError::VersionChain(
                "an installed floor must have a non-zero sequence".to_string(),
            ));
        }
        let active_manifest_sha256 = Hex32::from_hex(active_manifest_sha256).map_err(|_| {
            CurlProductReleaseVerifyError::Malformed(
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
struct DetachedProductSignature {
    schema: String,
    key_id: String,
    pk: String,
    signature: String,
    network_id: Option<String>,
    manifest_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CurlProductReleaseManifest {
    schema: String,
    channel: String,
    release_sequence: u64,
    release_version: String,
    source_revision: String,
    target_os: String,
    target_arch: String,
    minimum_mac_os: String,
    previous_manifest_sha256: RequiredPreviousManifestSha256,
    controller_protocol_version: u64,
    guest_manifest_sha256: String,
    guest_release_sequence: u64,
    guest_release_version: String,
    artifacts: Vec<ProductArtifactDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProductArtifactDescriptor {
    role: ProductArtifactRole,
    file_name: String,
    byte_length: u64,
    sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProductReleaseContract {
    FrozenV1,
    BootableV2,
    DirectBootV3,
}

enum ProductReleaseVersionExpectation<'a> {
    Successor(&'a CurlProductReleaseFloor),
    ExactActive { release_sequence: u64 },
}

impl ProductReleaseContract {
    const fn manifest_schema(self) -> &'static str {
        match self {
            Self::FrozenV1 => CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA,
            Self::BootableV2 => CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V2,
            Self::DirectBootV3 => CURL_PRODUCT_RELEASE_MANIFEST_SCHEMA_V3,
        }
    }

    const fn signing_context(self) -> &'static str {
        match self {
            Self::FrozenV1 => CURL_PRODUCT_RELEASE_SIGNING_CONTEXT,
            Self::BootableV2 => CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V2,
            Self::DirectBootV3 => CURL_PRODUCT_RELEASE_SIGNING_CONTEXT_V3,
        }
    }

    const fn guest_manifest_schema(self) -> &'static str {
        match self {
            Self::FrozenV1 => GUEST_UPDATE_MANIFEST_SCHEMA,
            Self::BootableV2 => GUEST_UPDATE_MANIFEST_SCHEMA_V2,
            Self::DirectBootV3 => GUEST_UPDATE_MANIFEST_SCHEMA_V3,
        }
    }

    const fn guest_signing_context(self) -> &'static str {
        match self {
            Self::FrozenV1 => NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT,
            Self::BootableV2 => NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V2,
            Self::DirectBootV3 => NATIVE_SHADOW_UPDATE_SIGNING_CONTEXT_V3,
        }
    }
}

#[derive(Debug)]
pub struct AuthenticatedCurlProductRelease {
    contract: ProductReleaseContract,
    release_sequence: u64,
    release_version: String,
    source_revision: String,
    guest_release_sequence: u64,
    guest_release_version: String,
    guest_manifest_sha256: String,
    manifest_sha256: String,
    descriptors: BTreeMap<ProductArtifactRole, ProductArtifactDescriptor>,
    verified_files: BTreeMap<ProductArtifactRole, File>,
}

impl AuthenticatedCurlProductRelease {
    pub fn release_sequence(&self) -> u64 {
        self.release_sequence
    }

    pub fn release_version(&self) -> &str {
        &self.release_version
    }

    pub fn artifact_file_name(&self, role: ProductArtifactRole) -> Option<&str> {
        self.descriptors
            .get(&role)
            .map(|descriptor| descriptor.file_name.as_str())
    }

    /// Declared byte length of an artifact before its bytes exist locally,
    /// so a transport can bound each download by the signed manifest instead
    /// of trusting server-controlled headers.
    pub fn artifact_byte_length(&self, role: ProductArtifactRole) -> Option<u64> {
        self.descriptors
            .get(&role)
            .map(|descriptor| descriptor.byte_length)
    }

    pub fn verify_artifact(
        &mut self,
        role: ProductArtifactRole,
        file: File,
    ) -> Result<(), CurlProductReleaseVerifyError> {
        if self.verified_files.contains_key(&role) {
            return Err(CurlProductReleaseVerifyError::ArtifactSet(format!(
                "{} was supplied more than once",
                role.as_str()
            )));
        }
        let descriptor = self.descriptors.get(&role).ok_or_else(|| {
            CurlProductReleaseVerifyError::ArtifactSet(format!("{} is not declared", role.as_str()))
        })?;

        // Only the embedded guest pair is buffered; its byteLength is capped
        // at validation time, so the capture stays small and bounded.
        let capture_embedded = matches!(
            role,
            ProductArtifactRole::GuestUpdateManifest | ProductArtifactRole::GuestUpdateSignature
        );
        let mut captured = Vec::new();
        let mut hasher = Sha256::new();
        let mut byte_length = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        let mut reader = &file;
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|error| CurlProductReleaseVerifyError::Io(error.to_string()))?;
            if read == 0 {
                break;
            }
            byte_length = byte_length.saturating_add(read as u64);
            if byte_length > descriptor.byte_length {
                return Err(CurlProductReleaseVerifyError::ArtifactMismatch(format!(
                    "{} exceeds declared byteLength",
                    role.as_str()
                )));
            }
            hasher.update(&buffer[..read]);
            if capture_embedded {
                captured.extend_from_slice(&buffer[..read]);
            }
        }
        if byte_length != descriptor.byte_length {
            return Err(CurlProductReleaseVerifyError::ArtifactMismatch(format!(
                "{} byteLength mismatch",
                role.as_str()
            )));
        }
        if hex::encode(hasher.finalize()) != descriptor.sha256 {
            return Err(CurlProductReleaseVerifyError::ArtifactMismatch(format!(
                "{} SHA-256 mismatch",
                role.as_str()
            )));
        }
        match role {
            ProductArtifactRole::GuestUpdateManifest => {
                self.check_embedded_guest_manifest(&captured)?;
            }
            ProductArtifactRole::GuestUpdateSignature => {
                self.check_embedded_guest_signature(&captured)?;
            }
            _ => {}
        }
        self.verified_files.insert(role, file);
        Ok(())
    }

    pub fn finish(self) -> Result<VerifiedCurlProductRelease, CurlProductReleaseVerifyError> {
        let missing: Vec<_> = ProductArtifactRole::ALL
            .into_iter()
            .filter(|role| !self.verified_files.contains_key(role))
            .map(ProductArtifactRole::as_str)
            .collect();
        if !missing.is_empty() {
            return Err(CurlProductReleaseVerifyError::ArtifactSet(format!(
                "missing staged artifacts: {}",
                missing.join(",")
            )));
        }
        Ok(VerifiedCurlProductRelease {
            manifest_schema: self.contract.manifest_schema(),
            guest_manifest_schema: self.contract.guest_manifest_schema(),
            release_sequence: self.release_sequence,
            release_version: self.release_version,
            source_revision: self.source_revision,
            guest_release_sequence: self.guest_release_sequence,
            guest_release_version: self.guest_release_version,
            guest_manifest_sha256: self.guest_manifest_sha256,
            manifest_sha256: self.manifest_sha256,
            descriptors: self.descriptors,
            verified_files: self.verified_files,
        })
    }

    fn check_embedded_guest_manifest(
        &self,
        raw: &[u8],
    ) -> Result<(), CurlProductReleaseVerifyError> {
        let value = parse_embedded_guest_json(
            "guest-update-manifest",
            raw,
            MAX_EMBEDDED_GUEST_MANIFEST_BYTES as usize,
        )?;
        if value["schema"] != self.contract.guest_manifest_schema() {
            return Err(CurlProductReleaseVerifyError::GuestBinding(
                "guest-update-manifest schema mismatch".to_string(),
            ));
        }
        match self.contract {
            ProductReleaseContract::FrozenV1 if value.get("bootFormatVersion").is_some() => {
                return Err(CurlProductReleaseVerifyError::GuestBinding(
                    "guest-update-manifest v1 must not declare bootFormatVersion".to_string(),
                ));
            }
            ProductReleaseContract::BootableV2 if value["bootFormatVersion"] != 1 => {
                return Err(CurlProductReleaseVerifyError::GuestBinding(
                    "guest-update-manifest v2 bootFormatVersion must be 1".to_string(),
                ));
            }
            ProductReleaseContract::DirectBootV3 if value["bootFormatVersion"] != 2 => {
                return Err(CurlProductReleaseVerifyError::GuestBinding(
                    "guest-update-manifest v3 bootFormatVersion must be 2".to_string(),
                ));
            }
            _ => {}
        }
        if self.contract == ProductReleaseContract::BootableV2 {
            require_bootable_guest_roles(&value)?;
        } else if self.contract == ProductReleaseContract::DirectBootV3 {
            require_direct_boot_guest_roles(&value)?;
        }
        if value["targetOs"] != "linux" || value["targetArch"] != "aarch64" {
            return Err(CurlProductReleaseVerifyError::GuestBinding(
                "guest-update-manifest target must be linux/aarch64".to_string(),
            ));
        }
        if value["releaseSequence"] != self.guest_release_sequence {
            return Err(CurlProductReleaseVerifyError::GuestBinding(
                "guest-update-manifest releaseSequence does not match guestReleaseSequence"
                    .to_string(),
            ));
        }
        if value["releaseVersion"].as_str() != Some(self.guest_release_version.as_str()) {
            return Err(CurlProductReleaseVerifyError::GuestBinding(
                "guest-update-manifest releaseVersion does not match guestReleaseVersion"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn check_embedded_guest_signature(
        &self,
        raw: &[u8],
    ) -> Result<(), CurlProductReleaseVerifyError> {
        let value = parse_embedded_guest_json(
            "guest-update-signature",
            raw,
            MAX_EMBEDDED_GUEST_SIGNATURE_BYTES as usize,
        )?;
        if value["schema"] != SIGNED_ENVELOPE_SCHEMA {
            return Err(CurlProductReleaseVerifyError::GuestBinding(
                "guest-update-signature schema mismatch".to_string(),
            ));
        }
        if value["networkId"] != self.contract.guest_signing_context() {
            return Err(CurlProductReleaseVerifyError::GuestBinding(
                "guest-update-signature is not in the guest-update signing domain".to_string(),
            ));
        }
        if value["manifestSha256"].as_str() != Some(self.guest_manifest_sha256.as_str()) {
            return Err(CurlProductReleaseVerifyError::GuestBinding(
                "guest-update-signature does not bind guestManifestSha256".to_string(),
            ));
        }
        Ok(())
    }
}

fn require_bootable_guest_roles(
    value: &serde_json::Value,
) -> Result<(), CurlProductReleaseVerifyError> {
    let artifacts = value["artifacts"].as_array().ok_or_else(|| {
        CurlProductReleaseVerifyError::GuestBinding(
            "guest-update-manifest v2 artifacts must be an array".to_string(),
        )
    })?;
    if artifacts.len() != GuestArtifactRole::BOOTABLE_ALL.len() {
        return Err(CurlProductReleaseVerifyError::GuestBinding(format!(
            "guest-update-manifest v2 must declare exactly {} artifacts",
            GuestArtifactRole::BOOTABLE_ALL.len()
        )));
    }
    for (descriptor, expected_role) in artifacts.iter().zip(GuestArtifactRole::BOOTABLE_ALL) {
        if descriptor["role"] != expected_role.as_str() {
            return Err(CurlProductReleaseVerifyError::GuestBinding(
                "guest-update-manifest v2 artifacts must use the fixed bootable role order"
                    .to_string(),
            ));
        }
    }
    Ok(())
}

fn require_direct_boot_guest_roles(
    value: &serde_json::Value,
) -> Result<(), CurlProductReleaseVerifyError> {
    let artifacts = value["artifacts"].as_array().ok_or_else(|| {
        CurlProductReleaseVerifyError::GuestBinding(
            "guest-update-manifest v3 artifacts must be an array".to_string(),
        )
    })?;
    if artifacts.len() != GuestArtifactRole::DIRECT_BOOT_ALL.len() {
        return Err(CurlProductReleaseVerifyError::GuestBinding(format!(
            "guest-update-manifest v3 must declare exactly {} artifacts",
            GuestArtifactRole::DIRECT_BOOT_ALL.len()
        )));
    }
    for (descriptor, expected_role) in artifacts.iter().zip(GuestArtifactRole::DIRECT_BOOT_ALL) {
        if descriptor["role"] != expected_role.as_str() {
            return Err(CurlProductReleaseVerifyError::GuestBinding(
                "guest-update-manifest v3 artifacts must use the fixed direct-boot role order"
                    .to_string(),
            ));
        }
    }
    Ok(())
}

#[derive(Debug)]
pub struct VerifiedCurlProductRelease {
    manifest_schema: &'static str,
    guest_manifest_schema: &'static str,
    release_sequence: u64,
    release_version: String,
    source_revision: String,
    guest_release_sequence: u64,
    guest_release_version: String,
    guest_manifest_sha256: String,
    manifest_sha256: String,
    descriptors: BTreeMap<ProductArtifactRole, ProductArtifactDescriptor>,
    verified_files: BTreeMap<ProductArtifactRole, File>,
}

impl VerifiedCurlProductRelease {
    pub fn manifest_schema(&self) -> &str {
        self.manifest_schema
    }

    pub fn guest_manifest_schema(&self) -> &str {
        self.guest_manifest_schema
    }

    pub fn release_sequence(&self) -> u64 {
        self.release_sequence
    }

    pub fn release_version(&self) -> &str {
        &self.release_version
    }

    pub fn source_revision(&self) -> &str {
        &self.source_revision
    }

    pub fn guest_release_sequence(&self) -> u64 {
        self.guest_release_sequence
    }

    pub fn guest_release_version(&self) -> &str {
        &self.guest_release_version
    }

    pub fn guest_manifest_sha256(&self) -> &str {
        &self.guest_manifest_sha256
    }

    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    pub fn artifact_file_name(&self, role: ProductArtifactRole) -> Option<&str> {
        self.descriptors
            .get(&role)
            .map(|descriptor| descriptor.file_name.as_str())
    }

    pub fn artifact_byte_length(&self, role: ProductArtifactRole) -> Option<u64> {
        self.descriptors
            .get(&role)
            .map(|descriptor| descriptor.byte_length)
    }

    pub fn artifact_sha256(&self, role: ProductArtifactRole) -> Option<&str> {
        self.descriptors
            .get(&role)
            .map(|descriptor| descriptor.sha256.as_str())
    }

    pub fn artifact_file(&self, role: ProductArtifactRole) -> Option<&File> {
        self.verified_files.get(&role)
    }
}

pub fn authenticate_curl_product_release(
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    trust_root: &CurlProductReleaseTrustRoot,
    floor: &CurlProductReleaseFloor,
) -> Result<AuthenticatedCurlProductRelease, CurlProductReleaseVerifyError> {
    authenticate_curl_product_release_for_contract(
        manifest_raw,
        detached_signature_raw,
        trust_root,
        ProductReleaseVersionExpectation::Successor(floor),
        ProductReleaseContract::FrozenV1,
    )
}

pub(crate) fn authenticate_active_curl_product_release(
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    trust_root: &CurlProductReleaseTrustRoot,
    expected_release_sequence: u64,
    expected_manifest_sha256: &str,
) -> Result<AuthenticatedCurlProductRelease, CurlProductReleaseVerifyError> {
    authenticate_active_curl_product_release_for_contract(
        manifest_raw,
        detached_signature_raw,
        trust_root,
        expected_release_sequence,
        expected_manifest_sha256,
        ProductReleaseContract::FrozenV1,
    )
}

pub(crate) fn authenticate_active_bootable_curl_product_release(
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    trust_root: &CurlProductReleaseTrustRoot,
    expected_release_sequence: u64,
    expected_manifest_sha256: &str,
) -> Result<AuthenticatedCurlProductRelease, CurlProductReleaseVerifyError> {
    authenticate_active_curl_product_release_for_contract(
        manifest_raw,
        detached_signature_raw,
        trust_root,
        expected_release_sequence,
        expected_manifest_sha256,
        ProductReleaseContract::BootableV2,
    )
}

pub(crate) fn authenticate_active_direct_boot_curl_product_release(
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    trust_root: &CurlProductReleaseTrustRoot,
    expected_release_sequence: u64,
    expected_manifest_sha256: &str,
) -> Result<AuthenticatedCurlProductRelease, CurlProductReleaseVerifyError> {
    authenticate_active_curl_product_release_for_contract(
        manifest_raw,
        detached_signature_raw,
        trust_root,
        expected_release_sequence,
        expected_manifest_sha256,
        ProductReleaseContract::DirectBootV3,
    )
}

fn authenticate_active_curl_product_release_for_contract(
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    trust_root: &CurlProductReleaseTrustRoot,
    expected_release_sequence: u64,
    expected_manifest_sha256: &str,
    contract: ProductReleaseContract,
) -> Result<AuthenticatedCurlProductRelease, CurlProductReleaseVerifyError> {
    if expected_release_sequence == 0 {
        return Err(CurlProductReleaseVerifyError::VersionChain(
            "the active release sequence must be non-zero".to_string(),
        ));
    }
    let expected_manifest_sha256 = Hex32::from_hex(expected_manifest_sha256).map_err(|_| {
        CurlProductReleaseVerifyError::Malformed(
            "active manifest digest must be lowercase SHA-256".to_string(),
        )
    })?;
    let observed_manifest_sha256 = Hex32::from_bytes(Sha256::digest(manifest_raw).into());
    if observed_manifest_sha256 != expected_manifest_sha256 {
        return Err(CurlProductReleaseVerifyError::ManifestDigestMismatch);
    }
    authenticate_curl_product_release_for_contract(
        manifest_raw,
        detached_signature_raw,
        trust_root,
        ProductReleaseVersionExpectation::ExactActive {
            release_sequence: expected_release_sequence,
        },
        contract,
    )
}

/// Authenticate the product-release successor that embeds the bootable guest
/// update v2 contract.  Keeping a separate entrypoint prevents the frozen v1
/// product manifest from being silently upgraded into a bootable release.
pub fn authenticate_bootable_curl_product_release(
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    trust_root: &CurlProductReleaseTrustRoot,
    floor: &CurlProductReleaseFloor,
) -> Result<AuthenticatedCurlProductRelease, CurlProductReleaseVerifyError> {
    authenticate_curl_product_release_for_contract(
        manifest_raw,
        detached_signature_raw,
        trust_root,
        ProductReleaseVersionExpectation::Successor(floor),
        ProductReleaseContract::BootableV2,
    )
}

/// Authenticate the direct-root product successor that embeds guest v3.
pub fn authenticate_direct_boot_curl_product_release(
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    trust_root: &CurlProductReleaseTrustRoot,
    floor: &CurlProductReleaseFloor,
) -> Result<AuthenticatedCurlProductRelease, CurlProductReleaseVerifyError> {
    authenticate_curl_product_release_for_contract(
        manifest_raw,
        detached_signature_raw,
        trust_root,
        ProductReleaseVersionExpectation::Successor(floor),
        ProductReleaseContract::DirectBootV3,
    )
}

fn authenticate_curl_product_release_for_contract(
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    trust_root: &CurlProductReleaseTrustRoot,
    version_expectation: ProductReleaseVersionExpectation<'_>,
    contract: ProductReleaseContract,
) -> Result<AuthenticatedCurlProductRelease, CurlProductReleaseVerifyError> {
    let manifest_value = parse_canonical_json(
        "manifest",
        manifest_raw,
        MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES,
    )?;
    let signature_value = parse_canonical_json(
        "detached signature",
        detached_signature_raw,
        MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES,
    )?;
    let signature: DetachedProductSignature = serde_json::from_value(signature_value)
        .map_err(|error| CurlProductReleaseVerifyError::Malformed(error.to_string()))?;
    if signature.schema != SIGNED_ENVELOPE_SCHEMA {
        return Err(CurlProductReleaseVerifyError::Malformed(
            "unexpected detached signature schema".to_string(),
        ));
    }
    if signature.key_id != trust_root.key_id || signature.pk != trust_root.public_key.to_hex() {
        return Err(CurlProductReleaseVerifyError::UntrustedKey);
    }
    if signature.network_id.as_deref() != Some(contract.signing_context()) {
        return Err(CurlProductReleaseVerifyError::InvalidSignatureContext);
    }
    let manifest_sha256 = hex::encode(Sha256::digest(manifest_raw));
    require_sha256("manifestSha256", &signature.manifest_sha256)?;
    if signature.manifest_sha256 != manifest_sha256 {
        return Err(CurlProductReleaseVerifyError::ManifestDigestMismatch);
    }
    match verify_signature_with_network(
        &signature.pk,
        &signature.signature,
        &manifest_value,
        signature.network_id.as_deref(),
    ) {
        Ok(true) => {}
        Ok(false) => return Err(CurlProductReleaseVerifyError::InvalidSignature),
        Err(error) => return Err(CurlProductReleaseVerifyError::Malformed(error)),
    }

    let manifest: CurlProductReleaseManifest = serde_json::from_value(manifest_value)
        .map_err(|error| CurlProductReleaseVerifyError::Malformed(error.to_string()))?;
    validate_manifest(&manifest, version_expectation, contract)?;
    let descriptors = manifest
        .artifacts
        .into_iter()
        .map(|descriptor| (descriptor.role, descriptor))
        .collect();
    Ok(AuthenticatedCurlProductRelease {
        contract,
        release_sequence: manifest.release_sequence,
        release_version: manifest.release_version,
        source_revision: manifest.source_revision,
        guest_release_sequence: manifest.guest_release_sequence,
        guest_release_version: manifest.guest_release_version,
        guest_manifest_sha256: manifest.guest_manifest_sha256,
        manifest_sha256,
        descriptors,
        verified_files: BTreeMap::new(),
    })
}

fn validate_manifest(
    manifest: &CurlProductReleaseManifest,
    version_expectation: ProductReleaseVersionExpectation<'_>,
    contract: ProductReleaseContract,
) -> Result<(), CurlProductReleaseVerifyError> {
    if manifest.schema != contract.manifest_schema() {
        return Err(CurlProductReleaseVerifyError::Malformed(
            "unexpected manifest schema".to_string(),
        ));
    }
    if manifest.channel != "stable" {
        return Err(CurlProductReleaseVerifyError::WrongTarget(
            "channel must be stable".to_string(),
        ));
    }
    if manifest.target_os != "macos" || manifest.target_arch != "arm64" {
        return Err(CurlProductReleaseVerifyError::WrongTarget(
            "target must be macos/arm64".to_string(),
        ));
    }
    if manifest.minimum_mac_os != CURL_PRODUCT_RELEASE_MINIMUM_MACOS {
        return Err(CurlProductReleaseVerifyError::WrongTarget(
            "minimumMacOs must be 14.0".to_string(),
        ));
    }
    if manifest.controller_protocol_version != CURL_PRODUCT_RELEASE_CONTROLLER_PROTOCOL_VERSION {
        return Err(CurlProductReleaseVerifyError::WrongTarget(
            "controllerProtocolVersion must be 1".to_string(),
        ));
    }
    require_safe_identifier("releaseVersion", &manifest.release_version)?;
    require_source_revision(&manifest.source_revision)?;
    require_safe_identifier("guestReleaseVersion", &manifest.guest_release_version)?;
    require_sha256("guestManifestSha256", &manifest.guest_manifest_sha256)?;

    match version_expectation {
        ProductReleaseVersionExpectation::Successor(floor) => {
            match (
                floor.highest_accepted_sequence,
                floor.active_manifest_sha256,
                floor.minimum_first_install_sequence,
                &manifest.previous_manifest_sha256.0,
            ) {
                (0, None, Some(minimum), None) if manifest.release_sequence >= minimum => {}
                (0, None, Some(_), _) => {
                    return Err(CurlProductReleaseVerifyError::VersionChain(
                        "candidate is below the pinned first-install minimum (which must be non-zero), or declares a predecessor"
                            .to_string(),
                    ));
                }
                (sequence, Some(active), None, Some(previous)) => {
                    require_sha256("previousManifestSha256", previous)?;
                    if sequence == u64::MAX {
                        return Err(CurlProductReleaseVerifyError::VersionChain(
                            "release sequence space exhausted".to_string(),
                        ));
                    }
                    if manifest.release_sequence <= sequence || previous != &active.to_hex() {
                        return Err(CurlProductReleaseVerifyError::VersionChain(
                            "candidate must advance the sequence and bind the exact active manifest"
                                .to_string(),
                        ));
                    }
                }
                _ => {
                    return Err(CurlProductReleaseVerifyError::VersionChain(
                        "release floor is internally inconsistent".to_string(),
                    ));
                }
            }
        }
        ProductReleaseVersionExpectation::ExactActive { release_sequence } => {
            if manifest.release_sequence != release_sequence {
                return Err(CurlProductReleaseVerifyError::VersionChain(
                    "active manifest sequence differs from installed state".to_string(),
                ));
            }
            if let Some(previous) = &manifest.previous_manifest_sha256.0 {
                require_sha256("previousManifestSha256", previous)?;
            }
        }
    }

    if manifest.artifacts.len() != ProductArtifactRole::ALL.len() {
        return Err(CurlProductReleaseVerifyError::ArtifactSet(format!(
            "expected {} artifact descriptors",
            ProductArtifactRole::ALL.len()
        )));
    }
    let mut names = BTreeSet::new();
    let mut host_payload_bytes = 0_u64;
    for (descriptor, expected_role) in manifest.artifacts.iter().zip(ProductArtifactRole::ALL) {
        if descriptor.role != expected_role {
            return Err(CurlProductReleaseVerifyError::ArtifactSet(
                "artifact descriptors must use the fixed role order".to_string(),
            ));
        }
        require_safe_file_name(&descriptor.file_name)?;
        if !names.insert(descriptor.file_name.as_str()) {
            return Err(CurlProductReleaseVerifyError::ArtifactSet(
                "artifact fileName values must be unique".to_string(),
            ));
        }
        if descriptor.byte_length == 0 {
            return Err(CurlProductReleaseVerifyError::ArtifactSet(format!(
                "{} byteLength must be non-zero",
                descriptor.role.as_str()
            )));
        }
        match descriptor.role {
            ProductArtifactRole::GuestUpdateManifest => {
                if descriptor.byte_length > MAX_EMBEDDED_GUEST_MANIFEST_BYTES {
                    return Err(CurlProductReleaseVerifyError::ArtifactSet(
                        "guest-update-manifest byteLength exceeds its embedded cap".to_string(),
                    ));
                }
            }
            ProductArtifactRole::GuestUpdateSignature => {
                if descriptor.byte_length > MAX_EMBEDDED_GUEST_SIGNATURE_BYTES {
                    return Err(CurlProductReleaseVerifyError::ArtifactSet(
                        "guest-update-signature byteLength exceeds its embedded cap".to_string(),
                    ));
                }
            }
            _ => {
                host_payload_bytes = host_payload_bytes
                    .checked_add(descriptor.byte_length)
                    .ok_or(CurlProductReleaseVerifyError::HostPayloadTooLarge)?;
                if host_payload_bytes > MAX_CURL_PRODUCT_HOST_PAYLOAD_BYTES {
                    return Err(CurlProductReleaseVerifyError::HostPayloadTooLarge);
                }
            }
        }
        require_sha256(
            &format!("{}.sha256", descriptor.role.as_str()),
            &descriptor.sha256,
        )?;
    }

    let guest_manifest_descriptor = manifest
        .artifacts
        .iter()
        .find(|descriptor| descriptor.role == ProductArtifactRole::GuestUpdateManifest)
        .expect("fixed role order guarantees a guest-update-manifest descriptor");
    if guest_manifest_descriptor.sha256 != manifest.guest_manifest_sha256 {
        return Err(CurlProductReleaseVerifyError::GuestBinding(
            "guestManifestSha256 does not match the guest-update-manifest descriptor".to_string(),
        ));
    }
    Ok(())
}

fn parse_embedded_guest_json(
    name: &str,
    raw: &[u8],
    max_bytes: usize,
) -> Result<serde_json::Value, CurlProductReleaseVerifyError> {
    release_contract_util::parse_canonical_json(name, raw, max_bytes).map_err(|error| match error {
        ContractJsonError::Malformed(reason) => CurlProductReleaseVerifyError::GuestBinding(
            format!("{name} is not valid JSON: {reason}"),
        ),
        ContractJsonError::NonCanonical(name) => {
            CurlProductReleaseVerifyError::GuestBinding(format!("{name} must be canonical JSON"))
        }
    })
}

fn parse_canonical_json(
    name: &str,
    raw: &[u8],
    max_bytes: usize,
) -> Result<serde_json::Value, CurlProductReleaseVerifyError> {
    release_contract_util::parse_canonical_json(name, raw, max_bytes).map_err(|error| match error {
        ContractJsonError::Malformed(reason) => CurlProductReleaseVerifyError::Malformed(reason),
        ContractJsonError::NonCanonical(name) => {
            CurlProductReleaseVerifyError::NonCanonicalJson(name)
        }
    })
}

fn require_safe_identifier(name: &str, value: &str) -> Result<(), CurlProductReleaseVerifyError> {
    release_contract_util::check_safe_identifier(name, value)
        .map_err(CurlProductReleaseVerifyError::Malformed)
}

fn require_safe_file_name(value: &str) -> Result<(), CurlProductReleaseVerifyError> {
    release_contract_util::check_safe_file_name(value)
        .map_err(CurlProductReleaseVerifyError::Malformed)
}

fn require_sha256(name: &str, value: &str) -> Result<(), CurlProductReleaseVerifyError> {
    release_contract_util::check_sha256(name, value)
        .map_err(CurlProductReleaseVerifyError::Malformed)
}

fn require_source_revision(value: &str) -> Result<(), CurlProductReleaseVerifyError> {
    if value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(CurlProductReleaseVerifyError::Malformed(
            "sourceRevision must be 40 lowercase hexadecimal characters".to_string(),
        ))
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CurlProductReleaseVerifyError {
    #[error("malformed product release contract: {0}")]
    Malformed(String),
    #[error("{0} must be canonical JSON")]
    NonCanonicalJson(String),
    #[error("signature key is not the injected trust root")]
    UntrustedKey,
    #[error("signature context is not the curl product-release domain")]
    InvalidSignatureContext,
    #[error("manifest SHA-256 does not match the detached signature")]
    ManifestDigestMismatch,
    #[error("manifest signature is invalid")]
    InvalidSignature,
    #[error("release target rejected: {0}")]
    WrongTarget(String),
    #[error("version chain rejected: {0}")]
    VersionChain(String),
    #[error("artifact set rejected: {0}")]
    ArtifactSet(String),
    #[error("staged artifact rejected: {0}")]
    ArtifactMismatch(String),
    #[error("host payload exceeds the frozen 512 MiB cap")]
    HostPayloadTooLarge,
    #[error("guest release binding rejected: {0}")]
    GuestBinding(String),
    #[error("staged artifact read failed: {0}")]
    Io(String),
}
