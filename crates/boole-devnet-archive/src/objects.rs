//! Locked, new-file-only acquisition cache; callers authenticate each object's class.
use crate::{journal::read_bound, state_dir};
use anyhow::{ensure, Result};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
pub struct Objects {
    root: PathBuf,
    guard: state_dir::StateDirGuard,
    class: &'static str,
    quarantines: AtomicU64,
    quarantine_logged: AtomicBool,
}
impl Objects {
    pub fn open(root: &Path) -> Result<Self> {
        Self::open_class(root, "object")
    }
    pub fn open_class(root: &Path, class: &'static str) -> Result<Self> {
        crate::fault::validate_environment()?;
        let guard = state_dir::acquire(root)?;
        crate::boot::cleanup_publications(root, &guard)?;
        Ok(Self {
            root: root.canonicalize()?,
            guard,
            class,
            quarantines: AtomicU64::new(0),
            quarantine_logged: AtomicBool::new(false),
        })
    }
    pub fn quarantines(&self) -> u64 {
        self.quarantines.load(Ordering::Relaxed)
    }
    pub fn check(&self) -> Result<()> {
        if !self.guard.is_current() {
            return Err(crate::error::integrity(
                0,
                "object-lock",
                "object store writer lock changed",
            ));
        }
        Ok(())
    }
    fn path(&self, key: &str) -> Result<PathBuf> {
        let hash = boole_derivation::attributes::hex_bytes(key, "object address")?;
        ensure!(hash.len() == 32, "object address width");
        Ok(self.root.join(hex::encode(hash)))
    }
    pub fn read(&self, key: &str, verify: impl Fn(&[u8]) -> Result<()>) -> Result<Option<Vec<u8>>> {
        self.check()?;
        let path = self.path(key)?;
        if !path.try_exists()? {
            return Ok(None);
        }
        let result = (|| -> Result<Vec<u8>> {
            let (bytes, _) = read_bound(&path, crate::input::ROW_MAX)?;
            verify(&bytes)?;
            Ok(bytes)
        })();
        match result {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) => {
                if error.is::<std::io::Error>() {
                    return Err(crate::error::availability(
                        0,
                        "object-io",
                        format!("object read unavailable: {error:#}"),
                    ));
                }
                ensure!(self.guard.is_current(), "object ownership changed");
                let corrupt = self.root.join("corrupt");
                let mut builder = std::fs::DirBuilder::new();
                builder.recursive(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(&corrupt)?;
                ensure!(
                    std::fs::symlink_metadata(&corrupt)?.is_dir()
                        && !std::fs::symlink_metadata(&corrupt)?
                            .file_type()
                            .is_symlink(),
                    "quarantine directory type"
                );
                let nonce = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_nanos();
                let saved = corrupt.join(format!(
                    "{}-{}-{nonce}",
                    path.file_name().unwrap().to_string_lossy(),
                    std::process::id()
                ));
                let mut builder = std::fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(&saved)?;
                std::fs::rename(&path, saved.join("object"))?;
                crate::durability::fsync_parent_dir(&path)?;
                std::fs::File::open(&saved)?.sync_all()?;
                std::fs::File::open(&corrupt)?.sync_all()?;
                let _ = self
                    .quarantines
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                        Some(n.saturating_add(1))
                    });
                if !self.quarantine_logged.swap(true, Ordering::Relaxed) {
                    eprintln!(
                        "archive integrity (object-quarantine): class={}; corrupt bytes preserved",
                        self.class
                    );
                }
                Err(crate::error::integrity(
                    0,
                    "object-quarantine",
                    format!("corrupt object preserved in quarantine: {error:#}"),
                ))
            }
        }
    }
    /// Remove a disposable authenticated cache copy, never an authoritative row.
    pub fn retire(&self, key: &str, verify: impl Fn(&[u8]) -> Result<()>) -> Result<()> {
        if self.root.join("retention.required.json").try_exists()? {
            return Err(crate::error::startup(
                20,
                "retirement of authoritative pin objects is refused",
            ));
        }
        self.check()?;
        let path = self.path(key)?;
        let before = crate::version::file_stamp(&path)?;
        match self.read(key, verify) {
            Ok(Some(_)) => {}
            Ok(None) => return Ok(()),
            Err(e) if quarantined(&e) => return Ok(()),
            Err(e) => return Err(e),
        }
        ensure!(
            before == crate::version::file_stamp(&path)?,
            "object changed before cache retirement"
        );
        self.check()?;
        std::fs::remove_file(&path)?;
        crate::durability::fsync_parent_dir(&path)
    }
    pub fn put(&self, key: &str, bytes: &[u8], verify: impl Fn(&[u8]) -> Result<()>) -> Result<()> {
        self.check()?;
        if crate::journal::free_bytes(&self.root)? < crate::journal::DISK_FLOOR {
            return Err(crate::error::availability(
                0,
                "disk",
                "S23: below 15 GiB disk floor",
            ));
        }
        verify(bytes)?;
        match self.read(key, &verify) {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => {}
            Err(e) if quarantined(&e) => {}
            Err(e) => return Err(e),
        }
        let height = serde_json::from_slice::<crate::input::InputRow>(bytes)
            .ok()
            .and_then(|r| r.block().ok())
            .map(|b| b.number);
        crate::journal::publish_new_at(&self.path(key)?, bytes, height.map(|h| (h, "staged-input")))
    }
}
fn quarantined(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<crate::error::Error>(),
        Some(crate::error::Error::Integrity {
            class: "object-quarantine",
            ..
        })
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    #[test]
    fn b9_valid_object_alias_is_removed_before_quarantine_or_a_historical_refetch() -> Result<()> {
        let d = crate::durability::PrivateTempDir::new("m4-object-alias")?;
        let key = format!("0x{}", "ab".repeat(32));
        let bytes = b"already authenticated acquisition input";
        let objects = Objects::open(d.path())?;
        objects.put(&key, bytes, |_| Ok(()))?;
        let target = objects.path(&key)?;
        drop(objects);
        let stage = crate::durability::PrivateTempDir::new_in(d.path(), ".devnet-new")?;
        std::fs::hard_link(&target, stage.path().join("value"))?;
        std::mem::forget(stage);
        let objects = Objects::open(d.path())?;
        assert_eq!(
            objects
                .read(&key, |got| {
                    ensure!(got == bytes, "changed object");
                    Ok(())
                })?
                .unwrap(),
            bytes
        );
        assert!(!d.path().join("corrupt").exists());
        Ok(())
    }
    #[test]
    fn empty_private_staging_is_removed_under_the_writer_lock() -> Result<()> {
        let d = crate::durability::PrivateTempDir::new("m4-empty-staging")?;
        let stage = crate::durability::PrivateTempDir::new_in(d.path(), ".devnet-new")?;
        let objects = Objects::open(d.path())?;
        assert!(!stage.path().exists());
        drop(objects);
        Ok(())
    }
    #[test]
    fn unpublished_value_with_foreign_mode_stops_typed_and_is_never_deleted() -> Result<()> {
        let d = crate::durability::PrivateTempDir::new("m4-unpublished")?;
        let stage = crate::durability::PrivateTempDir::new_in(d.path(), ".devnet-new")?;
        std::fs::write(stage.path().join("value"), b"preserve unpublished bytes")?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                stage.path().join("value"),
                std::fs::Permissions::from_mode(0o644),
            )?;
        }
        let result = Objects::open(d.path());
        let error = match result {
            Ok(_) => panic!("unpublished staging accepted"),
            Err(e) => e,
        };
        assert!(matches!(
            error.downcast_ref::<crate::error::Error>(),
            Some(crate::error::Error::Integrity { .. })
        ));
        assert_eq!(
            std::fs::read(stage.path().join("value"))?,
            b"preserve unpublished bytes"
        );
        Ok(())
    }
    #[test]
    fn t86_corrupt_objects_are_preserved_in_quarantine_before_refetch() -> Result<()> {
        let dir = crate::durability::PrivateTempDir::new("m4-object")?;
        let objects = Objects::open(dir.path())?;
        let good = b"verified public object";
        let hash = hex::encode(Sha256::digest(good));
        let key = format!("0x{hash}");
        let verify = |raw: &[u8]| {
            ensure!(
                hex::encode(Sha256::digest(raw)) == hash,
                "object digest mismatch"
            );
            Ok(())
        };
        objects.put(&key, good, verify)?;
        assert!(crate::journal::publish_new(&objects.path(&key)?, b"overwrite").is_err());
        assert_eq!(objects.read(&key, verify)?.unwrap(), good);
        let bad = b"preserve this corrupt object";
        std::fs::write(objects.path(&key)?, bad)?;
        assert!(objects.read(&key, verify).is_err());
        assert!(
            !objects.path(&key)?.exists(),
            "a corrupt address must be quarantined, never edited"
        );
        let saved: Vec<_> =
            std::fs::read_dir(dir.path().join("corrupt"))?.collect::<std::io::Result<_>>()?;
        assert_eq!(saved.len(), 1);
        assert_eq!(std::fs::read(saved[0].path().join("object"))?, bad);
        objects.put(&key, good, verify)?;
        assert_eq!(objects.read(&key, verify)?.unwrap(), good);
        Ok(())
    }
}

