//! P1.1 — state-directory advisory lock + manifest.
//!
//! L7 contract: a single boole-node owns its state directory while the
//! process runs. Two processes pointing at the same directory must not
//! interleave appends to the durable ledgers; the first one to call
//! [`acquire`] holds an exclusive `flock` on `<dir>/state.lock` and the
//! second call returns [`StateDirError::Locked`] without touching any
//! ledger.
//!
//! `<dir>/state.manifest.json` records `created_at`, `network_id`,
//! `binary_sha`, `genesis_hash` (N5.2 — the `GenesisSpec.hash()` identity),
//! `schema_versions`. The first boot writes it; later boots verify the
//! durable values match what the binary expects, so an empty directory and
//! a deleted directory are distinguishable and a directory built for a
//! different network or a foreign genesis is rejected before any write.
//!
//! This slice ships the foundation only — the API and its unit tests.
//! Wiring into `LocalNodeState::from_config` and the `--state-dir` CLI
//! flag is the next P1.1 slice; that follow-up adds the cross-process
//! integration test that two `boole-node` instances cannot share a
//! directory.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// File name of the advisory `flock` target written under the operator's
/// state directory. Exposed so the `/ready` predicate can confirm the
/// lock file is still present at the expected path on every request.
pub const STATE_LOCK_FILE: &str = "state.lock";
const STATE_MANIFEST_FILE: &str = "state.manifest.json";
static MANIFEST_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Compatibility generation of the node's current on-disk reader/writer set,
/// not a consensus version or an individual JSON `schemaVersion`. Generation
/// 1 covers the existing PersistedBlock/ledger serde formats and their existing
/// strict replay checks. A future incompatible reader/writer change must bump
/// this value and implement an explicit migration rather than infer it from a
/// package release number.
fn current_schema_versions() -> BTreeMap<String, u32> {
    BTreeMap::from([("node_storage".to_string(), 1)])
}

/// Failures from the state-directory contract. Distinct variants so the
/// caller (and the typed-error envelope a later slice will wrap them in)
/// can map each one to a stable operator-visible code.
#[derive(Debug, thiserror::Error)]
pub enum StateDirError {
    #[error("state directory is already locked by another process: {0}")]
    Locked(PathBuf),
    #[error("ledger is already locked by another process: {0}")]
    LedgerLocked(PathBuf),
    #[error(
        "state.manifest.json mismatch in {dir}: field `{field}` expected `{expected}`, found `{found}`"
    )]
    ManifestMismatch {
        dir: PathBuf,
        field: String,
        expected: String,
        found: String,
    },
    #[error("state.manifest.json at {0} is malformed: {1}")]
    ManifestMalformed(PathBuf, String),
    #[error("io error on state directory {0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),
}

/// Persisted manifest. `schema_versions` is a string→u32 map so a future
/// store can graduate its on-disk schema without a manifest format change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateManifest {
    pub created_at: String,
    pub network_id: String,
    pub binary_sha: String,
    /// N5.2 — the `GenesisSpec.hash()` this state dir was written under.
    /// Boot refuses a dir recorded under a foreign genesis. Empty on
    /// pre-N5.2 manifests (`serde(default)`); `ensure_manifest` backfills
    /// those once instead of refusing them.
    #[serde(default)]
    pub genesis_hash: String,
    pub schema_versions: BTreeMap<String, u32>,
}

impl StateManifest {
    /// Build a manifest stamped with the current Unix-second timestamp.
    /// Tests that need a deterministic `created_at` build the struct
    /// directly instead.
    pub fn now(network_id: &str, binary_sha: &str, genesis_hash: &str) -> Self {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            created_at: secs.to_string(),
            network_id: network_id.to_string(),
            binary_sha: binary_sha.to_string(),
            genesis_hash: genesis_hash.to_string(),
            schema_versions: current_schema_versions(),
        }
    }
}

