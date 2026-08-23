//! Exact portable runtime-rootfs verification for the named Linux replay.
//!
//! A caller may choose the scratch extraction path, but the path is never
//! authority: the directory must be a read-only mount and every entry must
//! match the frozen content manifest whose digest is compiled into this
//! launcher.  The resulting non-cloneable token retains the verified dirfd.

#[cfg(target_os = "linux")]
mod linux {
    #![allow(unsafe_code)]

    use std::collections::{BTreeMap, BTreeSet};
    use std::ffi::OsStr;
    use std::fs::{self, OpenOptions};
    use std::io::{self, Read};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::{Component, Path, PathBuf};

    use serde::Deserialize;
    use sha2::{Digest, Sha256};
    use thiserror::Error;

    const CONTENT_MANIFEST_SHA256: &str =
        "9858aa4ba3e3055580ae19fe1f46c2cbfc1d97e2773f9b40105942ab37266d83";
    const CONTENT_MANIFEST_SIZE: u64 = 1_275_874;
    const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

    #[derive(Debug, Error)]
    pub enum RuntimeRootfsReplayError {
        #[error("runtime rootfs replay I/O failed at {path}: {source}")]
        Io {
            path: String,
            #[source]
            source: io::Error,
        },
        #[error("runtime rootfs content manifest differs: {0}")]
        Manifest(&'static str),
        #[error("runtime rootfs entry differs at {0}")]
        Entry(String),
        #[error("runtime rootfs must be an already-mounted read-only directory")]
        NotReadOnlyMount,
    }

    #[derive(Debug)]
    #[allow(dead_code)] // Consumed by the active replay startup token below.
    pub struct VerifiedRuntimeRootfsReplay {
        directory: OwnedFd,
        device: u64,
        inode: u64,
        content_manifest_sha256: String,
    }

    #[allow(dead_code)]
    impl VerifiedRuntimeRootfsReplay {
        pub(crate) fn duplicate_directory_fd(&self) -> Result<OwnedFd, RuntimeRootfsReplayError> {
            self.reverify_descriptor()?;
            // SAFETY: this duplicates the retained verified directory fd into
            // one new CLOEXEC descriptor owned by the request permit.
            let fd = unsafe { libc::fcntl(self.directory.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 4) };
            if fd < 0 {
                return Err(io_error(
                    "verified rootfs dirfd",
                    io::Error::last_os_error(),
                ));
            }
            // SAFETY: fcntl returned one fresh descriptor.
            Ok(unsafe { OwnedFd::from_raw_fd(fd) })
        }

        pub(crate) fn content_manifest_sha256(&self) -> &str {
            &self.content_manifest_sha256
        }

