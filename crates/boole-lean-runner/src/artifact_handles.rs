//! Narrow Unix file-handle operations for trusted artifact transport. These do
//! not configure, relax or bypass the byte-adapter process sandbox.
use std::{fs::File, io, path::Path};
#[cfg(unix)]
use std::{
    fs::OpenOptions,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    },
};

#[cfg(unix)]
fn clear_nonblocking(file: &File) -> io::Result<()> {
    // SAFETY: fcntl operates only on the live borrowed descriptor, and these
    // commands do not read pointers or transfer ownership of that descriptor.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(unix)]
pub fn open_regular(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "artifact is not a regular file",
        ));
    }
    clear_nonblocking(&file)?;
    Ok(file)
}
#[cfg(unix)]
pub fn private_directory(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let meta = file.metadata()?;
    // SAFETY: geteuid has no arguments, ownership or memory effects.
    if !meta.is_dir()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.permissions().mode() & 0o7777 != 0o700
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "artifact output must be euid-owned mode 0700",
        ));
    }
    clear_nonblocking(&file)?;
    Ok(file)
}
#[cfg(unix)]
pub fn create_private(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "artifact stage is not regular",
        ));
    }
    clear_nonblocking(&file)?;
    Ok(file)
}
#[cfg(not(unix))]
pub fn open_regular(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Unix artifact handles required",
    ))
}
#[cfg(not(unix))]
pub fn private_directory(path: &Path) -> io::Result<File> {
    open_regular(path)
}
#[cfg(not(unix))]
pub fn create_private(path: &Path) -> io::Result<File> {
    open_regular(path)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn artifact_handles_refuse_symlinks_and_fifos_and_clear_nonblocking() -> io::Result<()> {
        use std::{
            ffi::CString,
            os::unix::ffi::OsStrExt,
            time::{SystemTime, UNIX_EPOCH},
        };
        let root = std::env::temp_dir().join(format!(
            "artifact-os-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root)?;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        let result = (|| {
            let path = root.join("file");
            let file = create_private(&path)?;
            assert_eq!(
                unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) } & libc::O_NONBLOCK,
                0
            );
            assert!(private_directory(&root).is_ok());
            let link = root.join("link");
            std::os::unix::fs::symlink(&path, &link)?;
            assert!(open_regular(&link).is_err());
            assert!(create_private(&link).is_err());
            let fifo = root.join("fifo");
            let name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
            // SAFETY: name is a terminated pathname owned by this fixture.
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            let now = std::time::Instant::now();
            assert!(open_regular(&fifo).is_err());
            assert!(now.elapsed() < std::time::Duration::from_secs(1));
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755))?;
            assert!(private_directory(&root).is_err());
            Ok(())
        })();
        std::fs::remove_dir_all(&root)?;
        result
    }
}
