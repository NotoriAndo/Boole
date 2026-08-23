//! Fixed-path lifetime lock for the privileged native-shadow launcher.

use thiserror::Error;

use crate::startup::VerifiedLauncherPrelockPrerequisites;

/// Opaque proof that the fixed launcher prerequisites remain owned and that
/// this process holds the one nonblocking launcher lock for its lifetime.
///
/// ```compile_fail
/// use boole_native_shadow_launcher::lifetime_lock::LauncherLifetimeLockGuard;
/// let _forged = LauncherLifetimeLockGuard {};
/// ```
///
/// ```compile_fail
/// use boole_native_shadow_launcher::{
///     lifetime_lock::acquire_fixed_launcher_lifetime_lock,
///     startup::verify_fixed_launcher_prelock_prerequisites,
/// };
/// let proof = verify_fixed_launcher_prelock_prerequisites().unwrap();
/// let _first = acquire_fixed_launcher_lifetime_lock(proof);
/// let _second = acquire_fixed_launcher_lifetime_lock(proof);
/// ```
///
/// ```compile_fail
/// use boole_native_shadow_launcher::{
///     lifetime_lock::acquire_fixed_launcher_lifetime_lock,
///     startup::verify_fixed_launcher_prelock_prerequisites,
/// };
/// let proof = verify_fixed_launcher_prelock_prerequisites().unwrap();
/// let guard = acquire_fixed_launcher_lifetime_lock(proof).unwrap();
/// std::thread::spawn(move || drop(guard));
/// ```
#[must_use]
#[derive(Debug)]
#[allow(dead_code)]
pub struct LauncherLifetimeLockGuard {
    prerequisites: VerifiedLauncherPrelockPrerequisites,
    #[cfg(target_os = "linux")]
    held: unix::HeldLauncherLock,
}

#[derive(Debug, Error)]
pub enum LauncherLifetimeLockError {
    #[error("native-shadow launcher lifetime lock requires Linux")]
    UnsupportedPlatform,
    #[error("launcher lifetime-lock I/O failed during {stage}: {source}")]
    Io {
        stage: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("launcher lifetime-lock metadata is unsafe at {stage}: {reason}")]
    UnsafeMetadata {
        stage: &'static str,
        reason: &'static str,
    },
    #[error("another native-shadow launcher already holds the lifetime lock")]
    Busy,
}

