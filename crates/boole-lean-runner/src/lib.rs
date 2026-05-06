//! Rust wrapper seam for Lean verifier execution.
//!
//! Lean remains the proof/checking truth source. This crate provides a small,
//! deterministic process boundary around `lake exec boole_check <proof.lean>`
//! and returns an evidence envelope that can be recorded by Boole runtime code.
//!
//! Hardening:
//! - The child runs in its own process group; on timeout the whole group is
//!   sent SIGKILL so `lake`'s spawned `lean` compiler cannot survive as an
//!   orphan.
//! - Stdout/stderr are drained on dedicated threads with a per-stream byte
//!   cap, so the child can never block on a full pipe (default 64 KiB) and
//!   stall the timeout poll loop.
//! - On Unix, RLIMIT_AS / RLIMIT_CPU / RLIMIT_FSIZE / RLIMIT_NOFILE are
//!   applied via `pre_exec` so `memory_limit_mb` is a real constraint, not a
//!   recorded-but-unenforced number.
//! - The child environment is wiped (`env_clear`) and a minimal PATH/HOME is
//!   restored so parent secrets do not leak into the untrusted Lean process.
//! - Proof files containing an unguarded `sorry` token are rejected before
//!   the checker runs; Lean's checker compiles `sorry` as a warning and would
//!   otherwise return success.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeanRunnerConfig {
    pub verifier_hash: String,
    pub package_dir: PathBuf,
    pub checker_exe: String,
    pub timeout_ms: u64,
    pub memory_limit_mb: u64,
    pub output_limit_bytes: usize,
}

impl LeanRunnerConfig {
    pub fn new(verifier_hash: impl Into<String>) -> Self {
        Self {
            verifier_hash: verifier_hash.into(),
            package_dir: PathBuf::from("."),
            checker_exe: "boole_check".to_string(),
            timeout_ms: 10_000,
            memory_limit_mb: 512,
            output_limit_bytes: 64 * 1024,
        }
    }

    pub fn with_package_dir(mut self, package_dir: impl Into<PathBuf>) -> Self {
        self.package_dir = package_dir.into();
        self
    }

