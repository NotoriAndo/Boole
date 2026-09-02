//! Offline packaging for one already-signed direct-boot product release.
//!
//! This module owns no signing key and performs no upload. It authenticates
//! the product and guest signing domains, verifies every declared artifact,
//! copies only from those verified open descriptors, re-verifies the staged
//! tree, and exposes the complete transport tree with one directory rename.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::{
    authenticate_direct_boot_curl_product_release,
    authenticate_staged_direct_boot_native_shadow_update, CurlProductReleaseFloor,
    CurlProductReleaseTrustRoot, CurlProductReleaseVerifyError, GuestArtifactRole,
    NativeShadowUpdateFloor, NativeShadowUpdateTrustRoot, NativeShadowUpdateVerifyError,
    ProductArtifactRole, VerifiedCurlProductRelease, VerifiedStagedNativeShadowUpdate,
    CURL_PRODUCT_INSTALLED_MANIFEST_FILE, CURL_PRODUCT_INSTALLED_SIGNATURE_FILE,
    MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES, MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES,
};

const GUEST_DIRECTORY: &str = "guest";
const MAX_EMBEDDED_GUEST_MANIFEST_BYTES: usize = 1_048_576;
const MAX_EMBEDDED_GUEST_SIGNATURE_BYTES: usize = 4_096;
static PACKAGE_ATTEMPT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagedDirectBootRelease {
    release_sequence: u64,
    release_version: String,
    manifest_sha256: String,
    guest_release_sequence: u64,
    guest_release_version: String,
    guest_manifest_sha256: String,
    file_count: usize,
    output_root: PathBuf,
}

impl PackagedDirectBootRelease {
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

    pub fn file_count(&self) -> usize {
        self.file_count
    }