/// RAII guard. Drop releases the `flock` (the kernel releases it on file
/// close even without an explicit `LOCK_UN`, so simply dropping the
/// `File` is enough). The guard also keeps the resolved directory path
/// for error reporting.
pub struct StateDirGuard {
    dir: PathBuf,
    _lock: File,
}

impl StateDirGuard {
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A pathname may be unlinked and recreated while our old inode remains
    /// locked. Presence alone is not proof that this process owns the path.
    pub fn is_current(&self) -> bool {
        locked_file_is_current(&self._lock, &self.dir.join(STATE_LOCK_FILE))
    }
}

impl std::fmt::Debug for StateDirGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateDirGuard")
            .field("dir", &self.dir)
            .finish()
    }
}

/// Acquire an exclusive non-blocking advisory lock on `<dir>/state.lock`.
/// Creates the directory and the lock file if they do not exist. The
/// returned guard must be held for the lifetime of the boole-node
/// process; dropping it releases the lock.
pub fn acquire(dir: &Path) -> Result<StateDirGuard, StateDirError> {
    create_private_parent(dir).map_err(|err| StateDirError::Io(dir.to_path_buf(), err))?;
    let lock_path = dir.join(STATE_LOCK_FILE);
    let file =
        open_lock_file(&lock_path).map_err(|err| StateDirError::Io(lock_path.clone(), err))?;
    flock_exclusive_nonblocking(&file).map_err(|err| {
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
            StateDirError::Locked(dir.to_path_buf())
        } else {
            StateDirError::Io(lock_path.clone(), err)
        }
    })?;
    if !locked_file_is_current(&file, &lock_path) {
        return Err(StateDirError::Io(
            lock_path,
            std::io::Error::other("state lock pathname changed during acquisition"),
        ));
    }
    Ok(StateDirGuard {
        dir: dir.to_path_buf(),
        _lock: file,
    })
}

/// Lifetime single-writer ownership of ledger pathnames, independent of an
/// optional state directory. The lock lives beside each ledger, not on the
/// ledger inode: atomic reorg/recovery replacements must not release ownership.
#[derive(Debug)]
pub struct LedgerLockSet {
    locks: Vec<(PathBuf, File)>,
}

impl LedgerLockSet {
    pub fn acquire(paths: impl IntoIterator<Item = PathBuf>) -> Result<Self, StateDirError> {
        let paths: BTreeSet<_> = paths
            .into_iter()
            .map(|path| Self::canonical_path(&path))
            .collect::<Result<_, _>>()?;
        let mut locks = Vec::with_capacity(paths.len());
        for path in paths {
            let mut name = std::ffi::OsString::from(".");
            name.push(path.file_name().expect("canonical ledger has filename"));
            name.push(".boole-lock");
            let lock_path = path.with_file_name(name);
            let file = open_lock_file(&lock_path)
                .map_err(|err| StateDirError::Io(lock_path.clone(), err))?;
            flock_exclusive_nonblocking(&file).map_err(|err| {
                if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
                    StateDirError::LedgerLocked(path.clone())
                } else {
                    StateDirError::Io(lock_path.clone(), err)
                }
            })?;
            if !locked_file_is_current(&file, &lock_path) {
                return Err(StateDirError::Io(
                    lock_path,
                    std::io::Error::other("ledger lock pathname changed during acquisition"),
                ));
            }
            locks.push((lock_path, file));
        }
        Ok(Self { locks })
    }

    /// Consumers retain this canonical pathname for subsequent reads/writes,
    /// so a parent-directory symlink cannot redirect the opened ledger later.
    pub fn canonical_path(path: &Path) -> Result<PathBuf, StateDirError> {
        let filename = path.file_name().ok_or_else(|| {
            StateDirError::Io(
                path.to_path_buf(),
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "ledger path has no filename",
                ),
            )
        })?;
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        create_private_parent(parent)
            .map_err(|err| StateDirError::Io(parent.to_path_buf(), err))?;
        let canonical = parent
            .canonicalize()
            .map_err(|err| StateDirError::Io(parent.to_path_buf(), err))?
            .join(filename);
        match std::fs::symlink_metadata(&canonical) {
            Ok(metadata) if !regular_single_link(&metadata) => {
                return Err(StateDirError::Io(
                    canonical,
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "ledger must be a regular file without symlinks or hard-link aliases",
                    ),
                ))
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(StateDirError::Io(canonical, err)),
        }
        Ok(canonical)
    }

    pub fn is_current(&self) -> bool {
        self.locks
            .iter()
            .all(|(path, file)| locked_file_is_current(file, path))
    }
}