#[cfg(test)]
mod fix2_tests {
    use super::*;
    #[test]
    fn m1_verified_republish_and_retirement_survive_quarantine() -> Result<()> {
        use sha2::{Digest, Sha256};
        let dir = crate::durability::PrivateTempDir::new("object-quarantine-retry")?;
        let bytes = b"verified replacement";
        let key = format!("0x{}", hex::encode(Sha256::digest(bytes)));
        let verify = |b: &[u8]| {
            ensure!(b == bytes, "changed public bytes");
            Ok(())
        };
        let objects = Objects::open(dir.path())?;
        objects.put(&key, bytes, verify)?;
        std::fs::write(objects.path(&key)?, b"first corrupt copy")?;
        objects.put(&key, bytes, verify)?;
        assert_eq!(
            objects.read(&key, verify)?.as_deref(),
            Some(bytes.as_slice())
        );
        std::fs::write(objects.path(&key)?, b"corrupt disposable header")?;
        objects.retire(&key, verify)?;
        assert!(objects.read(&key, verify)?.is_none());
        let mut saved = std::fs::read_dir(dir.path().join("corrupt"))?
            .map(|e| std::fs::read(e.unwrap().path().join("object")))
            .collect::<std::io::Result<Vec<_>>>()?;
        saved.sort();
        let mut expected = vec![
            b"first corrupt copy".to_vec(),
            b"corrupt disposable header".to_vec(),
        ];
        expected.sort();
        assert_eq!(saved, expected);
        Ok(())
    }
}
