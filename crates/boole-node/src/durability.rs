use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use rand_core::{OsRng, RngCore};

/// An exclusively-created private workspace. Unpredictable names and mode
/// 0700 keep a shared temporary namespace from becoming proof/file authority.
/// The guard removes only the directory inode created by this call.
pub(crate) struct PrivateTempDir {
    path: PathBuf,
    identity: fs::Metadata,
}

impl PrivateTempDir {
    pub(crate) fn new(prefix: &str) -> std::io::Result<Self> {
        Self::new_in(&std::env::temp_dir(), prefix)
    }

    fn new_in(parent: &Path, prefix: &str) -> std::io::Result<Self> {
        for _ in 0..32 {
            let mut random = [0u8; 16];
            OsRng
                .try_fill_bytes(&mut random)
                .map_err(std::io::Error::other)?;
            let path = parent.join(format!("{prefix}-{}", hex::encode(random)));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => {
                    let identity = fs::symlink_metadata(&path)?;
                    return Ok(Self { path, identity });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not exclusively create a private temporary workspace",
        ))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Publish proof bytes only inside this exclusively-owned workspace.
    pub(crate) fn write_proof(&self, bytes: &[u8]) -> std::io::Result<PathBuf> {
        let path = self.path.join("Proof.lean");
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(&path)?;
        file.write_all(bytes)?;
        Ok(path)
    }
}

impl Drop for PrivateTempDir {
    fn drop(&mut self) {
        let Ok(current) = fs::symlink_metadata(&self.path) else {
            return;
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if current.dev() != self.identity.dev() || current.ino() != self.identity.ino() {
                return;
            }
        }
        if current.is_dir() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// Find the byte length of the longest NDJSON prefix that ends on a newline.
///
/// A torn trailing line (the result of a crash mid-`writeln!`) is bytes after
/// the last `\n`; callers `set_len(stable_len)` to truncate it so the next
/// append starts from a known-good record boundary.
pub(crate) fn stable_jsonl_prefix_len(bytes: &[u8]) -> usize {
    if bytes.is_empty() || bytes.last() == Some(&b'\n') {
        return bytes.len();
    }
    bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .unwrap_or(0)
}

/// Durable NDJSON append: write one line, flush user-space buffers, fsync
/// the file, and fsync the parent directory if this call created the file.
/// Without the parent-dir fsync the new directory entry can be lost on crash
/// even after the file's own data hits disk.
pub(crate) fn append_ndjson_line_durable(path: &Path, line: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut create = OpenOptions::new();
    create.create_new(true).append(true);
    let (mut file, is_new_file) = match open_regular_file(path, &mut create) {
        Ok(file) => (file, true),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            let mut append = OpenOptions::new();
            append.append(true);
            (open_regular_file(path, &mut append)?, false)
        }
        Err(err) => return Err(err.into()),
    };
    writeln!(file, "{}", line)?;
    file.flush()?;
    file.sync_all()?;
    if is_new_file {
        fsync_parent_dir(path)?;
    }
    Ok(())
}

/// Durable NDJSON append through an already-open authoritative file
/// descriptor. Native-shadow uses this variant so its lifetime `flock`,
/// replay, torn-tail repair, appends and fsyncs all refer to the exact same
/// open file description instead of reopening a replaceable pathname.
pub(crate) fn append_ndjson_line_durable_on_file(
    file: &mut File,
    line: &str,
) -> anyhow::Result<()> {
    file.seek(SeekFrom::End(0))?;
    writeln!(file, "{}", line)?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

/// Atomically replace the NDJSON file at `path` with `lines` (one record per
/// entry, each newline-terminated). Writes a sibling temp file, fsyncs it, then
/// `rename`s it over `path` — an atomic swap on a POSIX filesystem, so a crash
/// leaves either the whole previous file or the whole new one, never a torn
/// mix. The reorg handler uses this to swap the block store / reward ledger to
/// a heavier chain without a window where the on-disk chain is truncated.
pub(crate) fn write_ndjson_lines_atomic(path: &Path, lines: &[String]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let workspace = PrivateTempDir::new_in(parent, ".boole-rewrite")?;
    let tmp = workspace.path().join("ledger");
    {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(&tmp)?;
        let mut buf = String::new();
        for line in lines {
            buf.push_str(line);
            buf.push('\n');
        }
        file.write_all(buf.as_bytes())?;
        file.flush()?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    fsync_parent_dir(path)?;
    Ok(())
}

/// Read the file at `path`, truncate any torn trailing line on disk, and
/// return the stable bytes as a UTF-8 string ready for line-by-line replay.
/// Returns `None` when the file does not exist so callers can short-circuit
/// to a fresh ledger.
pub(crate) fn read_stable_prefix(path: &Path) -> anyhow::Result<Option<String>> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    let mut file = match open_regular_file(path, &mut options) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    // Read and repair the exact descriptor opened above. Reopening the
    // pathname after reading would allow a replacement inode to be truncated.
    read_stable_prefix_on_file(&mut file).map(Some)
}

fn open_regular_file(path: &Path, options: &mut OpenOptions) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "ledger must be a regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "ledger must not have hard-link aliases",
            ));
        }
    }
    Ok(file)
}

