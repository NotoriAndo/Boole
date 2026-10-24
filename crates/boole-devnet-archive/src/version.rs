//! File-version binding copied from R1 native_node.rs at 01d665f.
use std::{fs, path::Path, time::SystemTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStamp {
    pub len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64),
    #[cfg(unix)]
    change_time: (i64, i64),
}
impl FileStamp {
    pub fn same_file(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            self.identity == other.identity
        }
        #[cfg(not(unix))]
        {
            let _ = other;
            false
        }
    }
}

pub fn file_stamp(path: &Path) -> anyhow::Result<Option<FileStamp>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    stamp(&metadata).map(Some)
}

pub fn stamp(metadata: &fs::Metadata) -> anyhow::Result<FileStamp> {
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "devnet state path is not a regular file"
    );
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    #[cfg(unix)]
    anyhow::ensure!(metadata.nlink() == 1, "devnet state has a hard-link alias");
    Ok(FileStamp {
        len: metadata.len(),
        modified: metadata.modified()?,
        #[cfg(unix)]
        identity: (metadata.dev(), metadata.ino()),
        #[cfg(unix)]
        change_time: (metadata.ctime(), metadata.ctime_nsec()),
    })
}
