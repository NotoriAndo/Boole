//! A verified own-writer staging alias is unavailable, not corrupt authority.
use super::*;
fn pending(path: &Path) -> Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let target = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        if !target.is_file() || target.file_type().is_symlink() || target.nlink() != 2 {
            return Ok(false);
        }
        let Some(parent) = path.parent() else {
            return Ok(false);
        };
        for entry in std::fs::read_dir(parent)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let Some(nonce) = name.strip_prefix(".devnet-new-") else {
                continue;
            };
            if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            let stage = std::fs::symlink_metadata(entry.path())?;
            if !stage.is_dir()
                || stage.file_type().is_symlink()
                || stage.uid() != target.uid()
                || stage.permissions().mode() & 0o777 != 0o700
            {
                continue;
            }
            let value = match std::fs::symlink_metadata(entry.path().join("value")) {
                Ok(m) => m,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            if value.is_file()
                && !value.file_type().is_symlink()
                && value.dev() == target.dev()
                && value.ino() == target.ino()
                && value.nlink() == 2
                && value.uid() == target.uid()
                && value.permissions().mode() & 0o777 == 0o600
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn classify(path: &Path, error: anyhow::Error) -> anyhow::Error {
    match pending(path) {
        Ok(true) => availability(
            0,
            "pin-publication",
            "S20: own publication is in progress or interrupted; writer recovery required",
        ),
        Ok(false) => error,
        Err(e) => availability(
            0,
            "pin-publication",
            format!("S20: publication status unavailable: {e:#}"),
        ),
    }
}
pub(super) fn pin_read(path: &Path, cap: usize) -> Result<(Vec<u8>, FileStamp)> {
    read_bound(path, cap).map_err(|e| classify(path, e))
}
pub(super) fn pin_stamp(path: &Path) -> Result<Option<FileStamp>> {
    #[cfg(test)]
    STAMP_FAILURE.with(|slot| {
        if slot.borrow().as_deref() == Some(path) {
            slot.borrow_mut().take();
            return Err(availability(
                0,
                "pin-publication",
                "injected post-publication stamp failure",
            ));
        }
        Ok(())
    })?;
    file_stamp(path).map_err(|e| classify(path, e))
}
#[cfg(test)]
thread_local! {
    pub(super) static STAMP_FAILURE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}