        fn reverify_descriptor(&self) -> Result<(), RuntimeRootfsReplayError> {
            let metadata = fd_metadata(self.directory.as_raw_fd())?;
            if metadata.st_dev != self.device
                || metadata.st_ino != self.inode
                || metadata.st_mode & libc::S_IFMT != libc::S_IFDIR
            {
                return Err(RuntimeRootfsReplayError::Entry(
                    "verified rootfs descriptor identity changed".to_string(),
                ));
            }
            require_read_only_mount(self.directory.as_raw_fd())
        }
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct ContentManifest {
        schema: String,
        activation_allowed: bool,
        production_byte_provenance_complete: bool,
        entries: Vec<ContentEntry>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct ContentEntry {
        logical_path: String,
        kind: String,
        mode: String,
        uid: u32,
        gid: u32,
        closures: Vec<String>,
        #[serde(default)]
        size_bytes: Option<u64>,
        #[serde(default)]
        sha256: Option<String>,
        #[serde(default)]
        target: Option<String>,
    }

    /// Verify one exact read-only extraction against the frozen portable
    /// content manifest. Both paths are gate-owned scratch paths; neither can
    /// influence the accepted bytes or tree shape.
    pub fn verify_runtime_rootfs_replay(
        rootfs: &Path,
        content_manifest: &Path,
    ) -> Result<VerifiedRuntimeRootfsReplay, RuntimeRootfsReplayError> {
        let directory = open_directory(rootfs)?;
        require_read_only_mount(directory.as_raw_fd())?;
        let manifest_raw = read_manifest(content_manifest)?;
        if hex::encode(Sha256::digest(&manifest_raw)) != CONTENT_MANIFEST_SHA256 {
            return Err(RuntimeRootfsReplayError::Manifest("SHA-256 mismatch"));
        }
        let manifest: ContentManifest = serde_json::from_slice(&manifest_raw)
            .map_err(|_| RuntimeRootfsReplayError::Manifest("strict schema mismatch"))?;
        if manifest.schema != "boole.native-shadow.rootfs-content-manifest.v1"
            || manifest.activation_allowed
            || manifest.production_byte_provenance_complete
        {
            return Err(RuntimeRootfsReplayError::Manifest(
                "authority boundary mismatch",
            ));
        }
        let expected = validate_manifest_entries(manifest.entries)?;
        let proc_path = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
        let observed = verify_tree(&proc_path, &expected)?;
        if observed != expected.keys().cloned().collect() {
            return Err(RuntimeRootfsReplayError::Manifest(
                "rootfs path set mismatch",
            ));
        }
        let metadata = fd_metadata(directory.as_raw_fd())?;
        Ok(VerifiedRuntimeRootfsReplay {
            directory,
            device: metadata.st_dev,
            inode: metadata.st_ino,
            content_manifest_sha256: CONTENT_MANIFEST_SHA256.to_string(),
        })
    }

    fn open_directory(path: &Path) -> Result<OwnedFd, RuntimeRootfsReplayError> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(path)
            .map_err(|source| io_error(path.display().to_string(), source))?;
        Ok(file.into())
    }

    fn read_manifest(path: &Path) -> Result<Vec<u8>, RuntimeRootfsReplayError> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|source| io_error(path.display().to_string(), source))?;
        if !metadata.file_type().is_file()
            || metadata.uid() != 0
            || metadata.gid() != 0
            || metadata.mode() & 0o7777 != 0o444
            || metadata.nlink() != 1
            || metadata.len() != CONTENT_MANIFEST_SIZE
        {
            return Err(RuntimeRootfsReplayError::Manifest(
                "content-manifest metadata mismatch",
            ));
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(path)
            .map_err(|source| io_error(path.display().to_string(), source))?;
        let mut bytes = Vec::with_capacity(CONTENT_MANIFEST_SIZE as usize);
        (&mut file)
            .take(CONTENT_MANIFEST_SIZE + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| io_error(path.display().to_string(), source))?;
        if bytes.len() as u64 != CONTENT_MANIFEST_SIZE {
            return Err(RuntimeRootfsReplayError::Manifest(
                "content-manifest length changed while reading",
            ));
        }
        Ok(bytes)
    }