/// Read a complete NDJSON file without changing it.
///
/// Observation paths (CLI verification, deep verification, `/status`, and
/// `/ready`) must never reuse crash-repair semantics: a torn final append is
/// evidence that the live file is not presently stable, not permission for a
/// reader to truncate bytes owned by another process. Boot/recovery keeps
/// using [`read_stable_prefix`]; observers use this strict variant and fail
/// closed when the last line is incomplete.
pub(crate) fn read_complete_ndjson_read_only(path: &Path) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw_bytes = fs::read(path)?;
    let stable_len = stable_jsonl_prefix_len(&raw_bytes);
    if stable_len < raw_bytes.len() {
        anyhow::bail!(
            "torn trailing line at byte {} ({} trailing byte(s)); read-only inspection did not repair the file",
            stable_len,
            raw_bytes.len() - stable_len,
        );
    }
    Ok(Some(String::from_utf8(raw_bytes)?))
}

/// Replay and repair an NDJSON journal through one already-open file
/// descriptor. A torn tail is truncated and fsynced before the stable prefix
/// is returned; the descriptor is left positioned at EOF for the next append.
pub(crate) fn read_stable_prefix_on_file(file: &mut File) -> anyhow::Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut raw_bytes = Vec::new();
    file.read_to_end(&mut raw_bytes)?;
    let stable_len = stable_jsonl_prefix_len(&raw_bytes);
    if stable_len < raw_bytes.len() {
        file.set_len(stable_len as u64)?;
        file.sync_all()?;
    }
    file.seek(SeekFrom::End(0))?;
    Ok(String::from_utf8(raw_bytes[..stable_len].to_vec())?)
}

