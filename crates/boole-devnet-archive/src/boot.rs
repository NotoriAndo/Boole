//! Cleanup is called only after the store's lifetime writer lock is acquired.
use crate::state_dir::StateDirGuard;
use anyhow::{ensure, Result};
use std::path::Path;
pub fn cleanup(
    root: &Path,
    guard: &StateDirGuard,
    pins: &[[u8; 32]],
    checkpoint: Option<&Path>,
) -> Result<()> {
    use sha2::{Digest, Sha256};
    ensure!(
        guard.is_current() && root.canonicalize()? == guard.dir().canonicalize()?,
        "cleanup requires the matching current writer lock"
    );
    let root = root.canonicalize()?;
    for entry in std::fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if let Some(nonce) = name.strip_prefix("scratch-0x") {
            if nonce.len() != 64 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            let meta = std::fs::symlink_metadata(&path)?;
            ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "scratch must be a real directory"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                ensure!(
                    meta.uid() == effective_uid() && meta.permissions().mode() & 0o077 == 0,
                    "scratch ownership/permissions"
                );
            }
            let executable = path.join("gate");
            for pid in process_ids()? {
                if process_path(pid).as_deref() != Some(executable.as_path()) {
                    continue;
                }
                let (raw, _) = crate::journal::read_bound(&executable, 128 * 1024 * 1024)?;
                let digest: [u8; 32] = Sha256::digest(raw).into();
                ensure!(
                    pins.contains(&digest),
                    "orphan gate binary is not configured; scratch preserved"
                );
                let before = process_birth(pid)?;
                ensure!(
                    process_group(pid) == pid,
                    "orphan gate is not its own process-group leader; preserved"
                );
                ensure!(
                    guard.is_current()
                        && process_path(pid).as_deref() == Some(executable.as_path())
                        && process_birth(pid)? == before,
                    "orphan process identity changed"
                );
                kill_group(pid)?;
                let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
                while process_path(pid).as_deref() == Some(executable.as_path()) {
                    ensure!(
                        std::time::Instant::now() < end,
                        "owned orphan did not terminate; scratch preserved"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
            ensure!(
                guard.is_current(),
                "writer lock changed before scratch cleanup"
            );
            std::fs::remove_dir_all(&path)?;
            crate::durability::fsync_parent_dir(&path)?;
        }
    }
    if let Some(checkpoint) = checkpoint {
        ensure!(
            checkpoint
                .parent()
                .map(Path::canonicalize)
                .transpose()?
                .as_ref()
                == Some(&root),
            "checkpoint cleanup outside locked root"
        );
        let prefix = checkpoint
            .with_extension("tmp-")
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(pid) = name
                .strip_prefix(&prefix)
                .and_then(|v| v.parse::<i32>().ok())
                .filter(|p| *p > 1)
            else {
                continue;
            };
            if process_exists(pid) {
                continue;
            }
            let Ok((raw, _)) = crate::journal::read_bound(&entry.path(), 65536) else {
                continue;
            };
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(&raw) else {
                continue;
            };
            if v["version"] != 1
                || v["height"].as_u64().is_none()
                || ["m2_root", "l2_hash", "l1_hash", "config_digest"]
                    .iter()
                    .any(|k| v[k].as_str().is_none())
            {
                continue;
            }
            ensure!(
                guard.is_current(),
                "writer lock changed before temp cleanup"
            );
            std::fs::remove_file(entry.path())?;
            crate::durability::fsync_parent_dir(&entry.path())?;
        }
    }
    Ok(())
}
fn process_ids() -> Result<Vec<i32>> {
    let out = std::process::Command::new("ps")
        .args(["-axo", "pid="])
        .output()?;
    ensure!(
        out.status.success() && out.stdout.len() < 4 * 1024 * 1024,
        "bounded process inventory unavailable"
    );
    Ok(std::str::from_utf8(&out.stdout)?
        .split_whitespace()
        .filter_map(|p| p.parse().ok())
        .collect())
}
fn process_birth(pid: i32) -> Result<Vec<u8>> {
    let out = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()?;
    ensure!(
        out.status.success() && !out.stdout.is_empty() && out.stdout.len() < 256,
        "process birth identity unavailable"
    );
    Ok(out.stdout)
}
#[cfg(target_os = "linux")]
fn process_path(pid: i32) -> Option<std::path::PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}
#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // Kernel executable path, not argv, identifies an owned orphan.
fn process_path(pid: i32) -> Option<std::path::PathBuf> {
    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_pidpath(pid: i32, buffer: *mut libc::c_void, buffersize: u32) -> i32;
    }
    let mut bytes = vec![0u8; 4096];
    let count = unsafe { proc_pidpath(pid, bytes.as_mut_ptr().cast(), bytes.len() as u32) };
    if count <= 0 {
        return None;
    }
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(count as usize);
    use std::os::unix::ffi::OsStrExt;
    Some(std::ffi::OsStr::from_bytes(&bytes[..end]).into())
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn process_path(_pid: i32) -> Option<std::path::PathBuf> {
    None
}
#[allow(unsafe_code)]
fn effective_uid() -> u32 {
    unsafe { libc::geteuid() }
}
#[allow(unsafe_code)]
fn process_group(pid: i32) -> i32 {
    unsafe { libc::getpgid(pid) }
}
#[allow(unsafe_code)]
fn process_exists(pid: i32) -> bool {
    (unsafe { libc::kill(pid, 0) == 0 })
        || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}