fn create_private_parent(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn open_lock_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !locked_file_is_current(&file, path) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "lock must be one regular file at its expected pathname",
        ));
    }
    Ok(file)
}

fn regular_single_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        metadata.is_file() && metadata.nlink() == 1
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

fn locked_file_is_current(file: &File, path: &Path) -> bool {
    let (Ok(held), Ok(current)) = (file.metadata(), std::fs::symlink_metadata(path)) else {
        return false;
    };
    if !regular_single_link(&held) || !regular_single_link(&current) {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        held.dev() == current.dev() && held.ino() == current.ino()
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Write `expected` into `<dir>/state.manifest.json` on a fresh
/// directory; on a directory that already has a manifest, verify that
/// network/genesis identity and the complete `schema_versions` map match.
/// `created_at` and `binary_sha` record creation provenance, not a required
/// running build; a compatible package upgrade keeps both unchanged.
pub fn ensure_manifest(dir: &Path, expected: &StateManifest) -> Result<(), StateDirError> {
    let path = dir.join(STATE_MANIFEST_FILE);
    if !path.exists() {
        let serialized = serde_json::to_string_pretty(expected)
            .expect("StateManifest serializes to JSON without io errors");
        write_manifest_atomic(&path, serialized.as_bytes())?;
        return Ok(());
    }
    let mut buf = String::new();
    OpenOptions::new()
        .read(true)
        .open(&path)
        .map_err(|err| StateDirError::Io(path.clone(), err))?
        .read_to_string(&mut buf)
        .map_err(|err| StateDirError::Io(path.clone(), err))?;
    let found: StateManifest = serde_json::from_str(&buf)
        .map_err(|err| StateDirError::ManifestMalformed(path.clone(), err.to_string()))?;
    if found.network_id != expected.network_id {
        return Err(StateDirError::ManifestMismatch {
            dir: dir.to_path_buf(),
            field: "network_id".to_string(),
            expected: expected.network_id.clone(),
            found: found.network_id.clone(),
        });
    }
    // N5.2 — genesis binding: a state dir written under a foreign genesis
    // must not boot. A pre-N5.2 manifest (empty recorded hash) is
    // backfilled once with the current genesis instead of refused.
    let needs_genesis_upgrade = found.genesis_hash.is_empty() && !expected.genesis_hash.is_empty();
    if !needs_genesis_upgrade && found.genesis_hash != expected.genesis_hash {
        return Err(StateDirError::ManifestMismatch {
            dir: dir.to_path_buf(),
            field: "genesis_hash".to_string(),
            expected: expected.genesis_hash.clone(),
            found: found.genesis_hash.clone(),
        });
    }
    // Before this compatibility generation existed, StateManifest::now in
    // package 0.1.0 wrote an empty map while these same ledger readers/writers
    // were in use. Only that known legacy provenance may acquire generation 1
    // metadata. Empty maps from unknown builds are not evidence of compatibility.
    let needs_schema_upgrade = found.schema_versions.is_empty()
        && found.binary_sha == "0.1.0"
        && expected.schema_versions == current_schema_versions();
    for (key, expected_version) in &expected.schema_versions {
        if needs_schema_upgrade {
            break;
        }
        match found.schema_versions.get(key) {
            Some(found_version) if found_version == expected_version => {}
            Some(found_version) => {
                return Err(StateDirError::ManifestMismatch {
                    dir: dir.to_path_buf(),
                    field: format!("schema_versions.{}", key),
                    expected: expected_version.to_string(),
                    found: found_version.to_string(),
                });
            }
            None => {
                return Err(StateDirError::ManifestMismatch {
                    dir: dir.to_path_buf(),
                    field: format!("schema_versions.{}", key),
                    expected: expected_version.to_string(),
                    found: "<missing>".to_string(),
                });
            }
        }
    }
    for (key, found_version) in &found.schema_versions {
        if !expected.schema_versions.contains_key(key) {
            return Err(StateDirError::ManifestMismatch {
                dir: dir.to_path_buf(),
                field: format!("schema_versions.{key}"),
                expected: "<absent>".to_string(),
                found: found_version.to_string(),
            });
        }
    }
    // Validate every persisted invariant before publishing an upgrade. A
    // schema mismatch must leave the original manifest byte-for-byte intact.
    if needs_genesis_upgrade || needs_schema_upgrade {
        let mut upgraded = found;
        if needs_genesis_upgrade {
            upgraded.genesis_hash = expected.genesis_hash.clone();
        }
        if needs_schema_upgrade {
            upgraded.schema_versions = expected.schema_versions.clone();
        }
        let serialized = serde_json::to_string_pretty(&upgraded)
            .expect("StateManifest serializes to JSON without io errors");
        write_manifest_atomic(&path, serialized.as_bytes())?;
    }
    Ok(())
}

fn write_manifest_atomic(path: &Path, bytes: &[u8]) -> Result<(), StateDirError> {
    write_manifest_atomic_with_hook(path, bytes, |_| Ok(()))
}

fn write_manifest_atomic_with_hook<F>(
    path: &Path,
    bytes: &[u8],
    before_rename: F,
) -> Result<(), StateDirError>
where
    F: FnOnce(&Path) -> std::io::Result<()>,
{
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let counter = MANIFEST_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let tmp_path = parent.join(format!(
        ".{STATE_MANIFEST_FILE}.tmp-{}-{nanos}-{counter}",
        std::process::id()
    ));

    write_manifest_atomic_at_temp_with_hook(path, &tmp_path, bytes, before_rename)
}

fn write_manifest_atomic_at_temp_with_hook<F>(
    path: &Path,
    tmp_path: &Path,
    bytes: &[u8],
    before_rename: F,
) -> Result<(), StateDirError>
where
    F: FnOnce(&Path) -> std::io::Result<()>,
{
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut created_by_this_call = false;
    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(tmp_path)?;
        created_by_this_call = true;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        before_rename(tmp_path)?;
        std::fs::rename(tmp_path, path)?;
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() && created_by_this_call {
        let _ = std::fs::remove_file(tmp_path);
    }
    result.map_err(|err| StateDirError::Io(path.to_path_buf(), err))
}

#[cfg(unix)]
// P0.6b — boole-node inherits the workspace `unsafe_code = "deny"` lint.
// The advisory state-dir lock requires a single libc::flock syscall that
// has no safe wrapper in std; scoping the allow to this one function
// keeps the rest of the crate under the deny gate.
#[allow(unsafe_code)]
pub(crate) fn flock_exclusive_nonblocking(file: &File) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
pub(crate) fn flock_exclusive_nonblocking(_file: &File) -> std::io::Result<()> {
    // Non-unix targets are not a supported boole-node deployment; the
    // master plan's L7 contract assumes flock semantics.
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "state-dir advisory lock requires a unix host",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "boole-node-state-dir-{}-{}-{}",
            label,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn acquire_creates_dir_and_lock_file() {
        let dir = fresh_dir("create");
        let guard = acquire(&dir).expect("first acquire");
        assert!(dir.join(STATE_LOCK_FILE).is_file());
        drop(guard);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_acquire_is_rejected_while_first_holds_lock() {
        let dir = fresh_dir("contend");
        let first = acquire(&dir).expect("first acquire");
        match acquire(&dir) {
            Err(StateDirError::Locked(reported)) => {
                assert_eq!(reported, dir);
            }
            other => panic!("expected Locked, got {:?}", other),
        }
        drop(first);
        // Once the first guard drops, the directory is acquirable again.
        let third = acquire(&dir).expect("re-acquire after drop");
        drop(third);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn lock_file_symlink_is_rejected_without_touching_its_target() {
        use std::os::unix::fs::symlink;
        let dir = fresh_dir("lock-symlink");
        std::fs::create_dir_all(&dir).expect("create fixture directory");
        let target = dir.join("unrelated-file");
        std::fs::write(&target, b"unrelated data").expect("write fixture target");
        symlink(&target, dir.join(STATE_LOCK_FILE)).expect("plant lock symlink");

        assert!(acquire(&dir).is_err(), "a lock must not follow a symlink");
        assert_eq!(
            std::fs::read(&target).expect("read target"),
            b"unrelated data"
        );
        std::fs::remove_dir_all(&dir).expect("remove fixture directory");
    }

    #[test]
    fn replaced_lock_file_does_not_report_old_inode_as_current() {
        let dir = fresh_dir("lock-replaced");
        let guard = acquire(&dir).expect("acquire original lock");
        assert!(guard.is_current());
        std::fs::remove_file(dir.join(STATE_LOCK_FILE)).expect("unlink old lock");
        std::fs::write(dir.join(STATE_LOCK_FILE), b"").expect("replace lock inode");
        assert!(!guard.is_current(), "a new file is not our held lock inode");
        drop(guard);
        std::fs::remove_dir_all(dir).expect("remove fixture");
    }

    #[cfg(unix)]
    #[test]
    fn ledger_aliases_share_one_writer_lock_even_without_a_state_directory() {
        use std::os::unix::fs::symlink;
        let dir = fresh_dir("ledger-alias");
        std::fs::create_dir_all(dir.join("real")).expect("create real parent");
        symlink(dir.join("real"), dir.join("alias")).expect("create parent alias");
        let real = dir.join("real/blocks.ndjson");
        let alias = dir.join("alias/blocks.ndjson");
        let guard = LedgerLockSet::acquire([real.clone(), alias.clone()]).expect("first writer");
        assert!(
            !real.exists(),
            "locking must not materialize an empty ledger"
        );
        assert!(matches!(
            LedgerLockSet::acquire([alias.clone()]),
            Err(StateDirError::LedgerLocked(_))
        ));
        assert!(guard.is_current());
        std::fs::write(&real, b"first\n").expect("first writer persists a record");
        let replacement = dir.join("real/replacement");
        std::fs::write(&replacement, b"reorg\n").expect("stage reorg");
        std::fs::rename(replacement, &real).expect("atomic ledger replacement");
        assert!(
            matches!(
                LedgerLockSet::acquire([real]),
                Err(StateDirError::LedgerLocked(_))
            ),
            "a reorg must not release the path's single-writer lock"
        );
        drop(guard);
        drop(LedgerLockSet::acquire([alias]).expect("lock released on drop"));
        std::fs::remove_dir_all(dir).expect("remove fixture");
    }

    #[test]
    fn replaced_ledger_lock_fails_ownership_check() {
        let dir = fresh_dir("ledger-lock-replaced");
        let guard = LedgerLockSet::acquire([dir.join("blocks.ndjson")]).expect("first writer");
        let lock = dir.join(".blocks.ndjson.boole-lock");
        std::fs::remove_file(&lock).expect("unlink held lock");
        std::fs::write(&lock, b"").expect("replace lock inode");
        assert!(!guard.is_current());
        drop(guard);
        std::fs::remove_dir_all(dir).expect("remove fixture");
    }

    #[test]
    fn ensure_manifest_writes_on_first_boot_and_keeps_created_at_on_reboot() {
        let dir = fresh_dir("manifest-fresh");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let m = StateManifest {
            created_at: "1700000000".to_string(),
            network_id: "boole-test".to_string(),
            binary_sha: "abc123".to_string(),
            genesis_hash: "gg".repeat(32),
            schema_versions: BTreeMap::from([("rewards".to_string(), 1u32)]),
        };
        ensure_manifest(&dir, &m).expect("first ensure writes");
        // Second boot with a *different* `created_at` must succeed
        // (created_at is informational and not compared) but must NOT
        // rewrite the file.
        let original = std::fs::read_to_string(dir.join(STATE_MANIFEST_FILE)).expect("read");
        let m2 = StateManifest {
            created_at: "9999999999".to_string(),
            ..m.clone()
        };
        ensure_manifest(&dir, &m2).expect("second ensure verifies");
        let after = std::fs::read_to_string(dir.join(STATE_MANIFEST_FILE)).expect("read");
        assert_eq!(after, original, "manifest file must not be rewritten");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_manifest_rejects_network_id_mismatch() {
        let dir = fresh_dir("manifest-network");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let original = StateManifest {
            created_at: "1700000000".to_string(),
            network_id: "boole-mainnet".to_string(),
            binary_sha: "abc123".to_string(),
            genesis_hash: "gg".repeat(32),
            schema_versions: BTreeMap::new(),
        };
        ensure_manifest(&dir, &original).expect("first ensure");
        let mismatched = StateManifest {
            network_id: "boole-testnet".to_string(),
            ..original.clone()
        };
        match ensure_manifest(&dir, &mismatched) {
            Err(StateDirError::ManifestMismatch {
                field,
                expected,
                found,
                ..
            }) => {
                assert_eq!(field, "network_id");
                assert_eq!(expected, "boole-testnet");
                assert_eq!(found, "boole-mainnet");
            }
            other => panic!("expected ManifestMismatch, got {:?}", other),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn binary_upgrade_with_same_storage_schema_preserves_creation_provenance() {
        let dir = fresh_dir("manifest-compatible-binary-upgrade");
        std::fs::create_dir_all(&dir).expect("create fixture");
        let original = StateManifest {
            created_at: "1700000000".to_string(),
            network_id: "n".to_string(),
            binary_sha: "0.1.0".to_string(),
            genesis_hash: "g".repeat(64),
            schema_versions: BTreeMap::from([("node_storage".to_string(), 1)]),
        };
        ensure_manifest(&dir, &original).expect("first boot");
        let before = std::fs::read(dir.join(STATE_MANIFEST_FILE)).expect("read initial manifest");
        let upgraded = StateManifest {
            binary_sha: "0.2.0".to_string(),
            ..original
        };
        let result = ensure_manifest(&dir, &upgraded);
        let after = std::fs::read(dir.join(STATE_MANIFEST_FILE)).expect("read final manifest");
        std::fs::remove_dir_all(&dir).expect("remove fixture");
        result.expect("a build upgrade with identical disk schema must remain bootable");
        assert_eq!(
            before, after,
            "creation provenance is not the running build version"
        );
    }

    #[test]
    fn known_010_legacy_manifest_gets_one_storage_generation_backfill() {
        let dir = fresh_dir("manifest-known-legacy");
        std::fs::create_dir_all(&dir).expect("create fixture");
        let legacy = StateManifest {
            created_at: "1700000000".to_string(),
            network_id: "n".to_string(),
            binary_sha: "0.1.0".to_string(),
            genesis_hash: "g".repeat(64),
            schema_versions: BTreeMap::new(),
        };
        ensure_manifest(&dir, &legacy).expect("write known legacy fixture");
        let expected = StateManifest::now("n", "0.2.0", &legacy.genesis_hash);
        ensure_manifest(&dir, &expected).expect("backfill disk compatibility generation");
        let bytes = std::fs::read(dir.join(STATE_MANIFEST_FILE)).expect("read upgraded manifest");
        let found: StateManifest = serde_json::from_slice(&bytes).expect("parse upgraded manifest");
        assert_eq!(found.schema_versions, current_schema_versions());
        assert_eq!(found.created_at, legacy.created_at);
        assert_eq!(found.binary_sha, legacy.binary_sha);
        ensure_manifest(
            &dir,
            &StateManifest::now("n", "0.3.0", &legacy.genesis_hash),
        )
        .expect("next compatible binary boots without another migration");
        assert_eq!(
            std::fs::read(dir.join(STATE_MANIFEST_FILE)).expect("read again"),
            bytes
        );
        std::fs::remove_dir_all(dir).expect("remove fixture");
    }

    #[test]
    fn unknown_missing_additional_and_newer_storage_generations_are_rejected_unchanged() {
        let variants = [
            ("unknown-legacy", "unknown-build", BTreeMap::new()),
            (
                "newer",
                "0.1.0",
                BTreeMap::from([("node_storage".to_string(), 2)]),
            ),
            (
                "missing",
                "0.1.0",
                BTreeMap::from([("other_store".to_string(), 1)]),
            ),
            (
                "additional",
                "0.1.0",
                BTreeMap::from([
                    ("node_storage".to_string(), 1),
                    ("unknown_store".to_string(), 1),
                ]),
            ),
        ];
        for (label, provenance, schemas) in variants {
            let dir = fresh_dir(label);
            std::fs::create_dir_all(&dir).expect("create fixture");
            let found = StateManifest {
                created_at: "1700000000".to_string(),
                network_id: "n".to_string(),
                binary_sha: provenance.to_string(),
                genesis_hash: String::new(),
                schema_versions: schemas,
            };
            ensure_manifest(&dir, &found).expect("write incompatible fixture");
            let before = std::fs::read(dir.join(STATE_MANIFEST_FILE)).expect("read original");
            assert!(
                matches!(
                    ensure_manifest(&dir, &StateManifest::now("n", "0.2.0", "g")),
                    Err(StateDirError::ManifestMismatch { .. })
                ),
                "{label} must be rejected"
            );
            assert_eq!(
                std::fs::read(dir.join(STATE_MANIFEST_FILE)).expect("read after refusal"),
                before,
                "{label} must not partially backfill even genesis metadata"
            );
            std::fs::remove_dir_all(dir).expect("remove fixture");
        }
    }

    #[test]
    fn ensure_manifest_rejects_schema_version_mismatch() {
        let dir = fresh_dir("manifest-schema");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let original = StateManifest {
            created_at: "1700000000".to_string(),
            network_id: "n".to_string(),
            binary_sha: "s".to_string(),
            genesis_hash: "gg".repeat(32),
            schema_versions: BTreeMap::from([("rewards".to_string(), 1u32)]),
        };
        ensure_manifest(&dir, &original).expect("first ensure");
        let bumped = StateManifest {
            schema_versions: BTreeMap::from([("rewards".to_string(), 2u32)]),
            ..original.clone()
        };
        match ensure_manifest(&dir, &bumped) {
            Err(StateDirError::ManifestMismatch { field, .. }) => {
                assert_eq!(field, "schema_versions.rewards");
            }
            other => panic!("expected ManifestMismatch, got {:?}", other),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn legacy_manifest_mismatch_is_rejected_without_mutating_existing_bytes() {
        let dir = fresh_dir("manifest-validate-before-upgrade");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let legacy = StateManifest {
            created_at: "1700000000".to_string(),
            network_id: "n".to_string(),
            binary_sha: "s".to_string(),
            genesis_hash: String::new(),
            schema_versions: BTreeMap::from([("rewards".to_string(), 1u32)]),
        };
        ensure_manifest(&dir, &legacy).expect("write legacy manifest");
        let path = dir.join(STATE_MANIFEST_FILE);
        let before = std::fs::read(&path).expect("read original bytes");

        let expected = StateManifest {
            genesis_hash: "gg".repeat(32),
            schema_versions: BTreeMap::from([("rewards".to_string(), 2u32)]),
            ..legacy
        };
        assert!(matches!(
            ensure_manifest(&dir, &expected),
            Err(StateDirError::ManifestMismatch { .. })
        ));
        assert_eq!(
            std::fs::read(&path).expect("read after rejection"),
            before,
            "a rejected boot must not partially upgrade or truncate the manifest"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn interrupted_manifest_upgrade_keeps_previous_manifest_bootable() {
        let dir = fresh_dir("manifest-interrupted-upgrade");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let legacy = StateManifest {
            created_at: "1700000000".to_string(),
            network_id: "n".to_string(),
            binary_sha: "s".to_string(),
            genesis_hash: String::new(),
            schema_versions: BTreeMap::new(),
        };
        ensure_manifest(&dir, &legacy).expect("write legacy manifest");
        let path = dir.join(STATE_MANIFEST_FILE);
        let before = std::fs::read(&path).expect("read original bytes");
        let upgraded = StateManifest {
            genesis_hash: "gg".repeat(32),
            ..legacy.clone()
        };
        let serialized = serde_json::to_vec_pretty(&upgraded).expect("serialize");

        let error = write_manifest_atomic_with_hook(&path, &serialized, |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "injected stop before rename",
            ))
        })
        .expect_err("injected interruption must fail the publish");
        assert!(matches!(error, StateDirError::Io(_, _)));
        assert_eq!(std::fs::read(&path).expect("read old manifest"), before);
        ensure_manifest(&dir, &legacy).expect("the previous manifest remains bootable");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_publish_never_removes_a_preexisting_temp_candidate() {
        let dir = fresh_dir("manifest-preexisting-temp");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join(STATE_MANIFEST_FILE);
        let foreign_temp = dir.join("foreign-temp");
        std::fs::write(&foreign_temp, b"owned by another writer").expect("precreate temp");

        write_manifest_atomic_at_temp_with_hook(&path, &foreign_temp, b"new manifest", |_| Ok(()))
            .expect_err("exclusive temp creation must reject a preexisting candidate");
        assert_eq!(
            std::fs::read(&foreign_temp).expect("foreign temp remains"),
            b"owned by another writer"
        );
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn interrupted_first_manifest_publish_leaves_no_malformed_final_file() {
        let dir = fresh_dir("manifest-interrupted-first-write");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join(STATE_MANIFEST_FILE);
        let manifest = StateManifest::now("n", "s", "g");
        let serialized = serde_json::to_vec_pretty(&manifest).expect("serialize");

        write_manifest_atomic_with_hook(&path, &serialized, |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "injected stop before rename",
            ))
        })
        .expect_err("injected interruption must fail the publish");
        assert!(
            !path.exists(),
            "a stopped first publish must not expose a partial final manifest"
        );
        ensure_manifest(&dir, &manifest).expect("the next boot creates a complete manifest");
        ensure_manifest(&dir, &manifest).expect("the complete manifest reboots cleanly");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_dir_and_deleted_dir_are_distinguishable_via_manifest_presence() {
        let dir = fresh_dir("manifest-presence");
        std::fs::create_dir_all(&dir).expect("mkdir");
        // Empty directory: no manifest yet.
        assert!(!dir.join(STATE_MANIFEST_FILE).exists());
        let m = StateManifest::now("n", "s", "g");
        ensure_manifest(&dir, &m).expect("write");
        assert!(dir.join(STATE_MANIFEST_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
        // After delete the manifest is gone — caller can detect a wiped
        // dir by checking the file's absence.
        assert!(!dir.join(STATE_MANIFEST_FILE).exists());
    }
}
