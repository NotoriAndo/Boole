use crate::codec::{private_file, Scratch};
use anyhow::{ensure, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};

pub struct Store {
    pub path: PathBuf,
    _lock: File,
}
impl Store {
    pub fn open(path: PathBuf) -> Result<Self> {
        if !path.exists() {
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&path)?;
        }
        let meta = fs::symlink_metadata(&path)?;
        ensure!(
            meta.is_dir() && !meta.file_type().is_symlink(),
            "private store must be a real directory"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                meta.permissions().mode() & 0o077 == 0,
                "store permissions must be 0700"
            );
        }
        let lock_path = path.join("client.lock");
        if !lock_path.exists() {
            private_file(&lock_path, &[])?;
        }
        check_private(&lock_path)?;
        let lock = OpenOptions::new().read(true).write(true).open(lock_path)?;
        lock.try_lock()
            .map_err(|_| anyhow::anyhow!("private store already in use"))?;
        Ok(Self { path, _lock: lock })
    }
    pub fn read<T: DeserializeOwned>(&self, name: &str) -> Result<T> {
        let p = self.path.join(name);
        check_private(&p)?;
        Ok(serde_json::from_slice(&crate::codec::read_bounded(
            &p,
            128 * 1024 * 1024,
        )?)?)
    }
    pub fn write<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        ensure!(
            !name.contains('/') && name.ends_with(".json"),
            "store file name"
        );
        let target = self.path.join(name);
        if target.exists() {
            check_private(&target)?;
        }
        let scratch = Scratch::new(&self.path)?;
        let tmp = scratch.0.join("next");
        private_file(&tmp, &serde_json::to_vec(value)?)?;
        fs::rename(tmp, target)?;
        if name.starts_with("submission-") {
            boole_devnet_archive::fault::hit("B11", 0, "client")?;
        }
        File::open(&self.path)?.sync_all()?;
        Ok(())
    }
    pub fn submissions(&self) -> Result<Vec<String>> {
        let mut names = vec![];
        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("submission-") && name.ends_with(".json") {
                check_private(&entry.path())?;
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }
}
fn check_private(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "store file must be regular"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        ensure!(
            meta.permissions().mode() & 0o077 == 0,
            "store file permissions must be 0600"
        );
    }
    Ok(())
}