/// Consume the input-free pre-lock proof and acquire the one fixed launcher
/// lifetime lock. No path or numeric identity is caller-selected.
pub fn acquire_fixed_launcher_lifetime_lock(
    prerequisites: VerifiedLauncherPrelockPrerequisites,
) -> Result<LauncherLifetimeLockGuard, LauncherLifetimeLockError> {
    #[cfg(target_os = "linux")]
    {
        let node_gid = prerequisites.node_gid();
        let held = unix::acquire_fixed_launcher_lock(node_gid)?;
        Ok(LauncherLifetimeLockGuard {
            prerequisites,
            held,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        drop(prerequisites);
        Err(LauncherLifetimeLockError::UnsupportedPlatform)
    }
}

#[cfg(any(target_os = "linux", all(test, unix)))]
mod unix {
    use std::ffi::CString;
    use std::fs::File;
    #[cfg(target_os = "linux")]
    use std::fs::OpenOptions;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::MetadataExt;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::OpenOptionsExt;

    use super::LauncherLifetimeLockError;

    const RUNTIME_COMPONENTS: [(&str, &str); 3] = [
        ("run", "/run"),
        ("boole", "/run/boole"),
        ("native-shadow", "/run/boole/native-shadow"),
    ];
    const RUNTIME_DIRECTORY_MODE: u32 = 0o2750;
    const LOCK_BASENAME: &str = "launcher.lock";
    const LOCK_STAGE: &str = "/run/boole/native-shadow/launcher.lock";
    const LOCK_MODE: u32 = 0o600;

    #[derive(Debug)]
    #[allow(dead_code)]
    pub(super) struct HeldLauncherLock {
        pub(super) runtime_directory: File,
        pub(super) lock_file: File,
    }

    #[cfg(target_os = "linux")]
    pub(super) fn acquire_fixed_launcher_lock(
        node_gid: u32,
    ) -> Result<HeldLauncherLock, LauncherLifetimeLockError> {
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open("/")
            .map_err(|source| io_error("/", source))?;
        acquire_launcher_lock_beneath(&root, 0, 0, node_gid, RUNTIME_DIRECTORY_MODE)
    }

    fn acquire_launcher_lock_beneath(
        root: &File,
        ancestor_uid: u32,
        ancestor_gid: u32,
        runtime_gid: u32,
        runtime_mode: u32,
    ) -> Result<HeldLauncherLock, LauncherLifetimeLockError> {
        validate_directory(root, "/", ancestor_uid, ancestor_gid, None)?;
        let mut current = root
            .try_clone()
            .map_err(|source| io_error("clone / directory descriptor", source))?;

        for (index, (basename, stage)) in RUNTIME_COMPONENTS.iter().enumerate() {
            let child = open_directory_child(&current, basename, stage)?;
            if index + 1 == RUNTIME_COMPONENTS.len() {
                validate_directory(&child, stage, ancestor_uid, runtime_gid, Some(runtime_mode))?;
            } else {
                validate_directory(&child, stage, ancestor_uid, ancestor_gid, None)?;
            }
            current = child;
        }

        let lock_file = open_lock_file(&current)?;
        validate_lock_metadata(&lock_file, ancestor_uid, runtime_gid)?;
        flock_exclusive_nonblocking(&lock_file)?;
        validate_lock_metadata(&lock_file, ancestor_uid, runtime_gid)?;

        Ok(HeldLauncherLock {
            runtime_directory: current,
            lock_file,
        })
    }

    #[allow(unsafe_code)]
    fn open_directory_child(
        parent: &File,
        basename: &str,
        stage: &'static str,
    ) -> Result<File, LauncherLifetimeLockError> {
        let basename = CString::new(basename).expect("fixed runtime components contain no NUL");
        let flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW;
        // SAFETY: `parent` owns a live directory descriptor, `basename` is a
        // fixed NUL-terminated component, and the returned descriptor is
        // transferred exactly once into `File`.
        let raw_fd = unsafe { libc::openat(parent.as_raw_fd(), basename.as_ptr(), flags) };
        if raw_fd < 0 {
            return Err(io_error(stage, io::Error::last_os_error()));
        }
        // SAFETY: `openat` returned a new owned descriptor.
        Ok(unsafe { File::from_raw_fd(raw_fd) })
    }

    fn validate_directory(
        directory: &File,
        stage: &'static str,
        required_uid: u32,
        required_gid: u32,
        exact_mode: Option<u32>,
    ) -> Result<(), LauncherLifetimeLockError> {
        let metadata = directory
            .metadata()
            .map_err(|source| io_error(stage, source))?;
        let mode = metadata.mode() & 0o7777;
        if !metadata.file_type().is_dir() {
            return Err(unsafe_metadata(stage, "component is not a directory"));
        }
        if metadata.uid() != required_uid || metadata.gid() != required_gid {
            return Err(unsafe_metadata(
                stage,
                "directory owner/group does not match the fixed runtime contract",
            ));
        }
        if mode & 0o022 != 0 {
            return Err(unsafe_metadata(
                stage,
                "directory is writable by group or other",
            ));
        }
        if exact_mode.is_some_and(|required| mode != required) {
            return Err(unsafe_metadata(
                stage,
                "runtime directory mode does not match the fixed contract",
            ));
        }
        Ok(())
    }

    #[allow(unsafe_code)]
    fn open_lock_file(directory: &File) -> Result<File, LauncherLifetimeLockError> {
        let basename = CString::new(LOCK_BASENAME).expect("fixed lock basename contains no NUL");
        let flags = libc::O_CREAT | libc::O_RDWR | libc::O_CLOEXEC | libc::O_NOFOLLOW;
        // SAFETY: `directory` owns a verified directory descriptor,
        // `basename` is fixed and NUL-terminated, and the new descriptor is
        // transferred exactly once into `File`.
        let raw_fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                basename.as_ptr(),
                flags,
                LOCK_MODE as libc::c_uint,
            )
        };
        if raw_fd < 0 {
            return Err(io_error(LOCK_STAGE, io::Error::last_os_error()));
        }
        // SAFETY: `openat` returned a new owned descriptor.
        Ok(unsafe { File::from_raw_fd(raw_fd) })
    }

    fn validate_lock_metadata(
        lock_file: &File,
        required_uid: u32,
        required_gid: u32,
    ) -> Result<(), LauncherLifetimeLockError> {
        let metadata = lock_file
            .metadata()
            .map_err(|source| io_error(LOCK_STAGE, source))?;
        if !metadata.file_type().is_file() {
            return Err(unsafe_metadata(LOCK_STAGE, "lock is not a regular file"));
        }
        if metadata.nlink() != 1 {
            return Err(unsafe_metadata(
                LOCK_STAGE,
                "lock must have exactly one hard link",
            ));
        }
        if metadata.uid() != required_uid || metadata.gid() != required_gid {
            return Err(unsafe_metadata(
                LOCK_STAGE,
                "lock owner/group does not match root:boole-node",
            ));
        }
        if metadata.mode() & 0o7777 != LOCK_MODE {
            return Err(unsafe_metadata(LOCK_STAGE, "lock mode is not exactly 0600"));
        }
        Ok(())
    }

    #[allow(unsafe_code)]
    fn flock_exclusive_nonblocking(lock_file: &File) -> Result<(), LauncherLifetimeLockError> {
        // SAFETY: the descriptor remains owned by `lock_file` for the entire
        // call and `flock` neither consumes nor aliases it.
        let result = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result == 0 {
            return Ok(());
        }
        let source = io::Error::last_os_error();
        if source.raw_os_error() == Some(libc::EWOULDBLOCK) {
            Err(LauncherLifetimeLockError::Busy)
        } else {
            Err(io_error("flock launcher lifetime lock", source))
        }
    }

    fn io_error(stage: &'static str, source: io::Error) -> LauncherLifetimeLockError {
        LauncherLifetimeLockError::Io { stage, source }
    }

    fn unsafe_metadata(stage: &'static str, reason: &'static str) -> LauncherLifetimeLockError {
        LauncherLifetimeLockError::UnsafeMetadata { stage, reason }
    }

    #[cfg(test)]
    mod tests {
        use std::fs::{self, File, OpenOptions};
        use std::os::fd::{AsRawFd, RawFd};
        use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
        #[cfg(target_os = "linux")]
        use std::os::unix::net::UnixListener;
        use std::path::{Path, PathBuf};
        #[cfg(target_os = "linux")]
        use std::process::Command;
        use std::sync::atomic::{AtomicU64, Ordering};

        use super::{
            acquire_launcher_lock_beneath, flock_exclusive_nonblocking, validate_lock_metadata,
            LauncherLifetimeLockError,
        };

        static NEXT_TREE: AtomicU64 = AtomicU64::new(0);
        #[cfg(target_os = "linux")]
        const REAL_CONTENDER_ROLE: &str = "BOOLE_NATIVE_SHADOW_LOCK_CONTENDER_ROLE";
        #[cfg(target_os = "linux")]
        const REAL_CONTENDER_BUSY_MARKER: &str = "native-shadow-lock-contender-observed-busy";
        #[cfg(target_os = "linux")]
        const REAL_PARENT_COMPLETE_MARKER: &str =
            "native-shadow-launcher-instance-identity-gate-complete";

        struct TestRuntimeTree {
            root: PathBuf,
            runtime: PathBuf,
            uid: u32,
            gid: u32,
        }

        impl TestRuntimeTree {
            fn new() -> Self {
                let suffix = NEXT_TREE.fetch_add(1, Ordering::Relaxed);
                let root = PathBuf::from("/tmp")
                    .join(format!("bnsl-lock-{}-{suffix}", std::process::id()));
                let runtime = root.join("run/boole/native-shadow");
                fs::create_dir_all(&runtime).expect("test runtime tree must be creatable");
                set_mode(&root, 0o755);
                set_mode(&root.join("run"), 0o755);
                set_mode(&root.join("run/boole"), 0o755);
                set_mode(&runtime, 0o750);
                let metadata = fs::metadata(&root).expect("test root metadata");
                Self {
                    root,
                    runtime,
                    uid: metadata.uid(),
                    gid: metadata.gid(),
                }
            }

            fn open_root(&self) -> File {
                OpenOptions::new()
                    .read(true)
                    .open(&self.root)
                    .expect("test root must open")
            }

            fn acquire(&self) -> Result<super::HeldLauncherLock, LauncherLifetimeLockError> {
                acquire_launcher_lock_beneath(
                    &self.open_root(),
                    self.uid,
                    self.gid,
                    self.gid,
                    0o750,
                )
            }

            fn lock_path(&self) -> PathBuf {
                self.runtime.join("launcher.lock")
            }
        }

        impl Drop for TestRuntimeTree {
            fn drop(&mut self) {
                let _ = set_mode_if_present(&self.runtime, 0o750);
                let _ = fs::remove_dir_all(&self.root);
            }
        }

        fn set_mode(path: &Path, mode: u32) {
            fs::set_permissions(path, fs::Permissions::from_mode(mode))
                .expect("test mode must be settable");
        }

        fn set_mode_if_present(path: &Path, mode: u32) -> std::io::Result<()> {
            fs::set_permissions(path, fs::Permissions::from_mode(mode))
        }

        #[allow(unsafe_code)]
        fn fd_flags(fd: RawFd) -> i32 {
            // SAFETY: `fd` is borrowed from a live `File`; F_GETFD does not
            // mutate or take ownership of it.
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert!(flags >= 0, "F_GETFD must succeed");
            flags
        }

        #[test]
        fn compile_time_lock_shape_matches_the_tracked_policy() {
            let policy: serde_json::Value = serde_json::from_slice(
                boole_native_shadow_protocol::TRACKED_EXECUTION_POLICY_BYTES,
            )
            .expect("tracked policy JSON");
            let installation = &policy["installation"];
            let socket_creation = &installation["socketCreation"];

            assert_eq!(
                format!(
                    "/{}",
                    super::RUNTIME_COMPONENTS
                        .iter()
                        .map(|(component, _)| *component)
                        .collect::<Vec<_>>()
                        .join("/")
                ),
                installation["socketDirectory"]
            );
            assert_eq!(installation["socketDirectoryOwner"], "root");
            assert_eq!(installation["socketDirectoryGroup"], "boole-node");
            assert_eq!(
                format!("{:04o}", super::RUNTIME_DIRECTORY_MODE),
                installation["socketDirectoryMode"]
            );
            assert_eq!(super::LOCK_STAGE, installation["launcherLockPath"]);
            assert_eq!(installation["launcherLockOwner"], "root");
            assert_eq!(installation["launcherLockGroup"], "boole-node");
            assert_eq!(
                format!("{:04o}", super::LOCK_MODE),
                installation["launcherLockMode"]
            );
            assert_eq!(
                socket_creation["lockOpen"],
                "openat-verified-socket-directory-fd:launcher.lock:O_CREAT|O_RDWR|O_CLOEXEC|O_NOFOLLOW:0600"
            );
            assert_eq!(
                socket_creation["lockValidation"],
                "fstat-regular-nlink1-root:boole-node-mode0600-before-flock"
            );
        }

        #[allow(unsafe_code)]
        fn make_fifo(path: &Path) {
            let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
                .expect("test FIFO path has no NUL");
            // SAFETY: `path` is a live NUL-terminated path and mkfifo retains
            // no pointer after the call.
            let result = unsafe { libc::mkfifo(path.as_ptr(), 0o600) };
            assert_eq!(result, 0, "test FIFO must be creatable");
        }

        #[test]
        fn exact_tree_creates_and_holds_one_cloexec_lock() {
            let tree = TestRuntimeTree::new();
            let guard = tree.acquire().expect("exact runtime tree must lock");
            let metadata = guard.lock_file.metadata().expect("lock metadata");

            assert!(metadata.file_type().is_file());
            assert_eq!(metadata.nlink(), 1);
            assert_eq!((metadata.uid(), metadata.gid()), (tree.uid, tree.gid));
            assert_eq!(metadata.mode() & 0o7777, 0o600);
            assert_ne!(fd_flags(guard.lock_file.as_raw_fd()) & libc::FD_CLOEXEC, 0);
            assert_ne!(
                fd_flags(guard.runtime_directory.as_raw_fd()) & libc::FD_CLOEXEC,
                0
            );
        }

        #[test]
        fn contention_is_busy_and_drop_reacquires_the_same_inode_without_unlink() {
            let tree = TestRuntimeTree::new();
            let first = tree.acquire().expect("first launcher must lock");
            let first_metadata = first.lock_file.metadata().expect("first lock metadata");

            assert!(matches!(
                tree.acquire(),
                Err(LauncherLifetimeLockError::Busy)
            ));
            assert!(tree.lock_path().exists(), "busy must not unlink the lock");

            drop(first);
            assert!(tree.lock_path().exists(), "drop must not unlink the lock");
            let second = tree
                .acquire()
                .expect("lock must be reacquirable after drop");
            let second_metadata = second.lock_file.metadata().expect("second lock metadata");
            assert_eq!(
                (first_metadata.dev(), first_metadata.ino()),
                (second_metadata.dev(), second_metadata.ino())
            );
        }

        #[test]
        fn writable_ancestor_fails_before_creating_a_lock() {
            let tree = TestRuntimeTree::new();
            set_mode(&tree.root.join("run/boole"), 0o775);

            assert!(matches!(
                tree.acquire(),
                Err(LauncherLifetimeLockError::UnsafeMetadata { .. })
            ));
            assert!(!tree.lock_path().exists());
        }

        #[test]
        fn every_symlinked_runtime_component_is_never_followed() {
            for relative in ["run", "run/boole", "run/boole/native-shadow"] {
                let tree = TestRuntimeTree::new();
                let component = tree.root.join(relative);
                let actual = tree
                    .root
                    .join(format!("actual-{}", relative.replace('/', "-")));
                fs::rename(&component, &actual).expect("test ancestor rename");
                symlink(&actual, &component).expect("test ancestor symlink");

                assert!(matches!(
                    tree.acquire(),
                    Err(LauncherLifetimeLockError::Io { .. })
                ));
            }
        }

        #[test]
        fn nondirectory_runtime_component_is_rejected() {
            let tree = TestRuntimeTree::new();
            let boole = tree.root.join("run/boole");
            fs::remove_dir_all(&boole).expect("test component removal");
            fs::write(&boole, b"not a directory").expect("test file component");

            assert!(matches!(
                tree.acquire(),
                Err(LauncherLifetimeLockError::Io { .. })
            ));
        }

        #[test]
        fn final_runtime_directory_requires_exact_owner_group_and_mode() {
            let tree = TestRuntimeTree::new();
            set_mode(&tree.runtime, 0o700);
            assert!(matches!(
                tree.acquire(),
                Err(LauncherLifetimeLockError::UnsafeMetadata { .. })
            ));

            set_mode(&tree.runtime, 0o750);
            assert!(matches!(
                acquire_launcher_lock_beneath(
                    &tree.open_root(),
                    tree.uid,
                    tree.gid,
                    tree.gid.wrapping_add(1),
                    0o750,
                ),
                Err(LauncherLifetimeLockError::UnsafeMetadata { .. })
            ));
        }

        #[test]
        fn lock_symlink_directory_hardlink_and_wrong_mode_are_rejected_without_repair() {
            for kind in ["symlink", "directory", "hardlink", "mode"] {
                let tree = TestRuntimeTree::new();
                let lock = tree.lock_path();
                match kind {
                    "symlink" => symlink("target", &lock).expect("test symlink"),
                    "directory" => fs::create_dir(&lock).expect("test directory"),
                    "hardlink" => {
                        let original = tree.runtime.join("original.lock");
                        fs::write(&original, b"").expect("test original");
                        set_mode(&original, 0o600);
                        fs::hard_link(&original, &lock).expect("test hardlink");
                    }
                    "mode" => {
                        fs::write(&lock, b"").expect("test lock");
                        set_mode(&lock, 0o640);
                    }
                    _ => unreachable!(),
                }

                assert!(tree.acquire().is_err(), "{kind} must fail closed");
                assert!(lock.exists() || fs::symlink_metadata(&lock).is_ok());
            }
        }

        #[test]
        fn fifo_lock_entry_is_rejected_without_removal() {
            let tree = TestRuntimeTree::new();
            let lock = tree.lock_path();
            make_fifo(&lock);

            assert!(tree.acquire().is_err(), "FIFO must fail closed");
            assert!(fs::symlink_metadata(&lock).is_ok(), "FIFO must remain");
        }

        #[cfg(target_os = "linux")]
        #[test]
        fn socket_lock_entry_is_rejected_without_removal() {
            let tree = TestRuntimeTree::new();
            let lock = tree.lock_path();
            let _listener = UnixListener::bind(&lock).expect("test socket");

            assert!(tree.acquire().is_err(), "socket must fail closed");
            assert!(fs::symlink_metadata(&lock).is_ok(), "socket must remain");
        }

        #[test]
        fn unsafe_lock_metadata_is_rejected_before_flock() {
            let tree = TestRuntimeTree::new();
            let lock = tree.lock_path();
            fs::write(&lock, b"").expect("test lock");
            set_mode(&lock, 0o640);
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lock)
                .expect("test lock open");

            assert!(matches!(
                validate_lock_metadata(&file, tree.uid, tree.gid),
                Err(LauncherLifetimeLockError::UnsafeMetadata { .. })
            ));
            flock_exclusive_nonblocking(&file)
                .expect("metadata rejection must happen before any flock is held");
        }

        #[test]
        fn lock_owner_and_group_must_match_exactly() {
            let tree = TestRuntimeTree::new();
            let lock = tree.lock_path();
            fs::write(&lock, b"").expect("test lock");
            set_mode(&lock, 0o600);
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&lock)
                .expect("test lock open");

            assert!(matches!(
                validate_lock_metadata(&file, tree.uid.wrapping_add(1), tree.gid),
                Err(LauncherLifetimeLockError::UnsafeMetadata { .. })
            ));
            assert!(matches!(
                validate_lock_metadata(&file, tree.uid, tree.gid.wrapping_add(1)),
                Err(LauncherLifetimeLockError::UnsafeMetadata { .. })
            ));
        }

        #[cfg(target_os = "linux")]
        fn acquire_production_lock(
        ) -> Result<crate::lifetime_lock::LauncherLifetimeLockGuard, LauncherLifetimeLockError>
        {
            let prerequisites = crate::startup::verify_fixed_launcher_prelock_prerequisites()
                .unwrap_or_else(|error| panic!("launcher pre-lock verification failed: {error}"));
            crate::lifetime_lock::acquire_fixed_launcher_lifetime_lock(prerequisites)
        }

        #[cfg(target_os = "linux")]
        #[test]
        #[ignore = "requires exact root capabilities, installed authority, fixed NSS, and runtime directory"]
        fn real_linux_fixed_launcher_lifetime_lock_is_single_instance() {
            assert!(
                std::env::var_os(REAL_CONTENDER_ROLE).is_none(),
                "aggregate lock test must not run in contender mode"
            );
            let lock_path = Path::new("/run/boole/native-shadow/launcher.lock");
            let first_lock =
                acquire_production_lock().expect("first launcher must acquire the lock");
            let first_metadata = first_lock
                .held
                .lock_file
                .metadata()
                .expect("first production lock metadata");
            let first = crate::instance_id::acquire_fresh_launcher_instance(first_lock)
                .expect("first launcher must obtain one real instance ID");

            let output = Command::new(std::env::current_exe().expect("current test executable"))
                .arg("lifetime_lock::unix::tests::real_linux_launcher_lifetime_lock_contender")
                .args(["--ignored", "--exact", "--nocapture"])
                .env(REAL_CONTENDER_ROLE, "busy")
                .output()
                .expect("independent contender process must start");
            assert!(
                output.status.success(),
                "independent contender failed: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8_lossy(&output.stdout).contains(REAL_CONTENDER_BUSY_MARKER),
                "independent contender did not execute the exact Busy assertion: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                lock_path.exists(),
                "busy contention must not unlink the lock"
            );

            drop(first);
            assert!(lock_path.exists(), "guard drop must not unlink the lock");
            let second = acquire_production_lock().expect("lock must be reacquirable after drop");
            let second_metadata = second
                .held
                .lock_file
                .metadata()
                .expect("second production lock metadata");
            assert_eq!(
                (first_metadata.dev(), first_metadata.ino()),
                (second_metadata.dev(), second_metadata.ino()),
                "reacquisition must use the same persistent lock inode"
            );
            println!("{REAL_PARENT_COMPLETE_MARKER}");
        }

        #[cfg(target_os = "linux")]
        #[test]
        #[ignore = "child role for the exact production lifetime-lock gate"]
        fn real_linux_launcher_lifetime_lock_contender() {
            assert_eq!(
                std::env::var(REAL_CONTENDER_ROLE).as_deref(),
                Ok("busy"),
                "contender helper may run only under the aggregate gate"
            );
            assert!(matches!(
                acquire_production_lock(),
                Err(LauncherLifetimeLockError::Busy)
            ));
            println!("{REAL_CONTENDER_BUSY_MARKER}");
        }
    }
}