    fn validate_manifest_entries(
        entries: Vec<ContentEntry>,
    ) -> Result<BTreeMap<String, ContentEntry>, RuntimeRootfsReplayError> {
        let mut expected = BTreeMap::new();
        let mut prior: Option<Vec<u8>> = None;
        for entry in entries {
            let relative = canonical_relative(&entry.logical_path)?;
            let key = relative.as_bytes().to_vec();
            if prior.as_ref().is_some_and(|value| value >= &key) {
                return Err(RuntimeRootfsReplayError::Manifest(
                    "entry order is not canonical or contains a duplicate",
                ));
            }
            prior = Some(key);
            if entry.uid != 0
                || entry.gid != 0
                || entry.mode.len() != 4
                || !entry.mode.bytes().all(|byte| matches!(byte, b'0'..=b'7'))
                || entry.closures.iter().any(|value| value.is_empty())
            {
                return Err(RuntimeRootfsReplayError::Manifest(
                    "entry common fields differ",
                ));
            }
            match entry.kind.as_str() {
                "directory" => {
                    if entry.size_bytes.is_some()
                        || entry.sha256.is_some()
                        || entry.target.is_some()
                    {
                        return Err(RuntimeRootfsReplayError::Manifest(
                            "directory entry has file/link fields",
                        ));
                    }
                }
                "file" => {
                    let size = entry
                        .size_bytes
                        .ok_or(RuntimeRootfsReplayError::Manifest("file entry lacks size"))?;
                    let digest =
                        entry
                            .sha256
                            .as_deref()
                            .ok_or(RuntimeRootfsReplayError::Manifest(
                                "file entry lacks SHA-256",
                            ))?;
                    if size > MAX_FILE_BYTES || !is_sha256(digest) || entry.target.is_some() {
                        return Err(RuntimeRootfsReplayError::Manifest(
                            "file entry fields differ",
                        ));
                    }
                }
                "symlink" => {
                    if entry.size_bytes.is_some()
                        || entry.sha256.is_some()
                        || entry.target.as_ref().is_none_or(String::is_empty)
                    {
                        return Err(RuntimeRootfsReplayError::Manifest(
                            "symlink entry fields differ",
                        ));
                    }
                }
                _ => {
                    return Err(RuntimeRootfsReplayError::Manifest(
                        "entry kind is outside the frozen vocabulary",
                    ));
                }
            }
            expected.insert(relative, entry);
        }
        Ok(expected)
    }

    fn canonical_relative(logical: &str) -> Result<String, RuntimeRootfsReplayError> {
        let path = Path::new(logical);
        if !path.is_absolute() || logical == "/" || logical.as_bytes().contains(&0) {
            return Err(RuntimeRootfsReplayError::Manifest(
                "logical path is not a canonical absolute child",
            ));
        }
        let mut output = Vec::new();
        for component in path.components() {
            match component {
                Component::RootDir => {}
                Component::Normal(value) if !value.as_bytes().is_empty() => {
                    output.push(value.to_str().ok_or(RuntimeRootfsReplayError::Manifest(
                        "logical path is not UTF-8",
                    ))?)
                }
                _ => {
                    return Err(RuntimeRootfsReplayError::Manifest(
                        "logical path contains a non-canonical component",
                    ));
                }
            }
        }
        Ok(output.join("/"))
    }

    fn verify_tree(
        root: &Path,
        expected: &BTreeMap<String, ContentEntry>,
    ) -> Result<BTreeSet<String>, RuntimeRootfsReplayError> {
        let mut observed = BTreeSet::new();
        let mut pending = vec![(root.to_path_buf(), String::new())];
        while let Some((directory, prefix)) = pending.pop() {
            let entries = fs::read_dir(&directory)
                .map_err(|source| io_error(directory.display().to_string(), source))?;
            for item in entries {
                let item =
                    item.map_err(|source| io_error(directory.display().to_string(), source))?;
                let name = item.file_name();
                let name = name.to_str().ok_or_else(|| {
                    RuntimeRootfsReplayError::Entry("non-UTF-8 rootfs path".to_string())
                })?;
                if name.is_empty() || name == "." || name == ".." || name.contains('/') {
                    return Err(RuntimeRootfsReplayError::Entry(
                        "non-canonical rootfs path".to_string(),
                    ));
                }
                let relative = if prefix.is_empty() {
                    name.to_string()
                } else {
                    format!("{prefix}/{name}")
                };
                let expected_entry = expected.get(&relative).ok_or_else(|| {
                    RuntimeRootfsReplayError::Entry(format!("unexpected path {relative}"))
                })?;
                let path = item.path();
                verify_entry(&path, &relative, expected_entry)?;
                observed.insert(relative.clone());
                if expected_entry.kind == "directory" {
                    pending.push((path, relative));
                }
            }
        }
        Ok(observed)
    }

