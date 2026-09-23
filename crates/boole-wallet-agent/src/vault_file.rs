//! Bounded vault-file access. The operator controls the containing directory;
//! these checks are not a sandbox against root or another process of the same user.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use anyhow::{ensure, Result};
use boole_core::vault::MAX_VAULT_JSON_BYTES;
use zeroize::Zeroizing;

pub struct Parent {
    path: PathBuf,
    held: File,
}

impl Parent {
    pub fn open(path: &Path, create: bool) -> Result<Self> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if create {
            create_private_directory(parent)?;
        }
        let held = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(parent)?;
        let directory = Self {
            path: parent.to_path_buf(),
            held,
        };
        directory.check()?;
        Ok(directory)
    }

    pub fn check(&self) -> Result<()> {
        let held = self.held.metadata()?;
        let current = fs::symlink_metadata(&self.path)?;
        ensure!(
            current.is_dir() && held.mode() & 0o022 == 0 && same_file(&held, &current),
            "vault parent must be a stable, non-symlink directory without group/world write access"
        );
        Ok(())
    }

    pub fn sync(&self) -> Result<()> {
        self.check()?;
        self.held.sync_all()?;
        Ok(())
    }
}

fn create_private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir() && metadata.mode() & 0o022 == 0,
                "vault parent must be a non-symlink directory without group/world write access"
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let ancestor = Parent::open(path, true)?;
            match fs::DirBuilder::new().mode(0o700).create(path) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(error) => return Err(error.into()),
            }
            ancestor.sync()?;
            let metadata = fs::symlink_metadata(path)?;
            ensure!(
                metadata.is_dir() && metadata.mode() & 0o022 == 0,
                "vault parent changed during creation"
            );
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn same_file(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

fn check_file(path: &Path, file: &File) -> Result<Metadata> {
    let held = file.metadata()?;
    let current = fs::symlink_metadata(path)?;
    ensure!(
        held.is_file()
            && current.is_file()
            && held.nlink() == 1
            && held.mode() & 0o7777 == 0o600
            && held.len() <= MAX_VAULT_JSON_BYTES as u64
            && same_file(&held, &current),
        "vault must be one regular, single-link 0600 file of at most 65536 bytes"
    );
    Ok(held)
}

pub fn read(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    let directory = Parent::open(path, false)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let before = check_file(path, &file)?;
    let mut bytes = Zeroizing::new(Vec::new());
    (&file)
        .take(MAX_VAULT_JSON_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let after = check_file(path, &file)?;
    ensure!(
        before.len() == bytes.len() as u64
            && before.len() == after.len()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec(),
        "vault changed while reading"
    );
    directory.check()?;
    Ok(bytes)
}
