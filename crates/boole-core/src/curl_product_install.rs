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

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;
use thiserror::Error;

use crate::canonicalize;
use crate::curl_product_release::{
    authenticate_curl_product_release, CurlProductReleaseFloor, CurlProductReleaseTrustRoot,
    CurlProductReleaseVerifyError, ProductArtifactRole, VerifiedCurlProductRelease,
};
use crate::release_contract_util::{self, ContractJsonError};

pub const CURL_PRODUCT_INSTALL_STATE_SCHEMA: &str = "boole.curl-product-install-state.v1";
pub const CURL_PRODUCT_INSTALL_STATE_FILE: &str = "installed-release.json";
pub const CURL_PRODUCT_INSTALL_STATE_TEMP_FILE: &str = "installed-release.json.tmp";
pub const CURL_PRODUCT_INSTALL_VERSIONS_DIRECTORY: &str = "versions";
pub const CURL_PRODUCT_INSTALL_STAGING_DIRECTORY: &str = "staging";
pub const CURL_PRODUCT_INSTALLED_MANIFEST_FILE: &str = "release-manifest.json";
pub const CURL_PRODUCT_INSTALLED_SIGNATURE_FILE: &str = "release-signature.json";
const MAX_INSTALL_STATE_BYTES: usize = 4_096;

#[derive(Debug, Error)]
pub enum CurlProductInstallError {
    #[error("release rejected: {0}")]
    Verify(#[from] CurlProductReleaseVerifyError),
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
    let floor = match read_installed_curl_product_state(install_root)? {
        Some(state) => {
            CurlProductReleaseFloor::installed(state.release_sequence, &state.manifest_sha256)
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CurlProductInstallStateFile {
    schema: String,
    release_sequence: u64,
    release_version: String,
    manifest_sha256: String,
    version_directory: String,
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
    let parsed: CurlProductInstallStateFile = serde_json::from_value(value)
        .map_err(|error| CurlProductInstallError::State(error.to_string()))?;
    if parsed.schema != CURL_PRODUCT_INSTALL_STATE_SCHEMA {
        return Err(CurlProductInstallError::State(
            "unexpected install state schema".to_string(),
        ));
    }
    if parsed.release_sequence == 0 {
        return Err(CurlProductInstallError::State(
            "releaseSequence must be a non-zero sequence".to_string(),
        ));
    }
    release_contract_util::check_safe_identifier("releaseVersion", &parsed.release_version)
        .map_err(CurlProductInstallError::State)?;
    release_contract_util::check_sha256("manifestSha256", &parsed.manifest_sha256)
        .map_err(CurlProductInstallError::State)?;
    release_contract_util::check_safe_identifier("versionDirectory", &parsed.version_directory)
        .map_err(CurlProductInstallError::State)?;
    Ok(CurlProductInstallState {
        release_sequence: parsed.release_sequence,
        release_version: parsed.release_version,
        manifest_sha256: parsed.manifest_sha256,
        version_directory: parsed.version_directory,
    })
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