    pub fn with_checker_exe(mut self, checker_exe: impl Into<String>) -> Self {
        self.checker_exe = checker_exe.into();
        self
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    pub fn with_memory_limit_mb(mut self, memory_limit_mb: u64) -> Self {
        self.memory_limit_mb = memory_limit_mb;
        self
    }

    pub fn with_output_limit_bytes(mut self, output_limit_bytes: usize) -> Self {
        self.output_limit_bytes = output_limit_bytes;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeanRunnerEvidence {
    pub verifier_hash: String,
    pub checker: String,
    pub checker_exe: String,
    pub checker_artifact_hash: String,
    pub package_dir: String,
    pub lean_version: String,
    pub lake_version: String,
    pub timeout_ms: u64,
    pub memory_limit_mb: u64,
    pub output_limit_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeanCheckResult {
    pub accepted: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub output_truncated: bool,
    pub evidence: LeanRunnerEvidence,
}

#[derive(Debug, Clone)]
pub struct LeanRunner {
    config: LeanRunnerConfig,
}

impl LeanRunner {
    pub fn new(config: LeanRunnerConfig) -> Self {
        Self { config }
    }

    pub fn check_file(&self, proof_path: impl AsRef<Path>) -> Result<LeanCheckResult> {
        let proof_path = proof_path.as_ref();
        if !proof_path.is_file() {
            return Err(anyhow!(
                "Lean proof file does not exist: {}",
                proof_path.display()
            ));
        }
        if !self.config.package_dir.is_dir() {
            return Err(anyhow!(
                "Lean package directory does not exist: {}",
                self.config.package_dir.display()
            ));
        }
        if let Some(line) = scan_for_sorry(proof_path)? {
            return Err(anyhow!(
                "Lean proof rejected: unguarded `sorry` token at {}:{}",
                proof_path.display(),
                line
            ));
        }

        let evidence = self.evidence()?;
        let mut command = Command::new("lake");
        command
            .arg("exec")
            .arg(&self.config.checker_exe)
            .arg(proof_path)
            .current_dir(&self.config.package_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_child_environment(&mut command);
        configure_child_sandbox(&mut command, &self.config);

        let mut child = command.spawn().with_context(|| {
            format!(
                "failed to run lake exec {} in {}",
                self.config.checker_exe,
                self.config.package_dir.display()
            )
        })?;

        let output_limit = self.config.output_limit_bytes;
        let stdout_pipe = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("child stdout was not captured"))?;
        let stderr_pipe = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("child stderr was not captured"))?;
        let stdout_buf: Arc<Mutex<DrainBuffer>> =
            Arc::new(Mutex::new(DrainBuffer::new(output_limit)));
        let stderr_buf: Arc<Mutex<DrainBuffer>> =
            Arc::new(Mutex::new(DrainBuffer::new(output_limit)));
        let stdout_handle = spawn_drain(stdout_pipe, Arc::clone(&stdout_buf));
        let stderr_handle = spawn_drain(stderr_pipe, Arc::clone(&stderr_buf));

        let deadline = Instant::now() + Duration::from_millis(self.config.timeout_ms);
        let timed_out = loop {
            match child.try_wait()? {
                Some(_) => break false,
                None => {
                    if Instant::now() >= deadline {
                        kill_child_group(&mut child);
                        // Reap the (now killed) child so wait_with_output below
                        // doesn't hang waiting for an already-collected exit.
                        let _ = child.wait();
                        break true;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
            }
        };

        let output_status = child.wait()?;
        // Drain threads exit when the child closes its pipe ends. After the
        // child is reaped, EOF is delivered and the threads finish.
        let _ = stdout_handle.join();
        let _ = stderr_handle.join();

        let stdout_drain = Arc::try_unwrap(stdout_buf)
            .ok()
            .ok_or_else(|| anyhow!("stdout buffer still shared"))?
            .into_inner()
            .map_err(|err| anyhow!("stdout mutex poisoned: {err}"))?;
        let stderr_drain = Arc::try_unwrap(stderr_buf)
            .ok()
            .ok_or_else(|| anyhow!("stderr buffer still shared"))?
            .into_inner()
            .map_err(|err| anyhow!("stderr mutex poisoned: {err}"))?;

        let mut stdout = String::from_utf8_lossy(&stdout_drain.bytes).to_string();
        let mut stderr = String::from_utf8_lossy(&stderr_drain.bytes).to_string();
        let mut stdout_truncated = stdout_drain.truncated;
        let mut stderr_truncated = stderr_drain.truncated;
        if timed_out {
            if !stderr.is_empty() && !stderr.ends_with('\n') {
                stderr.push('\n');
            }
            stderr.push_str(&format!(
                "lean runner timeout after {}ms",
                self.config.timeout_ms
            ));
        }
        // After appending the timeout marker stderr may have grown past the
        // limit; re-truncate to keep the recorded byte cap honest.
        stdout_truncated |= truncate_utf8_to_bytes(&mut stdout, output_limit);
        stderr_truncated |= truncate_utf8_to_bytes(&mut stderr, output_limit);

        Ok(LeanCheckResult {
            accepted: !timed_out && output_status.success(),
            exit_code: if timed_out {
                -1
            } else {
                output_status.code().unwrap_or(-1)
            },
            stdout,
            stderr,
            timed_out,
            output_truncated: stdout_truncated || stderr_truncated,
            evidence,
        })
    }

    pub fn evidence(&self) -> Result<LeanRunnerEvidence> {
        Ok(LeanRunnerEvidence {
            verifier_hash: self.config.verifier_hash.clone(),
            checker: format!("lake exec {}", self.config.checker_exe),
            checker_exe: self.config.checker_exe.clone(),
            checker_artifact_hash: checker_artifact_hash(&self.config.package_dir)?,
            package_dir: self.config.package_dir.display().to_string(),
            lean_version: command_version("lean")?,
            lake_version: command_version("lake")?,
            timeout_ms: self.config.timeout_ms,
            memory_limit_mb: self.config.memory_limit_mb,
            output_limit_bytes: self.config.output_limit_bytes,
        })
    }
}

struct DrainBuffer {
    bytes: Vec<u8>,
    limit: usize,
    truncated: bool,
}

impl DrainBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            truncated: false,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        if self.bytes.len() >= self.limit {
            self.truncated = true;
            return;
        }
        let remaining = self.limit - self.bytes.len();
        if chunk.len() > remaining {
            self.bytes.extend_from_slice(&chunk[..remaining]);
            self.truncated = true;
        } else {
            self.bytes.extend_from_slice(chunk);
        }
    }
}

fn spawn_drain<R>(mut reader: R, sink: Arc<Mutex<DrainBuffer>>) -> thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut guard) = sink.lock() {
                        guard.push(&chunk[..n]);
                    } else {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    })
}

