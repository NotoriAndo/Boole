//! Race-resistant opening of the three fixed installed authority files.
//!
//! The public entrypoint accepts no path. Each fixed component is opened
//! relative to the previously verified directory descriptor, never through a
//! caller-selected path or the process working directory.

use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

use thiserror::Error;

use crate::{
    closed_local_replay_grant::verify_closed_local_replay_grant_bytes, verify_authority_bundle,
    AuthorityError, ClosedLocalReplayGrantError, VerifiedAuthorityBundle,
    VerifiedClosedLocalReplayGrant, TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
    TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES, TRACKED_EXECUTION_POLICY_BYTES,
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
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::{
        AuthorityError, TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
        TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES, TRACKED_EXECUTION_POLICY_BYTES,
        TRACKED_REGISTRY_BYTES, TRACKED_TOOLCHAIN_IDENTITY_BYTES,
    };

    use super::{
        open_verified_authority_bundle_beneath, open_verified_closed_local_replay_grant_beneath,
        open_verified_installed_authority_bundle,
        open_verified_installed_closed_local_replay_grant, InstalledAuthorityError,
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
            ] {
                let path = authority_dir.join(basename);
                fs::write(&path, bytes).expect("authority fixture must be writable");
                set_mode(&path, 0o444);
            }
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
