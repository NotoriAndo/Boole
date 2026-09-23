//! Local hot transport keys, not owner-wallet or work-session authority.
//! The containing directory must already exist and must not be group/world writable.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use boole_p2p::{PeerId, TlsIdentity};
use zeroize::Zeroizing;

const MAX_KEY_BYTES: u64 = 4096;

fn parent(path: &Path) -> anyhow::Result<(&Path, File)> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(parent)?;
    check_parent(parent, &directory)?;
    Ok((parent, directory))
}

fn check_parent(path: &Path, held: &File) -> anyhow::Result<()> {
    let metadata = held.metadata()?;
    let current = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        current.is_dir() && metadata.mode() & 0o022 == 0 && same_file(&metadata, &current),
        "peer key parent must be a stable, non-symlink directory without group/world write access"
    );
    Ok(())
}

fn same_file(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

fn check_key(path: &Path, held: &File) -> anyhow::Result<Metadata> {
    let metadata = held.metadata()?;
    let current = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file()
            && current.is_file()
            && metadata.nlink() == 1
            && metadata.mode() & 0o7777 == 0o600
            && metadata.len() <= MAX_KEY_BYTES
            && same_file(&metadata, &current),
        "peer key must remain one regular, single-link 0600 file of at most 4096 bytes"
    );
    Ok(metadata)
}

/// Persist new random PKCS#8 bytes and return only their public transport identity.
/// Failures may leave a partial new file; existing files are never replaced or deleted.
pub fn create_native_peer_key(path: &Path) -> anyhow::Result<PeerId> {
    let (parent_path, directory) = parent(path)?;
    let bytes = TlsIdentity::generate_pkcs8()?;
    let id = TlsIdentity::from_pkcs8(&bytes)?.peer_id();
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    check_key(path, &file)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    check_key(path, &file)?;
    check_parent(parent_path, &directory)?;
    directory.sync_all()?;
    Ok(id)
}

/// Load once from the same bounded, no-follow file descriptor. No secret output.
pub fn load_native_peer_key(path: &Path) -> anyhow::Result<TlsIdentity> {
    let (parent_path, directory) = parent(path)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let before = check_key(path, &file)?;
    let mut bytes = Zeroizing::new(Vec::new());
    (&file).take(MAX_KEY_BYTES + 1).read_to_end(&mut bytes)?;
    let identity = TlsIdentity::from_pkcs8(&bytes)?;
    let after = check_key(path, &file)?;
    anyhow::ensure!(
        before.len() == bytes.len() as u64
            && before.len() == after.len()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec(),
        "peer key changed while loading"
    );
    check_parent(parent_path, &directory)?;
    Ok(identity)
}