#[allow(unsafe_code)]
fn kill_group(pid: i32) -> Result<()> {
    ensure!(
        pid > 1 && pid != unsafe { libc::getpgrp() },
        "refusing own or invalid process group"
    );
    ensure!(
        unsafe { libc::kill(-pid, libc::SIGKILL) } == 0,
        "owned orphan group termination failed"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t85_scratch_is_removed_only_after_lock_and_authority_files_are_preserved() -> Result<()> {
        let d = crate::durability::PrivateTempDir::new("m4-cleanup")?;
        let scratch = d.path().join(format!("scratch-0x{}", "ab".repeat(32)));
        std::fs::create_dir(&scratch)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700))?;
        }
        std::fs::write(scratch.join("input.json"), b"discarded work")?;
        let evidence = d.path().join("evidence.jsonl");
        std::fs::write(&evidence, b"preserve")?;
        let guard = crate::state_dir::acquire(d.path())?;
        cleanup(d.path(), &guard, &[], None)?;
        assert!(
            !scratch.exists(),
            "stale scratch must not accumulate after restart"
        );
        assert_eq!(std::fs::read(evidence)?, b"preserve");
        Ok(())
    }
    #[test]
    fn t83_b6_stale_checkpoint_temp_is_removed_but_saved_checkpoint_is_never_replaced() -> Result<()>
    {
        let d = crate::durability::PrivateTempDir::new("m4-stale-temp")?;
        let p = d.path().join("checkpoint.json");
        let tmp = p.with_extension("tmp-2147483647");
        std::fs::write(&p, b"{\"height\":1}")?;
        std::fs::write(&tmp,b"{\"version\":1,\"height\":2,\"m2_root\":\"0x00\",\"l2_hash\":\"0x00\",\"l1_hash\":\"0x00\",\"config_digest\":\"0x00\"}")?;
        let guard = crate::state_dir::acquire(d.path())?;
        cleanup(d.path(), &guard, &[], Some(&p))?;
        assert!(
            !tmp.exists(),
            "B6 stale checkpoint temp must be cleaned after taking the lock"
        );
        assert_eq!(std::fs::read(p)?, b"{\"height\":1}");
        Ok(())
    }
    #[test]
    #[ignore = "fixture subprocess only"]
    fn orphan_fixture_child() {
        use std::os::unix::process::CommandExt;
        let Some(root) = std::env::var_os("BOOLE_M4_ORPHAN_FIXTURE") else {
            return;
        };
        let root = std::path::PathBuf::from(root);
        let child = std::process::Command::new(root.join("gate"))
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        std::fs::write(root.join("pid"), child.id().to_string()).unwrap();
        std::mem::forget(child);
    }
    #[test]
    fn t85_only_a_pinned_worker_group_in_the_locked_scratch_tree_is_reaped() -> Result<()> {
        use sha2::{Digest, Sha256};
        use std::os::unix::{fs::PermissionsExt, net::UnixStream};
        use std::process::{Command, Stdio};
        let d = crate::durability::PrivateTempDir::new("m4-orphan")?;
        let root = d.path().join(format!("scratch-0x{}", "cd".repeat(32)));
        std::fs::create_dir(&root)?;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
        std::fs::copy("/bin/cat", root.join("gate"))?;
        let root = root.canonicalize()?;
        let pin: [u8; 32] = Sha256::digest(std::fs::read(root.join("gate"))?).into();
        let (read, held) = UnixStream::pair()?;
        let out = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "boot::tests::orphan_fixture_child",
                "--ignored",
                "--nocapture",
            ])
            .env("BOOLE_M4_ORPHAN_FIXTURE", &root)
            .stdin(Stdio::from(std::os::fd::OwnedFd::from(read)))
            .output()?;
        ensure!(out.status.success(), "orphan fixture parent failed");
        let pid = std::fs::read_to_string(root.join("pid"))?.parse::<i32>()?;
        let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while process_path(pid).as_deref() != Some(root.join("gate").as_path()) {
            ensure!(
                std::time::Instant::now() < end,
                "orphan executable unavailable"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let (read2, held2) = UnixStream::pair()?;
        let mut unrelated = Command::new("/bin/cat")
            .stdin(Stdio::from(std::os::fd::OwnedFd::from(read2)))
            .stdout(Stdio::null())
            .spawn()?;
        let guard = crate::state_dir::acquire(d.path())?;
        let result = cleanup(d.path(), &guard, &[pin], None);
        let unrelated_alive = unrelated.try_wait()?.is_none();
        let _ = unrelated.kill();
        let _ = unrelated.wait();
        drop((held, held2));
        if result.is_err() {
            let _ = kill_group(pid);
        }
        result?;
        assert!(unrelated_alive);
        assert!(!root.exists());
        assert!(process_path(pid).is_none());
        Ok(())
    }
}
