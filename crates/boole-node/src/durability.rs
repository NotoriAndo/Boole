use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

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
    let is_new_file = !path.exists();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", line)?;
    file.flush()?;
    file.sync_all()?;
    if is_new_file {
        fsync_parent_dir(path)?;
    }
    Ok(())
}

/// Read the file at `path`, truncate any torn trailing line on disk, and
/// return the stable bytes as a UTF-8 string ready for line-by-line replay.
/// Returns `None` when the file does not exist so callers can short-circuit
/// to a fresh ledger.
pub(crate) fn read_stable_prefix(path: &Path) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw_bytes = fs::read(path)?;
    let stable_len = stable_jsonl_prefix_len(&raw_bytes);
    if stable_len < raw_bytes.len() {
        OpenOptions::new()
            .write(true)
            .open(path)?
            .set_len(stable_len as u64)?;
    }
    let raw = String::from_utf8(raw_bytes[..stable_len].to_vec())?;
    Ok(Some(raw))
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
}