    pub fn output_root(&self) -> &Path {
        &self.output_root
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DirectBootReleasePackageError {
    #[error("release package layout rejected: {0}")]
    Layout(String),
    #[error("product release rejected: {0}")]
    ProductVerify(#[from] CurlProductReleaseVerifyError),
    #[error("guest release rejected: {0}")]
    GuestVerify(#[from] NativeShadowUpdateVerifyError),
    #[error("release package I/O failed: {0}")]
    Io(String),
}

struct VerifiedBundle {
    product_manifest_raw: Vec<u8>,
    product_signature_raw: Vec<u8>,
    product: VerifiedCurlProductRelease,
    guest: VerifiedStagedNativeShadowUpdate,
    guest_files: BTreeMap<GuestArtifactRole, File>,
}

#[allow(clippy::too_many_arguments)]
pub fn package_direct_boot_curl_product_release(
    source_root: &Path,
    output_root: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    product_floor: &CurlProductReleaseFloor,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
    guest_floor: &NativeShadowUpdateFloor,
) -> Result<PackagedDirectBootRelease, DirectBootReleasePackageError> {
    let (source_root, output_parent, output_root) =
        validate_package_paths(source_root, output_root)?;
    let verified = verify_bundle(
        &source_root,
        product_trust_root,
        product_floor,
        guest_trust_root,
        guest_floor,
    )?;

    let staging = unique_staging_path(&output_parent, &output_root)?;
    fs::create_dir(&staging)
        .map_err(|error| io_error("create staging directory", &staging, error))?;
    let mut published = false;
    let outcome = (|| {
        write_verified_tree(&staging, &verified)?;
        // Verification happens again over the exact bytes that will be
        // published, closing in-place source mutation during the copy.
        let staged = verify_bundle(
            &staging,
            product_trust_root,
            product_floor,
            guest_trust_root,
            guest_floor,
        )?;
        make_tree_read_only(&staging)?;
        sync_directory(&staging.join(GUEST_DIRECTORY))?;
        sync_directory(&staging)?;
        if output_root.exists() {
            return Err(DirectBootReleasePackageError::Layout(format!(
                "output already exists: {}",
                output_root.display()
            )));
        }
        fs::rename(&staging, &output_root)
            .map_err(|error| io_error("publish staged package", &output_root, error))?;
        published = true;
        sync_directory(&output_parent)?;
        Ok(PackagedDirectBootRelease {
            release_sequence: staged.product.release_sequence(),
            release_version: staged.product.release_version().to_string(),
            manifest_sha256: staged.product.manifest_sha256().to_string(),
            guest_release_sequence: staged.guest.release_sequence(),
            guest_release_version: staged.guest.release_version().to_string(),
            guest_manifest_sha256: staged.guest.manifest_sha256().to_string(),
            file_count: 2
                + ProductArtifactRole::ALL.len()
                + GuestArtifactRole::DIRECT_BOOT_ALL.len(),
            output_root: output_root.clone(),
        })
    })();
    if outcome.is_err() {
        let cleanup = if published { &output_root } else { &staging };
        let _ = make_tree_writable_for_cleanup(cleanup);
        let _ = fs::remove_dir_all(cleanup);
        if published {
            let _ = sync_directory(&output_parent);
        }
    }
    outcome
}

fn verify_bundle(
    root: &Path,
    product_trust_root: &CurlProductReleaseTrustRoot,
    product_floor: &CurlProductReleaseFloor,
    guest_trust_root: &NativeShadowUpdateTrustRoot,
    guest_floor: &NativeShadowUpdateFloor,
) -> Result<VerifiedBundle, DirectBootReleasePackageError> {
    let product_manifest_raw = read_capped_regular(
        &root.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE),
        MAX_CURL_PRODUCT_RELEASE_MANIFEST_BYTES,
    )?;
    let product_signature_raw = read_capped_regular(
        &root.join(CURL_PRODUCT_INSTALLED_SIGNATURE_FILE),
        MAX_CURL_PRODUCT_RELEASE_DETACHED_SIGNATURE_BYTES,
    )?;
    let mut authenticated_product = authenticate_direct_boot_curl_product_release(
        &product_manifest_raw,
        &product_signature_raw,
        product_trust_root,
        product_floor,
    )?;
    for role in ProductArtifactRole::ALL {
        let file_name = authenticated_product
            .artifact_file_name(role)
            .ok_or_else(|| missing_role("product", role.as_str()))?;
        authenticated_product.verify_artifact(role, open_regular(&root.join(file_name))?)?;
    }
    let product = authenticated_product.finish()?;

    let guest_manifest_raw = read_retained_capped(
        product
            .artifact_file(ProductArtifactRole::GuestUpdateManifest)
            .ok_or_else(|| {
                missing_role("product", ProductArtifactRole::GuestUpdateManifest.as_str())
            })?,
        MAX_EMBEDDED_GUEST_MANIFEST_BYTES,
        ProductArtifactRole::GuestUpdateManifest.as_str(),
    )?;
    let guest_signature_raw = read_retained_capped(
        product
            .artifact_file(ProductArtifactRole::GuestUpdateSignature)
            .ok_or_else(|| {
                missing_role(
                    "product",
                    ProductArtifactRole::GuestUpdateSignature.as_str(),
                )
            })?,
        MAX_EMBEDDED_GUEST_SIGNATURE_BYTES,
        ProductArtifactRole::GuestUpdateSignature.as_str(),
    )?;
    let mut authenticated_guest = authenticate_staged_direct_boot_native_shadow_update(
        &guest_manifest_raw,
        &guest_signature_raw,
        guest_trust_root,
        guest_floor,
    )?;
    let guest_root = root.join(GUEST_DIRECTORY);
    let mut guest_files = BTreeMap::new();
    for role in GuestArtifactRole::DIRECT_BOOT_ALL {
        let file_name = authenticated_guest
            .artifact_file_name(role)
            .ok_or_else(|| missing_role("guest", role.as_str()))?;
        let file = open_regular(&guest_root.join(file_name))?;
        authenticated_guest.verify_artifact(
            role,
            file.try_clone().map_err(|error| {
                DirectBootReleasePackageError::Io(format!("clone {}: {error}", role.as_str()))
            })?,
        )?;
        guest_files.insert(role, file);
    }
    let guest = authenticated_guest.finish()?;
    require_exact_tree(root, &product, &guest)?;
    Ok(VerifiedBundle {
        product_manifest_raw,
        product_signature_raw,
        product,
        guest,
        guest_files,
    })
}

fn require_exact_tree(
    root: &Path,
    product: &VerifiedCurlProductRelease,
    guest: &VerifiedStagedNativeShadowUpdate,
) -> Result<(), DirectBootReleasePackageError> {
    let mut root_files = BTreeSet::from([
        CURL_PRODUCT_INSTALLED_MANIFEST_FILE.to_string(),
        CURL_PRODUCT_INSTALLED_SIGNATURE_FILE.to_string(),
    ]);
    for role in ProductArtifactRole::ALL {
        let name = product
            .artifact_file_name(role)
            .ok_or_else(|| missing_role("product", role.as_str()))?;
        require_single_file_name(name)?;
        if name == GUEST_DIRECTORY || !root_files.insert(name.to_string()) {
            return Err(DirectBootReleasePackageError::Layout(format!(
                "product file name collides with a reserved package entry: {name}"
            )));
        }
    }
    require_directory_entries(root, &root_files, Some(GUEST_DIRECTORY))?;

    let mut guest_files = BTreeSet::new();
    for role in GuestArtifactRole::DIRECT_BOOT_ALL {
        let name = guest
            .artifact_file_name(role)
            .ok_or_else(|| missing_role("guest", role.as_str()))?;
        require_single_file_name(name)?;
        if !guest_files.insert(name.to_string()) {
            return Err(DirectBootReleasePackageError::Layout(format!(
                "guest file name is repeated: {name}"
            )));
        }
    }
    require_directory_entries(&root.join(GUEST_DIRECTORY), &guest_files, None)
}

fn require_directory_entries(
    directory: &Path,
    expected_files: &BTreeSet<String>,
    expected_directory: Option<&str>,
) -> Result<(), DirectBootReleasePackageError> {
    let metadata = fs::symlink_metadata(directory)
        .map_err(|error| io_error("inspect package directory", directory, error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(DirectBootReleasePackageError::Layout(format!(
            "package entry is not a real directory: {}",
            directory.display()
        )));
    }
    let mut seen_files = BTreeSet::new();
    let mut seen_directory = false;
    for entry in fs::read_dir(directory)
        .map_err(|error| io_error("read package directory", directory, error))?
    {
        let entry = entry.map_err(|error| {
            DirectBootReleasePackageError::Io(format!("read package entry: {error}"))
        })?;
        let name = entry.file_name().into_string().map_err(|_| {
            DirectBootReleasePackageError::Layout("package entry name is not UTF-8".to_string())
        })?;
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("inspect package entry", &entry.path(), error))?;
        if expected_directory == Some(name.as_str()) {
            if !file_type.is_dir() || file_type.is_symlink() || seen_directory {
                return Err(DirectBootReleasePackageError::Layout(format!(
                    "reserved package directory is invalid: {name}"
                )));
            }
            seen_directory = true;
        } else if expected_files.contains(&name) && file_type.is_file() && !file_type.is_symlink() {
            seen_files.insert(name);
        } else {
            return Err(DirectBootReleasePackageError::Layout(format!(
                "unexpected or unsafe package entry: {}",
                entry.path().display()
            )));
        }
    }
    if seen_files != *expected_files || expected_directory.is_some() != seen_directory {
        return Err(DirectBootReleasePackageError::Layout(format!(
            "package directory has missing entries: {}",
            directory.display()
        )));
    }
    Ok(())
}

fn write_verified_tree(
    staging: &Path,
    verified: &VerifiedBundle,
) -> Result<(), DirectBootReleasePackageError> {
    write_durable(
        &staging.join(CURL_PRODUCT_INSTALLED_MANIFEST_FILE),
        &verified.product_manifest_raw,
    )?;
    write_durable(
        &staging.join(CURL_PRODUCT_INSTALLED_SIGNATURE_FILE),
        &verified.product_signature_raw,
    )?;
    for role in ProductArtifactRole::ALL {
        let name = verified
            .product
            .artifact_file_name(role)
            .ok_or_else(|| missing_role("product", role.as_str()))?;
        copy_retained(
            verified
                .product
                .artifact_file(role)
                .ok_or_else(|| missing_role("product", role.as_str()))?,
            &staging.join(name),
            name,
        )?;
    }
    let guest_root = staging.join(GUEST_DIRECTORY);
    fs::create_dir(&guest_root)
        .map_err(|error| io_error("create guest directory", &guest_root, error))?;
    for role in GuestArtifactRole::DIRECT_BOOT_ALL {
        let name = verified
            .guest
            .artifact_file_name(role)
            .ok_or_else(|| missing_role("guest", role.as_str()))?;
        copy_retained(
            verified
                .guest_files
                .get(&role)
                .ok_or_else(|| missing_role("guest", role.as_str()))?,
            &guest_root.join(name),
            name,
        )?;
    }
    Ok(())
}

fn validate_package_paths(
    source_root: &Path,
    output_root: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf), DirectBootReleasePackageError> {
    let source = fs::canonicalize(source_root)
        .map_err(|error| io_error("canonicalize source root", source_root, error))?;
    if output_root.exists() {
        return Err(DirectBootReleasePackageError::Layout(format!(
            "output already exists: {}",
            output_root.display()
        )));
    }
    let output_name = output_root.file_name().ok_or_else(|| {
        DirectBootReleasePackageError::Layout("output root needs a final name".to_string())
    })?;
    if output_name == "." || output_name == ".." {
        return Err(DirectBootReleasePackageError::Layout(
            "output root name is unsafe".to_string(),
        ));
    }
    let parent = output_root.parent().ok_or_else(|| {
        DirectBootReleasePackageError::Layout("output root needs an existing parent".to_string())
    })?;
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let parent = fs::canonicalize(parent)
        .map_err(|error| io_error("canonicalize output parent", parent, error))?;
    let output = parent.join(output_name);
    if output.starts_with(&source) || source.starts_with(&output) {
        return Err(DirectBootReleasePackageError::Layout(
            "source and output trees must not overlap".to_string(),
        ));
    }
    Ok((source, parent, output))
}

fn unique_staging_path(
    parent: &Path,
    output: &Path,
) -> Result<PathBuf, DirectBootReleasePackageError> {
    let output_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            DirectBootReleasePackageError::Layout("output name is not UTF-8".to_string())
        })?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let attempt = PACKAGE_ATTEMPT.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(
        ".{output_name}.staging-{}-{now}-{attempt}",
        std::process::id()
    )))
}

