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
//! - Proof files containing an unsound escape token (`sorry`, `axiom`, or
//!   `native_decide`) are rejected before the checker runs: Lean compiles
//!   `sorry` as a mere warning (returning success), trusts `axiom` blindly,
//!   and `native_decide` discharges goals outside the trusted kernel.

// P0.6b — boole-lean-runner is the trusted OS-syscall boundary: configuring
// rlimits via `pre_exec` and killing process groups requires `unsafe` libc
// calls. Every other workspace member inherits `[workspace.lints.rust]
// unsafe_code = "deny"` via `[lints] workspace = true`; this crate inherits
// the same opt-in for forward compatibility with future workspace lints but
// locally relaxes the unsafe deny here, keeping the carve-out documented in
// code rather than hidden in a manifest exception.
#![allow(unsafe_code)]

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
            memory_limit_mb: 8192,
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
        if let Some((token, line)) = scan_for_forbidden_tokens(proof_path)? {
            return Err(anyhow!(
                "Lean proof rejected: forbidden `{}` token at {}:{}",
                token,
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

        let mut child = ChildKillOnDrop::new(command.spawn().with_context(|| {
            format!(
                "failed to run lake exec {} in {}",
                self.config.checker_exe,
                self.config.package_dir.display()
            )
        })?);

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

/// P1.9 — tokens that make a Lean proof unsound and must be rejected
/// before the proof is ever handed to the checker:
///
/// - `sorry` admits any goal without proof;
/// - `axiom` introduces an unverified postulate the kernel trusts blindly;
/// - `native_decide` discharges a goal via native compiled code, outside
///   the trusted kernel.
///
/// Each token is matched on a word boundary (after line comments are
/// stripped), so identifiers that merely contain the substring
/// (`my_axiom_lemma`, `native_decide_helper`) are never flagged.
const FORBIDDEN_TOKENS: &[(&[u8], &str)] = &[
    (b"sorry", "sorry"),
    (b"axiom", "axiom"),
    (b"native_decide", "native_decide"),
];

/// Returns the first forbidden token found in `path` together with its
/// 1-based line number, or `None` if the proof is free of all of them.
fn scan_for_forbidden_tokens(path: &Path) -> Result<Option<(&'static str, usize)>> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read proof file {}", path.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    // Blank the bytes of line comments, nested block comments, and string
    // literals (preserving newlines so line numbers stay accurate) so the
    // forbidden-token scan only fires on real CODE. A `sorry`/`axiom`/
    // `native_decide` that appears inside a comment or string is
    // documentation, not an unsound declaration, and must not be rejected.
    let code = blank_non_code(&text);
    for (idx, line) in code.lines().enumerate() {
        for &(needle, name) in FORBIDDEN_TOKENS {
            if contains_forbidden_token(line, needle) {
                return Ok(Some((name, idx + 1)));
            }
        }
    }
    Ok(None)
}

/// Replace the bytes of Lean line comments (`-- … eol`), nested block
/// comments (`/- … -/`), and double-quoted string literals with spaces,
/// preserving newlines so 1-based line numbers stay accurate.
///
/// A single left-to-right pass tracks the lexical state so that, crucially,
/// `/-` inside a string and `"` inside a comment are NOT misinterpreted — a
/// naive two-pass strip would treat `"/-"` as a comment-open and blank the
/// real code that follows, a false negative that would let an unsound
/// `axiom` through. Char literals (`'c'`) are left as-is: a single char can
/// never be a forbidden multi-byte keyword, and `'` is also an identifier
/// suffix in Lean (`x'`), so treating it as a delimiter would mangle code.
/// Only ASCII delimiters are matched; UTF-8 multi-byte code bytes are copied
/// through verbatim (their bytes never collide with the ASCII delimiters).
fn blank_non_code(text: &str) -> String {
    let b = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    let mut block_depth: usize = 0;
    while i < b.len() {
        let c = b[i];
        if block_depth > 0 {
            if c == b'/' && i + 1 < b.len() && b[i + 1] == b'-' {
                block_depth += 1;
                out.push(b' ');
                out.push(b' ');
                i += 2;
                continue;
            }
            if c == b'-' && i + 1 < b.len() && b[i + 1] == b'/' {
                block_depth -= 1;
                out.push(b' ');
                out.push(b' ');
                i += 2;
                continue;
            }
            out.push(if c == b'\n' { b'\n' } else { b' ' });
            i += 1;
            continue;
        }
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'-' {
            block_depth = 1;
            out.push(b' ');
            out.push(b' ');
            i += 2;
            continue;
        }
        if c == b'-' && i + 1 < b.len() && b[i + 1] == b'-' {
            while i < b.len() && b[i] != b'\n' {
                out.push(b' ');
                i += 1;
            }
            continue;
        }
        if c == b'"' {
            out.push(b' ');
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' && i + 1 < b.len() {
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                    continue;
                }
                if b[i] == b'"' {
                    out.push(b' ');
                    i += 1;
                    break;
                }
                out.push(if b[i] == b'\n' { b'\n' } else { b' ' });
                i += 1;
            }
            continue;
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

fn contains_forbidden_token(line: &str, needle: &[u8]) -> bool {
    let bytes = line.as_bytes();
    if needle.is_empty() || bytes.len() < needle.len() {
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

/// Back-compat shim used by the `sorry` unit tests; production code calls
/// [`scan_for_forbidden_tokens`].
#[cfg(test)]
fn contains_sorry_token(line: &str) -> bool {
    contains_forbidden_token(line, b"sorry")
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

// P1.7 — defense-in-depth wrapper that SIGKILLs and reaps the wrapped
// child if the guard is dropped while the child is still running. This
// closes the leak window between `Command::spawn` and the normal
// `child.wait()` path in `check_proof`: an early `?` propagation, a
// panic, or an upstream task cancellation (axum TimeoutLayer dropping
// the future before our own timeout loop fires) would otherwise leave
// the lake/lean subprocess alive until its RLIMIT_CPU cap eventually
// trips minutes later.
//
// `Deref`/`DerefMut` proxy to the inner `Child` so the existing
// timeout-loop code (`child.stdout.take()`, `child.try_wait()`,
// `child.wait()`) compiles unchanged. The Drop path is a no-op once
// the child has been reaped normally: `try_wait` returns
// `Ok(Some(_))` and the SIGKILL branch is skipped.
pub(crate) struct ChildKillOnDrop(Option<Child>);

impl ChildKillOnDrop {
    pub(crate) fn new(child: Child) -> Self {
        Self(Some(child))
    }
}

impl std::ops::Deref for ChildKillOnDrop {
    type Target = Child;
    fn deref(&self) -> &Child {
        self.0
            .as_ref()
            .expect("child already taken from ChildKillOnDrop")
    }
}

impl std::ops::DerefMut for ChildKillOnDrop {
    fn deref_mut(&mut self) -> &mut Child {
        self.0
            .as_mut()
            .expect("child already taken from ChildKillOnDrop")
    }
}

impl Drop for ChildKillOnDrop {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            // try_wait surfaces the same status the caller's wait()
            // returned earlier; only SIGKILL when the child is still
            // unreaped (Ok(None)) or its state is unreadable.
            let still_running = matches!(child.try_wait(), Ok(None) | Err(_));
            if still_running {
                kill_child_group(&mut child);
                let _ = child.wait();
            }
        }
    }
}

// Files the artifact hash always pins, in order. Anything outside this list
// must come from the recursive `BooleCheck/**` walk below.
// `Boole/Family/V0Helpers.lean` is pinned explicitly (D#6): proof files
// `import Boole.Family.V0Helpers`, so a tampered helper must be visible in
// the hash even though it lives outside `BooleCheck/`.
const CHECKER_PINNED_FILES: &[&str] = &[
    "lean-toolchain",
    "lakefile.lean",
    "lake-manifest.json",
    "Boole/Family/V0Helpers.lean",
];

/// SHA-256 over the checker package's pinned files plus every source under
/// `BooleCheck/**`. Public so tests and operator tooling can recompute the
/// hash with the EXACT production formula instead of mirroring it.
pub fn checker_artifact_hash(package_dir: &Path) -> Result<String> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for relative in CHECKER_PINNED_FILES {
        let path = package_dir.join(relative);
        let bytes = std::fs::read(&path)
            .with_context(|| format!("failed to read checker artifact {}", path.display()))?;
        entries.push(((*relative).to_string(), bytes));
    }
    collect_boole_check_sources(package_dir, &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    for (relative, bytes) in &entries {
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(bytes);
        hasher.update([0]);
    }
    Ok(hex::encode(hasher.finalize()))
}

// Walk `BooleCheck/**` and collect every file the checker source tree owns.
// The walk is deterministic (sorted by relative path during hashing) and
// rejects symlinks so an operator cannot smuggle a file in via a symlink that
// resolves outside the package.
fn collect_boole_check_sources(package_dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<()> {
    let root = package_dir.join("BooleCheck");
    if !root.exists() {
        return Ok(());
    }
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("failed to read checker dir {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let metadata = entry
                .metadata()
                .with_context(|| format!("failed to stat {}", path.display()))?;
            if metadata.file_type().is_symlink() {
                return Err(anyhow!(
                    "symlink not allowed inside checker package: {}",
                    path.display()
                ));
            }
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            let bytes = std::fs::read(&path)
                .with_context(|| format!("failed to read checker source {}", path.display()))?;
            let relative = path
                .strip_prefix(package_dir)
                .with_context(|| format!("path {} not inside package", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, bytes));
        }
    }
    Ok(())
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
        assert_eq!(cfg.memory_limit_mb, 8192);
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
        // blank_non_code blanks the `-- sorry` so the scan finds nothing.
        assert!(!contains_forbidden_token(
            &blank_non_code("foo -- sorry"),
            b"sorry"
        ));
    }

    #[test]
    fn ignores_forbidden_tokens_in_block_comments() {
        let code = blank_non_code(
            "/- this proof is axiom-independent and avoids native_decide -/\n\
             theorem t : True := trivial\n",
        );
        assert!(!contains_forbidden_token(&code, b"axiom"));
        assert!(!contains_forbidden_token(&code, b"native_decide"));
        assert!(!contains_forbidden_token(&code, b"sorry"));
    }

    #[test]
    fn ignores_forbidden_tokens_in_string_literals() {
        let code = blank_non_code(r#"def msg : String := "axiom is not permitted here""#);
        assert!(!contains_forbidden_token(&code, b"axiom"));
    }

    #[test]
    fn block_comment_open_inside_string_does_not_swallow_following_code() {
        // `/-` inside a string must NOT start a block comment that would blank
        // the real `axiom` on the next line (a false negative / unsound).
        let code = blank_non_code("def s : String := \"/-\"\naxiom sneaky : False\n");
        let line2 = code.lines().nth(1).unwrap_or("");
        assert!(
            contains_forbidden_token(line2, b"axiom"),
            "a real axiom after a string containing /- must still be caught; line2={line2:?}"
        );
    }

    #[test]
    fn real_forbidden_token_in_code_survives_blanking() {
        assert!(contains_forbidden_token(
            &blank_non_code("axiom bad : False\n"),
            b"axiom"
        ));
        assert!(contains_forbidden_token(
            &blank_non_code("theorem t : True := by native_decide\n"),
            b"native_decide"
        ));
    }

    #[test]
    fn detects_axiom_token() {
        assert!(contains_forbidden_token("axiom foo : 1 = 2", b"axiom"));
        assert!(contains_forbidden_token("  axiom", b"axiom"));
    }

    #[test]
    fn ignores_axiom_inside_identifiers() {
        assert!(!contains_forbidden_token("my_axiom_lemma", b"axiom"));
        assert!(!contains_forbidden_token("axiomFoo", b"axiom"));
        assert!(!contains_forbidden_token("Nat.axiomatic", b"axiom"));
    }

    #[test]
    fn detects_native_decide_token() {
        assert!(contains_forbidden_token(
            "by native_decide",
            b"native_decide"
        ));
        assert!(contains_forbidden_token("native_decide", b"native_decide"));
    }

    #[test]
    fn ignores_native_decide_inside_identifiers() {
        assert!(!contains_forbidden_token(
            "native_decide_helper",
            b"native_decide"
        ));
        assert!(!contains_forbidden_token(
            "my_native_decide",
            b"native_decide"
        ));
    }

    #[test]
    fn check_file_rejects_axiom_before_lake_spawn() {
        // A real (empty) package dir lets `check_file` pass its `is_dir`
        // precondition and reach the pre-spawn forbidden-token scan; the
        // error must name the token, proving the scan fires before any
        // `lake` invocation (so this test needs no lean toolchain).
        let dir = std::env::temp_dir().join(format!(
            "boole-fbscan-axiom-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).expect("create temp package dir");
        let proof = dir.join("Proof.lean");
        std::fs::write(&proof, "theorem t : True := by\n  axiom sneaky : False\n")
            .expect("write proof");
        let runner = LeanRunner::new(LeanRunnerConfig::new("test").with_package_dir(&dir));
        let err = runner
            .check_file(&proof)
            .expect_err("axiom must be rejected");
        assert!(
            err.to_string().contains("axiom"),
            "error should name the forbidden token, got: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_file_rejects_native_decide_before_lake_spawn() {
        let dir = std::env::temp_dir().join(format!(
            "boole-fbscan-nd-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).expect("create temp package dir");
        let proof = dir.join("Proof.lean");
        std::fs::write(&proof, "theorem t : True := by native_decide\n").expect("write proof");
        let runner = LeanRunner::new(LeanRunnerConfig::new("test").with_package_dir(&dir));
        let err = runner
            .check_file(&proof)
            .expect_err("native_decide must be rejected");
        assert!(
            err.to_string().contains("native_decide"),
            "error should name the forbidden token, got: {err}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // P1.7 — `ChildKillOnDrop` is the defense-in-depth backstop that
    // prevents a lake/lean subprocess from leaking when the calling
    // function returns early — e.g., axum's `TimeoutLayer` drops the
    // future before the timeout-loop reaches `kill_child_group`, or a
    // mid-function `?` propagates an unrelated error. Without it, the
    // child stays alive until its `RLIMIT_CPU` cap fires (could be
    // minutes); with it, dropping the guard SIGKILLs the whole process
    // group and reaps the zombie.
    //
    // We test the guard by spawning `/bin/sleep 60`, dropping the
    // guard, and confirming the pid is gone (`kill(pid, 0)` returns
    // ESRCH). The 60-second sleep gives the test plenty of slack on a
    // slow CI box without relying on wall-clock timing.
    #[cfg(unix)]
    #[test]
    fn child_kill_on_drop_kills_orphaned_unix_child() {
        let mut cmd = Command::new("/bin/sleep");
        cmd.arg("60");
        let child = cmd.spawn().expect("spawn sleep child");
        let pid = child.id() as libc::pid_t;
        {
            let _guard = ChildKillOnDrop::new(child);
            // guard dropped at end of scope -> SIGKILL + wait
        }
        // Give the kernel a few ms to deliver SIGKILL and update the
        // process table. Polling is bounded to ~500ms so a regression
        // (drop did not kill) surfaces as a real failure, not a hang.
        let mut still_alive = true;
        for _ in 0..50 {
            let rc = unsafe { libc::kill(pid, 0) };
            if rc == -1 {
                let err = std::io::Error::last_os_error().raw_os_error();
                if err == Some(libc::ESRCH) {
                    still_alive = false;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !still_alive,
            "ChildKillOnDrop must SIGKILL+reap the child on Drop; pid \
             {pid} still exists"
        );
    }

    // If the caller drains the child normally via `wait()`, dropping
    // the guard afterward must be a no-op — try_wait should observe
    // the already-reaped status and skip the kill path so we don't
    // double-wait on a zombie that no longer exists.
    #[cfg(unix)]
    #[test]
    fn child_kill_on_drop_is_noop_after_wait() {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg("exit 0");
        let child = cmd.spawn().expect("spawn /bin/sh");
        let mut guard = ChildKillOnDrop::new(child);
        let status = guard.wait().expect("wait child");
        assert!(status.success());
        // Drop runs at end of scope; the assertion is simply that we
        // don't panic or hang. Drop's `try_wait` returns
        // `Ok(Some(status))` so the SIGKILL branch never fires.
    }
}
