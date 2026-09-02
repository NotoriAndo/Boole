//! Race-resistant opening of the three fixed installed authority files.
//!
//! The public entrypoint accepts no path. Each fixed component is opened
//! relative to the previously verified directory descriptor, never through a
//! caller-selected path or the process working directory.

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

use thiserror::Error;

use crate::{
    closed_local_replay_grant::verify_closed_local_replay_grant_bytes,
    closed_local_replay_grant::{
        TRACKED_REAL_HISTORY_ANCHOR_BYTES, TRACKED_REAL_HISTORY_TASK_BYTES,
    },
    verify_authority_bundle, verify_closed_local_replay_execution_authority_bytes,
    verify_local_execution_authority_bytes, AuthorityError,
    ClosedLocalReplayExecutionAuthorityError, ClosedLocalReplayGrantError,
    LocalExecutionAuthorityError, VerifiedAuthorityBundle,
    VerifiedClosedLocalReplayExecutionAuthority, VerifiedClosedLocalReplayGrant,
    VerifiedLocalExecutionAuthority, TRACKED_CHECKER_BYTES, TRACKED_CHECKER_POLICY_BYTES,
    TRACKED_CHECKER_RELEASE_MANIFEST_BYTES, TRACKED_CLOSED_LOCAL_REPLAY_EXECUTION_AUTHORITY_BYTES,
    TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES, TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
    TRACKED_EXECUTION_POLICY_BYTES, TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES,
    TRACKED_REGISTRY_BYTES, TRACKED_TOOLCHAIN_IDENTITY_BYTES,
};

const AUTHORITY_DIRECTORY_COMPONENTS: [&str; 4] = ["usr", "share", "boole", "native-shadow"];
const AUTHORITY_DIRECTORY_MODE: u32 = 0o555;
const AUTHORITY_FILE_MODE: u32 = 0o444;