fn truncate_utf8_to_bytes(value: &mut String, limit: usize) -> bool {
    if value.len() <= limit {
        return false;
    }
    if limit == 0 {
        value.clear();
        return true;
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    true
}

fn scan_for_sorry(path: &Path) -> Result<Option<usize>> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read proof file {}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    for (idx, raw_line) in text.lines().enumerate() {
        let line = strip_line_comment(raw_line);
        if contains_sorry_token(line) {
            return Ok(Some(idx + 1));
        }
    }
    Ok(None)
}

fn strip_line_comment(line: &str) -> &str {
    if let Some(pos) = line.find("--") {
        &line[..pos]
    } else {
        line
    }
}

fn contains_sorry_token(line: &str) -> bool {
    let bytes = line.as_bytes();
    let needle = b"sorry";
    if bytes.len() < needle.len() {
        return false;
    }
    for start in 0..=(bytes.len() - needle.len()) {
        if &bytes[start..start + needle.len()] != needle {
            continue;
        }
        let before = if start == 0 {
            None
        } else {
            Some(bytes[start - 1])
        };
        let after = bytes.get(start + needle.len()).copied();
        let is_word_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        if before.map(is_word_char).unwrap_or(false) {
            continue;
        }
        if after.map(is_word_char).unwrap_or(false) {
            continue;
        }
        return true;
    }
    false
}

fn configure_child_environment(command: &mut Command) {
    command.env_clear();
    // A minimal PATH covering common locations for `lake`/`lean` on macOS and
    // Linux developer machines. Operators that install Lean elsewhere can set
    // BOOLE_LEAN_PATH to override.
    let path = std::env::var("BOOLE_LEAN_PATH")
        .ok()
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_else(|| "/usr/local/bin:/usr/bin:/bin".to_string());
    command.env("PATH", path);
    if let Ok(home) = std::env::var("HOME") {
        command.env("HOME", home);
    }
    command.env("LANG", "C.UTF-8");
}

#[cfg(unix)]
fn configure_child_sandbox(command: &mut Command, config: &LeanRunnerConfig) {
    use std::os::unix::process::CommandExt;
    // On Boole's supported Unix dev/test targets libc::rlim_t is u64, matching
    // the config fields, so no lossy cast is needed here.
    let mem_bytes: libc::rlim_t = config.memory_limit_mb.saturating_mul(1024 * 1024);
    let cpu_seconds: libc::rlim_t = (config.timeout_ms / 1000) + 5;
    // 256 MiB ceiling on any single file the child writes — it should not be
    // writing artifacts at runtime, so this is a defence-in-depth cap.
    let fsize_bytes: libc::rlim_t = 256 * 1024 * 1024;
    // 1024 file descriptors: lake spawns multiple subprocesses and reads many
    // .olean files. A tighter cap (e.g. 256) trips lake on real workloads.
    let nofile: libc::rlim_t = 1024;
    unsafe {
        command.pre_exec(move || {
            // Run in our own process group so the parent can SIGKILL the
            // entire group on timeout (lake -> lean child).
            if libc::setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            apply_address_space_rlimit(mem_bytes)?;
            set_rlimit(libc::RLIMIT_CPU, cpu_seconds)?;
            set_rlimit(libc::RLIMIT_FSIZE, fsize_bytes)?;
            set_rlimit(libc::RLIMIT_NOFILE, nofile)?;
            Ok(())
        });
    }
}

// `RLIMIT_AS` is the right knob on Linux and is the only reliable way to bound
// a Lean process's memory footprint there. On macOS the kernel rejects
// `setrlimit(RLIMIT_AS, ...)` with EINVAL: the constant is defined as an alias
// for `RLIMIT_RSS` but is not enforceable, and `RLIMIT_DATA` is also a no-op on
// Darwin. We therefore skip the address-space limit on macOS and rely on the
// wall-clock timeout + RLIMIT_CPU to bound runaway proofs.
#[cfg(all(unix, target_os = "linux"))]
unsafe fn apply_address_space_rlimit(mem_bytes: libc::rlim_t) -> std::io::Result<()> {
    set_rlimit(libc::RLIMIT_AS, mem_bytes)?;
    set_rlimit(libc::RLIMIT_DATA, mem_bytes)?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "linux")))]
