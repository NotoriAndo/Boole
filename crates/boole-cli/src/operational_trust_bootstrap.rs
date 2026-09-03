//! Atomic public packaging for a non-production operational trust ceremony.
//!
//! All inputs are bounded public metadata. Verification occurs before output
//! exists and again over the completed staging tree before one rename exposes
//! it. No private key, network request, release artifact, or runtime authority
//! crosses this module.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::{
    verify_operational_release_key_ceremony, OperationalReleaseKeyCeremonyError,
    MAX_OPERATIONAL_RELEASE_KEY_CEREMONY_BYTES,
    MAX_OPERATIONAL_RELEASE_KEY_CEREMONY_SIGNATURES_BYTES,
    MAX_OPERATIONAL_RELEASE_RECOVERY_ROOT_BYTES, MAX_OPERATIONAL_RELEASE_TRUST_POLICY_BYTES,
    MAX_OPERATIONAL_RELEASE_TRUST_POLICY_SIGNATURES_BYTES,
};

pub const TRUST_BOOTSTRAP_RECOVERY_ROOT_FILE: &str = "recovery-root.json";
pub const TRUST_BOOTSTRAP_POLICY_FILE: &str = "trust-policy.json";
pub const TRUST_BOOTSTRAP_POLICY_SIGNATURES_FILE: &str = "trust-policy-signatures.json";
pub const TRUST_BOOTSTRAP_KEY_CEREMONY_FILE: &str = "key-ceremony.json";
pub const TRUST_BOOTSTRAP_KEY_CEREMONY_SIGNATURES_FILE: &str = "key-ceremony-signatures.json";

