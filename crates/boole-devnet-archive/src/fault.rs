//! Fault dispatch exists only in explicit qualification-faults builds.
//! Ordinary entrypoints refuse the selector environment before taking a lock.
#[cfg(feature = "qualification-faults")]
use anyhow::Context;
use anyhow::Result;
#[cfg(feature = "qualification-faults")]
use std::io::Write;
use std::path::Path;
pub fn validate_environment() -> Result<()> {
    if !cfg!(feature = "qualification-faults")
        && std::env::vars_os().any(|(key, _)| key.to_string_lossy().starts_with("BOOLE_M4_KILL_"))
    {
        return Err(crate::error::integrity(0,"fault-hooks","BOOLE_M4_KILL_* requires an explicit qualification-faults build; ordinary binaries refuse these variables"));
    }
    Ok(())
}
#[cfg(not(feature = "qualification-faults"))]
pub fn enabled(_point: &str, _height: u64, _role: &str) -> bool {
    false
}
#[cfg(not(feature = "qualification-faults"))]
pub fn hit(_point: &str, _height: u64, _role: &str) -> Result<()> {
    validate_environment()
}
#[cfg(not(feature = "qualification-faults"))]
pub(crate) fn partial_append(
    _file: &mut std::fs::File,
    _line: &str,
    _path: Option<&Path>,
) -> Result<()> {
    Ok(())
}
#[cfg(feature = "qualification-faults")]
pub fn enabled(point: &str, height: u64, role: &str) -> bool {
    std::env::var("BOOLE_M4_KILL_POINT").ok().as_deref() == Some(point)
        && std::env::var("BOOLE_M4_KILL_HEIGHT")
            .ok()
            .is_some_and(|h| h == "*" || h.parse::<u64>().ok() == Some(height))
        && std::env::var("BOOLE_M4_KILL_ROLE").ok().as_deref() == Some(role)
}
#[cfg(feature = "qualification-faults")]
#[allow(unsafe_code)] // SIGSTOP is restricted to explicitly selected local kill qualification.
pub fn hit(point: &str, height: u64, role: &str) -> Result<()> {
    if !enabled(point, height, role) {
        return Ok(());
    }
    let marker =
        std::env::var_os("BOOLE_M4_KILL_MARKER").context("qualification marker required")?;
    let marker = Path::new(&marker);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(marker)?;
    serde_json::to_writer(
        &mut file,
        &serde_json::json!({"point":point,"height":height,"role":role,"pid":std::process::id()}),
    )?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    crate::durability::fsync_parent_dir(marker)?;
    // The coordinator sends SIGKILL to this exact owned PID. Never continue a
    // suspended qualification process as a normal writer.
    #[cfg(unix)]
    unsafe {
        libc::raise(libc::SIGSTOP);
    }
    anyhow::bail!("qualification process was unexpectedly resumed")
}
#[cfg(feature = "qualification-faults")]
pub(crate) fn partial_append(
    file: &mut std::fs::File,
    line: &str,
    path: Option<&Path>,
) -> Result<()> {
    if std::env::var("BOOLE_M4_KILL_POINT").ok().as_deref() != Some("B10") {
        return Ok(());
    }
    let value: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let (height, role) = if path
        .and_then(Path::file_name)
        .is_some_and(|n| n == "input.ndjson")
    {
        let row: crate::input::InputRow = serde_json::from_value(value)?;
        (row.block()?.number, "input")
    } else if value["outcome"] == "DERIVED" {
        (
            value["height"].as_u64().context("evidence height")?,
            "evidence",
        )
    } else {
        return Ok(());
    };
    if enabled("B10", height, role) {
        file.write_all(&line.as_bytes()[..line.len() / 2])?;
        file.flush()?;
        hit("B10", height, role)?;
    }
    Ok(())
}

/// A qualification coordinator may read the marker only after SIGSTOP. Mere
/// pathname presence races the writer's serialize/fsync sequence.
pub fn stopped_marker(path: &Path, pid: u32) -> Result<Option<serde_json::Value>> {
    if !path.try_exists()? {
        return Ok(None);
    }
    anyhow::ensure!(
        pid > 1 && pid != std::process::id(),
        "qualification child PID"
    );
    let status = std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "state="])
        .output()?;
    if !status.status.success() || !std::str::from_utf8(&status.stdout)?.trim().starts_with('T') {
        return Ok(None);
    }
    let (bytes, _) = crate::journal::read_bound(path, 65536)?;
    let marker: serde_json::Value = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        marker["pid"].as_u64() == Some(u64::from(pid)),
        "stopped marker PID mismatch"
    );
    Ok(Some(marker))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_partial_marker_cannot_authorize_killing_a_running_child() -> Result<()> {
        let dir = crate::durability::PrivateTempDir::new("m4-marker")?;
        let p = dir.path().join("marker.json");
        std::fs::write(&p, b"{\"")?;
        let mut child = std::process::Command::new("/bin/cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()?;
        let result = stopped_marker(&p, child.id());
        let _ = child.kill();
        let _ = child.wait();
        assert!(result?.is_none());
        Ok(())
    }
}