    fn verify_entry(
        path: &Path,
        relative: &str,
        expected: &ContentEntry,
    ) -> Result<(), RuntimeRootfsReplayError> {
        let metadata = fs::symlink_metadata(path).map_err(|source| io_error(relative, source))?;
        let mode = metadata.mode() & 0o7777;
        let expected_mode = u32::from_str_radix(&expected.mode, 8)
            .map_err(|_| RuntimeRootfsReplayError::Manifest("entry mode is malformed"))?;
        if metadata.uid() != 0 || metadata.gid() != 0 || mode != expected_mode {
            return Err(RuntimeRootfsReplayError::Entry(format!(
                "{relative}: owner or mode"
            )));
        }
        match expected.kind.as_str() {
            "directory" if metadata.file_type().is_dir() => Ok(()),
            "file" if metadata.file_type().is_file() => {
                if metadata.nlink() != 1 || Some(metadata.len()) != expected.size_bytes {
                    return Err(RuntimeRootfsReplayError::Entry(format!(
                        "{relative}: file metadata"
                    )));
                }
                let mut file = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
                    .open(path)
                    .map_err(|source| io_error(relative, source))?;
                let mut digest = Sha256::new();
                let mut buffer = [0_u8; 64 * 1024];
                loop {
                    let read = file
                        .read(&mut buffer)
                        .map_err(|source| io_error(relative, source))?;
                    if read == 0 {
                        break;
                    }
                    digest.update(&buffer[..read]);
                }
                if Some(hex::encode(digest.finalize())).as_ref() != expected.sha256.as_ref() {
                    return Err(RuntimeRootfsReplayError::Entry(format!(
                        "{relative}: file digest"
                    )));
                }
                Ok(())
            }
            "symlink" if metadata.file_type().is_symlink() => {
                let target = fs::read_link(path).map_err(|source| io_error(relative, source))?;
                if target.as_os_str().as_bytes()
                    != OsStr::new(expected.target.as_deref().unwrap_or_default()).as_bytes()
                {
                    return Err(RuntimeRootfsReplayError::Entry(format!(
                        "{relative}: symlink target"
                    )));
                }
                Ok(())
            }
            _ => Err(RuntimeRootfsReplayError::Entry(format!(
                "{relative}: file kind"
            ))),
        }
    }

    fn require_read_only_mount(fd: i32) -> Result<(), RuntimeRootfsReplayError> {
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: fd is a live directory descriptor and stat is writable.
        if unsafe { libc::fstatvfs(fd, &mut stat) } != 0 {
            return Err(io_error(
                "verified rootfs statvfs",
                io::Error::last_os_error(),
            ));
        }
        if stat.f_flag & libc::ST_RDONLY == 0 {
            return Err(RuntimeRootfsReplayError::NotReadOnlyMount);
        }
        Ok(())
    }

    fn fd_metadata(fd: i32) -> Result<libc::stat, RuntimeRootfsReplayError> {
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: fd is live and stat is writable.
        if unsafe { libc::fstat(fd, &mut stat) } != 0 {
            return Err(io_error(
                "verified rootfs fstat",
                io::Error::last_os_error(),
            ));
        }
        Ok(stat)
    }

    fn is_sha256(value: &str) -> bool {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }

    fn io_error(path: impl Into<String>, source: io::Error) -> RuntimeRootfsReplayError {
        RuntimeRootfsReplayError::Io {
            path: path.into(),
            source,
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::{
    verify_runtime_rootfs_replay, RuntimeRootfsReplayError, VerifiedRuntimeRootfsReplay,
};

#[cfg(not(target_os = "linux"))]
mod unsupported {
    use std::path::Path;

    use thiserror::Error;

    #[derive(Debug, Error)]
    #[error("runtime rootfs replay verification requires Linux")]
    pub struct RuntimeRootfsReplayError;

    #[derive(Debug)]
    pub struct VerifiedRuntimeRootfsReplay {
        _private: (),
    }

    pub fn verify_runtime_rootfs_replay(
        _rootfs: &Path,
        _content_manifest: &Path,
    ) -> Result<VerifiedRuntimeRootfsReplay, RuntimeRootfsReplayError> {
        Err(RuntimeRootfsReplayError)
    }
}

#[cfg(not(target_os = "linux"))]
pub use unsupported::{
    verify_runtime_rootfs_replay, RuntimeRootfsReplayError, VerifiedRuntimeRootfsReplay,
};