fn require_single_file_name(name: &str) -> Result<(), DirectBootReleasePackageError> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(DirectBootReleasePackageError::Layout(format!(
            "artifact file name is not one safe path component: {name}"
        )));
    }
    Ok(())
}

fn open_regular(path: &Path) -> Result<File, DirectBootReleasePackageError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| io_error("open package file", path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| io_error("inspect opened package file", path, error))?;
    if !metadata.is_file() {
        return Err(DirectBootReleasePackageError::Layout(format!(
            "package entry is not a regular file: {}",
            path.display()
        )));
    }
    Ok(file)
}

fn read_capped_regular(path: &Path, cap: usize) -> Result<Vec<u8>, DirectBootReleasePackageError> {
    let file = open_regular(path)?;
    read_retained_capped(&file, cap, &path.display().to_string())
}

fn read_retained_capped(
    file: &File,
    cap: usize,
    label: &str,
) -> Result<Vec<u8>, DirectBootReleasePackageError> {
    let mut reader = file
        .try_clone()
        .map_err(|error| DirectBootReleasePackageError::Io(format!("clone {label}: {error}")))?;
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| DirectBootReleasePackageError::Io(format!("rewind {label}: {error}")))?;
    let mut bytes = Vec::new();
    reader
        .take((cap as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| DirectBootReleasePackageError::Io(format!("read {label}: {error}")))?;
    if bytes.len() > cap {
        return Err(DirectBootReleasePackageError::Layout(format!(
            "{label} exceeds the {cap}-byte cap"
        )));
    }
    Ok(bytes)
}

