use thiserror::Error;

use super::{QualificationServerError, VerifiedQualificationStartup};

#[cfg(target_os = "linux")]
pub(crate) const FIXED_SOCKET_PATH: &str = "/run/boole/native-shadow/launcher.sock";

#[derive(Debug, Error)]
pub enum FixedQualificationListenerError {
    #[error("fixed qualification listener requires Linux")]
    UnsupportedPlatform,
    #[error("fixed launcher umask must be 0117, observed {actual}")]
    UnsafeUmask { actual: String },
    #[error("unsafe launcher socket entry at {stage}: {reason}")]
    UnsafeSocketEntry {
        stage: &'static str,
        reason: &'static str,
    },
    #[error("launcher socket entry changed before exact cleanup")]
    SocketEntryChanged,
    #[error("launcher socket I/O failed during {stage}: {source}")]
    Io {
        stage: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Qualification(#[from] QualificationServerError),
}

/// Bind the one fixed launcher socket, accept exactly one node connection and
/// serve the disabled qualification exchange.
///
/// The complete startup token is consumed so a safe caller cannot reuse one
/// verified startup for a second listener.
///
/// ```compile_fail
/// use boole_native_shadow_launcher::qualification::serve_connected_unix_qualification;
/// ```
pub fn serve_one_fixed_unix_qualification(
    startup: VerifiedQualificationStartup,
) -> Result<(), FixedQualificationListenerError> {
    #[cfg(target_os = "linux")]
    {
        require_fixed_umask()?;
        let mut listener = bind_listener_in_directory(
            startup.runtime_directory(),
            std::path::Path::new(FIXED_SOCKET_PATH),
            0,
            startup.node_gid(),
        )?;
        let stream = listener.accept_one_and_remove()?;
        super::unix::serve_connected_unix_qualification(stream, &startup)?;
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        drop(startup);
        Err(FixedQualificationListenerError::UnsupportedPlatform)
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn require_fixed_umask() -> Result<(), FixedQualificationListenerError> {
    let status = std::fs::read_to_string("/proc/thread-self/status").map_err(|source| {
        FixedQualificationListenerError::Io {
            stage: "read launcher thread status for Umask",
            source,
        }
    })?;
    let mut observed = status
        .lines()
        .filter_map(|line| line.strip_prefix("Umask:"))
        .map(str::trim);
    let actual = observed.next().unwrap_or("missing");
    if actual != "0117" || observed.next().is_some() {
        return Err(FixedQualificationListenerError::UnsafeUmask {
            actual: actual.to_string(),
        });
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SocketEntryIdentity {
    device: libc::dev_t,
    inode: libc::ino_t,
}

#[cfg(target_os = "linux")]
pub(crate) struct BoundQualificationListener<'a> {
    listener: std::os::unix::net::UnixListener,
    directory: &'a std::fs::File,
    path: &'a std::path::Path,
    basename: std::ffi::CString,
    identity: SocketEntryIdentity,
    expected_uid: u32,
    expected_gid: u32,
    removed: bool,
}

#[cfg(target_os = "linux")]
impl BoundQualificationListener<'_> {
    pub(crate) fn accept_one(
        &mut self,
    ) -> Result<std::os::unix::net::UnixStream, FixedQualificationListenerError> {
        let (stream, _) =
            self.listener
                .accept()
                .map_err(|source| FixedQualificationListenerError::Io {
                    stage: "accept fixed launcher connection",
                    source,
                })?;
        require_cloexec(
            std::os::fd::AsRawFd::as_raw_fd(&stream),
            "verify accepted launcher stream CLOEXEC",
        )?;
        Ok(stream)
    }

    fn accept_one_and_remove(
        &mut self,
    ) -> Result<std::os::unix::net::UnixStream, FixedQualificationListenerError> {
        let accepted = self.accept_one();
        let cleanup = self.remove_exact_bound_entry();
        cleanup?;
        accepted
    }

    pub(crate) fn remove_exact_bound_entry(
        &mut self,
    ) -> Result<(), FixedQualificationListenerError> {
        verify_parent_identity(self.directory, self.path)?;
        let metadata = socket_metadata_at(self.directory, &self.basename)?
            .ok_or(FixedQualificationListenerError::SocketEntryChanged)?;
        if !socket_metadata_is_owned_socket(&metadata, self.expected_uid, self.expected_gid)
            || socket_identity(&metadata) != self.identity
        {
            return Err(FixedQualificationListenerError::SocketEntryChanged);
        }
        unlink_entry(self.directory, &self.basename, "remove exact bound socket")?;
        if socket_metadata_at(self.directory, &self.basename)?.is_some() {
            return Err(FixedQualificationListenerError::SocketEntryChanged);
        }
        self.removed = true;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for BoundQualificationListener<'_> {
    fn drop(&mut self) {
        if !self.removed {
            let _ = self.remove_exact_bound_entry();
        }
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn bind_listener_in_directory<'a>(
    directory: &'a std::fs::File,
    path: &'a std::path::Path,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<BoundQualificationListener<'a>, FixedQualificationListenerError> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;

    verify_parent_identity(directory, path)?;
    let basename = std::ffi::CString::new(
        path.file_name()
            .ok_or(FixedQualificationListenerError::UnsafeSocketEntry {
                stage: "resolve fixed socket basename",
                reason: "socket path has no basename",
            })?
            .as_bytes(),
    )
    .map_err(|_| FixedQualificationListenerError::UnsafeSocketEntry {
        stage: "resolve fixed socket basename",
        reason: "socket basename contains NUL",
    })?;

    if let Some(metadata) = socket_metadata_at(directory, &basename)? {
        if !socket_metadata_is_exact(&metadata, expected_uid, expected_gid) {
            return Err(FixedQualificationListenerError::UnsafeSocketEntry {
                stage: "inspect stale launcher socket",
                reason: "entry is not a one-link socket with exact owner, group and mode",
            });
        }
        unlink_entry(directory, &basename, "remove exact stale socket")?;
    }
    if socket_metadata_at(directory, &basename)?.is_some() {
        return Err(FixedQualificationListenerError::SocketEntryChanged);
    }

    let listener = std::os::unix::net::UnixListener::bind(path).map_err(|source| {
        FixedQualificationListenerError::Io {
            stage: "bind fixed qualification socket",
            source,
        }
    })?;
    verify_parent_identity(directory, path)?;
    let metadata = socket_metadata_at(directory, &basename)?
        .ok_or(FixedQualificationListenerError::SocketEntryChanged)?;
    if !socket_metadata_is_owned_socket(&metadata, expected_uid, expected_gid) {
        return Err(FixedQualificationListenerError::UnsafeSocketEntry {
            stage: "verify newly bound launcher socket",
            reason: "new socket does not match exact owner, group and link contract",
        });
    }
    let identity = socket_identity(&metadata);
    let bound = BoundQualificationListener {
        listener,
        directory,
        path,
        basename,
        identity,
        expected_uid,
        expected_gid,
        removed: false,
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660)).map_err(|source| {
        FixedQualificationListenerError::Io {
            stage: "chmod fixed qualification socket",
            source,
        }
    })?;
    let metadata = socket_metadata_at(directory, &bound.basename)?
        .ok_or(FixedQualificationListenerError::SocketEntryChanged)?;
    if !socket_metadata_is_exact(&metadata, expected_uid, expected_gid)
        || socket_identity(&metadata) != identity
    {
        return Err(FixedQualificationListenerError::UnsafeSocketEntry {
            stage: "verify chmodded launcher socket",
            reason: "new socket does not match exact owner, group, mode and inode",
        });
    }
    require_cloexec(
        std::os::fd::AsRawFd::as_raw_fd(&bound.listener),
        "verify qualification listener CLOEXEC",
    )?;
    Ok(bound)
}

#[cfg(target_os = "linux")]
fn verify_parent_identity(
    directory: &std::fs::File,
    path: &std::path::Path,
) -> Result<(), FixedQualificationListenerError> {
    use std::os::unix::fs::MetadataExt;

    let expected = directory
        .metadata()
        .map_err(|source| FixedQualificationListenerError::Io {
            stage: "fstat verified runtime directory",
            source,
        })?;
    let parent = path
        .parent()
        .ok_or(FixedQualificationListenerError::UnsafeSocketEntry {
            stage: "resolve fixed socket parent",
            reason: "socket path has no parent",
        })?;
    let observed = std::fs::symlink_metadata(parent).map_err(|source| {
        FixedQualificationListenerError::Io {
            stage: "lstat fixed socket parent",
            source,
        }
    })?;
    if !observed.file_type().is_dir()
        || (expected.dev(), expected.ino()) != (observed.dev(), observed.ino())
    {
        return Err(FixedQualificationListenerError::UnsafeSocketEntry {
            stage: "verify fixed socket parent identity",
            reason: "literal parent does not name the retained runtime directory",
        });
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn socket_metadata_at(
    directory: &std::fs::File,
    basename: &std::ffi::CStr,
) -> Result<Option<libc::stat>, FixedQualificationListenerError> {
    use std::mem::MaybeUninit;
    use std::os::fd::AsRawFd;

    let mut metadata = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `directory` owns a live directory FD, `basename` is
    // NUL-terminated, and `metadata` is writable for one complete stat.
    let result = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            basename.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == 0 {
        // SAFETY: successful fstatat initialized the complete structure.
        return Ok(Some(unsafe { metadata.assume_init() }));
    }
    let source = std::io::Error::last_os_error();
    if source.raw_os_error() == Some(libc::ENOENT) {
        Ok(None)
    } else {
        Err(FixedQualificationListenerError::Io {
            stage: "inspect launcher socket entry relative to runtime directory",
            source,
        })
    }
}

#[cfg(target_os = "linux")]
fn socket_metadata_is_exact(metadata: &libc::stat, uid: u32, gid: u32) -> bool {
    let mode = metadata.st_mode & 0o7777;
    socket_metadata_is_owned_socket(metadata, uid, gid) && mode == 0o660
}

#[cfg(target_os = "linux")]
fn socket_metadata_is_owned_socket(metadata: &libc::stat, uid: u32, gid: u32) -> bool {
    let kind = metadata.st_mode & libc::S_IFMT;
    kind == libc::S_IFSOCK
        && metadata.st_uid == uid
        && metadata.st_gid == gid
        && metadata.st_nlink == 1
}

#[cfg(target_os = "linux")]
fn socket_identity(metadata: &libc::stat) -> SocketEntryIdentity {
    SocketEntryIdentity {
        device: metadata.st_dev,
        inode: metadata.st_ino,
    }
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn unlink_entry(
    directory: &std::fs::File,
    basename: &std::ffi::CStr,
    stage: &'static str,
) -> Result<(), FixedQualificationListenerError> {
    use std::os::fd::AsRawFd;

    // SAFETY: `directory` is a live directory FD and `basename` is a fixed
    // NUL-terminated single component. No pointer is retained.
    if unsafe { libc::unlinkat(directory.as_raw_fd(), basename.as_ptr(), 0) } != 0 {
        return Err(FixedQualificationListenerError::Io {
            stage,
            source: std::io::Error::last_os_error(),
        });
    }
    Ok(())
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn require_cloexec(
    descriptor: libc::c_int,
    stage: &'static str,
) -> Result<(), FixedQualificationListenerError> {
    // SAFETY: F_GETFD has no pointer argument and `descriptor` is live.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
    if flags < 0 {
        return Err(FixedQualificationListenerError::Io {
            stage,
            source: std::io::Error::last_os_error(),
        });
    }
    if flags & libc::FD_CLOEXEC == 0 {
        return Err(FixedQualificationListenerError::UnsafeSocketEntry {
            stage,
            reason: "socket descriptor is missing FD_CLOEXEC",
        });
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::fs::{self, File, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::{symlink, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{bind_listener_in_directory, FixedQualificationListenerError};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestSocketDirectory {
        path: PathBuf,
        directory: File,
        uid: u32,
        gid: u32,
    }

    impl TestSocketDirectory {
        fn new() -> Self {
            let suffix = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("bnsl-{}-{suffix}", std::process::id()));
            fs::create_dir(&path).expect("create test socket directory");
            let directory = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW)
                .open(&path)
                .expect("open test socket directory");
            let metadata = directory.metadata().expect("test directory metadata");
            Self {
                path,
                directory,
                uid: metadata.uid(),
                gid: metadata.gid(),
            }
        }

        fn socket_path(&self) -> PathBuf {
            self.path.join("launcher.sock")
        }
    }

    impl Drop for TestSocketDirectory {
        fn drop(&mut self) {
            let socket = self.socket_path();
            if let Ok(metadata) = fs::symlink_metadata(&socket) {
                if metadata.file_type().is_dir() {
                    let _ = fs::remove_dir(&socket);
                } else {
                    let _ = fs::remove_file(&socket);
                }
            }
            let _ = fs::remove_dir(&self.path);
        }
    }

    fn set_mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("set test entry mode");
    }

    #[allow(unsafe_code)]
    fn make_fifo(path: &Path) {
        use std::os::unix::ffi::OsStrExt;

        let path =
            std::ffi::CString::new(path.as_os_str().as_bytes()).expect("test FIFO path has no NUL");
        // SAFETY: `path` is a live NUL-terminated path and mkfifo retains no
        // pointer after the call.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o660) }, 0);
    }

    #[test]
    fn exact_stale_socket_is_replaced_and_new_socket_is_exact() {
        let tree = TestSocketDirectory::new();
        let path = tree.socket_path();
        let stale = UnixListener::bind(&path).expect("bind stale socket");
        set_mode(&path, 0o660);
        drop(stale);

        let mut bound = bind_listener_in_directory(&tree.directory, &path, tree.uid, tree.gid)
            .expect("exact stale socket may be replaced");
        let metadata = fs::symlink_metadata(&path).expect("new socket metadata");
        assert!(metadata.file_type().is_socket());
        assert_eq!((metadata.uid(), metadata.gid()), (tree.uid, tree.gid));
        assert_eq!(metadata.mode() & 0o7777, 0o660);
        bound.remove_exact_bound_entry().expect("remove new socket");
        assert!(!path.exists());
    }

    #[test]
    fn unsafe_stale_entries_are_preserved_without_repair() {
        enum Fixture {
            File,
            Directory,
            Symlink,
            Fifo,
            WrongModeSocket,
            WrongOwnerExpectation,
            WrongGroupExpectation,
        }

        for fixture in [
            Fixture::File,
            Fixture::Directory,
            Fixture::Symlink,
            Fixture::Fifo,
            Fixture::WrongModeSocket,
            Fixture::WrongOwnerExpectation,
            Fixture::WrongGroupExpectation,
        ] {
            let tree = TestSocketDirectory::new();
            let path = tree.socket_path();
            let mut expected_uid = tree.uid;
            let mut expected_gid = tree.gid;
            match fixture {
                Fixture::File => {
                    let mut file = File::create(&path).expect("create file fixture");
                    file.write_all(b"preserve").expect("write file fixture");
                }
                Fixture::Directory => fs::create_dir(&path).expect("create directory fixture"),
                Fixture::Symlink => {
                    symlink("missing-target", &path).expect("create symlink fixture")
                }
                Fixture::Fifo => make_fifo(&path),
                Fixture::WrongModeSocket => {
                    let _socket = UnixListener::bind(&path).expect("bind wrong-mode socket");
                    set_mode(&path, 0o600);
                }
                Fixture::WrongOwnerExpectation => {
                    let _socket = UnixListener::bind(&path).expect("bind owner socket");
                    set_mode(&path, 0o660);
                    expected_uid = tree.uid.wrapping_add(1);
                }
                Fixture::WrongGroupExpectation => {
                    let _socket = UnixListener::bind(&path).expect("bind group socket");
                    set_mode(&path, 0o660);
                    expected_gid = tree.gid.wrapping_add(1);
                }
            }

            assert!(matches!(
                bind_listener_in_directory(&tree.directory, &path, expected_uid, expected_gid,),
                Err(FixedQualificationListenerError::UnsafeSocketEntry { .. })
            ));
            assert!(
                fs::symlink_metadata(&path).is_ok(),
                "unsafe entry must remain for operator review"
            );
        }
    }

    #[test]
    fn cleanup_never_unlinks_a_replacement_inode() {
        let tree = TestSocketDirectory::new();
        let path = tree.socket_path();
        let mut bound = bind_listener_in_directory(&tree.directory, &path, tree.uid, tree.gid)
            .expect("bind test socket");
        fs::remove_file(&path).expect("remove bound socket name");
        fs::write(&path, b"replacement").expect("create replacement entry");

        assert!(matches!(
            bound.remove_exact_bound_entry(),
            Err(FixedQualificationListenerError::SocketEntryChanged)
        ));
        assert_eq!(
            fs::read(&path).expect("replacement remains"),
            b"replacement"
        );
    }

    #[test]
    fn one_accept_unlinks_the_listener_before_serving() {
        let tree = TestSocketDirectory::new();
        let path = tree.socket_path();
        let mut bound = bind_listener_in_directory(&tree.directory, &path, tree.uid, tree.gid)
            .expect("bind test socket");
        let client_path = path.clone();
        let client = std::thread::spawn(move || {
            UnixStream::connect(client_path).expect("first client connects")
        });

        let _accepted = bound.accept_one_and_remove().expect("accept first client");
        client.join().expect("client thread joins");
        assert!(fs::symlink_metadata(&path).is_err());
        assert!(
            UnixStream::connect(&path).is_err(),
            "second connect must fail"
        );
    }

    #[test]
    fn bounded_active_listener_keeps_one_exact_socket_for_three_accepts_then_removes_it() {
        let tree = TestSocketDirectory::new();
        let path = tree.socket_path();
        let mut bound = bind_listener_in_directory(&tree.directory, &path, tree.uid, tree.gid)
            .expect("bind active execution socket");

        for _ in 0..3 {
            let client_path = path.clone();
            let client = std::thread::spawn(move || {
                UnixStream::connect(client_path).expect("bounded client connects")
            });
            let _accepted = bound.accept_one().expect("accept bounded client");
            client.join().expect("client thread joins");
            assert!(
                fs::symlink_metadata(&path).is_ok(),
                "socket must remain through the bounded service"
            );
        }

        bound
            .remove_exact_bound_entry()
            .expect("remove exact active socket after the bound");
        assert!(fs::symlink_metadata(&path).is_err());
    }
}