unsafe fn apply_address_space_rlimit(_mem_bytes: libc::rlim_t) -> std::io::Result<()> {
    Ok(())
}

// libc exposes `setrlimit` with a platform-dependent first argument
// (`__rlimit_resource_t` on Linux, `c_int` on macOS/BSD). The constants like
// `RLIMIT_AS` already match that platform type, so we propagate it through a
// generic helper rather than spell it out per-OS.
#[cfg(unix)]
unsafe fn set_rlimit<R>(resource: R, value: libc::rlim_t) -> std::io::Result<()>
where
    R: SetRlimitArg,
{
    let limit = libc::rlimit {
        rlim_cur: value,
        rlim_max: value,
    };
    if resource.call(&limit) != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
trait SetRlimitArg: Copy {
    unsafe fn call(self, limit: &libc::rlimit) -> libc::c_int;
}

#[cfg(all(unix, target_os = "linux"))]
impl SetRlimitArg for libc::__rlimit_resource_t {
    unsafe fn call(self, limit: &libc::rlimit) -> libc::c_int {
        libc::setrlimit(self, limit)
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
impl SetRlimitArg for libc::c_int {
    unsafe fn call(self, limit: &libc::rlimit) -> libc::c_int {
        libc::setrlimit(self, limit)
    }
}

#[cfg(not(unix))]
fn configure_child_sandbox(_command: &mut Command, _config: &LeanRunnerConfig) {}

#[cfg(unix)]
fn kill_child_group(child: &mut Child) {
    let pid = child.id() as libc::pid_t;
    // Try to SIGKILL the whole process group first; fall back to the single
    // pid if the group call fails (e.g. setpgid never ran).
    unsafe {
        if libc::killpg(pid, libc::SIGKILL) != 0 {
            let _ = child.kill();
        }
    }
}

#[cfg(not(unix))]
fn kill_child_group(child: &mut Child) {
    let _ = child.kill();
}

fn checker_artifact_hash(package_dir: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    for relative in ["lakefile.lean", "BooleCheck/Main.lean"] {
        let path = package_dir.join(relative);
        let bytes = std::fs::read(&path)
            .with_context(|| format!("failed to read checker artifact {}", path.display()))?;
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(bytes);
        hasher.update([0]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn command_version(command: &str) -> Result<String> {
    let output = Command::new(command)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to execute `{command} --version`"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "`{} --version` failed: {}",
            command,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_records_verifier_hash() {
        let cfg = LeanRunnerConfig::new("abc");
        assert_eq!(cfg.verifier_hash, "abc");
        assert_eq!(cfg.checker_exe, "boole_check");
        assert_eq!(cfg.timeout_ms, 10_000);
        assert_eq!(cfg.memory_limit_mb, 512);
        assert_eq!(cfg.output_limit_bytes, 64 * 1024);
    }

    #[test]
    fn truncate_zero_limit_clears_string() {
        let mut s = String::from("héllo");
        let truncated = truncate_utf8_to_bytes(&mut s, 0);
        assert!(truncated);
        assert_eq!(s, "");
    }

    #[test]
    fn truncate_respects_char_boundary() {
        let mut s = String::from("héllo");
        let limit = s.len() - 1;
        let truncated = truncate_utf8_to_bytes(&mut s, limit);
        assert!(truncated);
        assert!(s.is_char_boundary(s.len()));
    }

    #[test]
    fn detects_sorry_token() {
        assert!(contains_sorry_token("  exact sorry"));
        assert!(contains_sorry_token("sorry"));
        assert!(contains_sorry_token("by sorry  "));
    }

    #[test]
    fn ignores_sorry_inside_identifiers() {
        assert!(!contains_sorry_token("notSorry"));
        assert!(!contains_sorry_token("sorry_lemma"));
        assert!(!contains_sorry_token("MySorry"));
    }

    #[test]
    fn ignores_sorry_in_line_comment() {
        assert!(!contains_sorry_token(strip_line_comment("foo -- sorry")));
    }
}