fn copy_retained(
    file: &File,
    target: &Path,
    label: &str,
) -> Result<(), DirectBootReleasePackageError> {
    let mut source = file
        .try_clone()
        .map_err(|error| DirectBootReleasePackageError::Io(format!("clone {label}: {error}")))?;
    source
        .seek(SeekFrom::Start(0))
        .map_err(|error| DirectBootReleasePackageError::Io(format!("rewind {label}: {error}")))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(target)
        .map_err(|error| io_error("create staged package file", target, error))?;
    std::io::copy(&mut source, &mut output)
        .map_err(|error| io_error("copy staged package file", target, error))?;
    output
        .sync_all()
        .map_err(|error| io_error("sync staged package file", target, error))
}

fn write_durable(path: &Path, bytes: &[u8]) -> Result<(), DirectBootReleasePackageError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| io_error("create package metadata", path, error))?;
    file.write_all(bytes)
        .map_err(|error| io_error("write package metadata", path, error))?;
    file.sync_all()
        .map_err(|error| io_error("sync package metadata", path, error))
}

fn make_tree_read_only(root: &Path) -> Result<(), DirectBootReleasePackageError> {
    for entry in fs::read_dir(root).map_err(|error| io_error("read staged tree", root, error))? {
        let entry = entry.map_err(|error| {
            DirectBootReleasePackageError::Io(format!("read staged entry: {error}"))
        })?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| io_error("inspect staged entry", &path, error))?;
        if file_type.is_dir() {
            make_tree_read_only(&path)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o555))
                .map_err(|error| io_error("lock staged directory", &path, error))?;
        } else {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o444))
                .map_err(|error| io_error("lock staged file", &path, error))?;
        }
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o555))
        .map_err(|error| io_error("lock staged package root", root, error))?;
    Ok(())
}

fn make_tree_writable_for_cleanup(root: &Path) -> std::io::Result<()> {
    if !root.exists() {
        return Ok(());
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            make_tree_writable_for_cleanup(&path)?;
        } else {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), DirectBootReleasePackageError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync package directory", path, error))
}

fn missing_role(domain: &str, role: &str) -> DirectBootReleasePackageError {
    DirectBootReleasePackageError::Layout(format!("{domain} role is missing: {role}"))
}

fn io_error(action: &str, path: &Path, error: std::io::Error) -> DirectBootReleasePackageError {
    DirectBootReleasePackageError::Io(format!("{action} {}: {error}", path.display()))
}
