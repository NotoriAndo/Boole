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
//! `binary_sha`, `schema_versions`. The first boot writes it; later boots
//! verify the durable values match what the binary expects, so an empty
//! directory and a deleted directory are distinguishable and a directory
//! built for a different network is rejected before any write.
//!
//! This slice ships the foundation only — the API and its unit tests.
//! Wiring into `LocalNodeState::from_config` and the `--state-dir` CLI
//! flag is the next P1.1 slice; that follow-up adds the cross-process
//! integration test that two `boole-node` instances cannot share a
//! directory.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const STATE_LOCK_FILE: &str = "state.lock";
const STATE_MANIFEST_FILE: &str = "state.manifest.json";

/// Failures from the state-directory contract. Distinct variants so the
/// caller (and the typed-error envelope a later slice will wrap them in)
/// can map each one to a stable operator-visible code.
#[derive(Debug, thiserror::Error)]
pub enum StateDirError {
    #[error("state directory is already locked by another process: {0}")]
    Locked(PathBuf),
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
    pub schema_versions: BTreeMap<String, u32>,
}

impl StateManifest {
    /// Build a manifest stamped with the current Unix-second timestamp.
    /// Tests that need a deterministic `created_at` build the struct
    /// directly instead.
    pub fn now(network_id: &str, binary_sha: &str) -> Self {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            created_at: secs.to_string(),
            network_id: network_id.to_string(),
            binary_sha: binary_sha.to_string(),
            schema_versions: BTreeMap::new(),
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
    std::fs::create_dir_all(dir).map_err(|err| StateDirError::Io(dir.to_path_buf(), err))?;
    let lock_path = dir.join(STATE_LOCK_FILE);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|err| StateDirError::Io(lock_path.clone(), err))?;
    flock_exclusive_nonblocking(&file).map_err(|err| {
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
            StateDirError::Locked(dir.to_path_buf())
        } else {
            StateDirError::Io(lock_path.clone(), err)
        }
    })?;
    Ok(StateDirGuard {
        dir: dir.to_path_buf(),
        _lock: file,
    })
}

/// Write `expected` into `<dir>/state.manifest.json` on a fresh
/// directory; on a directory that already has a manifest, verify that
/// `network_id`, `binary_sha`, and every key in `schema_versions` match.
/// `created_at` is only written, never compared, so a re-boot does not
/// rewrite the original boot timestamp.
pub fn ensure_manifest(dir: &Path, expected: &StateManifest) -> Result<(), StateDirError> {
    let path = dir.join(STATE_MANIFEST_FILE);
    if !path.exists() {
        let serialized = serde_json::to_string_pretty(expected)
            .expect("StateManifest serializes to JSON without io errors");
        let mut tmp = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .map_err(|err| StateDirError::Io(path.clone(), err))?;
        tmp.write_all(serialized.as_bytes())
            .map_err(|err| StateDirError::Io(path.clone(), err))?;
        tmp.sync_all()
            .map_err(|err| StateDirError::Io(path.clone(), err))?;
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
            found: found.network_id,
        });
    }
    if found.binary_sha != expected.binary_sha {
        return Err(StateDirError::ManifestMismatch {
            dir: dir.to_path_buf(),
            field: "binary_sha".to_string(),
            expected: expected.binary_sha.clone(),
            found: found.binary_sha,
        });
    }
    for (key, expected_version) in &expected.schema_versions {
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
    Ok(())
}

#[cfg(unix)]
// P0.6b — boole-node inherits the workspace `unsafe_code = "deny"` lint.
// The advisory state-dir lock requires a single libc::flock syscall that
// has no safe wrapper in std; scoping the allow to this one function
// keeps the rest of the crate under the deny gate.
#[allow(unsafe_code)]
fn flock_exclusive_nonblocking(file: &File) -> std::io::Result<()> {
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
fn flock_exclusive_nonblocking(_file: &File) -> std::io::Result<()> {
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

    #[test]
    fn ensure_manifest_writes_on_first_boot_and_keeps_created_at_on_reboot() {
        let dir = fresh_dir("manifest-fresh");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let m = StateManifest {
            created_at: "1700000000".to_string(),
            network_id: "boole-test".to_string(),
            binary_sha: "abc123".to_string(),
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
    fn ensure_manifest_rejects_schema_version_mismatch() {
        let dir = fresh_dir("manifest-schema");
        std::fs::create_dir_all(&dir).expect("mkdir");
        let original = StateManifest {
            created_at: "1700000000".to_string(),
            network_id: "n".to_string(),
            binary_sha: "s".to_string(),
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
    fn empty_dir_and_deleted_dir_are_distinguishable_via_manifest_presence() {
        let dir = fresh_dir("manifest-presence");
        std::fs::create_dir_all(&dir).expect("mkdir");
        // Empty directory: no manifest yet.
        assert!(!dir.join(STATE_MANIFEST_FILE).exists());
        let m = StateManifest::now("n", "s");
        ensure_manifest(&dir, &m).expect("write");
        assert!(dir.join(STATE_MANIFEST_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
        // After delete the manifest is gone — caller can detect a wiped
        // dir by checking the file's absence.
        assert!(!dir.join(STATE_MANIFEST_FILE).exists());
    }
}