#[derive(Debug, Error)]
pub enum InstalledAuthorityError {
    #[error("installed authority I/O failed at {label}: {source}")]
    Io {
        label: String,
        #[source]
        source: io::Error,
    },
    #[error("installed authority metadata is unsafe at {label}: {reason}")]
    UnsafeMetadata { label: String, reason: &'static str },
    #[error(transparent)]
    Authority(#[from] AuthorityError),
    #[error(transparent)]
    ReplayGrant(#[from] ClosedLocalReplayGrantError),
    #[error(transparent)]
    ReplayExecutionAuthority(#[from] ClosedLocalReplayExecutionAuthorityError),
    #[error(transparent)]
    LocalExecutionAuthority(#[from] LocalExecutionAuthorityError),
}

/// Exact installed grant, execution authority, checker, checker policy and
/// checker release proven through fixed descriptor-relative paths.
#[derive(Debug)]
pub struct VerifiedInstalledClosedLocalReplayExecutionAuthorities {
    runtime: VerifiedClosedLocalReplayExecutionAuthorities,
    material_root: File,
    required_uid: u32,
    required_gid: u32,
}

/// Path-independent replay authority created only after every retained,
/// signed release handle has been checked against the compiled contract.
/// This is the authority shape consumed by both the fixed Linux install and
/// the curl-installed Mac host route.
#[derive(Debug)]
pub struct VerifiedClosedLocalReplayExecutionAuthorities {
    grant: VerifiedClosedLocalReplayGrant,
    local_execution_authority: VerifiedLocalExecutionAuthority,
    execution_authority: VerifiedClosedLocalReplayExecutionAuthority,
}

impl VerifiedClosedLocalReplayExecutionAuthorities {
    pub fn grant(&self) -> &VerifiedClosedLocalReplayGrant {
        &self.grant
    }

    pub fn local_execution_authority(&self) -> &VerifiedLocalExecutionAuthority {
        &self.local_execution_authority
    }

    pub fn execution_authority(&self) -> &VerifiedClosedLocalReplayExecutionAuthority {
        &self.execution_authority
    }
}

/// Exact retained handles required to reconstruct the closed-local replay
/// authority without reopening any pathname selected by a request or by the
/// surrounding process environment.
#[derive(Debug, Clone, Copy)]
pub struct RetainedClosedLocalReplayAuthorityFiles<'a> {
    pub registry: &'a File,
    pub execution_policy: &'a File,
    pub toolchain_identity: &'a File,
    pub replay_grant: &'a File,
    pub registry_overlay: &'a File,
    pub local_execution_authority: &'a File,
    pub replay_execution_authority: &'a File,
    pub checker_release_manifest: &'a File,
}

/// Per-request proof that the installed checker release and replay fixture
/// still match the compiled authority. The exact installed task/anchor bytes
/// are retained so the executor consumes what was re-opened, not a parallel
/// compile-time copy.
#[derive(Debug)]
pub struct VerifiedInstalledClosedLocalReplayExecutionMaterials {
    task: Vec<u8>,
    anchor: Vec<u8>,
}

impl VerifiedInstalledClosedLocalReplayExecutionMaterials {
    pub fn task_bytes(&self) -> &[u8] {
        &self.task
    }

    pub fn anchor_bytes(&self) -> &[u8] {
        &self.anchor
    }
}

impl VerifiedInstalledClosedLocalReplayExecutionAuthorities {
    pub fn grant(&self) -> &VerifiedClosedLocalReplayGrant {
        self.runtime.grant()
    }

    pub fn execution_authority(&self) -> &VerifiedClosedLocalReplayExecutionAuthority {
        self.runtime.execution_authority()
    }

    pub fn into_runtime_authorities(self) -> VerifiedClosedLocalReplayExecutionAuthorities {
        self.runtime
    }

    pub fn reverify_execution_materials(
        &self,
    ) -> Result<VerifiedInstalledClosedLocalReplayExecutionMaterials, InstalledAuthorityError> {
        verify_closed_local_replay_execution_materials_beneath(
            &self.material_root,
            self.required_uid,
            self.required_gid,
        )
    }
}

/// Verify authority files already retained by a signed release verifier.
/// File ownership and install-path selection are intentionally outside this
/// function: the caller has already authenticated those handles. This layer
/// proves that their bytes are the exact closed-local, non-issuable contract.
pub fn verify_closed_local_replay_execution_authorities_from_retained_files(
    files: RetainedClosedLocalReplayAuthorityFiles<'_>,
) -> Result<VerifiedClosedLocalReplayExecutionAuthorities, InstalledAuthorityError> {
    let registry =
        read_retained_exact(files.registry, "retained registry", TRACKED_REGISTRY_BYTES)?;
    let execution_policy = read_retained_exact(
        files.execution_policy,
        "retained execution policy",
        TRACKED_EXECUTION_POLICY_BYTES,
    )?;
    let toolchain_identity = read_retained_exact(
        files.toolchain_identity,
        "retained toolchain identity",
        TRACKED_TOOLCHAIN_IDENTITY_BYTES,
    )?;
    let authority = verify_authority_bundle(&registry, &execution_policy, &toolchain_identity)?;
    let replay_grant = read_retained_exact(
        files.replay_grant,
        "retained replay grant",
        TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
    )?;
    let registry_overlay = read_retained_exact(
        files.registry_overlay,
        "retained registry overlay",
        TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
    )?;
    let grant =
        verify_closed_local_replay_grant_bytes(&replay_grant, &registry_overlay, &authority)?;
    let local_execution_authority = read_retained_exact(
        files.local_execution_authority,
        "retained local execution authority",
        TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES,
    )?;
    let local_execution_authority =
        verify_local_execution_authority_bytes(&local_execution_authority)?;
    let replay_execution_authority = read_retained_exact(
        files.replay_execution_authority,
        "retained replay execution authority",
        TRACKED_CLOSED_LOCAL_REPLAY_EXECUTION_AUTHORITY_BYTES,
    )?;
    let execution_authority =
        verify_closed_local_replay_execution_authority_bytes(&replay_execution_authority)?;
    read_retained_exact(
        files.checker_release_manifest,
        "retained checker release manifest",
        TRACKED_CHECKER_RELEASE_MANIFEST_BYTES,
    )?;
    Ok(VerifiedClosedLocalReplayExecutionAuthorities {
        grant,
        local_execution_authority,
        execution_authority,
    })
}

fn read_retained_exact(
    file: &File,
    label: &str,
    expected: &[u8],
) -> Result<Vec<u8>, InstalledAuthorityError> {
    let mut file = file
        .try_clone()
        .map_err(|source| io_failure(label, source))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|source| io_failure(label, source))?;
    let metadata = file
        .metadata()
        .map_err(|source| io_failure(label, source))?;
    if !metadata.is_file() || metadata.len() != expected.len() as u64 {
        return Err(unsafe_metadata(
            label,
            "retained authority is not a regular file of the tracked length",
        ));
    }
    let mut bytes = Vec::with_capacity(expected.len());
    file.take(expected.len() as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| io_failure(label, source))?;
    if bytes != expected {
        return Err(unsafe_metadata(
            label,
            "retained authority bytes differ from the compiled contract",
        ));
    }
    Ok(bytes)
}

struct ExpectedAuthorityFile {
    label: &'static str,
    basename: &'static str,
    tracked: &'static [u8],
}

const EXPECTED_AUTHORITY_FILES: [ExpectedAuthorityFile; 3] = [
    ExpectedAuthorityFile {
        label: "registry",
        basename: "registry-v1.json",
        tracked: TRACKED_REGISTRY_BYTES,
    },
    ExpectedAuthorityFile {
        label: "execution policy",
        basename: "execution-policy-v1.json",
        tracked: TRACKED_EXECUTION_POLICY_BYTES,
    },
    ExpectedAuthorityFile {
        label: "toolchain identity",
        basename: "toolchain-identity-v1.json",
        tracked: TRACKED_TOOLCHAIN_IDENTITY_BYTES,
    },
];

/// Open and verify the only installed authority bundle accepted by the
/// disabled native-shadow qualification path.
///
/// No request, environment variable, current directory, or caller can choose
/// any component of these paths. The opener itself is Unix-portable so it can
/// be unit-tested on macOS; the launcher and execution path remain Linux-only.
pub fn open_verified_installed_authority_bundle(
) -> Result<VerifiedAuthorityBundle, InstalledAuthorityError> {
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open("/")
        .map_err(|source| io_failure("/", source))?;
    open_verified_authority_bundle_beneath(&root, 0, 0)
}

/// Open the only installed replay grant that can authorize the permanently
/// non-issuable real-history fixture. The caller supplies no path. The same
/// fixed root-owned authority bundle is verified first, so this exception can
/// never turn the production registry into an issuable registry.
pub fn open_verified_installed_closed_local_replay_grant(
) -> Result<VerifiedClosedLocalReplayGrant, InstalledAuthorityError> {
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open("/")
        .map_err(|source| io_failure("/", source))?;
    open_verified_closed_local_replay_grant_beneath(&root, 0, 0)
}

/// Open the exact installed closed-local successor authority through the same
/// descriptor-relative, root-owned hierarchy as the qualification bundle.
pub fn open_verified_installed_local_execution_authority(
) -> Result<VerifiedLocalExecutionAuthority, InstalledAuthorityError> {
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open("/")
        .map_err(|source| io_failure("/", source))?;
    open_verified_local_execution_authority_beneath(&root, 0, 0)
}

/// Open the complete installed replay execution authority. No path or release
/// selector is accepted from the caller.
pub fn open_verified_installed_closed_local_replay_execution_authorities(
) -> Result<VerifiedInstalledClosedLocalReplayExecutionAuthorities, InstalledAuthorityError> {
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open("/")
        .map_err(|source| io_failure("/", source))?;
    open_verified_closed_local_replay_execution_authorities_beneath(&root, 0, 0)
}

fn open_verified_closed_local_replay_execution_authorities_beneath(
    root: &File,
    required_uid: u32,
    required_gid: u32,
) -> Result<VerifiedInstalledClosedLocalReplayExecutionAuthorities, InstalledAuthorityError> {
    let material_root = root
        .try_clone()
        .map_err(|source| io_failure("duplicate installed material root", source))?;
    let grant = open_verified_closed_local_replay_grant_beneath(root, required_uid, required_gid)?;
    let local_execution_authority =
        open_verified_local_execution_authority_beneath(root, required_uid, required_gid)?;
    let (directory, display_path) =
        open_verified_authority_directory(root, required_uid, required_gid)?;
    let basename = "closed-local-replay-execution-authority-v1.json";
    let label = format!("{display_path}/{basename}");
    let file = open_child(&directory, basename, false, &label)?;
    let bytes = read_verified_file(
        file,
        &label,
        required_uid,
        required_gid,
        TRACKED_CLOSED_LOCAL_REPLAY_EXECUTION_AUTHORITY_BYTES.len(),
    )?;
    let execution_authority = verify_closed_local_replay_execution_authority_bytes(&bytes)?;
    verify_closed_local_replay_execution_materials_from_directory(
        directory,
        &display_path,
        required_uid,
        required_gid,
    )?;
    Ok(VerifiedInstalledClosedLocalReplayExecutionAuthorities {
        runtime: VerifiedClosedLocalReplayExecutionAuthorities {
            grant,
            local_execution_authority,
            execution_authority,
        },
        material_root,
        required_uid,
        required_gid,
    })
}

fn verify_closed_local_replay_execution_materials_beneath(
    root: &File,
    required_uid: u32,
    required_gid: u32,
) -> Result<VerifiedInstalledClosedLocalReplayExecutionMaterials, InstalledAuthorityError> {
    let (directory, display_path) =
        open_verified_authority_directory(root, required_uid, required_gid)?;
    verify_closed_local_replay_execution_materials_from_directory(
        directory,
        &display_path,
        required_uid,
        required_gid,
    )
}

fn verify_closed_local_replay_execution_materials_from_directory(
    mut directory: File,
    display_path: &str,
    required_uid: u32,
    required_gid: u32,
) -> Result<VerifiedInstalledClosedLocalReplayExecutionMaterials, InstalledAuthorityError> {
    let fixtures_label = format!("{display_path}/fixtures");
    let fixtures = open_child(&directory, "fixtures", true, &fixtures_label)?;
    validate_directory(
        &fixtures,
        &fixtures_label,
        required_uid,
        required_gid,
        Some(AUTHORITY_DIRECTORY_MODE),
    )?;
    let fixture_label = format!("{fixtures_label}/a-rooted-native-mining-e2e-v1-real-history");
    let fixture = open_child(
        &fixtures,
        "a-rooted-native-mining-e2e-v1-real-history",
        true,
        &fixture_label,
    )?;
    validate_directory(
        &fixture,
        &fixture_label,
        required_uid,
        required_gid,
        Some(AUTHORITY_DIRECTORY_MODE),
    )?;
    let mut verified_fixture_bytes = Vec::with_capacity(2);
    for (basename, tracked) in [
        ("task.json", TRACKED_REAL_HISTORY_TASK_BYTES),
        ("anchor.rs", TRACKED_REAL_HISTORY_ANCHOR_BYTES),
    ] {
        let label = format!("{fixture_label}/{basename}");
        let file = open_child(&fixture, basename, false, &label)?;
        let bytes = read_verified_file(file, &label, required_uid, required_gid, tracked.len())?;
        if bytes != tracked {
            return Err(unsafe_metadata(
                label,
                "installed replay fixture bytes differ from the compiled authority",
            ));
        }
        verified_fixture_bytes.push(bytes);
    }

    let checkers_label = format!("{display_path}/checkers");
    directory = open_child(&directory, "checkers", true, &checkers_label)?;
    validate_directory(
        &directory,
        &checkers_label,
        required_uid,
        required_gid,
        Some(AUTHORITY_DIRECTORY_MODE),
    )?;
    let release_label = format!("{checkers_label}/rust-tuple-struct-project-v1");
    directory = open_child(
        &directory,
        "rust-tuple-struct-project-v1",
        true,
        &release_label,
    )?;
    validate_directory(
        &directory,
        &release_label,
        required_uid,
        required_gid,
        Some(AUTHORITY_DIRECTORY_MODE),
    )?;
    for (basename, tracked) in [
        ("checker.py", TRACKED_CHECKER_BYTES),
        ("policy.json", TRACKED_CHECKER_POLICY_BYTES),
        (
            "RELEASE-MANIFEST.json",
            TRACKED_CHECKER_RELEASE_MANIFEST_BYTES,
        ),
    ] {
        let label = format!("{release_label}/{basename}");
        let file = open_child(&directory, basename, false, &label)?;
        let bytes = read_verified_file(file, &label, required_uid, required_gid, tracked.len())?;
        if bytes != tracked {
            return Err(unsafe_metadata(
                label,
                "installed checker release bytes differ from the compiled release",
            ));
        }
    }
    let mut verified_fixture_bytes = verified_fixture_bytes.into_iter();
    Ok(VerifiedInstalledClosedLocalReplayExecutionMaterials {
        task: verified_fixture_bytes
            .next()
            .expect("the fixed task fixture was pushed"),
        anchor: verified_fixture_bytes
            .next()
            .expect("the fixed anchor fixture was pushed"),
    })
}

fn open_verified_closed_local_replay_grant_beneath(
    root: &File,
    required_uid: u32,
    required_gid: u32,
) -> Result<VerifiedClosedLocalReplayGrant, InstalledAuthorityError> {
    let authority = open_verified_authority_bundle_beneath(root, required_uid, required_gid)?;

    validate_directory(root, "/", required_uid, required_gid, None)?;
    let mut current = root.try_clone().map_err(|source| io_failure("/", source))?;
    let mut display_path = String::new();
    for (index, component) in AUTHORITY_DIRECTORY_COMPONENTS.iter().enumerate() {
        display_path.push('/');
        display_path.push_str(component);
        let child = open_child(&current, component, true, &display_path)?;
        let exact_mode =
            (index + 1 == AUTHORITY_DIRECTORY_COMPONENTS.len()).then_some(AUTHORITY_DIRECTORY_MODE);
        validate_directory(
            &child,
            &display_path,
            required_uid,
            required_gid,
            exact_mode,
        )?;
        current = child;
    }

    let overlay_label = format!("{display_path}/closed-local-replay-registry-overlay-v1.json");
    let overlay_file = open_child(
        &current,
        "closed-local-replay-registry-overlay-v1.json",
        false,
        &overlay_label,
    )?;
    let overlay_raw = read_verified_file(
        overlay_file,
        &overlay_label,
        required_uid,
        required_gid,
        TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES.len(),
    )?;

    let grant_label = format!("{display_path}/closed-local-replay-grant-v1.json");
    let grant_file = open_child(
        &current,
        "closed-local-replay-grant-v1.json",
        false,
        &grant_label,
    )?;
    let raw = read_verified_file(
        grant_file,
        &grant_label,
        required_uid,
        required_gid,
        TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES.len(),
    )?;
    verify_closed_local_replay_grant_bytes(&raw, &overlay_raw, &authority)
        .map_err(InstalledAuthorityError::from)
}

fn open_verified_authority_bundle_beneath(
    root: &File,
    required_uid: u32,
    required_gid: u32,
) -> Result<VerifiedAuthorityBundle, InstalledAuthorityError> {
    let (current, display_path) =
        open_verified_authority_directory(root, required_uid, required_gid)?;

    let mut opened = Vec::with_capacity(EXPECTED_AUTHORITY_FILES.len());
    for expected in &EXPECTED_AUTHORITY_FILES {
        let label = format!("{display_path}/{}", expected.basename);
        let file = open_child(&current, expected.basename, false, &label)?;
        let bytes = read_verified_file(
            file,
            &label,
            required_uid,
            required_gid,
            expected.tracked.len(),
        )?;
        opened.push((expected.label, bytes));
    }

    verify_authority_bundle(&opened[0].1, &opened[1].1, &opened[2].1)
        .map_err(InstalledAuthorityError::from)
}

fn open_verified_local_execution_authority_beneath(
    root: &File,
    required_uid: u32,
    required_gid: u32,
) -> Result<VerifiedLocalExecutionAuthority, InstalledAuthorityError> {
    let (directory, display_path) =
        open_verified_authority_directory(root, required_uid, required_gid)?;
    let basename = "local-execution-authority-v1.json";
    let label = format!("{display_path}/{basename}");
    let file = open_child(&directory, basename, false, &label)?;
    let bytes = read_verified_file(
        file,
        &label,
        required_uid,
        required_gid,
        TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES.len(),
    )?;
    verify_local_execution_authority_bytes(&bytes).map_err(|error| {
        InstalledAuthorityError::UnsafeMetadata {
            label,
            reason: match error {
                crate::LocalExecutionAuthorityError::ByteMismatch => {
                    "local execution authority differs from tracked bytes"
                }
                _ => "local execution authority violates its strict contract",
            },
        }
    })
}

fn open_verified_authority_directory(
    root: &File,
    required_uid: u32,
    required_gid: u32,
) -> Result<(File, String), InstalledAuthorityError> {
    validate_directory(root, "/", required_uid, required_gid, None)?;

    let mut current = root.try_clone().map_err(|source| io_failure("/", source))?;
    let mut display_path = String::new();
    for (index, component) in AUTHORITY_DIRECTORY_COMPONENTS.iter().enumerate() {
        display_path.push('/');
        display_path.push_str(component);
        let child = open_child(&current, component, true, &display_path)?;
        let exact_mode =
            (index + 1 == AUTHORITY_DIRECTORY_COMPONENTS.len()).then_some(AUTHORITY_DIRECTORY_MODE);
        validate_directory(
            &child,
            &display_path,
            required_uid,
            required_gid,
            exact_mode,
        )?;
        current = child;
    }

    Ok((current, display_path))
}

#[allow(unsafe_code)]
fn open_child(
    parent: &File,
    basename: &str,
    directory: bool,
    label: &str,
) -> Result<File, InstalledAuthorityError> {
    let basename = CString::new(basename).expect("fixed authority basenames contain no NUL");
    let mut flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK;
    if directory {
        flags |= libc::O_DIRECTORY;
    }

    // SAFETY: `parent` owns a live descriptor, `basename` is NUL-terminated,
    // and the returned descriptor is immediately transferred exactly once to
    // `File`, which closes it on every return path.
    let raw_fd = unsafe { libc::openat(parent.as_raw_fd(), basename.as_ptr(), flags) };
    if raw_fd < 0 {
        return Err(io_failure(label, io::Error::last_os_error()));
    }
    // SAFETY: `openat` returned a new owned descriptor and no other owner is
    // constructed from it.
    Ok(unsafe { File::from_raw_fd(raw_fd) })
}

fn validate_directory(
    directory: &File,
    label: &str,
    required_uid: u32,
    required_gid: u32,
    exact_mode: Option<u32>,
) -> Result<(), InstalledAuthorityError> {
    let metadata = directory
        .metadata()
        .map_err(|source| io_failure(label, source))?;
    let mode = metadata.mode() & 0o7777;
    if !metadata.file_type().is_dir() {
        return Err(unsafe_metadata(label, "component is not a directory"));
    }
    if metadata.uid() != required_uid || metadata.gid() != required_gid {
        return Err(unsafe_metadata(
            label,
            "directory owner/group does not match the fixed authority owner",
        ));
    }
    if mode & 0o022 != 0 {
        return Err(unsafe_metadata(
            label,
            "directory is writable by group or other",
        ));
    }
    if exact_mode.is_some_and(|required| mode != required) {
        return Err(unsafe_metadata(
            label,
            "authority directory mode is not exactly 0555",
        ));
    }
    Ok(())
}

fn read_verified_file(
    mut file: File,
    label: &str,
    required_uid: u32,
    required_gid: u32,
    tracked_len: usize,
) -> Result<Vec<u8>, InstalledAuthorityError> {
    validate_file_metadata(&file, label, required_uid, required_gid, tracked_len)?;
    let mut bytes = Vec::with_capacity(tracked_len);
    (&mut file)
        .take((tracked_len + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| io_failure(label, source))?;
    validate_file_metadata(&file, label, required_uid, required_gid, tracked_len)?;
    if bytes.len() != tracked_len {
        return Err(unsafe_metadata(
            label,
            "authority byte length changed while reading",
        ));
    }
    Ok(bytes)
}

fn validate_file_metadata(
    file: &File,
    label: &str,
    required_uid: u32,
    required_gid: u32,
    tracked_len: usize,
) -> Result<(), InstalledAuthorityError> {
    let metadata = file
        .metadata()
        .map_err(|source| io_failure(label, source))?;
    if !metadata.file_type().is_file() {
        return Err(unsafe_metadata(label, "authority is not a regular file"));
    }
    if metadata.nlink() != 1 {
        return Err(unsafe_metadata(
            label,
            "authority must have exactly one hard link",
        ));
    }
    if metadata.uid() != required_uid || metadata.gid() != required_gid {
        return Err(unsafe_metadata(
            label,
            "authority owner/group does not match the fixed authority owner",
        ));
    }
    if metadata.mode() & 0o7777 != AUTHORITY_FILE_MODE {
        return Err(unsafe_metadata(label, "authority mode is not exactly 0444"));
    }
    if metadata.len() != tracked_len as u64 {
        return Err(unsafe_metadata(
            label,
            "authority byte length differs from the compiled authority",
        ));
    }
    Ok(())
}

fn io_failure(label: impl Into<String>, source: io::Error) -> InstalledAuthorityError {
    InstalledAuthorityError::Io {
        label: label.into(),
        source,
    }
}

fn unsafe_metadata(label: impl Into<String>, reason: &'static str) -> InstalledAuthorityError {
    InstalledAuthorityError::UnsafeMetadata {
        label: label.into(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File, OpenOptions};
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::{
        closed_local_replay_grant::{
            TRACKED_REAL_HISTORY_ANCHOR_BYTES, TRACKED_REAL_HISTORY_TASK_BYTES,
        },
        AuthorityError, TRACKED_CHECKER_BYTES, TRACKED_CHECKER_POLICY_BYTES,
        TRACKED_CHECKER_RELEASE_MANIFEST_BYTES,
        TRACKED_CLOSED_LOCAL_REPLAY_EXECUTION_AUTHORITY_BYTES,
        TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
        TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES, TRACKED_EXECUTION_POLICY_BYTES,
        TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES, TRACKED_REGISTRY_BYTES,
        TRACKED_TOOLCHAIN_IDENTITY_BYTES,
    };

    use super::{
        open_verified_authority_bundle_beneath,
        open_verified_closed_local_replay_execution_authorities_beneath,
        open_verified_closed_local_replay_grant_beneath, open_verified_installed_authority_bundle,
        open_verified_installed_closed_local_replay_grant,
        open_verified_installed_local_execution_authority,
        open_verified_local_execution_authority_beneath,
        verify_closed_local_replay_execution_authorities_from_retained_files,
        InstalledAuthorityError, RetainedClosedLocalReplayAuthorityFiles,
    };

    static NEXT_TEST_TREE: AtomicU64 = AtomicU64::new(0);

    struct TestAuthorityTree {
        root: PathBuf,
        authority_dir: PathBuf,
        uid: u32,
        gid: u32,
    }

    impl TestAuthorityTree {
        fn new() -> Self {
            let suffix = NEXT_TEST_TREE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "boole-native-authority-{}-{suffix}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("unique test root must be creatable");
            let authority_dir = root.join("usr/share/boole/native-shadow");
            fs::create_dir_all(&authority_dir).expect("authority tree must be creatable");

            for (basename, bytes) in [
                ("registry-v1.json", TRACKED_REGISTRY_BYTES),
                ("execution-policy-v1.json", TRACKED_EXECUTION_POLICY_BYTES),
                (
                    "toolchain-identity-v1.json",
                    TRACKED_TOOLCHAIN_IDENTITY_BYTES,
                ),
                (
                    "closed-local-replay-grant-v1.json",
                    TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
                ),
                (
                    "closed-local-replay-registry-overlay-v1.json",
                    TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
                ),
                (
                    "local-execution-authority-v1.json",
                    TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES,
                ),
                (
                    "closed-local-replay-execution-authority-v1.json",
                    TRACKED_CLOSED_LOCAL_REPLAY_EXECUTION_AUTHORITY_BYTES,
                ),
            ] {
                let path = authority_dir.join(basename);
                fs::write(&path, bytes).expect("authority fixture must be writable");
                set_mode(&path, 0o444);
            }
            let checker_dir = authority_dir.join("checkers/rust-tuple-struct-project-v1");
            fs::create_dir_all(&checker_dir).expect("checker tree must be creatable");
            for (basename, bytes) in [
                ("checker.py", TRACKED_CHECKER_BYTES),
                ("policy.json", TRACKED_CHECKER_POLICY_BYTES),
                (
                    "RELEASE-MANIFEST.json",
                    TRACKED_CHECKER_RELEASE_MANIFEST_BYTES,
                ),
            ] {
                let path = checker_dir.join(basename);
                fs::write(&path, bytes).expect("checker fixture must be writable");
                set_mode(&path, 0o444);
            }
            set_mode(&checker_dir, 0o555);
            set_mode(checker_dir.parent().expect("checkers parent"), 0o555);
            let fixture_dir =
                authority_dir.join("fixtures/a-rooted-native-mining-e2e-v1-real-history");
            fs::create_dir_all(&fixture_dir).expect("replay fixture tree must be creatable");
            for (basename, bytes) in [
                ("task.json", TRACKED_REAL_HISTORY_TASK_BYTES),
                ("anchor.rs", TRACKED_REAL_HISTORY_ANCHOR_BYTES),
            ] {
                let path = fixture_dir.join(basename);
                fs::write(&path, bytes).expect("replay fixture must be writable");
                set_mode(&path, 0o444);
            }
            set_mode(&fixture_dir, 0o555);
            set_mode(fixture_dir.parent().expect("fixtures parent"), 0o555);
            set_mode(&authority_dir, 0o555);
            let metadata = fs::metadata(&root).expect("test root metadata");
            Self {
                root,
                authority_dir,
                uid: metadata.uid(),
                gid: metadata.gid(),
            }
        }

        fn open(&self) -> Result<crate::VerifiedAuthorityBundle, InstalledAuthorityError> {
            let root = OpenOptions::new()
                .read(true)
                .open(&self.root)
                .expect("test root must open");
            open_verified_authority_bundle_beneath(&root, self.uid, self.gid)
        }

        fn open_replay_grant(
            &self,
        ) -> Result<crate::VerifiedClosedLocalReplayGrant, InstalledAuthorityError> {
            let root = OpenOptions::new()
                .read(true)
                .open(&self.root)
                .expect("test root must open");
            open_verified_closed_local_replay_grant_beneath(&root, self.uid, self.gid)
        }

        fn open_replay_execution_authorities(
            &self,
        ) -> Result<
            super::VerifiedInstalledClosedLocalReplayExecutionAuthorities,
            InstalledAuthorityError,
        > {
            let root = OpenOptions::new()
                .read(true)
                .open(&self.root)
                .expect("test root must open");
            open_verified_closed_local_replay_execution_authorities_beneath(
                &root, self.uid, self.gid,
            )
        }

        fn make_authority_dir_writable(&self) {
            set_mode(&self.authority_dir, 0o755);
        }

        fn path(&self, basename: &str) -> PathBuf {
            self.authority_dir.join(basename)
        }
    }

    impl Drop for TestAuthorityTree {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.authority_dir, fs::Permissions::from_mode(0o755));
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn set_mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .expect("test fixture mode must be settable");
    }

    #[test]
    fn installed_authority_has_one_fixed_path_entrypoint() {
        let _entrypoint: fn() -> Result<_, _> = open_verified_installed_authority_bundle;
        let _replay_entrypoint: fn() -> Result<_, _> =
            open_verified_installed_closed_local_replay_grant;
        let _successor_entrypoint: fn() -> Result<_, _> =
            open_verified_installed_local_execution_authority;
    }

    #[test]
    fn same_descriptor_walk_accepts_the_exact_three_file_bundle() {
        let tree = TestAuthorityTree::new();
        let bundle = tree.open().expect("exact bundle must verify");

        assert_eq!(bundle.registry().templates().len(), 1);
        assert_eq!(bundle.registry_digest().len(), 64);
        assert_eq!(bundle.execution_policy_digest().len(), 64);
        assert_eq!(bundle.toolchain_identity_digest().len(), 64);
    }

    #[test]
    fn fixed_installed_grant_requires_exact_root_owned_grant_and_overlay_files() {
        let tree = TestAuthorityTree::new();

        let grant = tree
            .open_replay_grant()
            .expect("the exact fourth authority file must verify");
        assert_eq!(grant.max_matrix_requests_total(), 4);
        assert_eq!(grant.max_checker_executions_total(), 3);
        assert_eq!(
            fs::read(tree.path("closed-local-replay-grant-v1.json"))
                .expect("installed grant bytes"),
            TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES
        );
    }

    #[test]
    fn replay_execution_authorities_require_the_exact_authority_and_checker_release() {
        let tree = TestAuthorityTree::new();
        let installed = tree
            .open_replay_execution_authorities()
            .expect("exact execution authorities must verify");
        assert_eq!(installed.grant().max_checker_executions_total(), 3);
        assert!(!installed.execution_authority().activation_allowed());
        assert!(installed
            .execution_authority()
            .requires_runtime_rootfs_replay());

        for relative in [
            "closed-local-replay-execution-authority-v1.json",
            "checkers/rust-tuple-struct-project-v1/checker.py",
            "checkers/rust-tuple-struct-project-v1/policy.json",
            "checkers/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json",
            "fixtures/a-rooted-native-mining-e2e-v1-real-history/task.json",
            "fixtures/a-rooted-native-mining-e2e-v1-real-history/anchor.rs",
        ] {
            let tree = TestAuthorityTree::new();
            let path = tree.path(relative);
            set_mode(&path, 0o644);
            let mut bytes = fs::read(&path).expect("installed bytes");
            bytes[0] ^= 1;
            fs::write(&path, bytes).expect("drift installed bytes");
            set_mode(&path, 0o444);
            assert!(
                tree.open_replay_execution_authorities().is_err(),
                "{relative}"
            );
        }
    }

    #[test]
    fn retained_release_handles_create_runtime_authority_without_reopening_paths() {
        let tree = TestAuthorityTree::new();
        let registry = File::open(tree.path("registry-v1.json")).expect("registry handle");
        let execution_policy =
            File::open(tree.path("execution-policy-v1.json")).expect("execution-policy handle");
        let toolchain_identity =
            File::open(tree.path("toolchain-identity-v1.json")).expect("toolchain handle");
        let replay_grant =
            File::open(tree.path("closed-local-replay-grant-v1.json")).expect("grant handle");
        let registry_overlay =
            File::open(tree.path("closed-local-replay-registry-overlay-v1.json"))
                .expect("overlay handle");
        let local_execution_authority = File::open(tree.path("local-execution-authority-v1.json"))
            .expect("local execution authority handle");
        let replay_execution_authority =
            File::open(tree.path("closed-local-replay-execution-authority-v1.json"))
                .expect("replay execution authority handle");
        let checker_release_manifest =
            File::open(tree.path("checkers/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json"))
                .expect("checker release handle");

        tree.make_authority_dir_writable();
        fs::rename(
            tree.path("closed-local-replay-grant-v1.json"),
            tree.path("retained-original-grant.json"),
        )
        .expect("move retained grant path");
        fs::write(
            tree.path("closed-local-replay-grant-v1.json"),
            b"replacement",
        )
        .expect("replace grant path");

        let runtime = verify_closed_local_replay_execution_authorities_from_retained_files(
            RetainedClosedLocalReplayAuthorityFiles {
                registry: &registry,
                execution_policy: &execution_policy,
                toolchain_identity: &toolchain_identity,
                replay_grant: &replay_grant,
                registry_overlay: &registry_overlay,
                local_execution_authority: &local_execution_authority,
                replay_execution_authority: &replay_execution_authority,
                checker_release_manifest: &checker_release_manifest,
            },
        )
        .expect("retained verified bytes must create runtime authority");
        assert_eq!(runtime.grant().max_checker_executions_total(), 3);
        assert!(runtime
            .execution_authority()
            .requires_runtime_rootfs_replay());
    }

    #[test]
    fn replay_execution_reopens_task_and_anchor_for_every_permit() {
        for relative in [
            "fixtures/a-rooted-native-mining-e2e-v1-real-history/task.json",
            "fixtures/a-rooted-native-mining-e2e-v1-real-history/anchor.rs",
        ] {
            let tree = TestAuthorityTree::new();
            let installed = tree
                .open_replay_execution_authorities()
                .expect("exact startup authority must verify");
            let path = tree.path(relative);
            set_mode(&path, 0o644);
            let mut bytes = fs::read(&path).expect("installed fixture bytes");
            bytes[0] ^= 1;
            fs::write(&path, bytes).expect("drift installed fixture after startup");
            set_mode(&path, 0o444);

            assert!(
                installed.reverify_execution_materials().is_err(),
                "per-request revalidation missed {relative}"
            );
        }
    }

    #[test]
    fn replay_grant_or_overlay_byte_drift_is_rejected() {
        for basename in [
            "closed-local-replay-grant-v1.json",
            "closed-local-replay-registry-overlay-v1.json",
        ] {
            let tree = TestAuthorityTree::new();
            let path = tree.path(basename);
            set_mode(&path, 0o644);
            let mut bytes = fs::read(&path).expect("authority bytes");
            *bytes.last_mut().expect("nonempty authority") ^= 1;
            fs::write(&path, bytes).expect("test mutation");
            set_mode(&path, 0o444);

            assert!(
                tree.open_replay_grant().is_err(),
                "byte drift must fail: {basename}"
            );
        }
    }

    #[test]
    fn replay_grant_uses_only_its_fixed_basename_and_refuses_symlinks() {
        let renamed = TestAuthorityTree::new();
        renamed.make_authority_dir_writable();
        fs::rename(
            renamed.path("closed-local-replay-grant-v1.json"),
            renamed.path("caller-selected-grant.json"),
        )
        .expect("rename test grant");
        set_mode(&renamed.authority_dir, 0o555);
        assert!(matches!(
            renamed.open_replay_grant(),
            Err(InstalledAuthorityError::Io { .. })
        ));

        let linked = TestAuthorityTree::new();
        linked.make_authority_dir_writable();
        fs::rename(
            linked.path("closed-local-replay-grant-v1.json"),
            linked.path("real-grant.json"),
        )
        .expect("move real grant");
        symlink(
            "real-grant.json",
            linked.path("closed-local-replay-grant-v1.json"),
        )
        .expect("symlink test grant");
        set_mode(&linked.authority_dir, 0o555);
        assert!(matches!(
            linked.open_replay_grant(),
            Err(InstalledAuthorityError::Io { .. })
        ));
    }

    #[test]
    fn installed_successor_authority_uses_the_same_safe_descriptor_walk() {
        let tree = TestAuthorityTree::new();
        let root = OpenOptions::new()
            .read(true)
            .open(&tree.root)
            .expect("test root must open");
        let authority = open_verified_local_execution_authority_beneath(&root, tree.uid, tree.gid)
            .expect("exact installed successor authority verifies");

        assert!(!authority.activation_allowed());
        assert!(authority.loopback_only());
        assert!(!authority.mineable_now());
    }

    #[test]
    fn byte_drift_is_rejected_after_safe_open() {
        let tree = TestAuthorityTree::new();
        let path = tree.path("execution-policy-v1.json");
        set_mode(&path, 0o644);
        let mut drifted = TRACKED_EXECUTION_POLICY_BYTES.to_vec();
        let last = drifted.last_mut().expect("tracked policy is nonempty");
        *last ^= 1;
        fs::write(&path, drifted).expect("test drift write");
        set_mode(&path, 0o444);

        assert!(matches!(
            tree.open(),
            Err(InstalledAuthorityError::Authority(
                AuthorityError::ByteMismatch("execution policy")
            ))
        ));
    }

    #[test]
    fn writable_or_wrong_mode_authority_directory_is_rejected() {
        let tree = TestAuthorityTree::new();
        tree.make_authority_dir_writable();

        assert!(matches!(
            tree.open(),
            Err(InstalledAuthorityError::UnsafeMetadata { .. })
        ));
    }

    #[test]
    fn writable_intermediate_ancestor_is_rejected() {
        let tree = TestAuthorityTree::new();
        set_mode(&tree.root.join("usr/share"), 0o775);

        assert!(matches!(
            tree.open(),
            Err(InstalledAuthorityError::UnsafeMetadata { .. })
        ));
    }

    #[test]
    fn intermediate_symlink_is_rejected_without_following_it() {
        let tree = TestAuthorityTree::new();
        let usr = tree.root.join("usr");
        let actual = tree.root.join("actual-usr");
        fs::rename(&usr, &actual).expect("test ancestor rename");
        symlink("actual-usr", &usr).expect("test ancestor symlink creation");

        assert!(matches!(
            tree.open(),
            Err(InstalledAuthorityError::Io { .. })
        ));
    }

    #[test]
    fn final_symlink_is_rejected_without_following_it() {
        let tree = TestAuthorityTree::new();
        tree.make_authority_dir_writable();
        let registry = tree.path("registry-v1.json");
        fs::remove_file(&registry).expect("test registry removal");
        symlink("execution-policy-v1.json", &registry).expect("test symlink creation");
        set_mode(&tree.authority_dir, 0o555);

        assert!(matches!(
            tree.open(),
            Err(InstalledAuthorityError::Io { .. })
        ));
    }

    #[test]
    fn hard_linked_authority_is_rejected() {
        let tree = TestAuthorityTree::new();
        tree.make_authority_dir_writable();
        let registry = tree.path("registry-v1.json");
        let original = tree.path("registry-original.json");
        fs::rename(&registry, &original).expect("test registry rename");
        fs::hard_link(&original, &registry).expect("test hard link creation");
        set_mode(&tree.authority_dir, 0o555);

        assert!(matches!(
            tree.open(),
            Err(InstalledAuthorityError::UnsafeMetadata { .. })
        ));
    }
}
