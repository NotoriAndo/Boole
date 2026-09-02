//! Local installer core for the curl-first macOS arm64 product (CURL.2).
//!
//! This module consumes the frozen `boole.curl-product-release.v1` verifier
//! and turns a locally staged release bundle (manifest + detached signature +
//! artifact files) into a durable install: every byte is verified first, the
//! release is copied into an immutable per-version directory, and a canonical
//! `installed-release.json` state record is adopted atomically. That record
//! becomes the replay/rollback floor for every later install.
//!
//! Boundaries frozen here:
//! - Fail-closed: the install root is never mutated before the whole release
//!   authenticates and every artifact byte matches its pinned digest. A
//!   corrupt or non-canonical state record aborts the install; it is never
//!   silently replaced by the first-install floor.
//! - Crash-safe: all writes land in a transient `staging/` tree, version
//!   directories appear via a single `rename`, and the state record is
//!   replaced via a fsynced temp-file rename. The durable state is always
//!   either the old release or the new one, never a mix.
//! - TOCTOU carry-through: adopted bytes are copied from the exact file
//!   handles the verifier streamed, never re-opened by path.
//! - No transport: this module never downloads. Fetching the bundle is a
//!   separate gate; URLs and GitHub Releases stay transport, not trust.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;
use thiserror::Error;

use crate::canonicalize;
use crate::curl_product_release::{
    authenticate_active_bootable_curl_product_release, authenticate_active_curl_product_release,
    authenticate_active_direct_boot_curl_product_release,
    authenticate_bootable_curl_product_release, authenticate_curl_product_release,
    authenticate_direct_boot_curl_product_release, CurlProductReleaseFloor,
    CurlProductReleaseTrustRoot, CurlProductReleaseVerifyError, ProductArtifactRole,
    VerifiedCurlProductRelease, MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES,
    MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES,
};
use crate::native_shadow_update::{
    authenticate_active_staged_bootable_native_shadow_update,
    authenticate_active_staged_direct_boot_native_shadow_update,
    authenticate_staged_bootable_native_shadow_update,
    authenticate_staged_direct_boot_native_shadow_update, GuestArtifactRole,
    NativeShadowUpdateFloor, NativeShadowUpdateTrustRoot, NativeShadowUpdateVerifyError,
    VerifiedStagedNativeShadowUpdate,
};

#[derive(Clone, Copy)]
enum InstalledGuestContract {
    BootableV2,
    DirectBootV3,
}

impl InstalledGuestContract {
    const fn roles(self) -> &'static [GuestArtifactRole] {
        match self {
            Self::BootableV2 => &GuestArtifactRole::BOOTABLE_ALL,
            Self::DirectBootV3 => &GuestArtifactRole::DIRECT_BOOT_ALL,
        }
    }
}
use crate::release_contract_util::{self, ContractJsonError};

pub const CURL_PRODUCT_INSTALL_STATE_SCHEMA: &str = "boole.curl-product-install-state.v1";
pub const CURL_PRODUCT_INSTALL_STATE_SCHEMA_V2: &str = "boole.curl-product-install-state.v2";
pub const CURL_PRODUCT_INSTALL_STATE_FILE: &str = "installed-release.json";
pub const CURL_PRODUCT_INSTALL_STATE_TEMP_FILE: &str = "installed-release.json.tmp";
pub const CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY: &str = "versions";
pub const CURL_PRODUCT_INSTALL_STAGING_DIRECTORY: &str = "staging";
pub const CURL_PRODUCT_INSTALLED_MANIFEST_FILE: &str = "release-manifest.json";
pub const CURL_PRODUCT_INSTALLED_SIGNATURE_FILE: &str = "release-signature.json";
pub const CURL_PRODUCT_INSTALLED_GUEST_DIRECTORY: &str = "guest-artifacts";
const MAX_INSTALL_STATE_BYTES: usize = 4_096;

#[derive(Debug, Error)]
pub enum CurlProductInstallError {
    #[error("release rejected: {0}")]
    Verify(#[from] CurlProductReleaseVerifyError),
    #[error("guest release rejected: {0}")]
    GuestVerify(#[from] NativeShadowUpdateVerifyError),
    #[error("install state rejected: {0}")]
    State(String),
    #[error("artifact source rejected: {0}")]
    ArtifactSource(String),
    #[error("install filesystem operation failed: {0}")]
    Io(String),
}

/// Durable `installed-release.json` record: which release is active and where
/// its immutable version directory lives, relative to the install root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurlProductInstallState {
    release_sequence: u64,
    release_version: String,
    manifest_sha256: String,
    version_directory: String,
    release_floor: InstalledReleaseIdentity,
    guest_release_floor: Option<InstalledGuestReleaseIdentity>,
    rollback_release: Option<InstalledReleaseIdentity>,
}

impl CurlProductInstallState {
    pub fn release_sequence(&self) -> u64 {
        self.release_sequence
    }

    pub fn release_version(&self) -> &str {
        &self.release_version
    }

    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    pub fn version_directory(&self) -> &str {
        &self.version_directory
    }

    /// Highest authenticated release ever adopted. This never moves
    /// backwards when the active release is rolled back.
    pub fn release_floor_sequence(&self) -> u64 {
        self.release_floor.release_sequence
    }

    pub fn release_floor_manifest_sha256(&self) -> &str {
        &self.release_floor.manifest_sha256
    }

    pub fn rollback_release_sequence(&self) -> Option<u64> {
        self.rollback_release
            .as_ref()
            .map(|release| release.release_sequence)
    }

    pub fn guest_release_floor_sequence(&self) -> Option<u64> {
        self.guest_release_floor
            .as_ref()
            .map(|release| release.release_sequence)
    }

    pub fn guest_release_floor_manifest_sha256(&self) -> Option<&str> {
        self.guest_release_floor
            .as_ref()
            .map(|release| release.manifest_sha256.as_str())
    }

