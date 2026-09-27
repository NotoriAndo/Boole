//! Local generated-task package re-verification, disconnected from receipt,
//! block, reward, registry and activation paths. Packages contain data only;
//! they cannot select an executable, implementation or resource policy.

use boole_core::{
    CanonicalPackage, LocalPackageStore, LocalPackageStoreConfig, LocalPackageStoreError,
    PackageFile, PackageRoot, PackageSidecarError,
};
use boole_native_rust_meter::tuple_verifier::{self, InputError, Verification};
use thiserror::Error;

pub const MAX_METERED_PACKAGE_BYTES: usize = 16 * 1024;

#[derive(Debug, Error)]
pub enum PackageError {
    #[error("package root does not match the independently expected root")]
    RootMismatch,
    #[error("metered package exceeds its fixed byte limit")]
    TooLarge,
    #[error("package must contain exactly answer.rs, task.json and verifier.json")]
    InvalidLayout,
    #[error("package verifier contract differs from the locally compiled contract")]
    ContractMismatch,
    #[error(transparent)]
    Input(#[from] InputError),
    #[error(transparent)]
    Sidecar(#[from] PackageSidecarError),
}

#[derive(Debug, Error)]
pub enum ReverifyError {
    #[error(transparent)]
    Package(#[from] PackageError),
    #[error(transparent)]
    Store(#[from] LocalPackageStoreError),
}

#[derive(Debug)]
pub enum StoredVerification {
    Verified(Verification),
    /// The exact root is now in the existing durable fetch-intent queue. This
    /// is not an answer verdict; the caller may fetch/retry after a restart.
    RetryableUnavailable,
}

/// Invalid answers can be packaged for independent rejection; malformed task
/// specifications/oversize inputs cannot produce verification evidence.
pub fn build_package(task: &[u8], answer: &[u8]) -> Result<CanonicalPackage, PackageError> {
    tuple_verifier::verify(task, answer)?;
    Ok(CanonicalPackage::new(vec![
        PackageFile::new("task.json", task),
        PackageFile::new("answer.rs", answer),
        PackageFile::new("verifier.json", tuple_verifier::contract_bytes()),
    ])?)
}

/// Every invocation executes the actual fixed-budget interpreter; neither a
/// producer's claimed verdict nor a local successful-read cache is authority.
pub fn verify_package(
    package: &CanonicalPackage,
    expected_root: PackageRoot,
) -> Result<Verification, PackageError> {
    if package.root() != expected_root {
        return Err(PackageError::RootMismatch);
    }
    if package.size_bytes() > MAX_METERED_PACKAGE_BYTES {
        return Err(PackageError::TooLarge);
    }
    let mut files = package.files();
    let (
        Some((b"answer.rs", answer)),
        Some((b"task.json", task)),
        Some((b"verifier.json", contract)),
        None,
    ) = (files.next(), files.next(), files.next(), files.next())
    else {
        return Err(PackageError::InvalidLayout);
    };
    if contract != tuple_verifier::contract_bytes() {
        return Err(PackageError::ContractMismatch);
    }
    Ok(tuple_verifier::verify(task, answer)?)
}

fn reference(root: PackageRoot) -> String {
    format!("metered-tuple:{}", root.to_hex())
}

fn decode_and_verify(
    expected_root: PackageRoot,
    bytes: &[u8],
) -> Result<(CanonicalPackage, Verification), PackageError> {
    if bytes.len() > MAX_METERED_PACKAGE_BYTES {
        return Err(PackageError::TooLarge);
    }
    let package = CanonicalPackage::from_canonical_bytes(bytes)?;
    let verification = verify_package(&package, expected_root)?;
    Ok((package, verification))
}

/// Import only complete, root-bound, locally compatible data. Wrong answers
/// remain useful rejection evidence and can be staged. Existing durable CAS
/// publication/backpressure apply; no successful result is cached or signed.
pub fn import_package(
    store: &mut LocalPackageStore,
    expected_root: PackageRoot,
    canonical_bytes: &[u8],
) -> Result<(), ReverifyError> {
    let (package, _) = decode_and_verify(expected_root, canonical_bytes)?;
    let reference = reference(expected_root);
    store.stage(&package, &reference)?;
    store.complete_fetch_intent(expected_root, &reference)?;
    Ok(())
}

/// Explicit offline restoration for a previously staged object lost from disk.
/// This never relaxes ordinary CAS open or rewrites a present corrupt object.
pub fn restore_package(
    store_path: impl AsRef<std::path::Path>,
    config: LocalPackageStoreConfig,
    expected_root: PackageRoot,
    canonical_bytes: &[u8],
) -> Result<(), ReverifyError> {
    let (package, _) = decode_and_verify(expected_root, canonical_bytes)?;
    LocalPackageStore::restore_missing_object(store_path, config, &package)?;
    Ok(())
}

/// The caller exclusively owns this store (including across processes). A
/// successful read is never evidence: re-run verification on every invocation.
/// Storage corruption is an error, not a deterministic rejection of the answer.
pub fn reverify_stored(
    store: &mut LocalPackageStore,
    expected_root: PackageRoot,
) -> Result<StoredVerification, ReverifyError> {
    let bytes = match store.read(expected_root) {
        Ok(bytes) => bytes,
        Err(LocalPackageStoreError::MissingObject { .. }) => {
            store.register_fetch_intents(&[(expected_root, reference(expected_root))])?;
            return Ok(StoredVerification::RetryableUnavailable);
        }
        Err(error) => return Err(error.into()),
    };
    let (_, verification) = decode_and_verify(expected_root, &bytes)?;
    Ok(StoredVerification::Verified(verification))
}
