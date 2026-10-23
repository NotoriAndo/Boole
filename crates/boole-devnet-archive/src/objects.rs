//! Locked, new-file-only acquisition cache; callers authenticate each object's class.
use crate::{journal::read_bound, state_dir};
use anyhow::{ensure, Result};
use std::path::{Path, PathBuf};
pub struct Objects {
    root: PathBuf,
    guard: state_dir::StateDirGuard,
}
impl Objects {
    pub fn open(root: &Path) -> Result<Self> {
        let guard = state_dir::acquire(root)?;
        Ok(Self {
            root: root.canonicalize()?,
            guard,
        })
    }
    fn path(&self, key: &str) -> Result<PathBuf> {
        let hash = boole_derivation::attributes::hex_bytes(key, "object address")?;
        ensure!(hash.len() == 32, "object address width");
        Ok(self.root.join(hex::encode(hash)))
    }
    pub fn read(&self, key: &str, verify: impl Fn(&[u8]) -> Result<()>) -> Result<Option<Vec<u8>>> {
        ensure!(self.guard.is_current(), "object store lock changed");
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
                Err(error.context("corrupt object preserved in quarantine"))
            }
        }
    }
    pub fn put(&self, key: &str, bytes: &[u8], verify: impl Fn(&[u8]) -> Result<()>) -> Result<()> {
        ensure!(self.guard.is_current(), "object store lock changed");
        if crate::journal::free_bytes(&self.root)? < crate::journal::DISK_FLOOR {
            return Err(crate::error::availability(
                0,
                "disk",
                "S23: below 15 GiB disk floor",
            ));
        }
        verify(bytes)?;
        if self.read(key, &verify)?.is_some() {
            return Ok(());
        }
        let height = serde_json::from_slice::<crate::input::InputRow>(bytes)
            .ok()
            .and_then(|r| r.block().ok())
            .map(|b| b.number);
        crate::journal::publish_new_at(&self.path(key)?, bytes, height.map(|h| (h, "staged-input")))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
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