    fn active_identity(&self) -> InstalledReleaseIdentity {
        InstalledReleaseIdentity {
            release_sequence: self.release_sequence,
            release_version: self.release_version.clone(),
            manifest_sha256: self.manifest_sha256.clone(),
            version_directory: self.version_directory.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstalledReleaseIdentity {
    release_sequence: u64,
    release_version: String,
    manifest_sha256: String,
    version_directory: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstalledGuestReleaseIdentity {
    release_sequence: u64,
    manifest_sha256: String,
}

/// Success result of one install: the adopted release identity plus the
/// absolute version directory holding its verified artifacts.
#[derive(Debug)]
pub struct InstalledCurlProduct {
    release_sequence: u64,
    release_version: String,
    manifest_sha256: String,
    version_directory: PathBuf,
    artifact_file_names: std::collections::BTreeMap<ProductArtifactRole, String>,
}

impl InstalledCurlProduct {
    pub fn release_sequence(&self) -> u64 {
        self.release_sequence
    }

    pub fn release_version(&self) -> &str {
        &self.release_version
    }

    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }

    pub fn version_directory(&self) -> &Path {
        &self.version_directory
    }

    pub fn artifact_path(&self, role: ProductArtifactRole) -> Option<PathBuf> {
        self.artifact_file_names
            .get(&role)
            .map(|file_name| self.version_directory.join(file_name))
    }
}

#[derive(Debug)]
pub struct InstalledBootableCurlProduct {
    product: InstalledCurlProduct,
    guest_release_sequence: u64,
    guest_release_version: String,
    guest_artifact_file_names: BTreeMap<GuestArtifactRole, String>,
}

impl InstalledBootableCurlProduct {
    pub fn product(&self) -> &InstalledCurlProduct {
        &self.product
    }

    pub fn guest_release_sequence(&self) -> u64 {
        self.guest_release_sequence
    }

    pub fn guest_release_version(&self) -> &str {
        &self.guest_release_version
    }

    pub fn guest_artifact_path(&self, role: GuestArtifactRole) -> Option<PathBuf> {
        self.guest_artifact_file_names.get(&role).map(|file_name| {
            self.product
                .version_directory
                .join(CURL_PRODUCT_INSTALLED_GUEST_DIRECTORY)
                .join(file_name)
        })
    }
}

#[derive(Debug)]
pub struct VerifiedInstalledBootableCurlProductRelease {
    product: VerifiedCurlProductRelease,
    guest: VerifiedStagedNativeShadowUpdate,
    guest_files: BTreeMap<GuestArtifactRole, File>,
}

impl VerifiedInstalledBootableCurlProductRelease {
    pub fn product(&self) -> &VerifiedCurlProductRelease {
        &self.product
    }

    pub fn guest(&self) -> &VerifiedStagedNativeShadowUpdate {
        &self.guest
    }

    pub fn guest_artifact_file(&self, role: GuestArtifactRole) -> Option<&File> {
        self.guest_files.get(&role)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedInstalledDirectBootGeneration {
    release_sequence: u64,
    release_version: String,
    manifest_sha256: String,
    guest_release_sequence: u64,
    guest_release_version: String,
    guest_manifest_sha256: String,
}

impl VerifiedInstalledDirectBootGeneration {
    pub fn release_sequence(&self) -> u64 {
        self.release_sequence
    }

    pub fn release_version(&self) -> &str {
        &self.release_version
    }

    pub fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedInstalledDirectBootStatus {
    active: VerifiedInstalledDirectBootGeneration,
    release_floor_sequence: u64,
    release_floor_manifest_sha256: String,
    guest_release_floor_sequence: u64,
    guest_release_floor_manifest_sha256: String,
    rollback: Option<VerifiedInstalledDirectBootGeneration>,
    version_directory_count: u64,
    unreferenced_version_directory_count: u64,
    unexpected_entry_count: u64,
}

impl VerifiedInstalledDirectBootStatus {
    pub fn active(&self) -> &VerifiedInstalledDirectBootGeneration {
        &self.active
    }

    pub fn release_floor_sequence(&self) -> u64 {
        self.release_floor_sequence
    }

    pub fn release_floor_manifest_sha256(&self) -> &str {
        &self.release_floor_manifest_sha256
    }

    pub fn guest_release_floor_sequence(&self) -> u64 {
        self.guest_release_floor_sequence
    }

    pub fn guest_release_floor_manifest_sha256(&self) -> &str {
        &self.guest_release_floor_manifest_sha256
    }

    pub fn rollback(&self) -> Option<&VerifiedInstalledDirectBootGeneration> {
        self.rollback.as_ref()
    }

    pub fn version_directory_count(&self) -> u64 {
        self.version_directory_count
    }

    pub fn unreferenced_version_directory_count(&self) -> u64 {
        self.unreferenced_version_directory_count
    }

    pub fn unexpected_entry_count(&self) -> u64 {
        self.unexpected_entry_count
    }

    pub fn storage_is_clean(&self) -> bool {
        self.unreferenced_version_directory_count == 0 && self.unexpected_entry_count == 0
    }
}

/// Verify a locally staged release bundle end to end and adopt it into the
/// install root. `first_install_minimum_sequence` pins the floor used only
/// when no durable state exists yet; once a state record is present it is the
/// sole floor source and must parse strictly.
pub fn install_curl_product_release(
    install_root: &Path,
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    trust_root: &CurlProductReleaseTrustRoot,
    first_install_minimum_sequence: u64,
    artifact_source_dir: &Path,
) -> Result<InstalledCurlProduct, CurlProductInstallError> {
    let state = read_installed_curl_product_state(install_root)?;
    let floor = match state.as_ref() {
        Some(state) => {
            // Product-only updates may not replace a bootable/direct-boot
            // installation through a weaker signing context.
            open_verified_installed_curl_product_release(install_root, trust_root)?;
            CurlProductReleaseFloor::installed(
                state.release_floor.release_sequence,
                &state.release_floor.manifest_sha256,
            )
            .map_err(|error| {
                CurlProductInstallError::State(format!(
                    "{CURL_PRODUCT_INSTALL_STATE_FILE} is internally inconsistent: {error}"
                ))
            })?
        }
        None => CurlProductReleaseFloor::first_install(first_install_minimum_sequence)?,
    };

    let mut authenticated = authenticate_curl_product_release(
        manifest_raw,
        detached_signature_raw,
        trust_root,
        &floor,
    )?;
    for role in ProductArtifactRole::ALL {
        let file_name = authenticated
            .artifact_file_name(role)
            .expect("an authenticated release declares every role")
            .to_string();
        let path = artifact_source_dir.join(&file_name);
        let file = File::open(&path).map_err(|error| {
            CurlProductInstallError::ArtifactSource(format!(
                "{} cannot be opened: {error}",
                path.display()
            ))
        })?;
        authenticated.verify_artifact(role, file)?;
    }
    let verified = authenticated.finish()?;

    adopt_verified_release(
        install_root,
        manifest_raw,
        detached_signature_raw,
        &verified,
    )
}

/// Verify and atomically install the product-v2 envelope together with the
/// exact twelve-artifact bootable guest it embeds. Neither install state nor
/// staging is created until both signature domains and all eighteen artifact
/// handles have passed their independent contracts.
#[allow(clippy::too_many_arguments)]
pub fn install_bootable_curl_product_release(
    install_root: &Path,
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    product_trust_root: &CurlProductReleaseTrustRoot,
    first_product_sequence: u64,
    product_artifact_source_dir: &Path,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
    first_guest_sequence: u64,
    guest_artifact_source_dir: &Path,
) -> Result<InstalledBootableCurlProduct, CurlProductInstallError> {
    install_bootable_curl_product_release_for_contract(
        install_root,
        manifest_raw,
        detached_signature_raw,
        product_trust_root,
        first_product_sequence,
        product_artifact_source_dir,
        guest_trust_root,
        first_guest_sequence,
        guest_artifact_source_dir,
        InstalledGuestContract::BootableV2,
    )
}

/// Verify and atomically install the product-v3 direct-root successor.
#[allow(clippy::too_many_arguments)]
pub fn install_direct_boot_curl_product_release(
    install_root: &Path,
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    product_trust_root: &CurlProductReleaseTrustRoot,
    first_product_sequence: u64,
    product_artifact_source_dir: &Path,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
    first_guest_sequence: u64,
    guest_artifact_source_dir: &Path,
) -> Result<InstalledBootableCurlProduct, CurlProductInstallError> {
    install_bootable_curl_product_release_for_contract(
        install_root,
        manifest_raw,
        detached_signature_raw,
        product_trust_root,
        first_product_sequence,
        product_artifact_source_dir,
        guest_trust_root,
        first_guest_sequence,
        guest_artifact_source_dir,
        InstalledGuestContract::DirectBootV3,
    )
}

#[allow(clippy::too_many_arguments)]
fn install_bootable_curl_product_release_for_contract(
    install_root: &Path,
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    product_trust_root: &CurlProductReleaseTrustRoot,
    first_product_sequence: u64,
    product_artifact_source_dir: &Path,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
    first_guest_sequence: u64,
    guest_artifact_source_dir: &Path,
    contract: InstalledGuestContract,
) -> Result<InstalledBootableCurlProduct, CurlProductInstallError> {
    let state = read_installed_curl_product_state(install_root)?;
    let (product_floor, guest_floor, rollback_target) = match state.as_ref() {
        Some(state) => {
            let product_floor = CurlProductReleaseFloor::installed(
                state.release_floor.release_sequence,
                &state.release_floor.manifest_sha256,
            )
            .map_err(|error| {
                CurlProductInstallError::State(format!(
                    "{CURL_PRODUCT_INSTALL_STATE_FILE} is internally inconsistent: {error}"
                ))
            })?;
            let verified_active =
                open_verified_installed_bootable_curl_product_release_for_identity(
                    install_root,
                    product_trust_root,
                    guest_trust_root,
                    contract,
                    &state.active_identity(),
                );
            let guest_floor = if let Some(guest_floor) = &state.guest_release_floor {
                NativeShadowUpdateFloor::installed(
                    guest_floor.release_sequence,
                    &guest_floor.manifest_sha256,
                )?
            } else {
                match open_verified_installed_bootable_curl_product_release_for_identity(
                    install_root,
                    product_trust_root,
                    guest_trust_root,
                    contract,
                    &state.active_identity(),
                ) {
                    Ok(active) => NativeShadowUpdateFloor::installed(
                        active.guest().release_sequence(),
                        active.guest().manifest_sha256(),
                    )?,
                    Err(CurlProductInstallError::Verify(
                        CurlProductReleaseVerifyError::InvalidSignatureContext,
                    )) => {
                        open_verified_installed_curl_product_release(
                            install_root,
                            product_trust_root,
                        )?;
                        NativeShadowUpdateFloor::first_install(first_guest_sequence)?
                    }
                    Err(error) => return Err(error),
                }
            };
            let rollback_target = match verified_active {
                Ok(_) => Some(state.active_identity()),
                Err(CurlProductInstallError::Verify(
                    CurlProductReleaseVerifyError::InvalidSignatureContext,
                )) if state.guest_release_floor.is_none() => None,
                Err(error) => return Err(error),
            };
            (product_floor, guest_floor, rollback_target)
        }
        None => (
            CurlProductReleaseFloor::first_install(first_product_sequence)?,
            NativeShadowUpdateFloor::first_install(first_guest_sequence)?,
            None,
        ),
    };

    let (product, guest, guest_files) = verify_bootable_release_sources(
        manifest_raw,
        detached_signature_raw,
        product_trust_root,
        &product_floor,
        product_artifact_source_dir,
        guest_trust_root,
        &guest_floor,
        guest_artifact_source_dir,
        contract,
    )?;
    adopt_verified_bootable_release(
        install_root,
        manifest_raw,
        detached_signature_raw,
        &product,
        &guest,
        &guest_files,
        BootableAdoptionPlan {
            guest_roles: contract.roles(),
            previous_state: state.as_ref(),
            rollback_target,
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn verify_bootable_release_sources(
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    product_trust_root: &CurlProductReleaseTrustRoot,
    product_floor: &CurlProductReleaseFloor,
    product_artifact_source_dir: &Path,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
    guest_floor: &NativeShadowUpdateFloor,
    guest_artifact_source_dir: &Path,
    contract: InstalledGuestContract,
) -> Result<
    (
        VerifiedCurlProductRelease,
        VerifiedStagedNativeShadowUpdate,
        BTreeMap<GuestArtifactRole, File>,
    ),
    CurlProductInstallError,
> {
    let mut authenticated = match contract {
        InstalledGuestContract::BootableV2 => authenticate_bootable_curl_product_release(
            manifest_raw,
            detached_signature_raw,
            product_trust_root,
            product_floor,
        )?,
        InstalledGuestContract::DirectBootV3 => authenticate_direct_boot_curl_product_release(
            manifest_raw,
            detached_signature_raw,
            product_trust_root,
            product_floor,
        )?,
    };
    for role in ProductArtifactRole::ALL {
        let file_name = authenticated
            .artifact_file_name(role)
            .expect("an authenticated release declares every role")
            .to_string();
        let path = product_artifact_source_dir.join(&file_name);
        let file = File::open(&path).map_err(|error| {
            CurlProductInstallError::ArtifactSource(format!(
                "{} cannot be opened: {error}",
                path.display()
            ))
        })?;
        authenticated.verify_artifact(role, file)?;
    }
    let product = authenticated.finish()?;
    let guest_manifest = read_verified_handle(
        product
            .artifact_file(ProductArtifactRole::GuestUpdateManifest)
            .expect("verified product retains guest manifest"),
        MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES,
        "verified guest manifest",
    )?;
    let guest_signature = read_verified_handle(
        product
            .artifact_file(ProductArtifactRole::GuestUpdateSignature)
            .expect("verified product retains guest signature"),
        MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES,
        "verified guest signature",
    )?;
    let mut authenticated_guest = match contract {
        InstalledGuestContract::BootableV2 => authenticate_staged_bootable_native_shadow_update(
            &guest_manifest,
            &guest_signature,
            guest_trust_root,
            guest_floor,
        )?,
        InstalledGuestContract::DirectBootV3 => {
            authenticate_staged_direct_boot_native_shadow_update(
                &guest_manifest,
                &guest_signature,
                guest_trust_root,
                guest_floor,
            )?
        }
    };
    let mut guest_files = BTreeMap::new();
    for &role in contract.roles() {
        let file_name = authenticated_guest
            .artifact_file_name(role)
            .expect("an authenticated guest release declares every role")
            .to_string();
        let path = guest_artifact_source_dir.join(&file_name);
        let file = File::open(&path).map_err(|error| {
            CurlProductInstallError::ArtifactSource(format!(
                "{} cannot be opened: {error}",
                path.display()
            ))
        })?;
        authenticated_guest.verify_artifact(
            role,
            file.try_clone().map_err(|error| {
                CurlProductInstallError::ArtifactSource(format!(
                    "{} handle cannot be retained: {error}",
                    path.display()
                ))
            })?,
        )?;
        guest_files.insert(role, file);
    }
    let guest = authenticated_guest.finish()?;
    require_product_guest_identity(&product, &guest)?;
    Ok((product, guest, guest_files))
}

fn require_product_guest_identity(
    product: &VerifiedCurlProductRelease,
    guest: &VerifiedStagedNativeShadowUpdate,
) -> Result<(), CurlProductInstallError> {
    if product.guest_release_sequence() != guest.release_sequence()
        || product.guest_release_version() != guest.release_version()
        || product.guest_manifest_sha256() != guest.manifest_sha256()
    {
        return Err(CurlProductInstallError::ArtifactSource(
            "product and guest release identities differ".to_string(),
        ));
    }
    Ok(())
}

fn read_verified_handle(
    file: &File,
    cap: usize,
    label: &str,
) -> Result<Vec<u8>, CurlProductInstallError> {
    let mut file = file
        .try_clone()
        .map_err(|error| CurlProductInstallError::Io(format!("clone {label}: {error}")))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| CurlProductInstallError::Io(format!("rewind {label}: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| CurlProductInstallError::Io(format!("inspect {label}: {error}")))?;
    if !metadata.is_file() || metadata.len() > cap as u64 {
        return Err(CurlProductInstallError::ArtifactSource(format!(
            "{label} is not a regular file within its byte cap"
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut bytes)
        .map_err(|error| CurlProductInstallError::Io(format!("read {label}: {error}")))?;
    Ok(bytes)
}

/// Read the durable install state strictly. A missing state file is a clean
/// first-install signal (`Ok(None)`); any present-but-invalid state record is
/// an error, never a silent fallback.
pub fn read_installed_curl_product_state(
    install_root: &Path,
) -> Result<Option<CurlProductInstallState>, CurlProductInstallError> {
    let path = install_root.join(CURL_PRODUCT_INSTALL_STATE_FILE);
    let raw = match fs::read(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(io_error("read install state", &path, error));
        }
    };
    parse_install_state(&raw).map(Some)
}

/// Re-authenticate the exact active release and retain verified artifact file
/// handles for a runtime consumer.
///
/// The durable state chooses the only admissible version directory. The
/// stored manifest and signature are authenticated again, every installed
/// artifact is streamed through the signed size/digest contract, and the
/// returned verifier object retains those exact open handles. The caller must
/// consume those handles rather than reopening a swappable path.
pub fn open_verified_installed_curl_product_release(
    install_root: &Path,
    trust_root: &CurlProductReleaseTrustRoot,
) -> Result<VerifiedCurlProductRelease, CurlProductInstallError> {
    let state = read_installed_curl_product_state(install_root)?.ok_or_else(|| {
        CurlProductInstallError::State("installed release state is absent".to_string())
    })?;
    let expected_directory = format!(
        "{:012}-{}",
        state.release_sequence,
        &state.manifest_sha256[..12]
    );
    if state.version_directory != expected_directory {
        return Err(CurlProductInstallError::State(
            "versionDirectory differs from the active sequence and manifest digest".to_string(),
        ));
    }
    let version_directory = install_root
        .join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY)
        .join(&state.version_directory);
    let manifest_path = version_directory.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE);
    let signature_path = version_directory.join(CURL_PRODUCT_INSTALLED_SIGNATURE_FILE);
    let manifest_raw = read_bounded_installed_file(
        &manifest_path,
        MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES,
        "installed release manifest",
    )?;
    let signature_raw = read_bounded_installed_file(
        &signature_path,
        MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES,
        "installed release signature",
    )?;
    let mut authenticated = authenticate_active_curl_product_release(
        &manifest_raw,
        &signature_raw,
        trust_root,
        state.release_sequence,
        &state.manifest_sha256,
    )?;
    if authenticated.release_version() != state.release_version {
        return Err(CurlProductInstallError::State(
            "releaseVersion differs from the authenticated active manifest".to_string(),
        ));
    }
    for role in ProductArtifactRole::ALL {
        let file_name = authenticated
            .artifact_file_name(role)
            .expect("an authenticated release declares every role");
        let path = version_directory.join(file_name);
        let file = File::open(&path).map_err(|error| {
            CurlProductInstallError::ArtifactSource(format!(
                "installed {} cannot be opened at {}: {error}",
                role.as_str(),
                path.display()
            ))
        })?;
        authenticated.verify_artifact(role, file)?;
    }
    authenticated.finish().map_err(Into::into)
}

pub fn open_verified_installed_bootable_curl_product_release(
    install_root: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
) -> Result<VerifiedInstalledBootableCurlProductRelease, CurlProductInstallError> {
    open_verified_installed_bootable_curl_product_release_for_contract(
        install_root,
        product_trust_root,
        guest_trust_root,
        InstalledGuestContract::BootableV2,
    )
}

pub fn open_verified_installed_direct_boot_curl_product_release(
    install_root: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
) -> Result<VerifiedInstalledBootableCurlProductRelease, CurlProductInstallError> {
    open_verified_installed_bootable_curl_product_release_for_contract(
        install_root,
        product_trust_root,
        guest_trust_root,
        InstalledGuestContract::DirectBootV3,
    )
}

/// Re-authenticate every generation selected by durable direct-boot state and
/// report its rollback floors plus storage residue without mutating the
/// installation. Trust-root key material is consumed only for verification
/// and is never included in the returned status.
pub fn inspect_verified_installed_direct_boot_curl_product_release(
    install_root: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
) -> Result<VerifiedInstalledDirectBootStatus, CurlProductInstallError> {
    let state = read_installed_curl_product_state(install_root)?.ok_or_else(|| {
        CurlProductInstallError::State("installed release state is absent".to_string())
    })?;
    let active_verified = open_verified_installed_bootable_curl_product_release_for_identity(
        install_root,
        product_trust_root,
        guest_trust_root,
        InstalledGuestContract::DirectBootV3,
        &state.active_identity(),
    )?;
    let active = direct_boot_generation_status(&active_verified);
    let rollback = state
        .rollback_release
        .as_ref()
        .map(|identity| {
            open_verified_installed_bootable_curl_product_release_for_identity(
                install_root,
                product_trust_root,
                guest_trust_root,
                InstalledGuestContract::DirectBootV3,
                identity,
            )
            .map(|verified| direct_boot_generation_status(&verified))
        })
        .transpose()?;

    let retained: BTreeSet<&str> = [
        Some(state.version_directory.as_str()),
        state
            .rollback_release
            .as_ref()
            .map(|release| release.version_directory.as_str()),
    ]
    .into_iter()
    .flatten()
    .collect();
    let versions = install_root.join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY);
    let mut version_directory_count = 0_u64;
    let mut unreferenced_version_directory_count = 0_u64;
    let mut unexpected_entry_count = 0_u64;
    for entry in fs::read_dir(&versions)
        .map_err(|error| io_error("read versions directory for inspection", &versions, error))?
    {
        let entry = entry.map_err(|error| {
            CurlProductInstallError::Io(format!(
                "read version directory entry for inspection: {error}"
            ))
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            io_error(
                "inspect version directory entry for status",
                &entry.path(),
                error,
            )
        })?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || !is_version_directory_name(&name)
        {
            unexpected_entry_count += 1;
            continue;
        }
        version_directory_count += 1;
        if !retained.contains(name.as_str()) {
            unreferenced_version_directory_count += 1;
        }
    }

    Ok(VerifiedInstalledDirectBootStatus {
        active,
        release_floor_sequence: state.release_floor_sequence(),
        release_floor_manifest_sha256: state.release_floor_manifest_sha256().to_string(),
        guest_release_floor_sequence: state.guest_release_floor_sequence().ok_or_else(|| {
            CurlProductInstallError::State(
                "direct-boot state is missing its guest release floor".to_string(),
            )
        })?,
        guest_release_floor_manifest_sha256: state
            .guest_release_floor_manifest_sha256()
            .ok_or_else(|| {
                CurlProductInstallError::State(
                    "direct-boot state is missing its guest release floor digest".to_string(),
                )
            })?
            .to_string(),
        rollback,
        version_directory_count,
        unreferenced_version_directory_count,
        unexpected_entry_count,
    })
}

fn direct_boot_generation_status(
    verified: &VerifiedInstalledBootableCurlProductRelease,
) -> VerifiedInstalledDirectBootGeneration {
    VerifiedInstalledDirectBootGeneration {
        release_sequence: verified.product().release_sequence(),
        release_version: verified.product().release_version().to_string(),
        manifest_sha256: verified.product().manifest_sha256().to_string(),
        guest_release_sequence: verified.guest().release_sequence(),
        guest_release_version: verified.guest().release_version().to_string(),
        guest_manifest_sha256: verified.guest().manifest_sha256().to_string(),
    }
}

/// Switch the active direct-boot release to the one retained rollback
/// generation while keeping the highest accepted release as the immutable
/// update floor. Both the current and target generations are authenticated
/// through the product and guest trust roots before the single durable state
/// file is replaced.
pub fn rollback_installed_direct_boot_curl_product_release(
    install_root: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
) -> Result<CurlProductInstallState, CurlProductInstallError> {
    let state = read_installed_curl_product_state(install_root)?.ok_or_else(|| {
        CurlProductInstallError::State("installed release state is absent".to_string())
    })?;
    let target = state.rollback_release.clone().ok_or_else(|| {
        CurlProductInstallError::State("no verified rollback generation is retained".to_string())
    })?;

    open_verified_installed_bootable_curl_product_release_for_identity(
        install_root,
        product_trust_root,
        guest_trust_root,
        InstalledGuestContract::DirectBootV3,
        &state.active_identity(),
    )?;
    open_verified_installed_bootable_curl_product_release_for_identity(
        install_root,
        product_trust_root,
        guest_trust_root,
        InstalledGuestContract::DirectBootV3,
        &target,
    )?;

    let previous_active = state.active_identity();
    let rolled_back = state_from_identities(
        target,
        state.release_floor,
        state.guest_release_floor,
        Some(previous_active),
    );
    // The rolled-back state still retains both the selected target and the
    // previous active generation. Pruning against that complete retained set
    // before the atomic state replacement is therefore crash-safe: the old
    // state and the new state both reference every directory that survives.
    // This also converges residue left when an earlier install was killed
    // after its state swap but before its best-effort cleanup completed.
    prune_unreferenced_version_directories(install_root, &rolled_back)?;
    write_install_state(install_root, &rolled_back, true)?;
    Ok(rolled_back)
}

/// Reopen the active direct-boot generation and, only when that verification
/// fails, atomically select the independently verified rollback generation.
/// A malformed durable state or an unverifiable rollback target remains a
/// hard failure. The accepted product and guest floors never move backwards.
pub fn recover_corrupt_installed_direct_boot_curl_product_release(
    install_root: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
) -> Result<CurlProductInstallState, CurlProductInstallError> {
    let state = read_installed_curl_product_state(install_root)?.ok_or_else(|| {
        CurlProductInstallError::State("installed release state is absent".to_string())
    })?;
    if open_verified_installed_bootable_curl_product_release_for_identity(
        install_root,
        product_trust_root,
        guest_trust_root,
        InstalledGuestContract::DirectBootV3,
        &state.active_identity(),
    )
    .is_ok()
    {
        return Ok(state);
    }

    let target = state.rollback_release.clone().ok_or_else(|| {
        CurlProductInstallError::State(
            "active release is invalid and no verified rollback generation is retained".to_string(),
        )
    })?;
    open_verified_installed_bootable_curl_product_release_for_identity(
        install_root,
        product_trust_root,
        guest_trust_root,
        InstalledGuestContract::DirectBootV3,
        &target,
    )?;
    let recovered =
        state_from_identities(target, state.release_floor, state.guest_release_floor, None);
    write_install_state(install_root, &recovered, true)?;
    Ok(recovered)
}

fn open_verified_installed_bootable_curl_product_release_for_contract(
    install_root: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
    contract: InstalledGuestContract,
) -> Result<VerifiedInstalledBootableCurlProductRelease, CurlProductInstallError> {
    let state = read_installed_curl_product_state(install_root)?.ok_or_else(|| {
        CurlProductInstallError::State("installed release state is absent".to_string())
    })?;
    open_verified_installed_bootable_curl_product_release_for_identity(
        install_root,
        product_trust_root,
        guest_trust_root,
        contract,
        &state.active_identity(),
    )
}

fn open_verified_installed_bootable_curl_product_release_for_identity(
    install_root: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
    contract: InstalledGuestContract,
    identity: &InstalledReleaseIdentity,
) -> Result<VerifiedInstalledBootableCurlProductRelease, CurlProductInstallError> {
    let expected_directory = format!(
        "{:012}-{}",
        identity.release_sequence,
        &identity.manifest_sha256[..12]
    );
    if identity.version_directory != expected_directory {
        return Err(CurlProductInstallError::State(
            "versionDirectory differs from the active sequence and manifest digest".to_string(),
        ));
    }
    let version_directory = install_root
        .join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY)
        .join(&identity.version_directory);
    let manifest_raw = read_bounded_installed_file(
        &version_directory.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE),
        MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES,
        "installed bootable release manifest",
    )?;
    let signature_raw = read_bounded_installed_file(
        &version_directory.join(CURL_PRODUCT_INSTALLED_SIGNATURE_FILE),
        MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES,
        "installed bootable release signature",
    )?;
    let mut authenticated = match contract {
        InstalledGuestContract::BootableV2 => authenticate_active_bootable_curl_product_release(
            &manifest_raw,
            &signature_raw,
            product_trust_root,
            identity.release_sequence,
            &identity.manifest_sha256,
        )?,
        InstalledGuestContract::DirectBootV3 => {
            authenticate_active_direct_boot_curl_product_release(
                &manifest_raw,
                &signature_raw,
                product_trust_root,
                identity.release_sequence,
                &identity.manifest_sha256,
            )?
        }
    };
    if authenticated.release_version() != identity.release_version {
        return Err(CurlProductInstallError::State(
            "releaseVersion differs from the authenticated active manifest".to_string(),
        ));
    }
    for role in ProductArtifactRole::ALL {
        let file_name = authenticated
            .artifact_file_name(role)
            .expect("an authenticated release declares every role");
        let path = version_directory.join(file_name);
        let file = File::open(&path).map_err(|error| {
            CurlProductInstallError::ArtifactSource(format!(
                "installed {} cannot be opened at {}: {error}",
                role.as_str(),
                path.display()
            ))
        })?;
        authenticated.verify_artifact(role, file)?;
    }
    let product = authenticated.finish()?;
    let guest_manifest = read_verified_handle(
        product
            .artifact_file(ProductArtifactRole::GuestUpdateManifest)
            .expect("verified product retains guest manifest"),
        MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES,
        "installed guest manifest",
    )?;
    let guest_signature = read_verified_handle(
        product
            .artifact_file(ProductArtifactRole::GuestUpdateSignature)
            .expect("verified product retains guest signature"),
        MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES,
        "installed guest signature",
    )?;
    let mut authenticated_guest = match contract {
        InstalledGuestContract::BootableV2 => {
            authenticate_active_staged_bootable_native_shadow_update(
                &guest_manifest,
                &guest_signature,
                guest_trust_root,
                product.guest_release_sequence(),
                product.guest_manifest_sha256(),
            )?
        }
        InstalledGuestContract::DirectBootV3 => {
            authenticate_active_staged_direct_boot_native_shadow_update(
                &guest_manifest,
                &guest_signature,
                guest_trust_root,
                product.guest_release_sequence(),
                product.guest_manifest_sha256(),
            )?
        }
    };
    let guest_directory = version_directory.join(CURL_PRODUCT_INSTALLED_GUEST_DIRECTORY);
    let mut guest_files = BTreeMap::new();
    for &role in contract.roles() {
        let file_name = authenticated_guest
            .artifact_file_name(role)
            .expect("an authenticated guest release declares every role");
        let path = guest_directory.join(file_name);
        let file = File::open(&path).map_err(|error| {
            CurlProductInstallError::ArtifactSource(format!(
                "installed {} cannot be opened at {}: {error}",
                role.as_str(),
                path.display()
            ))
        })?;
        authenticated_guest.verify_artifact(
            role,
            file.try_clone().map_err(|error| {
                CurlProductInstallError::ArtifactSource(format!(
                    "installed {} handle cannot be retained: {error}",
                    role.as_str()
                ))
            })?,
        )?;
        guest_files.insert(role, file);
    }
    let guest = authenticated_guest.finish()?;
    require_product_guest_identity(&product, &guest)?;
    Ok(VerifiedInstalledBootableCurlProductRelease {
        product,
        guest,
        guest_files,
    })
}

fn read_bounded_installed_file(
    path: &Path,
    cap: usize,
    name: &str,
) -> Result<Vec<u8>, CurlProductInstallError> {
    let file = File::open(path).map_err(|error| io_error(&format!("open {name}"), path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| io_error(&format!("inspect {name}"), path, error))?;
    if !metadata.is_file() || metadata.len() > cap as u64 {
        return Err(CurlProductInstallError::ArtifactSource(format!(
            "{name} is not a regular file within its byte cap"
        )));
    }
    let mut raw = Vec::with_capacity(metadata.len() as usize);
    file.take(cap as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|error| io_error(&format!("read {name}"), path, error))?;
    if raw.len() > cap {
        return Err(CurlProductInstallError::ArtifactSource(format!(
            "{name} exceeds its byte cap while being read"
        )));
    }
    Ok(raw)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CurlProductInstallStateFile {
    schema: String,
    release_sequence: u64,
    release_version: String,
    manifest_sha256: String,
    version_directory: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CurlProductInstallStateFileV2 {
    schema: String,
    active_release: InstalledReleaseIdentity,
    release_floor: InstalledReleaseIdentity,
    guest_release_floor: InstalledGuestReleaseIdentity,
    rollback_release: Option<InstalledReleaseIdentity>,
}

fn parse_install_state(raw: &[u8]) -> Result<CurlProductInstallState, CurlProductInstallError> {
    let value = release_contract_util::parse_canonical_json(
        CURL_PRODUCT_INSTALL_STATE_FILE,
        raw,
        MAX_INSTALL_STATE_BYTES,
    )
    .map_err(|error| match error {
        ContractJsonError::Malformed(message) => CurlProductInstallError::State(message),
        ContractJsonError::NonCanonical(name) => {
            CurlProductInstallError::State(format!("{name} must be canonical JSON"))
        }
    })?;
    match value.get("schema").and_then(serde_json::Value::as_str) {
        Some(CURL_PRODUCT_INSTALL_STATE_SCHEMA) => {
            let parsed: CurlProductInstallStateFile = serde_json::from_value(value)
                .map_err(|error| CurlProductInstallError::State(error.to_string()))?;
            if parsed.schema != CURL_PRODUCT_INSTALL_STATE_SCHEMA {
                return Err(CurlProductInstallError::State(
                    "unexpected install state schema".to_string(),
                ));
            }
            let active = InstalledReleaseIdentity {
                release_sequence: parsed.release_sequence,
                release_version: parsed.release_version,
                manifest_sha256: parsed.manifest_sha256,
                version_directory: parsed.version_directory,
            };
            validate_installed_release_identity("activeRelease", &active)?;
            Ok(state_from_identities(active.clone(), active, None, None))
        }
        Some(CURL_PRODUCT_INSTALL_STATE_SCHEMA_V2) => {
            let parsed: CurlProductInstallStateFileV2 = serde_json::from_value(value)
                .map_err(|error| CurlProductInstallError::State(error.to_string()))?;
            if parsed.schema != CURL_PRODUCT_INSTALL_STATE_SCHEMA_V2 {
                return Err(CurlProductInstallError::State(
                    "unexpected install state schema".to_string(),
                ));
            }
            validate_installed_release_identity("activeRelease", &parsed.active_release)?;
            validate_installed_release_identity("releaseFloor", &parsed.release_floor)?;
            validate_installed_guest_release_identity(&parsed.guest_release_floor)?;
            if let Some(rollback) = &parsed.rollback_release {
                validate_installed_release_identity("rollbackRelease", rollback)?;
            }
            if parsed.active_release.release_sequence > parsed.release_floor.release_sequence {
                return Err(CurlProductInstallError::State(
                    "activeRelease cannot exceed releaseFloor".to_string(),
                ));
            }
            require_same_sequence_identity(
                "activeRelease",
                &parsed.active_release,
                "releaseFloor",
                &parsed.release_floor,
            )?;
            if let Some(rollback) = &parsed.rollback_release {
                if rollback.release_sequence > parsed.release_floor.release_sequence {
                    return Err(CurlProductInstallError::State(
                        "rollbackRelease cannot exceed releaseFloor".to_string(),
                    ));
                }
                require_same_sequence_identity(
                    "rollbackRelease",
                    rollback,
                    "releaseFloor",
                    &parsed.release_floor,
                )?;
                require_same_sequence_identity(
                    "activeRelease",
                    &parsed.active_release,
                    "rollbackRelease",
                    rollback,
                )?;
            }
            Ok(state_from_identities(
                parsed.active_release,
                parsed.release_floor,
                Some(parsed.guest_release_floor),
                parsed.rollback_release,
            ))
        }
        _ => Err(CurlProductInstallError::State(
            "unexpected install state schema".to_string(),
        )),
    }
}

fn state_from_identities(
    active: InstalledReleaseIdentity,
    release_floor: InstalledReleaseIdentity,
    guest_release_floor: Option<InstalledGuestReleaseIdentity>,
    rollback_release: Option<InstalledReleaseIdentity>,
) -> CurlProductInstallState {
    CurlProductInstallState {
        release_sequence: active.release_sequence,
        release_version: active.release_version,
        manifest_sha256: active.manifest_sha256,
        version_directory: active.version_directory,
        release_floor,
        guest_release_floor,
        rollback_release,
    }
}

fn validate_installed_guest_release_identity(
    identity: &InstalledGuestReleaseIdentity,
) -> Result<(), CurlProductInstallError> {
    if identity.release_sequence == 0 {
        return Err(CurlProductInstallError::State(
            "guestReleaseFloor.releaseSequence must be non-zero".to_string(),
        ));
    }
    release_contract_util::check_sha256(
        "guestReleaseFloor.manifestSha256",
        &identity.manifest_sha256,
    )
    .map_err(CurlProductInstallError::State)
}

fn validate_installed_release_identity(
    label: &str,
    identity: &InstalledReleaseIdentity,
) -> Result<(), CurlProductInstallError> {
    if identity.release_sequence == 0 {
        return Err(CurlProductInstallError::State(format!(
            "{label}.releaseSequence must be non-zero"
        )));
    }
    release_contract_util::check_safe_identifier(
        &format!("{label}.releaseVersion"),
        &identity.release_version,
    )
    .map_err(CurlProductInstallError::State)?;
    release_contract_util::check_sha256(
        &format!("{label}.manifestSha256"),
        &identity.manifest_sha256,
    )
    .map_err(CurlProductInstallError::State)?;
    release_contract_util::check_safe_identifier(
        &format!("{label}.versionDirectory"),
        &identity.version_directory,
    )
    .map_err(CurlProductInstallError::State)?;
    let expected = format!(
        "{:012}-{}",
        identity.release_sequence,
        &identity.manifest_sha256[..12]
    );
    if identity.version_directory != expected {
        return Err(CurlProductInstallError::State(format!(
            "{label}.versionDirectory differs from its sequence and manifest digest"
        )));
    }
    Ok(())
}

fn require_same_sequence_identity(
    left_label: &str,
    left: &InstalledReleaseIdentity,
    right_label: &str,
    right: &InstalledReleaseIdentity,
) -> Result<(), CurlProductInstallError> {
    if left.release_sequence == right.release_sequence && left != right {
        return Err(CurlProductInstallError::State(format!(
            "{left_label} and {right_label} disagree at the same release sequence"
        )));
    }
    Ok(())
}

fn release_identity_json(identity: &InstalledReleaseIdentity) -> serde_json::Value {
    json!({
        "releaseSequence": identity.release_sequence,
        "releaseVersion": identity.release_version,
        "manifestSha256": identity.manifest_sha256,
        "versionDirectory": identity.version_directory,
    })
}

fn write_install_state(
    install_root: &Path,
    state: &CurlProductInstallState,
    lifecycle_v2: bool,
) -> Result<(), CurlProductInstallError> {
    let active = state.active_identity();
    let state_bytes = if lifecycle_v2 {
        let guest_floor = state.guest_release_floor.as_ref().ok_or_else(|| {
            CurlProductInstallError::State(
                "guestReleaseFloor is required by lifecycle state v2".to_string(),
            )
        })?;
        canonicalize(&json!({
            "schema": CURL_PRODUCT_INSTALL_STATE_SCHEMA_V2,
            "activeRelease": release_identity_json(&active),
            "releaseFloor": release_identity_json(&state.release_floor),
            "guestReleaseFloor": {
                "releaseSequence": guest_floor.release_sequence,
                "manifestSha256": guest_floor.manifest_sha256,
            },
            "rollbackRelease": state.rollback_release.as_ref().map(release_identity_json),
        }))
    } else {
        canonicalize(&json!({
            "schema": CURL_PRODUCT_INSTALL_STATE_SCHEMA,
            "releaseSequence": active.release_sequence,
            "releaseVersion": active.release_version,
            "manifestSha256": active.manifest_sha256,
            "versionDirectory": active.version_directory,
        }))
    };
    let temp_path = install_root.join(CURL_PRODUCT_INSTALL_STATE_TEMP_FILE);
    let state_path = install_root.join(CURL_PRODUCT_INSTALL_STATE_FILE);
    write_durable(&temp_path, &state_bytes)?;
    fs::rename(&temp_path, &state_path)
        .map_err(|error| io_error("adopt install state", &state_path, error))?;
    sync_dir(install_root)
}

struct BootableAdoptionPlan<'a> {
    guest_roles: &'static [GuestArtifactRole],
    previous_state: Option<&'a CurlProductInstallState>,
    rollback_target: Option<InstalledReleaseIdentity>,
}

fn adopt_verified_bootable_release(
    install_root: &Path,
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    product: &VerifiedCurlProductRelease,
    guest: &VerifiedStagedNativeShadowUpdate,
    guest_files: &BTreeMap<GuestArtifactRole, File>,
    plan: BootableAdoptionPlan<'_>,
) -> Result<InstalledBootableCurlProduct, CurlProductInstallError> {
    let manifest_sha256 = product.manifest_sha256().to_string();
    let version_directory_name = format!(
        "{:012}-{}",
        product.release_sequence(),
        &manifest_sha256[..12]
    );
    fs::create_dir_all(install_root)
        .map_err(|error| io_error("create install root", install_root, error))?;
    let versions_dir = install_root.join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY);
    fs::create_dir_all(&versions_dir)
        .map_err(|error| io_error("create versions directory", &versions_dir, error))?;
    let staging_root = install_root.join(CURL_PRODUCT_INSTALL_STAGING_DIRECTORY);
    remove_dir_if_present(&staging_root)?;
    let stage_dir = staging_root.join(&version_directory_name);
    fs::create_dir_all(&stage_dir)
        .map_err(|error| io_error("create bootable staging directory", &stage_dir, error))?;

    let mut product_file_names = BTreeMap::new();
    for role in ProductArtifactRole::ALL {
        let file_name = product
            .artifact_file_name(role)
            .expect("a verified product declares every role")
            .to_string();
        let installed_path = stage_dir.join(&file_name);
        copy_retained_file(
            product
                .artifact_file(role)
                .expect("a verified product retains every artifact handle"),
            &installed_path,
            &file_name,
        )?;
        if matches!(
            role,
            ProductArtifactRole::HostCli
                | ProductArtifactRole::HostNode
                | ProductArtifactRole::HostWalletAgent
                | ProductArtifactRole::HostController
        ) {
            fs::set_permissions(&installed_path, fs::Permissions::from_mode(0o555)).map_err(
                |error| {
                    io_error(
                        "make installed host artifact executable",
                        &installed_path,
                        error,
                    )
                },
            )?;
        }
        product_file_names.insert(role, file_name);
    }
    write_durable(
        &stage_dir.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE),
        manifest_raw,
    )?;
    write_durable(
        &stage_dir.join(CURL_PRODUCT_INSTALLED_SIGNATURE_FILE),
        detached_signature_raw,
    )?;

    let guest_dir = stage_dir.join(CURL_PRODUCT_INSTALLED_GUEST_DIRECTORY);
    fs::create_dir(&guest_dir)
        .map_err(|error| io_error("create staged guest directory", &guest_dir, error))?;
    let mut guest_file_names = BTreeMap::new();
    for &role in plan.guest_roles {
        let file_name = guest
            .artifact_file_name(role)
            .expect("a verified guest release declares every role")
            .to_string();
        copy_retained_file(
            guest_files
                .get(&role)
                .expect("a verified guest release retains every artifact handle"),
            &guest_dir.join(&file_name),
            &file_name,
        )?;
        guest_file_names.insert(role, file_name);
    }
    sync_dir(&guest_dir)?;
    sync_dir(&stage_dir)?;

    let final_dir = versions_dir.join(&version_directory_name);
    remove_dir_if_present(&final_dir)?;
    fs::rename(&stage_dir, &final_dir)
        .map_err(|error| io_error("adopt bootable version directory", &final_dir, error))?;
    sync_dir(&versions_dir)?;

    let active = InstalledReleaseIdentity {
        release_sequence: product.release_sequence(),
        release_version: product.release_version().to_string(),
        manifest_sha256: manifest_sha256.clone(),
        version_directory: version_directory_name.clone(),
    };
    let state = match plan.previous_state {
        Some(_previous) => state_from_identities(
            active.clone(),
            active,
            Some(InstalledGuestReleaseIdentity {
                release_sequence: guest.release_sequence(),
                manifest_sha256: guest.manifest_sha256().to_string(),
            }),
            plan.rollback_target,
        ),
        None => state_from_identities(active.clone(), active, None, None),
    };
    write_install_state(install_root, &state, plan.previous_state.is_some())?;
    prune_unreferenced_version_directories(install_root, &state)?;
    let _ = fs::remove_dir_all(&staging_root);

    Ok(InstalledBootableCurlProduct {
        product: InstalledCurlProduct {
            release_sequence: product.release_sequence(),
            release_version: product.release_version().to_string(),
            manifest_sha256,
            version_directory: final_dir,
            artifact_file_names: product_file_names,
        },
        guest_release_sequence: guest.release_sequence(),
        guest_release_version: guest.release_version().to_string(),
        guest_artifact_file_names: guest_file_names,
    })
}

fn prune_unreferenced_version_directories(
    install_root: &Path,
    state: &CurlProductInstallState,
) -> Result<(), CurlProductInstallError> {
    let retained: BTreeSet<&str> = [
        Some(state.version_directory.as_str()),
        Some(state.release_floor.version_directory.as_str()),
        state
            .rollback_release
            .as_ref()
            .map(|release| release.version_directory.as_str()),
    ]
    .into_iter()
    .flatten()
    .collect();
    let versions = install_root.join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY);
    for entry in fs::read_dir(&versions)
        .map_err(|error| io_error("read versions directory", &versions, error))?
    {
        let entry = entry.map_err(|error| {
            CurlProductInstallError::Io(format!("read version directory entry: {error}"))
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if retained.contains(name.as_str()) {
            continue;
        }
        if !is_version_directory_name(&name) {
            return Err(CurlProductInstallError::State(format!(
                "unrecognized entry in versions directory: {name}"
            )));
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
            io_error(
                "inspect unreferenced version directory",
                &entry.path(),
                error,
            )
        })?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(CurlProductInstallError::State(format!(
                "unreferenced version entry is not a directory: {name}"
            )));
        }
        fs::remove_dir_all(entry.path()).map_err(|error| {
            io_error(
                "remove unreferenced version directory",
                &entry.path(),
                error,
            )
        })?;
    }
    sync_dir(&versions)
}

fn is_version_directory_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 25
        && bytes[..12].iter().all(u8::is_ascii_digit)
        && bytes[12] == b'-'
        && bytes[13..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn copy_retained_file(
    source: &File,
    target_path: &Path,
    label: &str,
) -> Result<(), CurlProductInstallError> {
    let mut source = source
        .try_clone()
        .map_err(|error| CurlProductInstallError::Io(format!("clone {label}: {error}")))?;
    source
        .seek(SeekFrom::Start(0))
        .map_err(|error| CurlProductInstallError::Io(format!("rewind {label}: {error}")))?;
    let mut target = File::create(target_path)
        .map_err(|error| io_error("create retained artifact copy", target_path, error))?;
    std::io::copy(&mut source, &mut target)
        .map_err(|error| io_error("copy retained artifact", target_path, error))?;
    target
        .sync_all()
        .map_err(|error| io_error("sync retained artifact", target_path, error))
}

/// Copy the verified release into `staging/`, flip it into `versions/` with a
/// single rename, then replace the state record via a fsynced temp-file
/// rename. Every byte is taken from the verifier's retained handles, so a
/// source-directory swap after verification cannot change what is adopted.
fn adopt_verified_release(
    install_root: &Path,
    manifest_raw: &[u8],
    detached_signature_raw: &[u8],
    verified: &VerifiedCurlProductRelease,
) -> Result<InstalledCurlProduct, CurlProductInstallError> {
    let manifest_sha256 = verified.manifest_sha256().to_string();
    let version_directory_name = format!(
        "{:012}-{}",
        verified.release_sequence(),
        &manifest_sha256[..12]
    );

    fs::create_dir_all(install_root)
        .map_err(|error| io_error("create install root", install_root, error))?;
    let versions_dir = install_root.join(CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY);
    fs::create_dir_all(&versions_dir)
        .map_err(|error| io_error("create versions directory", &versions_dir, error))?;
    let staging_root = install_root.join(CURL_PRODUCT_INSTALL_STAGING_DIRECTORY);
    remove_dir_if_present(&staging_root)?;
    let stage_dir = staging_root.join(&version_directory_name);
    fs::create_dir_all(&stage_dir)
        .map_err(|error| io_error("create staging directory", &stage_dir, error))?;

    let mut artifact_file_names = BTreeMap::new();
    for role in ProductArtifactRole::ALL {
        let file_name = verified
            .artifact_file_name(role)
            .expect("a verified release declares every role")
            .to_string();
        let mut source = verified
            .artifact_file(role)
            .expect("a verified release retains every artifact handle");
        source.seek(SeekFrom::Start(0)).map_err(|error| {
            CurlProductInstallError::Io(format!(
                "rewind verified {file_name} handle failed: {error}"
            ))
        })?;
        let target_path = stage_dir.join(&file_name);
        let mut target = File::create(&target_path)
            .map_err(|error| io_error("create staged artifact", &target_path, error))?;
        std::io::copy(&mut source, &mut target)
            .map_err(|error| io_error("copy verified artifact", &target_path, error))?;
        target
            .sync_all()
            .map_err(|error| io_error("sync staged artifact", &target_path, error))?;
        artifact_file_names.insert(role, file_name);
    }
    write_durable(
        &stage_dir.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE),
        manifest_raw,
    )?;
    write_durable(
        &stage_dir.join(CURL_PRODUCT_INSTALLED_SIGNATURE_FILE),
        detached_signature_raw,
    )?;
    sync_dir(&stage_dir)?;

    // A directory already at the final name can only be crash residue: the
    // durable state never points at a sequence the floor would re-admit.
    let final_dir = versions_dir.join(&version_directory_name);
    remove_dir_if_present(&final_dir)?;
    fs::rename(&stage_dir, &final_dir)
        .map_err(|error| io_error("adopt version directory", &final_dir, error))?;
    sync_dir(&versions_dir)?;

    let state_bytes = canonicalize(&json!({
        "schema": CURL_PRODUCT_INSTALL_STATE_SCHEMA,
        "releaseSequence": verified.release_sequence(),
        "releaseVersion": verified.release_version(),
        "manifestSha256": manifest_sha256,
        "versionDirectory": version_directory_name,
    }));
    let temp_path = install_root.join(CURL_PRODUCT_INSTALL_STATE_TEMP_FILE);
    let state_path = install_root.join(CURL_PRODUCT_INSTALL_STATE_FILE);
    write_durable(&temp_path, &state_bytes)?;
    fs::rename(&temp_path, &state_path)
        .map_err(|error| io_error("adopt install state", &state_path, error))?;
    sync_dir(install_root)?;
    let _ = fs::remove_dir_all(&staging_root);

    Ok(InstalledCurlProduct {
        release_sequence: verified.release_sequence(),
        release_version: verified.release_version().to_string(),
        manifest_sha256,
        version_directory: final_dir,
        artifact_file_names,
    })
}

fn remove_dir_if_present(path: &Path) -> Result<(), CurlProductInstallError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error("remove directory", path, error)),
    }
}

fn write_durable(path: &Path, bytes: &[u8]) -> Result<(), CurlProductInstallError> {
    let mut file =
        File::create(path).map_err(|error| io_error("create durable file", path, error))?;
    file.write_all(bytes)
        .map_err(|error| io_error("write durable file", path, error))?;
    file.sync_all()
        .map_err(|error| io_error("sync durable file", path, error))
}

fn sync_dir(path: &Path) -> Result<(), CurlProductInstallError> {
    File::open(path)
        .and_then(|dir| dir.sync_all())
        .map_err(|error| io_error("sync directory", path, error))
}

fn io_error(action: &str, path: &Path, error: std::io::Error) -> CurlProductInstallError {
    CurlProductInstallError::Io(format!("{action} {} failed: {error}", path.display()))
}