#[cfg(unix)]
pub(crate) fn fsync_parent_dir(path: &Path) -> anyhow::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let dir_file = File::open(dir)?;
    dir_file.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn fsync_parent_dir(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_has_zero_stable_prefix() {
        assert_eq!(stable_jsonl_prefix_len(b""), 0);
    }

    #[test]
    fn single_complete_line_keeps_all_bytes() {
        let bytes = b"{\"a\":1}\n";
        assert_eq!(stable_jsonl_prefix_len(bytes), bytes.len());
    }

    #[test]
    fn torn_trailing_line_is_excluded_from_stable_prefix() {
        let bytes = b"{\"a\":1}\n{\"b\":";
        // First 8 bytes ("{\"a\":1}\n") are stable; the partial second line is dropped.
        assert_eq!(stable_jsonl_prefix_len(bytes), 8);
    }

    #[test]
    fn no_newline_anywhere_means_zero_stable_prefix() {
        let bytes = b"partial-line-without-newline";
        assert_eq!(stable_jsonl_prefix_len(bytes), 0);
    }

    #[test]
    fn multiple_complete_lines_keep_all_bytes() {
        let bytes = b"a\nb\nc\n";
        assert_eq!(stable_jsonl_prefix_len(bytes), bytes.len());
    }

    #[cfg(unix)]
    #[test]
    fn atomic_rewrite_does_not_follow_a_preplanted_temp_symlink() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!(
            "boole-rewrite-link-{}-{}",
            std::process::id(),
            boole_testkit::rand_suffix()
        ));
        fs::create_dir(&dir).expect("create fixture");
        let path = dir.join("blocks.ndjson");
        let unrelated = dir.join("unrelated");
        fs::write(&path, b"old\n").expect("old ledger");
        fs::write(&unrelated, b"must not change").expect("unrelated fixture");
        symlink(&unrelated, dir.join("blocks.ndjson.reorg-tmp")).expect("plant symlink");

        write_ndjson_lines_atomic(&path, &["replacement".to_string()]).expect("atomic rewrite");
        let unrelated_after = fs::read(&unrelated).expect("read unrelated file");
        let ledger_after = fs::read(&path).expect("read ledger");
        fs::remove_dir_all(&dir).expect("remove fixture");
        assert_eq!(
            unrelated_after, b"must not change",
            "rewriting a ledger must not write through someone else's symlink"
        );
        assert_eq!(ledger_after, b"replacement\n");
    }

    #[cfg(unix)]
    #[test]
    fn recovery_refuses_symlink_instead_of_truncating_an_unrelated_file() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!(
            "boole-recovery-link-{}-{}",
            std::process::id(),
            boole_testkit::rand_suffix()
        ));
        fs::create_dir(&dir).expect("create fixture");
        let target = dir.join("unrelated");
        let ledger = dir.join("ledger.ndjson");
        let bytes = b"unrelated complete line\nunrelated partial line";
        fs::write(&target, bytes).expect("write unrelated fixture");
        symlink(&target, &ledger).expect("plant ledger symlink");
        let result = read_stable_prefix(&ledger);
        let after = fs::read(&target).expect("read unrelated fixture");
        fs::remove_dir_all(&dir).expect("remove fixture");
        assert!(
            result.is_err(),
            "crash recovery must reject symlinked ledgers"
        );
        assert_eq!(after, bytes, "recovery must not truncate a symlink target");
    }

    #[cfg(unix)]
    #[test]
    fn append_refuses_symlink_without_modifying_its_target() {
        use std::os::unix::fs::symlink;
        let fixture = PrivateTempDir::new("boole-append-link-test").expect("fixture");
        let target = fixture.path().join("unrelated");
        let ledger = fixture.path().join("ledger.ndjson");
        fs::write(&target, b"original\n").expect("write unrelated fixture");
        symlink(&target, &ledger).expect("plant symlink");

        let result = append_ndjson_line_durable(&ledger, "injected");
        assert!(result.is_err(), "append must not follow a ledger symlink");
        assert_eq!(fs::read(&target).expect("read target"), b"original\n");
    }

    #[cfg(unix)]
    #[test]
    fn append_and_recovery_refuse_hard_link_aliases_without_changes() {
        let fixture = PrivateTempDir::new("boole-hard-link-test").expect("fixture");
        let target = fixture.path().join("unrelated");
        let ledger = fixture.path().join("ledger.ndjson");
        let bytes = b"original\ntorn";
        fs::write(&target, bytes).expect("write fixture");
        fs::hard_link(&target, &ledger).expect("plant hard link");
        assert!(append_ndjson_line_durable(&ledger, "injected").is_err());
        assert!(read_stable_prefix(&ledger).is_err());
        assert_eq!(fs::read(&target).expect("read target"), bytes);
    }

    #[test]
    fn recovery_repairs_the_open_inode_after_pathname_replacement() {
        let fixture = PrivateTempDir::new("boole-recovery-inode-test").expect("fixture");
        let ledger = fixture.path().join("ledger.ndjson");
        let opened_ledger = fixture.path().join("opened-ledger.ndjson");
        fs::write(&ledger, b"stable\ntorn").expect("write ledger");
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        let mut file = open_regular_file(&ledger, &mut options).expect("open authoritative ledger");
        fs::rename(&ledger, &opened_ledger).expect("move opened inode");
        let replacement = b"replacement\nleave this partial tail unchanged";
        fs::write(&ledger, replacement).expect("replace pathname");

        assert_eq!(
            read_stable_prefix_on_file(&mut file).expect("repair opened inode"),
            "stable\n"
        );
        assert_eq!(fs::read(&opened_ledger).expect("opened inode"), b"stable\n");
        assert_eq!(fs::read(&ledger).expect("replacement inode"), replacement);
        append_ndjson_line_durable_on_file(&mut file, "next").expect("append opened inode");
        assert_eq!(
            fs::read(&opened_ledger).expect("opened inode"),
            b"stable\nnext\n"
        );
        assert_eq!(fs::read(&ledger).expect("replacement inode"), replacement);
    }

    #[cfg(unix)]
    #[test]
    fn proof_workspace_is_private_exclusive_and_cleaned_on_error() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let fixture = PrivateTempDir::new("boole-proof-workspace-test").expect("fixture");
        let unrelated = fixture.path().join("unrelated");
        fs::write(&unrelated, b"keep").expect("unrelated fixture");
        let workspace_path;
        {
            let workspace = PrivateTempDir::new_in(fixture.path(), "proof").expect("workspace");
            workspace_path = workspace.path().to_path_buf();
            assert_eq!(
                fs::metadata(&workspace_path)
                    .expect("workspace metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            let proof = workspace
                .write_proof(b"theorem t : True := by trivial")
                .expect("proof");
            assert_eq!(
                fs::metadata(&proof)
                    .expect("proof metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert!(
                workspace.write_proof(b"overwrite").is_err(),
                "proof file must be create-only"
            );
            fs::remove_file(&proof).expect("remove our proof fixture");
            symlink(&unrelated, &proof).expect("plant proof link");
            assert!(workspace.write_proof(b"overwrite target").is_err());
        }
        assert!(
            !workspace_path.exists(),
            "failed proof writes must still drop the workspace"
        );
        assert_eq!(
            fs::read(&unrelated).expect("read unrelated fixture"),
            b"keep"
        );
    }

    #[cfg(unix)]
    #[test]
    fn replaced_workspace_is_not_removed_by_the_original_guard() {
        let fixture = PrivateTempDir::new("boole-proof-drop-test").expect("fixture");
        let workspace = PrivateTempDir::new_in(fixture.path(), "proof").expect("workspace");
        let path = workspace.path().to_path_buf();
        let original = fixture.path().join("original");
        fs::rename(&path, &original).expect("move original workspace");
        fs::create_dir(&path).expect("replace workspace");
        fs::write(path.join("unrelated"), b"keep").expect("write replacement fixture");
        drop(workspace);
        assert_eq!(
            fs::read(path.join("unrelated")).expect("replacement survives"),
            b"keep"
        );
    }

    #[test]
    fn append_then_read_stable_prefix_returns_full_content() {
        let dir = std::env::temp_dir().join(format!(
            "boole-node-durability-append-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("tmp dir");
        let path = dir.join("ledger.ndjson");

        append_ndjson_line_durable(&path, "first").expect("durable append");
        append_ndjson_line_durable(&path, "second").expect("durable append");

        let raw = read_stable_prefix(&path)
            .expect("read stable prefix")
            .expect("file exists");
        assert_eq!(raw, "first\nsecond\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_stable_prefix_returns_none_for_missing_file() {
        let path = std::env::temp_dir().join(format!(
            "boole-node-durability-missing-{}-never-created.ndjson",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        assert!(read_stable_prefix(&path).expect("ok").is_none());
    }

    #[test]
    fn read_stable_prefix_truncates_torn_tail_on_disk() {
        let dir = std::env::temp_dir().join(format!(
            "boole-node-durability-truncate-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("tmp dir");
        let path = dir.join("ledger.ndjson");

        append_ndjson_line_durable(&path, "stable").expect("durable append");
        let stable_len = fs::metadata(&path).expect("metadata").len();

        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open for torn write")
            .write_all(b"torn-partial-without-newline")
            .expect("write partial");

        let raw = read_stable_prefix(&path)
            .expect("read stable prefix")
            .expect("file exists");
        assert_eq!(raw, "stable\n");
        assert_eq!(
            fs::metadata(&path).expect("post-truncate").len(),
            stable_len,
            "torn tail must be truncated on disk"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_only_inspection_rejects_torn_tail_without_truncating() {
        let dir = std::env::temp_dir().join(format!(
            "boole-node-durability-read-only-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("tmp dir");
        let path = dir.join("ledger.ndjson");
        let bytes = b"stable\ntorn-partial-without-newline";
        fs::write(&path, bytes).expect("write fixture");

        let err = read_complete_ndjson_read_only(&path).expect_err("torn tail must fail");
        assert!(err.to_string().contains("torn trailing line"));
        assert_eq!(fs::read(&path).expect("read after inspection"), bytes);

        let _ = fs::remove_dir_all(&dir);
    }
}