static PACKAGE_ATTEMPT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct OperationalTrustBootstrapPublicFiles {
    pub recovery_root: Vec<u8>,
    pub trust_policy: Vec<u8>,
    pub trust_policy_signatures: Vec<u8>,
    pub key_ceremony: Vec<u8>,
    pub key_ceremony_signatures: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagedOperationalTrustBootstrap {
    output_root: PathBuf,
    ceremony_id: String,
    ceremony_sha256: String,
    recovery_root_sha256: String,
    trust_policy_sha256: String,
    signer_count: usize,
}

#[derive(Debug, Clone)]
pub struct VerifiedOperationalTrustBootstrap {
    files: OperationalTrustBootstrapPublicFiles,
    ceremony: boole_core::VerifiedOperationalReleaseKeyCeremony,
}

impl VerifiedOperationalTrustBootstrap {
    pub fn public_files(&self) -> &OperationalTrustBootstrapPublicFiles {
        &self.files
    }

    pub fn ceremony(&self) -> &boole_core::VerifiedOperationalReleaseKeyCeremony {
        &self.ceremony
    }
}

impl PackagedOperationalTrustBootstrap {
    pub fn output_root(&self) -> &Path {
        &self.output_root
    }

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

#[derive(Debug, thiserror::Error)]
pub enum OperationalTrustBootstrapError {
    #[error("trust bootstrap rejected: {0}")]
    Verify(#[from] OperationalReleaseKeyCeremonyError),
    #[error("trust bootstrap recovery-root pin rejected: {0}")]
    Pin(String),
    #[error("trust bootstrap layout rejected: {0}")]
    Layout(String),
    #[error("trust bootstrap I/O failed: {0}")]
    Io(String),
}

/// Open an exact public bootstrap package and require a recovery-root digest
/// learned outside that package. The same-channel package never chooses its
/// own trust anchor.
pub fn open_operational_trust_bootstrap(
    root: &Path,
    expected_recovery_root_sha256: &str,
) -> Result<VerifiedOperationalTrustBootstrap, OperationalTrustBootstrapError> {
    boole_core::Hex32::from_hex(expected_recovery_root_sha256).map_err(|_| {
        OperationalTrustBootstrapError::Pin(
            "expected recovery-root SHA-256 must be 64 lowercase hexadecimal characters"
                .to_string(),
        )
    })?;
    let files = read_exact_package(root)?;
    let ceremony = verify_files(&files)?;
    if ceremony.recovery_root_sha256() != expected_recovery_root_sha256 {
        return Err(OperationalTrustBootstrapError::Pin(
            "the independently supplied recovery-root SHA-256 does not match the package"
                .to_string(),
        ));
    }
    Ok(VerifiedOperationalTrustBootstrap { files, ceremony })
}

pub fn package_operational_trust_bootstrap(
    output_root: &Path,
    files: &OperationalTrustBootstrapPublicFiles,
) -> Result<PackagedOperationalTrustBootstrap, OperationalTrustBootstrapError> {
    let verified = verify_files(files)?;
    let (parent, output) = validate_output_path(output_root)?;
    let staging = unique_staging_path(&parent, &output)?;
    fs::create_dir(&staging)
        .map_err(|error| io_error("create staging directory", &staging, error))?;
    let mut published = false;
    let outcome = (|| {
        for (name, bytes) in file_entries(files) {
            write_durable(&staging.join(name), bytes)?;
        }
        let staged_files = read_exact_package(&staging)?;
        let staged = verify_files(&staged_files)?;
        if staged.ceremony_sha256() != verified.ceremony_sha256()
            || staged.recovery_root_sha256() != verified.recovery_root_sha256()
            || staged.trust_policy_sha256() != verified.trust_policy_sha256()
        {
            return Err(OperationalTrustBootstrapError::Layout(
                "staged public documents differ from the verified inputs".to_string(),
            ));
        }
        make_tree_read_only(&staging)?;
        sync_directory(&staging)?;
        if output.exists() {
            return Err(OperationalTrustBootstrapError::Layout(format!(
                "output already exists: {}",
                output.display()
            )));
        }
        fs::rename(&staging, &output)
            .map_err(|error| io_error("publish staged trust bootstrap", &output, error))?;
        published = true;
        sync_directory(&parent)?;
        Ok(PackagedOperationalTrustBootstrap {
            output_root: output.clone(),
            ceremony_id: staged.ceremony_id().to_string(),
            ceremony_sha256: staged.ceremony_sha256().to_string(),
            recovery_root_sha256: staged.recovery_root_sha256().to_string(),
            trust_policy_sha256: staged.trust_policy_sha256().to_string(),
            signer_count: staged.signer_count(),
        })
    })();
    if outcome.is_err() {
        let cleanup = if published { &output } else { &staging };
        let _ = make_tree_writable(cleanup);
        let _ = fs::remove_dir_all(cleanup);
        if published {
            let _ = sync_directory(&parent);
        }
    }
    outcome
}

fn verify_files(
    files: &OperationalTrustBootstrapPublicFiles,
) -> Result<boole_core::VerifiedOperationalReleaseKeyCeremony, OperationalTrustBootstrapError> {
    verify_operational_release_key_ceremony(
        &files.recovery_root,
        &files.trust_policy,
        &files.trust_policy_signatures,
        &files.key_ceremony,
        &files.key_ceremony_signatures,
    )
    .map_err(Into::into)
}

fn file_entries(files: &OperationalTrustBootstrapPublicFiles) -> [(&'static str, &[u8]); 5] {
    [
        (TRUST_BOOTSTRAP_RECOVERY_ROOT_FILE, &files.recovery_root),
        (TRUST_BOOTSTRAP_POLICY_FILE, &files.trust_policy),
        (
            TRUST_BOOTSTRAP_POLICY_SIGNATURES_FILE,
            &files.trust_policy_signatures,
        ),
        (TRUST_BOOTSTRAP_KEY_CEREMONY_FILE, &files.key_ceremony),
        (
            TRUST_BOOTSTRAP_KEY_CEREMONY_SIGNATURES_FILE,
            &files.key_ceremony_signatures,
        ),
    ]
}

fn read_exact_package(
    root: &Path,
) -> Result<OperationalTrustBootstrapPublicFiles, OperationalTrustBootstrapError> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| io_error("inspect trust bootstrap directory", root, error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(OperationalTrustBootstrapError::Layout(
            "trust bootstrap root must be a real directory".to_string(),
        ));
    }
    let expected = BTreeSet::from([
        TRUST_BOOTSTRAP_RECOVERY_ROOT_FILE,
        TRUST_BOOTSTRAP_POLICY_FILE,
        TRUST_BOOTSTRAP_POLICY_SIGNATURES_FILE,
        TRUST_BOOTSTRAP_KEY_CEREMONY_FILE,
        TRUST_BOOTSTRAP_KEY_CEREMONY_SIGNATURES_FILE,
    ]);
    let mut seen = BTreeSet::new();
    for entry in fs::read_dir(root)
        .map_err(|error| io_error("read trust bootstrap directory", root, error))?
    {
        let entry = entry.map_err(|error| {
            OperationalTrustBootstrapError::Io(format!("read trust bootstrap entry: {error}"))
        })?;
        let name = entry.file_name().into_string().map_err(|_| {
            OperationalTrustBootstrapError::Layout(
                "trust bootstrap entry name is not UTF-8".to_string(),
            )
        })?;
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("inspect trust bootstrap entry", &entry.path(), error))?;
        if !expected.contains(name.as_str()) || !file_type.is_file() || file_type.is_symlink() {
            return Err(OperationalTrustBootstrapError::Layout(format!(
                "unexpected or unsafe trust bootstrap entry: {}",
                entry.path().display()
            )));
        }
        seen.insert(name);
    }
    if seen.len() != expected.len() {
        return Err(OperationalTrustBootstrapError::Layout(
            "trust bootstrap package has missing public documents".to_string(),
        ));
    }
    Ok(OperationalTrustBootstrapPublicFiles {
        recovery_root: read_capped(
            &root.join(TRUST_BOOTSTRAP_RECOVERY_ROOT_FILE),
            MAX_OPERATIONAL_RELEASE_RECOVERY_ROOT_BYTES,
        )?,
        trust_policy: read_capped(
            &root.join(TRUST_BOOTSTRAP_POLICY_FILE),
            MAX_OPERATIONAL_RELEASE_TRUST_POLICY_BYTES,
        )?,
        trust_policy_signatures: read_capped(
            &root.join(TRUST_BOOTSTRAP_POLICY_SIGNATURES_FILE),
            MAX_OPERATIONAL_RELEASE_TRUST_POLICY_SIGNATURES_BYTES,
        )?,
        key_ceremony: read_capped(
            &root.join(TRUST_BOOTSTRAP_KEY_CEREMONY_FILE),
            MAX_OPERATIONAL_RELEASE_KEY_CEREMONY_BYTES,
        )?,
        key_ceremony_signatures: read_capped(
            &root.join(TRUST_BOOTSTRAP_KEY_CEREMONY_SIGNATURES_FILE),
            MAX_OPERATIONAL_RELEASE_KEY_CEREMONY_SIGNATURES_BYTES,
        )?,
    })
}

fn read_capped(path: &Path, cap: usize) -> Result<Vec<u8>, OperationalTrustBootstrapError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| io_error("open trust bootstrap file", path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| io_error("inspect trust bootstrap file", path, error))?;
    if !metadata.is_file() {
        return Err(OperationalTrustBootstrapError::Layout(format!(
            "trust bootstrap entry is not a regular file: {}",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    file.take((cap as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| io_error("read trust bootstrap file", path, error))?;
    if bytes.len() > cap {
        return Err(OperationalTrustBootstrapError::Layout(format!(
            "trust bootstrap file exceeds its {cap}-byte cap: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

fn validate_output_path(
    output: &Path,
) -> Result<(PathBuf, PathBuf), OperationalTrustBootstrapError> {
    if output.exists() {
        return Err(OperationalTrustBootstrapError::Layout(format!(
            "output already exists: {}",
            output.display()
        )));
    }
    let name = output.file_name().ok_or_else(|| {
        OperationalTrustBootstrapError::Layout("output root needs a final name".to_string())
    })?;
    if name == "." || name == ".." {
        return Err(OperationalTrustBootstrapError::Layout(
            "output root name is unsafe".to_string(),
        ));
    }
    let parent = output.parent().ok_or_else(|| {
        OperationalTrustBootstrapError::Layout("output root needs an existing parent".to_string())
    })?;
    let parent = fs::canonicalize(if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    })
    .map_err(|error| io_error("canonicalize output parent", parent, error))?;
    Ok((parent.clone(), parent.join(name)))
}

fn unique_staging_path(
    parent: &Path,
    output: &Path,
) -> Result<PathBuf, OperationalTrustBootstrapError> {
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            OperationalTrustBootstrapError::Layout("output name is not UTF-8".to_string())
        })?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let attempt = PACKAGE_ATTEMPT.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(
        ".{name}.staging-{}-{now}-{attempt}",
        std::process::id()
    )))
}

fn write_durable(path: &Path, bytes: &[u8]) -> Result<(), OperationalTrustBootstrapError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| io_error("create trust bootstrap file", path, error))?;
    file.write_all(bytes)
        .map_err(|error| io_error("write trust bootstrap file", path, error))?;
    file.sync_all()
        .map_err(|error| io_error("sync trust bootstrap file", path, error))
}

fn make_tree_read_only(root: &Path) -> Result<(), OperationalTrustBootstrapError> {
    for entry in
        fs::read_dir(root).map_err(|error| io_error("read staged trust bootstrap", root, error))?
    {
        let path = entry
            .map_err(|error| {
                OperationalTrustBootstrapError::Io(format!("read staged entry: {error}"))
            })?
            .path();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444))
            .map_err(|error| io_error("lock staged trust bootstrap file", &path, error))?;
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o555))
        .map_err(|error| io_error("lock staged trust bootstrap root", root, error))
}

fn make_tree_writable(root: &Path) -> std::io::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    for entry in fs::read_dir(root)? {
        fs::set_permissions(entry?.path(), fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), OperationalTrustBootstrapError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync trust bootstrap directory", path, error))
}

fn io_error(action: &str, path: &Path, error: std::io::Error) -> OperationalTrustBootstrapError {
    OperationalTrustBootstrapError::Io(format!("{action} {}: {error}", path.display()))
}
