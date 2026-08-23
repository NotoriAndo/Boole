//! BF.6a local package availability staging.
//!
//! This module is deliberately disconnected from P2P, blocks, rewards and
//! consensus.  Its default configuration is OFF, making construction and
//! attempted staging a disk no-op until an operator explicitly enables it.
//! Enabled mode requires an already-existing parent, creates at most the final
//! store directory, and retains root/objects directory descriptors so later
//! path or current-directory changes cannot redirect authority. One process
//! must own that directory for the store lifetime; cross-process arbitration
//! is deliberately outside this local, default-OFF slice.

use std::ffi::{CStr, CString, OsStr};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::hash::h_protocol;
use crate::package_sidecar::{
    CanonicalPackage, PackageRoot, MAX_PACKAGE_CANONICAL_BYTES, PACKAGE_SIDECAR_ROOT_DOMAIN,
};

pub const DEFAULT_MAX_PENDING_PACKAGES: usize = 64;
pub const DEFAULT_MAX_PENDING_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_PACKAGE_REFERENCE_BYTES: usize = 256;
pub const PACKAGE_OBJECTS_DIRECTORY: &str = "objects";
pub const PACKAGE_PENDING_FILE: &str = "pending-v1.json";
pub const PACKAGE_PENDING_TEMP_FILE: &str = ".pending-v1.json.tmp";
pub const MAX_PENDING_SNAPSHOT_BYTES: u64 = 4 * 1024 * 1024;
const PACKAGE_PENDING_SCHEMA: &str = "boole.useful-work.package-pending.v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingCapacityPolicy {
    /// Preserve the durable FIFO exactly as written and reject the proposed
    /// newest entry. No implicit data-loss or result-dependent eviction.
    RejectNewest,
}

pub const PENDING_CAPACITY_POLICY: PendingCapacityPolicy = PendingCapacityPolicy::RejectNewest;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPackageStoreConfig {
    pub enabled: bool,
    /// Maximum number of root/reference records in the FIFO.
    pub max_pending_packages: usize,
    /// Maximum sum of `size_bytes` across FIFO records. Repeated references to
    /// one CAS object count repeatedly, which is deterministic and conservative.
    pub max_pending_bytes: u64,
}

impl Default for LocalPackageStoreConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_pending_packages: DEFAULT_MAX_PENDING_PACKAGES,
            max_pending_bytes: DEFAULT_MAX_PENDING_BYTES,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagePackageOutcome {
    Staged,
    AlreadyPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcknowledgePackageOutcome {
    Acknowledged,
    NotPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageStoreCommitPhase {
    Object,
    PendingSnapshot,
    ObjectCleanup,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LocalPackageStoreError {
    #[error("local package store is disabled")]
    Disabled,
    #[error("local package store is poisoned after an outcome-unknown durable commit")]
    Poisoned,
    #[error("local package store commit outcome is unknown at {phase:?}")]
    CommitOutcomeUnknown { phase: PackageStoreCommitPhase },
    #[error("package reference must not be empty")]
    EmptyReference,
    #[error("package reference is {size} bytes; maximum is {max}")]
    ReferenceTooLarge { size: usize, max: usize },
    #[error("pending package count would exceed {max}")]
    PendingCountExceeded { max: usize },
    #[error("pending package bytes would exceed {max}")]
    PendingBytesExceeded { max: u64 },
    #[error("pending package conflicts with an existing root/reference pair")]
    PendingConflict,
    #[error("local package store I/O failed: {0}")]
    Io(String),
    #[error("unsafe local package store path: {0}")]
    UnsafePath(String),
    #[error("CAS object {root} is missing")]
    MissingObject { root: String },
    #[error("pending snapshot is {size} bytes; maximum is {max}")]
    PendingSnapshotTooLarge { size: u64, max: u64 },
    #[error("CAS object is {size} bytes; maximum is {max}")]
    ObjectTooLarge { size: u64, max: u64 },
    #[error("CAS object size mismatch: expected {expected}, got {actual}")]
    ObjectSizeMismatch { expected: u64, actual: u64 },
    #[error("CAS object root mismatch: expected {expected}, got {actual}")]
    ObjectRootMismatch { expected: String, actual: String },
    #[error("local package store is corrupt: {0}")]
    Corrupt(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPackageRef {
    root: PackageRoot,
    size_bytes: u64,
    reference: String,
}

impl PendingPackageRef {
    pub fn root(&self) -> PackageRoot {
        self.root
    }

    pub fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPendingPackageRef {
    root: String,
    size_bytes: u64,
    reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingSnapshot {
    schema: String,
    entries: Vec<StoredPendingPackageRef>,
}

pub struct LocalPackageStore {
    root: PathBuf,
    authority: Option<StoreAuthority>,
    config: LocalPackageStoreConfig,
    pending: Vec<PendingPackageRef>,
    fault_injector: Arc<dyn CommitFaultInjector>,
    poisoned: bool,
}

struct StoreAuthority {
    root: File,
    objects: File,
}

impl std::fmt::Debug for StoreAuthority {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoreAuthority")
            .field("root_fd", &self.root.as_raw_fd())
            .field("objects_fd", &self.objects.as_raw_fd())
            .finish()
    }
}

impl std::fmt::Debug for LocalPackageStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalPackageStore")
            .field("root", &self.root)
            .field("authority", &self.authority)
            .field("config", &self.config)
            .field("pending", &self.pending)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitFaultPoint {
    ObjectDurability,
    PendingPrecommit,
    PendingDurability,
}

trait CommitFaultInjector: Send + Sync {
    fn check(&self, point: CommitFaultPoint) -> Result<(), LocalPackageStoreError>;
}

#[derive(Debug)]
struct NoCommitFaults;

impl CommitFaultInjector for NoCommitFaults {
    fn check(&self, _point: CommitFaultPoint) -> Result<(), LocalPackageStoreError> {
        Ok(())
    }
}

#[derive(Debug)]
enum AtomicCommitFailure {
    BeforeRename(LocalPackageStoreError),
    AfterRename(LocalPackageStoreError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObjectWriteOutcome {
    Existing,
    Created,
}

impl LocalPackageStore {
    pub fn open(
        root: impl AsRef<Path>,
        config: LocalPackageStoreConfig,
    ) -> Result<Self, LocalPackageStoreError> {
        Self::open_with_fault_injector(root, config, Arc::new(NoCommitFaults))
    }

    fn open_with_fault_injector(
        root: impl AsRef<Path>,
        config: LocalPackageStoreConfig,
        fault_injector: Arc<dyn CommitFaultInjector>,
    ) -> Result<Self, LocalPackageStoreError> {
        let root = root.as_ref().to_path_buf();
        if !config.enabled {
            return Ok(Self {
                root,
                authority: None,
                config,
                pending: Vec::new(),
                fault_injector,
                poisoned: false,
            });
        }

        let authority = open_store_authority(&root)?;
        remove_crash_temp(&authority.root, PACKAGE_PENDING_TEMP_FILE)?;
        remove_object_temps(&authority.objects)?;
        let pending = load_pending(&authority, &config)?;
        collect_orphan_objects(&authority.objects, &pending)?;

        Ok(Self {
            root,
            authority: Some(authority),
            config,
            pending,
            fault_injector,
            poisoned: false,
        })
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    fn ensure_healthy(&self) -> Result<(), LocalPackageStoreError> {
        if self.poisoned {
            Err(LocalPackageStoreError::Poisoned)
        } else {
            Ok(())
        }
    }

    fn poison(&mut self) {
        self.poisoned = true;
    }

    fn authority(&self) -> Result<&StoreAuthority, LocalPackageStoreError> {
        self.authority.as_ref().ok_or_else(|| {
            LocalPackageStoreError::Corrupt("enabled store has no directory authority".into())
        })
    }

    pub fn stage(
        &mut self,
        package: &CanonicalPackage,
        reference: &str,
    ) -> Result<StagePackageOutcome, LocalPackageStoreError> {
        if !self.config.enabled {
            return Err(LocalPackageStoreError::Disabled);
        }
        self.ensure_healthy()?;

        validate_reference(reference)?;
        let size_bytes = u64::try_from(package.size_bytes())
            .map_err(|_| LocalPackageStoreError::Corrupt("package size does not fit u64".into()))?;
        if let Some(existing) = self
            .pending
            .iter()
            .find(|entry| entry.root == package.root() && entry.reference == reference)
        {
            if existing.size_bytes != size_bytes {
                return Err(LocalPackageStoreError::PendingConflict);
            }
            verify_object(&self.authority()?.objects, package.root(), size_bytes)?;
            return Ok(StagePackageOutcome::AlreadyPending);
        }

        if self.pending.len() >= self.config.max_pending_packages {
            return Err(LocalPackageStoreError::PendingCountExceeded {
                max: self.config.max_pending_packages,
            });
        }
        let current_bytes = pending_bytes(&self.pending)?;
        let next_bytes = current_bytes.checked_add(size_bytes).ok_or(
            LocalPackageStoreError::PendingBytesExceeded {
                max: self.config.max_pending_bytes,
            },
        )?;
        if next_bytes > self.config.max_pending_bytes {
            return Err(LocalPackageStoreError::PendingBytesExceeded {
                max: self.config.max_pending_bytes,
            });
        }

        let object_outcome = match write_object(self, package) {
            Ok(outcome) => outcome,
            Err(AtomicCommitFailure::BeforeRename(error)) => return Err(error),
            Err(AtomicCommitFailure::AfterRename(_error)) => {
                self.poison();
                return Err(LocalPackageStoreError::CommitOutcomeUnknown {
                    phase: PackageStoreCommitPhase::Object,
                });
            }
        };
        let mut next = self.pending.clone();
        next.push(PendingPackageRef {
            root: package.root(),
            size_bytes,
            reference: reference.to_owned(),
        });
        match write_pending(&self.authority()?.root, &next, self.fault_injector.as_ref()) {
            Ok(()) => self.pending = next,
            Err(AtomicCommitFailure::BeforeRename(error)) => {
                if object_outcome == ObjectWriteOutcome::Created
                    && remove_new_object(&self.authority()?.objects, package.root()).is_err()
                {
                    self.poison();
                    return Err(LocalPackageStoreError::CommitOutcomeUnknown {
                        phase: PackageStoreCommitPhase::ObjectCleanup,
                    });
                }
                return Err(error);
            }
            Err(AtomicCommitFailure::AfterRename(_error)) => {
                self.pending = next;
                self.poison();
                return Err(LocalPackageStoreError::CommitOutcomeUnknown {
                    phase: PackageStoreCommitPhase::PendingSnapshot,
                });
            }
        }
        Ok(StagePackageOutcome::Staged)
    }

    pub fn pending(&self) -> &[PendingPackageRef] {
        &self.pending
    }

    /// Durably remove one root/reference pair from the pending FIFO. The
    /// snapshot is committed before an unreferenced CAS object is removed, so
    /// a crash can leave only a recoverable orphan, never a pending entry whose
    /// object was deleted by this operation.
    pub fn acknowledge(
        &mut self,
        root: PackageRoot,
        reference: &str,
    ) -> Result<AcknowledgePackageOutcome, LocalPackageStoreError> {
        if !self.config.enabled {
            return Err(LocalPackageStoreError::Disabled);
        }
        self.ensure_healthy()?;
        validate_reference(reference)?;
        let Some(index) = self
            .pending
            .iter()
            .position(|entry| entry.root == root && entry.reference == reference)
        else {
            return Ok(AcknowledgePackageOutcome::NotPending);
        };

        let entry = &self.pending[index];
        verify_object(&self.authority()?.objects, root, entry.size_bytes)?;
        let mut next = self.pending.clone();
        next.remove(index);
        match write_pending(&self.authority()?.root, &next, self.fault_injector.as_ref()) {
            Ok(()) => self.pending = next,
            Err(AtomicCommitFailure::BeforeRename(error)) => return Err(error),
            Err(AtomicCommitFailure::AfterRename(_error)) => {
                self.pending = next;
                self.poison();
                return Err(LocalPackageStoreError::CommitOutcomeUnknown {
                    phase: PackageStoreCommitPhase::PendingSnapshot,
                });
            }
        }

        if !self.pending.iter().any(|entry| entry.root == root)
            && (unlink_regular_at(
                &self.authority()?.objects,
                &object_basename(root)?,
                "acknowledged CAS object",
            )
            .is_err()
                || fsync_directory_file(&self.authority()?.objects).is_err())
        {
            self.poison();
            return Err(LocalPackageStoreError::CommitOutcomeUnknown {
                phase: PackageStoreCommitPhase::ObjectCleanup,
            });
        }
        Ok(AcknowledgePackageOutcome::Acknowledged)
    }

    pub fn read(&self, root: PackageRoot) -> Result<Vec<u8>, LocalPackageStoreError> {
        if !self.config.enabled {
            return Err(LocalPackageStoreError::Disabled);
        }
        self.ensure_healthy()?;
        let bytes = read_regular_file_bounded(
            &self.authority()?.objects,
            &object_basename(root)?,
            MAX_PACKAGE_CANONICAL_BYTES as u64,
            BoundedReadKind::Object { root },
        )?;
        verify_bytes(&bytes, root, None)?;
        Ok(bytes)
    }
}

fn validate_reference(reference: &str) -> Result<(), LocalPackageStoreError> {
    if reference.is_empty() {
        return Err(LocalPackageStoreError::EmptyReference);
    }
    if reference.len() > MAX_PACKAGE_REFERENCE_BYTES {
        return Err(LocalPackageStoreError::ReferenceTooLarge {
            size: reference.len(),
            max: MAX_PACKAGE_REFERENCE_BYTES,
        });
    }
    Ok(())
}

fn open_store_authority(root_path: &Path) -> Result<StoreAuthority, LocalPackageStoreError> {
    let basename = root_path.file_name().ok_or_else(|| {
        LocalPackageStoreError::UnsafePath(format!(
            "store root must name one final directory: {}",
            root_path.display()
        ))
    })?;
    let root_name = component_name(basename, "store root")?;
    let parent_path = root_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canonical_parent = fs::canonicalize(parent_path).map_err(io_error)?;
    let parent = open_existing_directory_path(&canonical_parent)?;

    let root_created = ensure_directory_entry(&parent, &root_name, "store root")?;
    let root = open_directory_at(&parent, &root_name, "store root")?;
    require_same_entry(&parent, &root_name, &root, libc::S_IFDIR, "store root")?;
    if root_created {
        fsync_directory_file(&parent)?;
    }

    let objects_name = fixed_component(PACKAGE_OBJECTS_DIRECTORY);
    let objects_created = ensure_directory_entry(&root, &objects_name, "objects directory")?;
    let objects = open_directory_at(&root, &objects_name, "objects directory")?;
    require_same_entry(
        &root,
        &objects_name,
        &objects,
        libc::S_IFDIR,
        "objects directory",
    )?;
    if objects_created {
        fsync_directory_file(&root)?;
    }
    fsync_directory_file(&root)?;
    fsync_directory_file(&objects)?;
    Ok(StoreAuthority { root, objects })
}

#[allow(unsafe_code)]
fn open_existing_directory_path(path: &Path) -> Result<File, LocalPackageStoreError> {
    if !path.is_absolute() {
        return Err(LocalPackageStoreError::UnsafePath(format!(
            "canonical parent is not absolute: {}",
            path.display()
        )));
    }
    let slash = fixed_component("/");
    let raw_fd = unsafe {
        // SAFETY: the fixed slash CString is live and NUL-terminated, and the
        // returned descriptor is transferred exactly once into `File`.
        libc::open(
            slash.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    if raw_fd < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    let mut current = unsafe {
        // SAFETY: `open` returned a fresh descriptor owned by this function.
        File::from_raw_fd(raw_fd)
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(value) => {
                let name = component_name(value, "store parent component")?;
                current = open_directory_at(&current, &name, "store parent component")?;
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(LocalPackageStoreError::UnsafePath(format!(
                    "canonical parent contains a non-normal component: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(current)
}

fn component_name(value: &OsStr, label: &str) -> Result<CString, LocalPackageStoreError> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        return Err(LocalPackageStoreError::UnsafePath(format!(
            "{label} must be one ordinary path component"
        )));
    }
    CString::new(bytes).map_err(|_| {
        LocalPackageStoreError::UnsafePath(format!("{label} contains an interior NUL"))
    })
}

fn fixed_component(value: &str) -> CString {
    CString::new(value).expect("fixed package-store component contains no interior NUL")
}

fn object_basename(root: PackageRoot) -> Result<CString, LocalPackageStoreError> {
    component_name(
        OsStr::new(&format!("{}.pkg", root.to_hex())),
        "CAS object basename",
    )
}

#[allow(unsafe_code)]
fn ensure_directory_entry(
    parent: &File,
    name: &CStr,
    label: &str,
) -> Result<bool, LocalPackageStoreError> {
    match stat_at(parent, name)? {
        Some(stat) if stat.st_mode & libc::S_IFMT == libc::S_IFDIR => Ok(false),
        Some(_) => Err(LocalPackageStoreError::UnsafePath(format!(
            "{label} must be a real directory"
        ))),
        None => {
            let result = unsafe {
                // SAFETY: `parent` is a live directory descriptor and `name`
                // is a single NUL-terminated component.
                libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700)
            };
            if result != 0 {
                return Err(io_error(std::io::Error::last_os_error()));
            }
            Ok(true)
        }
    }
}

#[allow(unsafe_code)]
fn open_directory_at(
    parent: &File,
    name: &CStr,
    label: &str,
) -> Result<File, LocalPackageStoreError> {
    let flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW;
    let raw_fd = unsafe {
        // SAFETY: `parent` is live, `name` is NUL-terminated, and ownership of
        // the returned descriptor moves exactly once into `File`.
        libc::openat(parent.as_raw_fd(), name.as_ptr(), flags)
    };
    if raw_fd < 0 {
        return Err(LocalPackageStoreError::UnsafePath(format!(
            "cannot open {label} without following links: {}",
            std::io::Error::last_os_error()
        )));
    }
    let file = unsafe {
        // SAFETY: `openat` returned a fresh descriptor owned here.
        File::from_raw_fd(raw_fd)
    };
    let stat = descriptor_stat(&file)?;
    if stat.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return Err(LocalPackageStoreError::UnsafePath(format!(
            "{label} is not a directory"
        )));
    }
    Ok(file)
}

#[allow(unsafe_code)]
fn open_new_regular_at(directory: &File, name: &CStr) -> Result<File, LocalPackageStoreError> {
    let flags = libc::O_WRONLY
        | libc::O_CREAT
        | libc::O_EXCL
        | libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK;
    let raw_fd = unsafe {
        // SAFETY: `directory` is live, `name` is one NUL-terminated component,
        // and ownership of a successful descriptor transfers once to `File`.
        libc::openat(directory.as_raw_fd(), name.as_ptr(), flags, 0o600)
    };
    if raw_fd < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    let file = unsafe {
        // SAFETY: `openat` returned a fresh descriptor owned here.
        File::from_raw_fd(raw_fd)
    };
    if descriptor_stat(&file)?.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(LocalPackageStoreError::UnsafePath(
            "new file is not regular".into(),
        ));
    }
    Ok(file)
}

#[derive(Debug, Clone, Copy)]
enum BoundedReadKind {
    PendingSnapshot,
    Object { root: PackageRoot },
}

impl BoundedReadKind {
    fn too_large(self, size: u64, max: u64) -> LocalPackageStoreError {
        match self {
            Self::PendingSnapshot => LocalPackageStoreError::PendingSnapshotTooLarge { size, max },
            Self::Object { .. } => LocalPackageStoreError::ObjectTooLarge { size, max },
        }
    }

    fn missing(self) -> LocalPackageStoreError {
        match self {
            Self::PendingSnapshot => {
                LocalPackageStoreError::Corrupt("pending snapshot disappeared during read".into())
            }
            Self::Object { root } => LocalPackageStoreError::MissingObject {
                root: root.to_hex(),
            },
        }
    }
}

#[allow(unsafe_code)]
fn read_regular_file_bounded(
    directory: &File,
    name: &CStr,
    max_bytes: u64,
    kind: BoundedReadKind,
) -> Result<Vec<u8>, LocalPackageStoreError> {
    let flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK;
    let raw_fd = unsafe {
        // SAFETY: `directory` is live, `name` is NUL-terminated, and ownership
        // of a successful descriptor transfers once into `File`.
        libc::openat(directory.as_raw_fd(), name.as_ptr(), flags)
    };
    if raw_fd < 0 {
        let error = std::io::Error::last_os_error();
        return Err(if error.raw_os_error() == Some(libc::ENOENT) {
            kind.missing()
        } else {
            LocalPackageStoreError::UnsafePath(format!(
                "bounded read target cannot be opened without following links: {error}"
            ))
        });
    }
    let mut file = unsafe {
        // SAFETY: `openat` returned a fresh descriptor owned here.
        File::from_raw_fd(raw_fd)
    };
    let stat = descriptor_stat(&file)?;
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(LocalPackageStoreError::UnsafePath(
            "bounded read target must be a regular file".into(),
        ));
    }
    let size = u64::try_from(stat.st_size).map_err(|_| {
        LocalPackageStoreError::UnsafePath("bounded read target has a negative size".into())
    })?;
    if size > max_bytes {
        return Err(kind.too_large(size, max_bytes));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    (&mut file)
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > max_bytes {
        return Err(kind.too_large(bytes.len() as u64, max_bytes));
    }
    let after = descriptor_stat(&file)?;
    if after.st_dev != stat.st_dev || after.st_ino != stat.st_ino || after.st_size != stat.st_size {
        return Err(LocalPackageStoreError::UnsafePath(
            "bounded read target changed while it was read".into(),
        ));
    }
    Ok(bytes)
}

fn write_object(
    store: &LocalPackageStore,
    package: &CanonicalPackage,
) -> Result<ObjectWriteOutcome, AtomicCommitFailure> {
    let objects = &store
        .authority()
        .map_err(AtomicCommitFailure::BeforeRename)?
        .objects;
    let name = object_basename(package.root()).map_err(AtomicCommitFailure::BeforeRename)?;
    let size_bytes = package.size_bytes() as u64;
    if stat_at(objects, &name)
        .map_err(AtomicCommitFailure::BeforeRename)?
        .is_some()
    {
        verify_object(objects, package.root(), size_bytes)
            .map_err(AtomicCommitFailure::BeforeRename)?;
        return Ok(ObjectWriteOutcome::Existing);
    }

    let temp = fixed_component(&format!("{}.tmp", name.to_string_lossy()));
    remove_crash_temp(objects, temp.to_str().expect("generated CAS temp is UTF-8"))
        .map_err(AtomicCommitFailure::BeforeRename)?;
    let before_rename = (|| {
        let mut file = open_new_regular_at(objects, &temp)?;
        file.write_all(package.canonical_bytes())
            .map_err(io_error)?;
        file.flush().map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        rename_at(objects, &temp, objects, &name)?;
        Ok(())
    })();
    if let Err(error) = before_rename {
        let _ = remove_crash_temp(objects, temp.to_str().expect("generated CAS temp is UTF-8"));
        return Err(AtomicCommitFailure::BeforeRename(error));
    }
    store
        .fault_injector
        .check(CommitFaultPoint::ObjectDurability)
        .map_err(AtomicCommitFailure::AfterRename)?;
    fsync_directory_file(objects).map_err(AtomicCommitFailure::AfterRename)?;
    Ok(ObjectWriteOutcome::Created)
}

fn write_pending(
    root: &File,
    pending: &[PendingPackageRef],
    fault_injector: &dyn CommitFaultInjector,
) -> Result<(), AtomicCommitFailure> {
    let path = fixed_component(PACKAGE_PENDING_FILE);
    let temp = fixed_component(PACKAGE_PENDING_TEMP_FILE);
    if let Some(stat) = stat_at(root, &path).map_err(AtomicCommitFailure::BeforeRename)? {
        if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(AtomicCommitFailure::BeforeRename(
                LocalPackageStoreError::UnsafePath(
                    "pending snapshot must be a regular file".into(),
                ),
            ));
        }
    }
    remove_crash_temp(root, PACKAGE_PENDING_TEMP_FILE)
        .map_err(AtomicCommitFailure::BeforeRename)?;
    let snapshot = PendingSnapshot {
        schema: PACKAGE_PENDING_SCHEMA.to_owned(),
        entries: pending
            .iter()
            .map(|entry| StoredPendingPackageRef {
                root: entry.root.to_hex(),
                size_bytes: entry.size_bytes,
                reference: entry.reference.clone(),
            })
            .collect(),
    };
    let bytes = serde_json::to_vec(&snapshot)
        .map_err(|error| LocalPackageStoreError::Corrupt(error.to_string()))
        .map_err(AtomicCommitFailure::BeforeRename)?;
    if bytes.len() as u64 > MAX_PENDING_SNAPSHOT_BYTES {
        return Err(AtomicCommitFailure::BeforeRename(
            LocalPackageStoreError::PendingSnapshotTooLarge {
                size: bytes.len() as u64,
                max: MAX_PENDING_SNAPSHOT_BYTES,
            },
        ));
    }
    let before_rename = (|| {
        let mut file = open_new_regular_at(root, &temp)?;
        file.write_all(&bytes).map_err(io_error)?;
        file.flush().map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        fault_injector.check(CommitFaultPoint::PendingPrecommit)?;
        rename_at(root, &temp, root, &path)?;
        Ok(())
    })();
    if let Err(error) = before_rename {
        let _ = remove_crash_temp(root, PACKAGE_PENDING_TEMP_FILE);
        return Err(AtomicCommitFailure::BeforeRename(error));
    }
    fault_injector
        .check(CommitFaultPoint::PendingDurability)
        .map_err(AtomicCommitFailure::AfterRename)?;
    fsync_directory_file(root).map_err(AtomicCommitFailure::AfterRename)
}

fn remove_new_object(objects: &File, root: PackageRoot) -> Result<(), LocalPackageStoreError> {
    unlink_regular_at(objects, &object_basename(root)?, "new CAS object cleanup")?;
    fsync_directory_file(objects)
}

fn load_pending(
    authority: &StoreAuthority,
    config: &LocalPackageStoreConfig,
) -> Result<Vec<PendingPackageRef>, LocalPackageStoreError> {
    let pending_name = fixed_component(PACKAGE_PENDING_FILE);
    if stat_at(&authority.root, &pending_name)?.is_none() {
        return Ok(Vec::new());
    }
    let bytes = read_regular_file_bounded(
        &authority.root,
        &pending_name,
        MAX_PENDING_SNAPSHOT_BYTES,
        BoundedReadKind::PendingSnapshot,
    )?;
    let stored: PendingSnapshot = serde_json::from_slice(&bytes)
        .map_err(|error| LocalPackageStoreError::Corrupt(error.to_string()))?;
    if stored.schema != PACKAGE_PENDING_SCHEMA {
        return Err(LocalPackageStoreError::Corrupt(
            "unknown pending snapshot schema".into(),
        ));
    }
    if stored.entries.len() > config.max_pending_packages {
        return Err(LocalPackageStoreError::Corrupt(
            "pending count exceeds configured bound".into(),
        ));
    }

    let mut pending = Vec::with_capacity(stored.entries.len());
    let mut seen_pairs = std::collections::BTreeSet::<(PackageRoot, String)>::new();
    let mut unique_roots = std::collections::BTreeMap::<PackageRoot, u64>::new();
    for stored_entry in stored.entries {
        validate_reference(&stored_entry.reference)?;
        if stored_entry.size_bytes > MAX_PACKAGE_CANONICAL_BYTES as u64 {
            return Err(LocalPackageStoreError::Corrupt(
                "pending package exceeds canonical package bound".into(),
            ));
        }
        let root_value = PackageRoot::from_hex(&stored_entry.root)
            .map_err(|error| LocalPackageStoreError::Corrupt(error.to_string()))?;
        if !seen_pairs.insert((root_value, stored_entry.reference.clone())) {
            return Err(LocalPackageStoreError::Corrupt(
                "duplicate root/reference pending entry".into(),
            ));
        }
        match unique_roots.entry(root_value) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(stored_entry.size_bytes);
            }
            std::collections::btree_map::Entry::Occupied(entry)
                if *entry.get() != stored_entry.size_bytes =>
            {
                return Err(LocalPackageStoreError::Corrupt(
                    "conflicting sizes for same pending root".into(),
                ));
            }
            std::collections::btree_map::Entry::Occupied(_) => {}
        }
        pending.push(PendingPackageRef {
            root: root_value,
            size_bytes: stored_entry.size_bytes,
            reference: stored_entry.reference,
        });
    }
    if pending_bytes(&pending)? > config.max_pending_bytes {
        return Err(LocalPackageStoreError::Corrupt(
            "pending bytes exceed configured bound".into(),
        ));
    }
    // Disk I/O starts only after every record and the aggregate bound pass;
    // repeated references share one CAS verification by construction.
    for (root, size_bytes) in unique_roots {
        verify_object(&authority.objects, root, size_bytes)?;
    }
    Ok(pending)
}

fn verify_object(
    objects: &File,
    root: PackageRoot,
    size_bytes: u64,
) -> Result<(), LocalPackageStoreError> {
    let bytes = read_regular_file_bounded(
        objects,
        &object_basename(root)?,
        MAX_PACKAGE_CANONICAL_BYTES as u64,
        BoundedReadKind::Object { root },
    )?;
    verify_bytes(&bytes, root, Some(size_bytes))
}

fn verify_bytes(
    bytes: &[u8],
    root: PackageRoot,
    expected_size: Option<u64>,
) -> Result<(), LocalPackageStoreError> {
    if bytes.len() > MAX_PACKAGE_CANONICAL_BYTES {
        return Err(LocalPackageStoreError::ObjectTooLarge {
            size: bytes.len() as u64,
            max: MAX_PACKAGE_CANONICAL_BYTES as u64,
        });
    }
    if let Some(expected) = expected_size {
        let actual = bytes.len() as u64;
        if expected != actual {
            return Err(LocalPackageStoreError::ObjectSizeMismatch { expected, actual });
        }
    }
    let actual = h_protocol(PACKAGE_SIDECAR_ROOT_DOMAIN, &[bytes]);
    if actual.as_bytes() != root.as_bytes() {
        return Err(LocalPackageStoreError::ObjectRootMismatch {
            expected: root.to_hex(),
            actual: actual.to_hex(),
        });
    }
    Ok(())
}

fn pending_bytes(pending: &[PendingPackageRef]) -> Result<u64, LocalPackageStoreError> {
    pending.iter().try_fold(0u64, |total, entry| {
        total
            .checked_add(entry.size_bytes)
            .ok_or_else(|| LocalPackageStoreError::Corrupt("pending byte total overflow".into()))
    })
}

fn remove_crash_temp(directory: &File, basename: &str) -> Result<(), LocalPackageStoreError> {
    let name = fixed_component(basename);
    let Some(stat) = stat_at(directory, &name)? else {
        return Ok(());
    };
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(LocalPackageStoreError::UnsafePath(format!(
            "crash temp must be a regular file: {basename}"
        )));
    }
    unlink_regular_at(directory, &name, "crash temp")?;
    fsync_directory_file(directory)
}

fn remove_object_temps(objects: &File) -> Result<(), LocalPackageStoreError> {
    for name in directory_entry_names(objects)? {
        if name.ends_with(b".pkg.tmp") {
            let name = std::str::from_utf8(&name).map_err(|_| {
                LocalPackageStoreError::UnsafePath("CAS temp name is not UTF-8".into())
            })?;
            remove_crash_temp(objects, name)?;
        }
    }
    Ok(())
}

fn collect_orphan_objects(
    objects: &File,
    pending: &[PendingPackageRef],
) -> Result<(), LocalPackageStoreError> {
    let referenced: std::collections::BTreeSet<String> =
        pending.iter().map(|entry| entry.root.to_hex()).collect();
    let mut removed = false;
    for name in directory_entry_names(objects)? {
        let Some(root_hex) = name.strip_suffix(b".pkg") else {
            continue;
        };
        let name_c = CString::new(name.clone()).expect("directory entries contain no interior NUL");
        let stat = stat_at(objects, &name_c)?.ok_or_else(|| {
            LocalPackageStoreError::UnsafePath(
                "CAS object changed while the directory was scanned".into(),
            )
        })?;
        if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
            return Err(LocalPackageStoreError::UnsafePath(format!(
                "CAS object must be a regular file: {}",
                String::from_utf8_lossy(&name)
            )));
        }
        let root_hex = std::str::from_utf8(root_hex).map_err(|_| {
            LocalPackageStoreError::Corrupt("invalid UTF-8 CAS object filename".into())
        })?;
        if PackageRoot::from_hex(root_hex).is_err() {
            return Err(LocalPackageStoreError::Corrupt(
                "invalid CAS object filename".into(),
            ));
        }
        if !referenced.contains(root_hex) {
            unlink_regular_at(objects, &name_c, "orphan CAS object")?;
            removed = true;
        }
    }
    if removed {
        fsync_directory_file(objects)?;
    }
    Ok(())
}

fn fsync_directory_file(directory: &File) -> Result<(), LocalPackageStoreError> {
    if descriptor_stat(directory)?.st_mode & libc::S_IFMT != libc::S_IFDIR {
        return Err(LocalPackageStoreError::UnsafePath(
            "directory authority stopped being a directory".into(),
        ));
    }
    directory.sync_all().map_err(io_error)
}

#[allow(unsafe_code)]
fn descriptor_stat(file: &File) -> Result<libc::stat, LocalPackageStoreError> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        // SAFETY: `file` owns a live descriptor and `stat` is writable output
        // storage for one complete `libc::stat` value.
        libc::fstat(file.as_raw_fd(), stat.as_mut_ptr())
    };
    if result != 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(unsafe {
        // SAFETY: successful `fstat` initialized the complete value.
        stat.assume_init()
    })
}

#[allow(unsafe_code)]
fn stat_at(directory: &File, name: &CStr) -> Result<Option<libc::stat>, LocalPackageStoreError> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        // SAFETY: `directory` is a live directory descriptor, `name` is one
        // NUL-terminated component, and `stat` is writable output storage.
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        return Ok(Some(unsafe {
            // SAFETY: successful `fstatat` initialized the complete value.
            stat.assume_init()
        }));
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ENOENT) {
        Ok(None)
    } else {
        Err(io_error(error))
    }
}

fn require_same_entry(
    directory: &File,
    name: &CStr,
    opened: &File,
    expected_kind: libc::mode_t,
    label: &str,
) -> Result<(), LocalPackageStoreError> {
    let opened_stat = descriptor_stat(opened)?;
    let named_stat = stat_at(directory, name)?.ok_or_else(|| {
        LocalPackageStoreError::UnsafePath(format!("{label} disappeared while it was opened"))
    })?;
    if opened_stat.st_mode & libc::S_IFMT != expected_kind
        || named_stat.st_mode & libc::S_IFMT != expected_kind
        || opened_stat.st_dev != named_stat.st_dev
        || opened_stat.st_ino != named_stat.st_ino
    {
        return Err(LocalPackageStoreError::UnsafePath(format!(
            "{label} changed identity while it was opened"
        )));
    }
    Ok(())
}

#[allow(unsafe_code)]
fn rename_at(
    old_directory: &File,
    old_name: &CStr,
    new_directory: &File,
    new_name: &CStr,
) -> Result<(), LocalPackageStoreError> {
    let result = unsafe {
        // SAFETY: both directory descriptors are live and both names are
        // single NUL-terminated components. `renameat` retains no pointers.
        libc::renameat(
            old_directory.as_raw_fd(),
            old_name.as_ptr(),
            new_directory.as_raw_fd(),
            new_name.as_ptr(),
        )
    };
    if result != 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[allow(unsafe_code)]
fn unlink_regular_at(
    directory: &File,
    name: &CStr,
    label: &str,
) -> Result<(), LocalPackageStoreError> {
    let flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK;
    let raw_fd = unsafe {
        // SAFETY: `directory` is live and `name` is one NUL-terminated
        // component. The returned descriptor is transferred once to `File`.
        libc::openat(directory.as_raw_fd(), name.as_ptr(), flags)
    };
    if raw_fd < 0 {
        return Err(LocalPackageStoreError::UnsafePath(format!(
            "cannot open {label} for safe removal: {}",
            std::io::Error::last_os_error()
        )));
    }
    let opened = unsafe {
        // SAFETY: `openat` returned a fresh descriptor owned here.
        File::from_raw_fd(raw_fd)
    };
    require_same_entry(directory, name, &opened, libc::S_IFREG, label)?;
    let result = unsafe {
        // SAFETY: the directory descriptor is retained, the name is a single
        // component, and unlinkat never follows a symlink target. This slice
        // explicitly has one process owner, so no peer mutates the entry after
        // the identity check.
        libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0)
    };
    if result != 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[allow(unsafe_code)]
fn directory_entry_names(directory: &File) -> Result<Vec<Vec<u8>>, LocalPackageStoreError> {
    let dot = fixed_component(".");
    let flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW;
    let scan_fd = unsafe {
        // SAFETY: `directory` is live and the fixed dot component is
        // NUL-terminated. A distinct open description gives each scan its own
        // directory offset.
        libc::openat(directory.as_raw_fd(), dot.as_ptr(), flags)
    };
    if scan_fd < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    let stream = unsafe {
        // SAFETY: `scan_fd` is a fresh owned directory descriptor. On success
        // `fdopendir` assumes ownership; on failure it remains ours to close.
        libc::fdopendir(scan_fd)
    };
    if stream.is_null() {
        let error = std::io::Error::last_os_error();
        unsafe {
            // SAFETY: fdopendir failed and did not consume `scan_fd`.
            libc::close(scan_fd);
        }
        return Err(io_error(error));
    }
    let stream = DirectoryStream(stream);
    let mut names = Vec::new();
    loop {
        clear_errno();
        let entry = unsafe {
            // SAFETY: `stream` owns a live DIR pointer for this loop.
            libc::readdir(stream.0)
        };
        if entry.is_null() {
            let error = errno_value();
            if error != 0 {
                return Err(io_error(std::io::Error::from_raw_os_error(error)));
            }
            break;
        }
        let name = unsafe {
            // SAFETY: readdir returned a live entry with NUL-terminated d_name.
            CStr::from_ptr((*entry).d_name.as_ptr())
        }
        .to_bytes();
        if name != b"." && name != b".." {
            names.push(name.to_vec());
        }
    }
    names.sort();
    Ok(names)
}

struct DirectoryStream(*mut libc::DIR);

impl Drop for DirectoryStream {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        unsafe {
            // SAFETY: this guard is the sole owner of the DIR pointer.
            libc::closedir(self.0);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[allow(unsafe_code)]
fn errno_pointer() -> *mut libc::c_int {
    unsafe {
        // SAFETY: libc returns the calling thread's errno storage.
        libc::__errno_location()
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[allow(unsafe_code)]
fn errno_pointer() -> *mut libc::c_int {
    unsafe {
        // SAFETY: libc returns the calling thread's errno storage.
        libc::__error()
    }
}

#[allow(unsafe_code)]
fn clear_errno() {
    unsafe {
        // SAFETY: errno_pointer returns writable thread-local errno storage.
        *errno_pointer() = 0;
    }
}

#[allow(unsafe_code)]
fn errno_value() -> libc::c_int {
    unsafe {
        // SAFETY: errno_pointer returns readable thread-local errno storage.
        *errno_pointer()
    }
}

fn io_error(error: std::io::Error) -> LocalPackageStoreError {
    LocalPackageStoreError::Io(error.to_string())
}

#[cfg(test)]
mod durability_failure_tests {
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::package_sidecar::PackageFile;

    #[derive(Default)]
    struct FailController {
        next: Mutex<Option<CommitFaultPoint>>,
    }

    impl FailController {
        fn arm(&self, point: CommitFaultPoint) {
            *self.next.lock().expect("fault lock") = Some(point);
        }
    }

    impl CommitFaultInjector for FailController {
        fn check(&self, point: CommitFaultPoint) -> Result<(), LocalPackageStoreError> {
            let mut next = self.next.lock().expect("fault lock");
            if next.as_ref() == Some(&point) {
                next.take();
                return Err(LocalPackageStoreError::Io(format!(
                    "injected fault at {point:?}"
                )));
            }
            Ok(())
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "boole-bf6a-fault-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn config() -> LocalPackageStoreConfig {
        LocalPackageStoreConfig {
            enabled: true,
            max_pending_packages: 4,
            max_pending_bytes: 1024 * 1024,
        }
    }

    fn package(name: &[u8], value: &[u8]) -> CanonicalPackage {
        CanonicalPackage::new(vec![PackageFile::new(name, value)]).expect("package")
    }

    #[test]
    fn pending_rename_then_fsync_failure_aligns_memory_and_poisoned_visible_snapshot() {
        let root = temp_root("pending-after-rename");
        let faults = Arc::new(FailController::default());
        let mut store =
            LocalPackageStore::open_with_fault_injector(&root, config(), faults.clone())
                .expect("open store");
        let package = package(b"result", b"pending");
        faults.arm(CommitFaultPoint::PendingDurability);

        assert_eq!(
            store.stage(&package, "receipt:pending"),
            Err(LocalPackageStoreError::CommitOutcomeUnknown {
                phase: PackageStoreCommitPhase::PendingSnapshot,
            })
        );
        assert!(store.is_poisoned());
        assert_eq!(store.pending().len(), 1, "memory follows visible rename");
        assert!(matches!(
            store.read(package.root()),
            Err(LocalPackageStoreError::Poisoned)
        ));
        drop(store);

        let recovered =
            LocalPackageStore::open(&root, config()).expect("visible snapshot recovers");
        assert_eq!(recovered.pending().len(), 1);
        assert_eq!(recovered.pending()[0].root(), package.root());
        fs::remove_dir_all(root).expect("remove store");
    }

    #[test]
    fn object_rename_then_fsync_failure_poisoned_store_and_recovers_as_orphan() {
        let root = temp_root("object-after-rename");
        let faults = Arc::new(FailController::default());
        let mut store =
            LocalPackageStore::open_with_fault_injector(&root, config(), faults.clone())
                .expect("open store");
        let package = package(b"result", b"object");
        faults.arm(CommitFaultPoint::ObjectDurability);

        assert_eq!(
            store.stage(&package, "receipt:object"),
            Err(LocalPackageStoreError::CommitOutcomeUnknown {
                phase: PackageStoreCommitPhase::Object,
            })
        );
        assert!(store.is_poisoned());
        assert!(store.pending().is_empty());
        drop(store);

        let recovered = LocalPackageStore::open(&root, config()).expect("orphan is collected");
        assert!(recovered.pending().is_empty());
        let recovered_read = recovered.read(package.root());
        assert!(
            matches!(
                recovered_read,
                Err(LocalPackageStoreError::MissingObject { .. })
            ),
            "unexpected recovered read: {recovered_read:?}"
        );
        fs::remove_dir_all(root).expect("remove store");
    }

    #[test]
    fn acknowledge_pending_rename_then_fsync_failure_exposes_the_new_empty_snapshot() {
        let root = temp_root("ack-after-rename");
        let faults = Arc::new(FailController::default());
        let mut store =
            LocalPackageStore::open_with_fault_injector(&root, config(), faults.clone())
                .expect("open store");
        let package = package(b"result", b"acknowledge");
        store.stage(&package, "receipt:ack").expect("stage package");
        faults.arm(CommitFaultPoint::PendingDurability);

        assert_eq!(
            store.acknowledge(package.root(), "receipt:ack"),
            Err(LocalPackageStoreError::CommitOutcomeUnknown {
                phase: PackageStoreCommitPhase::PendingSnapshot,
            })
        );
        assert!(store.is_poisoned());
        assert!(store.pending().is_empty(), "memory follows visible rename");
        drop(store);

        let recovered = LocalPackageStore::open(&root, config()).expect("empty snapshot recovers");
        assert!(recovered.pending().is_empty());
        assert!(matches!(
            recovered.read(package.root()),
            Err(LocalPackageStoreError::MissingObject { .. })
        ));
        fs::remove_dir_all(root).expect("remove store");
    }

    #[test]
    fn pending_pre_rename_failure_removes_only_a_new_object_and_keeps_existing_objects() {
        let root = temp_root("pending-before-rename");
        let faults = Arc::new(FailController::default());
        let mut store =
            LocalPackageStore::open_with_fault_injector(&root, config(), faults.clone())
                .expect("open store");
        let first = package(b"first", b"new-object");
        faults.arm(CommitFaultPoint::PendingPrecommit);
        assert!(matches!(
            store.stage(&first, "receipt:first"),
            Err(LocalPackageStoreError::Io(_))
        ));
        assert!(!store.is_poisoned());
        assert!(store.pending().is_empty());
        let failed_read = store.read(first.root());
        assert!(
            matches!(
                failed_read,
                Err(LocalPackageStoreError::MissingObject { .. })
            ),
            "unexpected failed-stage read: {failed_read:?}"
        );

        store
            .stage(&first, "receipt:kept")
            .expect("stage existing object reference");
        faults.arm(CommitFaultPoint::PendingPrecommit);
        assert!(matches!(
            store.stage(&first, "receipt:failed-second"),
            Err(LocalPackageStoreError::Io(_))
        ));
        assert_eq!(store.pending().len(), 1);
        assert!(
            store.read(first.root()).is_ok(),
            "existing CAS must survive"
        );
        fs::remove_dir_all(root).expect("remove store");
    }
}
