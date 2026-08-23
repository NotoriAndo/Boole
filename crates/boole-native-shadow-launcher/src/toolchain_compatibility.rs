//! Fixed, pre-bind toolchain compatibility verification.
//!
//! This proof is deliberately narrower than installed-byte provenance or
//! execution activation. It can only follow completed startup cgroup recovery.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::time::Duration;

use thiserror::Error;

use crate::startup_recovery::VerifiedStartupCgroupRecovery;

/// Opaque proof that the four fixed compatibility probes matched.
///
/// It is not a readiness, provenance, listener, or execution proof.
///
/// ```compile_fail
/// use boole_native_shadow_launcher::toolchain_compatibility::VerifiedStartupToolchainCompatibility;
/// let _forged = VerifiedStartupToolchainCompatibility {};
/// ```
#[must_use]
#[allow(dead_code)]
pub struct VerifiedStartupToolchainCompatibility {
    recovery: VerifiedStartupCgroupRecovery,
    #[cfg(target_os = "linux")]
    installed_files: InstalledToolchainFiles,
}

impl VerifiedStartupToolchainCompatibility {
    pub(crate) fn recovery(&self) -> &VerifiedStartupCgroupRecovery {
        &self.recovery
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn reverify_for_execution(&self) -> Result<(), ToolchainProbeFailure> {
        self.recovery
            .verify_cgroup_state_after_trusted_probes()
            .map_err(ToolchainProbeFailure::ManagerDrift)?;
        let observed = verify_fixed_installed_paths()?;
        require_same_installed_toolchain_files(self.installed_files, observed)
    }
}

pub(crate) const PROBE_DEADLINE: Duration = Duration::from_secs(10);
pub(crate) const STREAM_LIMIT: usize = 65_536;
pub(crate) const FIXED_ENVIRONMENT: [(&str, &str); 4] = [
    ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin"),
    ("LC_ALL", "C"),
    ("LANG", "C"),
    ("TZ", "UTC"),
];

const RUSTC_RELEASE: &str = "1.99.0-nightly";
const RUSTC_COMMIT: &str = "e7795af6d2449fb05a6393c3320ced873a999eb3";
const CARGO_RELEASE: &str = "1.99.0-nightly";
const CARGO_COMMIT: &str = "3efb1f477e99b42974b982d939fd100303cdf7db";
const PYTHON_VERSION_PREFIX: &str = "Python 3.12.";
const PYTHON_IMPLEMENTATION: &str = "cpython\n";

#[derive(Clone, Debug, Error)]
pub enum ToolchainProbeFailure {
    #[error("toolchain probe platform failure: {0}")]
    Platform(String),
    #[error("toolchain probe timed out")]
    Timeout,
    #[error("toolchain probe stdout exceeded {limit} bytes")]
    StdoutOverflow { limit: usize },
    #[error("toolchain probe stderr exceeded {limit} bytes")]
    StderrOverflow { limit: usize },
    #[error("toolchain probe exited unsuccessfully")]
    NonZeroExit,
    #[error("toolchain probe {stream} was not valid UTF-8")]
    InvalidUtf8 { stream: &'static str },
    #[error("toolchain probe wrote unexpected stderr")]
    UnexpectedStderr,
    #[error("toolchain probe output mismatch: {0}")]
    OutputMismatch(String),
    #[error("unsafe installed toolchain path: {0}")]
    UnsafeInstalledPath(String),
    #[error("manager cgroup drift after toolchain probes: {0}")]
    ManagerDrift(String),
}

#[derive(Debug, Error)]
pub enum ToolchainCompatibilityError {
    #[error("native-shadow toolchain compatibility verification requires Linux")]
    UnsupportedPlatform,
    #[error("toolchain compatibility verification failed after manager movement during {probe}: {failure}")]
    PostMoveFatal {
        probe: &'static str,
        #[source]
        failure: ToolchainProbeFailure,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProbeRequest {
    pub(crate) program: &'static str,
    pub(crate) arguments: Vec<&'static str>,
    pub(crate) cwd: &'static str,
    pub(crate) environment: [(&'static str, &'static str); 4],
    pub(crate) deadline: Duration,
    pub(crate) stdout_limit: usize,
    pub(crate) stderr_limit: usize,
}

#[derive(Debug)]
pub(crate) struct ProbeExecution {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) exit_success: bool,
}

pub(crate) trait ProbeRunner {
    fn run(&mut self, request: &ProbeRequest) -> Result<ProbeExecution, ToolchainProbeFailure>;
}

#[derive(Clone, Copy)]
enum ProbeExpectation {
    RustVersion {
        tool: &'static str,
        release: &'static str,
        commit: &'static str,
    },
    PythonVersionPrefix(&'static str),
    Exact(&'static str),
}

#[derive(Clone, Copy)]
struct FixedProbe {
    label: &'static str,
    program: &'static str,
    arguments: &'static [&'static str],
    expectation: ProbeExpectation,
}

const FIXED_PROBES: [FixedProbe; 4] = [
    FixedProbe {
        label: "rustc compatibility probe",
        program: "/opt/boole/native-checker-toolchain/bin/rustc",
        arguments: &["-vV"],
        expectation: ProbeExpectation::RustVersion {
            tool: "rustc",
            release: RUSTC_RELEASE,
            commit: RUSTC_COMMIT,
        },
    },
    FixedProbe {
        label: "cargo compatibility probe",
        program: "/opt/boole/native-checker-toolchain/bin/cargo",
        arguments: &["-Vv"],
        expectation: ProbeExpectation::RustVersion {
            tool: "cargo",
            release: CARGO_RELEASE,
            commit: CARGO_COMMIT,
        },
    },
    FixedProbe {
        label: "python version compatibility probe",
        program: "/usr/bin/python3.12",
        arguments: &["--version"],
        expectation: ProbeExpectation::PythonVersionPrefix(PYTHON_VERSION_PREFIX),
    },
    FixedProbe {
        label: "python implementation compatibility probe",
        program: "/usr/bin/python3.12",
        arguments: &[
            "-I",
            "-S",
            "-c",
            "import sys;print(sys.implementation.name)",
        ],
        expectation: ProbeExpectation::Exact(PYTHON_IMPLEMENTATION),
    },
];

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InstalledFileIdentity {
    device: u64,
    inode: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    hard_links: u64,
    size: u64,
}

#[cfg(any(target_os = "linux", test))]
impl InstalledFileIdentity {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;

        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            mode: metadata.mode(),
            uid: metadata.uid(),
            gid: metadata.gid(),
            hard_links: metadata.nlink(),
            size: metadata.size(),
        }
    }
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InstalledToolchainFiles {
    rustc: InstalledFileIdentity,
    cargo: InstalledFileIdentity,
    python: InstalledFileIdentity,
}

#[cfg(any(target_os = "linux", test))]
impl InstalledToolchainFiles {
    fn for_program(&self, program: &str) -> Option<InstalledFileIdentity> {
        match program {
            "/opt/boole/native-checker-toolchain/bin/rustc" => Some(self.rustc),
            "/opt/boole/native-checker-toolchain/bin/cargo" => Some(self.cargo),
            "/usr/bin/python3.12" => Some(self.python),
            _ => None,
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn require_same_installed_toolchain_files(
    expected: InstalledToolchainFiles,
    observed: InstalledToolchainFiles,
) -> Result<(), ToolchainProbeFailure> {
    if expected == observed {
        Ok(())
    } else {
        Err(ToolchainProbeFailure::UnsafeInstalledPath(
            "installed executable identity changed after startup verification".to_string(),
        ))
    }
}

pub(crate) fn verify_probe_sequence<R: ProbeRunner>(
    runner: &mut R,
) -> Result<(), ToolchainCompatibilityError> {
    for probe in FIXED_PROBES {
        let request = ProbeRequest {
            program: probe.program,
            arguments: probe.arguments.to_vec(),
            cwd: "/",
            environment: FIXED_ENVIRONMENT,
            deadline: PROBE_DEADLINE,
            stdout_limit: STREAM_LIMIT,
            stderr_limit: STREAM_LIMIT,
        };
        let execution = runner
            .run(&request)
            .map_err(|failure| fatal(probe.label, failure))?;
        validate_execution(execution, probe.expectation)
            .map_err(|failure| fatal(probe.label, failure))?;
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
pub(crate) struct ProcessProbeRunner {
    expected_files: Option<InstalledToolchainFiles>,
}

#[cfg(any(target_os = "linux", test))]
impl ProcessProbeRunner {
    fn fixed(expected_files: InstalledToolchainFiles) -> Self {
        Self {
            expected_files: Some(expected_files),
        }
    }

    #[cfg(test)]
    fn unbound_for_tests() -> Self {
        Self {
            expected_files: None,
        }
    }
}

#[cfg(any(target_os = "linux", test))]
impl ProbeRunner for ProcessProbeRunner {
    fn run(&mut self, request: &ProbeRequest) -> Result<ProbeExecution, ToolchainProbeFailure> {
        let expected_file = self
            .expected_files
            .as_ref()
            .and_then(|files| files.for_program(request.program));
        process_runner::run(request, expected_file)
    }
}

#[cfg(any(target_os = "linux", test))]
mod process_runner {
    use std::io::{self, Read};
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{InstalledFileIdentity, ProbeExecution, ProbeRequest, ToolchainProbeFailure};

    pub(super) fn run(
        request: &ProbeRequest,
        expected_file: Option<InstalledFileIdentity>,
    ) -> Result<ProbeExecution, ToolchainProbeFailure> {
        #[cfg(target_os = "linux")]
        let bound_program = expected_file
            .map(|expected| open_bound_program(request.program, expected))
            .transpose()?;
        #[cfg(target_os = "linux")]
        let program = bound_program
            .as_ref()
            .map_or_else(|| request.program.to_string(), BoundProgram::proc_path);
        #[cfg(not(target_os = "linux"))]
        let _ = expected_file;
        #[cfg(not(target_os = "linux"))]
        let program = request.program.to_string();

        let mut command = Command::new(&program);
        command
            .args(&request.arguments)
            .current_dir(request.cwd)
            .env_clear()
            .envs(request.environment)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command.spawn().map_err(|source| {
            ToolchainProbeFailure::Platform(format!(
                "spawn fixed probe {}: {source}",
                request.program
            ))
        })?;
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                terminate_process_group_and_reap(&mut child)?;
                return Err(ToolchainProbeFailure::Platform(
                    "fixed probe stdout pipe missing".to_string(),
                ));
            }
        };
        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                terminate_process_group_and_reap(&mut child)?;
                return Err(ToolchainProbeFailure::Platform(
                    "fixed probe stderr pipe missing".to_string(),
                ));
            }
        };
        if let Err(failure) = set_nonblocking(&stdout, "stdout") {
            terminate_process_group_and_reap(&mut child)?;
            return Err(failure);
        }
        if let Err(failure) = set_nonblocking(&stderr, "stderr") {
            terminate_process_group_and_reap(&mut child)?;
            return Err(failure);
        }
        let mut stdout = BoundedPipe::stdout(stdout, request.stdout_limit);
        let mut stderr = BoundedPipe::stderr(stderr, request.stderr_limit);
        let deadline = Instant::now() + request.deadline;
        let mut exit_status = None;

        loop {
            if let Err(failure) = stdout.drain() {
                terminate_process_group_and_reap(&mut child)?;
                return Err(failure);
            }
            if let Err(failure) = stderr.drain() {
                terminate_process_group_and_reap(&mut child)?;
                return Err(failure);
            }
            if stdout.eof && stderr.eof && exit_status.is_none() {
                match child.try_wait() {
                    Ok(status) => exit_status = status,
                    Err(source) => {
                        let failure = ToolchainProbeFailure::Platform(format!(
                            "poll fixed probe {}: {source}",
                            request.program
                        ));
                        terminate_process_group_and_reap(&mut child)?;
                        return Err(failure);
                    }
                }
            }
            if let Some(status) = exit_status.filter(|_| stdout.eof && stderr.eof) {
                return Ok(ProbeExecution {
                    stdout: stdout.bytes,
                    stderr: stderr.bytes,
                    exit_success: status.success(),
                });
            }
            if Instant::now() >= deadline {
                terminate_process_group_and_reap(&mut child)?;
                return Err(ToolchainProbeFailure::Timeout);
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    enum ProbePipe {
        Stdout(ChildStdout),
        Stderr(ChildStderr),
    }

    impl Read for ProbePipe {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            match self {
                Self::Stdout(reader) => reader.read(buffer),
                Self::Stderr(reader) => reader.read(buffer),
            }
        }
    }

    struct BoundedPipe {
        pipe: ProbePipe,
        bytes: Vec<u8>,
        limit: usize,
        stream: &'static str,
        eof: bool,
    }

    impl BoundedPipe {
        fn stdout(pipe: ChildStdout, limit: usize) -> Self {
            Self::new(ProbePipe::Stdout(pipe), limit, "stdout")
        }

        fn stderr(pipe: ChildStderr, limit: usize) -> Self {
            Self::new(ProbePipe::Stderr(pipe), limit, "stderr")
        }

        fn new(pipe: ProbePipe, limit: usize, stream: &'static str) -> Self {
            Self {
                pipe,
                bytes: Vec::with_capacity(limit.min(8192)),
                limit,
                stream,
                eof: false,
            }
        }

        fn drain(&mut self) -> Result<(), ToolchainProbeFailure> {
            if self.eof {
                return Ok(());
            }
            let mut buffer = [0_u8; 8192];
            loop {
                match self.pipe.read(&mut buffer) {
                    Ok(0) => {
                        self.eof = true;
                        return Ok(());
                    }
                    Ok(count) => {
                        if self.bytes.len().saturating_add(count) > self.limit {
                            return Err(if self.stream == "stdout" {
                                ToolchainProbeFailure::StdoutOverflow { limit: self.limit }
                            } else {
                                ToolchainProbeFailure::StderrOverflow { limit: self.limit }
                            });
                        }
                        self.bytes.extend_from_slice(&buffer[..count]);
                    }
                    Err(source) if source.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                    Err(source) if source.kind() == io::ErrorKind::Interrupted => continue,
                    Err(source) => {
                        return Err(ToolchainProbeFailure::Platform(format!(
                            "read fixed probe {}: {source}",
                            self.stream
                        )));
                    }
                }
            }
        }
    }

    #[allow(unsafe_code)]
    fn set_nonblocking<T: AsRawFd>(pipe: &T, stream: &str) -> Result<(), ToolchainProbeFailure> {
        // SAFETY: `pipe` owns a live pipe descriptor for the duration of both calls.
        let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
        if flags < 0 {
            return Err(ToolchainProbeFailure::Platform(format!(
                "read fixed probe {stream} descriptor flags: {}",
                io::Error::last_os_error()
            )));
        }
        // SAFETY: the descriptor remains live and `flags | O_NONBLOCK` is a valid F_SETFL value.
        if unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } != 0 {
            return Err(ToolchainProbeFailure::Platform(format!(
                "set fixed probe {stream} nonblocking: {}",
                io::Error::last_os_error()
            )));
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    struct BoundProgram {
        file: std::fs::File,
    }

    #[cfg(target_os = "linux")]
    impl BoundProgram {
        fn proc_path(&self) -> String {
            format!("/proc/self/fd/{}", self.file.as_raw_fd())
        }
    }

    #[cfg(target_os = "linux")]
    fn open_bound_program(
        path: &str,
        expected: InstalledFileIdentity,
    ) -> Result<BoundProgram, ToolchainProbeFailure> {
        use std::os::unix::fs::OpenOptionsExt;

        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|source| {
                ToolchainProbeFailure::UnsafeInstalledPath(format!(
                    "open descriptor-bound executable {path}: {source}"
                ))
            })?;
        let actual = InstalledFileIdentity::from_metadata(&file.metadata().map_err(|source| {
            ToolchainProbeFailure::UnsafeInstalledPath(format!(
                "inspect descriptor-bound executable {path}: {source}"
            ))
        })?);
        if actual != expected {
            return Err(ToolchainProbeFailure::UnsafeInstalledPath(format!(
                "descriptor-bound executable identity changed: {path}"
            )));
        }
        Ok(BoundProgram { file })
    }

    #[allow(unsafe_code)]
    fn terminate_process_group_and_reap(child: &mut Child) -> Result<(), ToolchainProbeFailure> {
        let process_group = -(child.id() as i32);
        // SAFETY: CommandExt::process_group(0) made the child the leader of this group.
        let result = unsafe { libc::kill(process_group, libc::SIGKILL) };
        if result != 0 {
            let source = io::Error::last_os_error();
            if source.raw_os_error() != Some(libc::ESRCH) {
                child.kill().map_err(|fallback| {
                    ToolchainProbeFailure::Platform(format!(
                        "kill fixed probe process group failed ({source}); leader fallback failed: {fallback}"
                    ))
                })?;
            }
        }
        child.wait().map_err(|source| {
            ToolchainProbeFailure::Platform(format!("reap fixed probe process: {source}"))
        })?;
        Ok(())
    }
}

fn validate_execution(
    execution: ProbeExecution,
    expectation: ProbeExpectation,
) -> Result<(), ToolchainProbeFailure> {
    if !execution.exit_success {
        return Err(ToolchainProbeFailure::NonZeroExit);
    }
    std::str::from_utf8(&execution.stderr)
        .map_err(|_| ToolchainProbeFailure::InvalidUtf8 { stream: "stderr" })?;
    if !execution.stderr.is_empty() {
        return Err(ToolchainProbeFailure::UnexpectedStderr);
    }
    let stdout = std::str::from_utf8(&execution.stdout)
        .map_err(|_| ToolchainProbeFailure::InvalidUtf8 { stream: "stdout" })?;
    match expectation {
        ProbeExpectation::RustVersion {
            tool,
            release,
            commit,
        } => validate_rust_version_output(stdout, tool, release, commit),
        ProbeExpectation::PythonVersionPrefix(prefix) => {
            let line = exact_single_lf_line(stdout, "Python version")?;
            let patch = line.strip_prefix(prefix).ok_or_else(|| {
                ToolchainProbeFailure::OutputMismatch(
                    "Python version output has the wrong fixed prefix".to_string(),
                )
            })?;
            if patch.is_empty() || !patch.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(ToolchainProbeFailure::OutputMismatch(
                    "Python version output lacks a concrete numeric patch version".to_string(),
                ));
            }
            Ok(())
        }
        ProbeExpectation::Exact(expected) => {
            if stdout == expected {
                Ok(())
            } else {
                Err(ToolchainProbeFailure::OutputMismatch(
                    "exact output differs".to_string(),
                ))
            }
        }
    }
}

fn validate_rust_version_output(
    output: &str,
    tool: &'static str,
    release: &str,
    commit: &str,
) -> Result<(), ToolchainProbeFailure> {
    if output.contains('\r') || !output.ends_with('\n') {
        return Err(ToolchainProbeFailure::OutputMismatch(
            "Rust version output is not exact LF-terminated text".to_string(),
        ));
    }
    let lines = output
        .strip_suffix('\n')
        .expect("checked suffix")
        .split('\n')
        .collect::<Vec<_>>();
    if lines.iter().any(|line| line.is_empty()) {
        return Err(ToolchainProbeFailure::OutputMismatch(
            "Rust version output contains an empty line".to_string(),
        ));
    }
    let field = |index: usize, name: &str| -> Result<&str, ToolchainProbeFailure> {
        lines
            .get(index)
            .and_then(|line| line.strip_prefix(&format!("{name}: ")))
            .ok_or_else(|| {
                ToolchainProbeFailure::OutputMismatch(format!(
                    "Rust version output is missing ordered field {name:?}"
                ))
            })
    };
    let short_commit = commit.get(..9).ok_or_else(|| {
        ToolchainProbeFailure::OutputMismatch("fixed commit hash is too short".to_string())
    })?;
    let commit_date = field(3, "commit-date")?;
    require_iso_date(commit_date)?;
    let expected_banner = format!("{tool} {release} ({short_commit} {commit_date})");
    if lines.first().copied() != Some(expected_banner.as_str()) {
        return Err(ToolchainProbeFailure::OutputMismatch(
            "Rust version banner differs from its fixed identity fields".to_string(),
        ));
    }

    match tool {
        "rustc" => {
            if lines.len() != 7
                || field(1, "binary")? != "rustc"
                || field(2, "commit-hash")? != commit
                || field(4, "host")? != "x86_64-unknown-linux-gnu"
                || field(5, "release")? != release
                || field(6, "LLVM version")?.trim().is_empty()
            {
                return Err(ToolchainProbeFailure::OutputMismatch(
                    "rustc -vV output differs from the fixed seven-line schema".to_string(),
                ));
            }
        }
        "cargo" => {
            if !(lines.len() == 8 || lines.len() == 9)
                || field(1, "release")? != release
                || field(2, "commit-hash")? != commit
                || field(4, "host")? != "x86_64-unknown-linux-gnu"
                || field(5, "libgit2")?.trim().is_empty()
                || field(6, "libcurl")?.trim().is_empty()
            {
                return Err(ToolchainProbeFailure::OutputMismatch(
                    "cargo -Vv output differs from the fixed schema".to_string(),
                ));
            }
            let os_index = if lines.len() == 9 {
                if field(7, "ssl")?.trim().is_empty() {
                    return Err(ToolchainProbeFailure::OutputMismatch(
                        "cargo ssl field is empty".to_string(),
                    ));
                }
                8
            } else {
                7
            };
            if field(os_index, "os")?.trim().is_empty() {
                return Err(ToolchainProbeFailure::OutputMismatch(
                    "cargo os field is empty".to_string(),
                ));
            }
        }
        _ => {
            return Err(ToolchainProbeFailure::OutputMismatch(
                "unknown fixed Rust tool expectation".to_string(),
            ));
        }
    }
    Ok(())
}

fn exact_single_lf_line<'a>(
    output: &'a str,
    label: &str,
) -> Result<&'a str, ToolchainProbeFailure> {
    if output.contains('\r') || !output.ends_with('\n') {
        return Err(ToolchainProbeFailure::OutputMismatch(format!(
            "{label} output is not exact LF-terminated text"
        )));
    }
    let line = output.strip_suffix('\n').expect("checked suffix");
    if line.is_empty() || line.contains('\n') {
        return Err(ToolchainProbeFailure::OutputMismatch(format!(
            "{label} output is not exactly one nonempty line"
        )));
    }
    Ok(line)
}

fn require_iso_date(value: &str) -> Result<(), ToolchainProbeFailure> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return Err(ToolchainProbeFailure::OutputMismatch(
            "Rust commit date is not exact YYYY-MM-DD".to_string(),
        ));
    }
    Ok(())
}

fn fatal(probe: &'static str, failure: ToolchainProbeFailure) -> ToolchainCompatibilityError {
    ToolchainCompatibilityError::PostMoveFatal { probe, failure }
}

#[cfg(target_os = "linux")]
fn verify_fixed_installed_paths() -> Result<InstalledToolchainFiles, ToolchainProbeFailure> {
    verify_installed_paths_beneath(std::path::Path::new("/"), 0, 0)
}

#[cfg(any(target_os = "linux", test))]
fn verify_installed_paths_beneath(
    root: &std::path::Path,
    required_uid: u32,
    required_gid: u32,
) -> Result<InstalledToolchainFiles, ToolchainProbeFailure> {
    fn metadata(path: &std::path::Path) -> Result<std::fs::Metadata, ToolchainProbeFailure> {
        std::fs::symlink_metadata(path).map_err(|source| {
            ToolchainProbeFailure::UnsafeInstalledPath(format!(
                "inspect {}: {source}",
                path.display()
            ))
        })
    }

    fn verify_directory(
        path: &std::path::Path,
        required_uid: u32,
        required_gid: u32,
        exact_mode: Option<u32>,
    ) -> Result<(), ToolchainProbeFailure> {
        use std::os::unix::fs::MetadataExt;

        let metadata = metadata(path)?;
        let mode = metadata.mode() & 0o7777;
        if !metadata.file_type().is_dir() {
            return Err(unsafe_path(path, "component is not a nonsymlink directory"));
        }
        if metadata.uid() != required_uid || metadata.gid() != required_gid {
            return Err(unsafe_path(path, "directory owner/group differs"));
        }
        if mode & 0o022 != 0 {
            return Err(unsafe_path(path, "directory is group/other writable"));
        }
        if exact_mode.is_some_and(|expected| mode != expected) {
            return Err(unsafe_path(
                path,
                "directory mode differs from fixed contract",
            ));
        }
        Ok(())
    }

    fn verify_executable(
        path: &std::path::Path,
        required_uid: u32,
        required_gid: u32,
        exact_mode: Option<u32>,
    ) -> Result<InstalledFileIdentity, ToolchainProbeFailure> {
        use std::os::unix::fs::MetadataExt;

        let metadata = metadata(path)?;
        let mode = metadata.mode() & 0o7777;
        if !metadata.file_type().is_file() {
            return Err(unsafe_path(
                path,
                "executable is not a nonsymlink regular file",
            ));
        }
        if metadata.nlink() != 1 {
            return Err(unsafe_path(path, "executable must have one hard link"));
        }
        if metadata.uid() != required_uid || metadata.gid() != required_gid {
            return Err(unsafe_path(path, "executable owner/group differs"));
        }
        if mode & 0o022 != 0 || mode & 0o100 == 0 || mode & 0o7000 != 0 {
            return Err(unsafe_path(
                path,
                "executable is writable outside owner, lacks owner execute, or has special bits",
            ));
        }
        if exact_mode.is_some_and(|expected| mode != expected) {
            return Err(unsafe_path(
                path,
                "executable mode differs from fixed contract",
            ));
        }
        Ok(InstalledFileIdentity::from_metadata(&metadata))
    }

    let directories = [
        ("", None),
        ("opt", None),
        ("opt/boole", None),
        ("opt/boole/native-checker-toolchain", Some(0o555)),
        ("opt/boole/native-checker-toolchain/bin", Some(0o555)),
        ("usr", None),
        ("usr/bin", None),
    ];
    for (relative, exact_mode) in directories {
        let path = if relative.is_empty() {
            root.to_path_buf()
        } else {
            root.join(relative)
        };
        verify_directory(&path, required_uid, required_gid, exact_mode)?;
    }
    let rustc = verify_executable(
        &root.join("opt/boole/native-checker-toolchain/bin/rustc"),
        required_uid,
        required_gid,
        None,
    )?;
    let cargo = verify_executable(
        &root.join("opt/boole/native-checker-toolchain/bin/cargo"),
        required_uid,
        required_gid,
        None,
    )?;
    let python = verify_executable(
        &root.join("usr/bin/python3.12"),
        required_uid,
        required_gid,
        Some(0o755),
    )?;
    Ok(InstalledToolchainFiles {
        rustc,
        cargo,
        python,
    })
}

#[cfg(any(target_os = "linux", test))]
fn unsafe_path(path: &std::path::Path, reason: &str) -> ToolchainProbeFailure {
    ToolchainProbeFailure::UnsafeInstalledPath(format!("{}: {reason}", path.display()))
}

/// Consume startup recovery and verify the fixed toolchain identities.
///
/// No executable, argument, environment, timeout, or output limit is caller
/// selected.
pub fn verify_fixed_startup_toolchain_compatibility(
    recovery: VerifiedStartupCgroupRecovery,
) -> Result<VerifiedStartupToolchainCompatibility, ToolchainCompatibilityError> {
    #[cfg(target_os = "linux")]
    {
        let initial_files = verify_fixed_installed_paths();
        let compatibility = match &initial_files {
            Ok(files) => {
                let mut runner = ProcessProbeRunner::fixed(*files);
                verify_probe_sequence(&mut runner)
            }
            Err(failure) => Err(fatal(
                "verify fixed installed toolchain paths",
                failure.clone(),
            )),
        };
        let post_paths = verify_fixed_installed_paths();
        let post_cgroup = recovery.verify_cgroup_state_after_trusted_probes();
        if let Err(reason) = post_cgroup {
            return Err(fatal(
                "reverify service and manager cgroups after trusted probes",
                ToolchainProbeFailure::ManagerDrift(reason),
            ));
        }
        let post_files = post_paths.map_err(|failure| {
            fatal(
                "reverify fixed installed toolchain paths after trusted probes",
                failure,
            )
        })?;
        if let Ok(initial_files) = initial_files {
            if initial_files != post_files {
                return Err(fatal(
                    "reverify fixed installed toolchain paths after trusted probes",
                    ToolchainProbeFailure::UnsafeInstalledPath(
                        "installed executable identity changed during trusted probes".to_string(),
                    ),
                ));
            }
        }
        compatibility?;
        Ok(VerifiedStartupToolchainCompatibility {
            recovery,
            installed_files: post_files,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        drop(recovery);
        Err(ToolchainCompatibilityError::UnsupportedPlatform)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs;
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        require_same_installed_toolchain_files, validate_execution, verify_installed_paths_beneath,
        verify_probe_sequence,
        InstalledToolchainFiles, ProbeExecution, ProbeExpectation, ProbeRequest, ProbeRunner,
        ProcessProbeRunner, ToolchainCompatibilityError, ToolchainProbeFailure, CARGO_COMMIT,
        CARGO_RELEASE, FIXED_ENVIRONMENT, FIXED_PROBES, PROBE_DEADLINE, PYTHON_IMPLEMENTATION,
        PYTHON_VERSION_PREFIX, RUSTC_COMMIT, RUSTC_RELEASE, STREAM_LIMIT,
    };

    const RUSTC_OK: &[u8] = b"rustc 1.99.0-nightly (e7795af6d 2026-07-22)\n\
binary: rustc\n\
commit-hash: e7795af6d2449fb05a6393c3320ced873a999eb3\n\
commit-date: 2026-07-22\n\
host: x86_64-unknown-linux-gnu\n\
release: 1.99.0-nightly\n\
LLVM version: 21.1.0\n";
    const CARGO_OK: &[u8] = b"cargo 1.99.0-nightly (3efb1f477 2026-07-21)\n\
release: 1.99.0-nightly\n\
commit-hash: 3efb1f477e99b42974b982d939fd100303cdf7db\n\
commit-date: 2026-07-21\n\
host: x86_64-unknown-linux-gnu\n\
libgit2: 1.9.1 (sys:0.20.2 vendored)\n\
libcurl: 8.14.1-DEV (sys:0.4.82+curl-8.14.1 vendored ssl:OpenSSL/1.1.1w)\n\
ssl: OpenSSL 1.1.1w  11 Sep 2023\n\
os: Ubuntu 24.04 (noble) [64-bit]\n";
    static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

    struct InstalledTree {
        root: PathBuf,
        uid: u32,
        gid: u32,
    }

    impl InstalledTree {
        fn new() -> Self {
            let suffix = NEXT_TREE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "boole-native-toolchain-paths-{}-{suffix}",
                std::process::id()
            ));
            fs::create_dir_all(root.join("opt/boole/native-checker-toolchain/bin"))
                .expect("toolchain test directories");
            fs::create_dir_all(root.join("usr/bin")).expect("Python test directories");
            for path in [
                root.join("opt/boole/native-checker-toolchain/bin/rustc"),
                root.join("opt/boole/native-checker-toolchain/bin/cargo"),
                root.join("usr/bin/python3.12"),
            ] {
                fs::write(&path, b"fixture executable").expect("test executable write");
                set_mode(&path, 0o755);
            }
            set_mode(&root.join("opt/boole/native-checker-toolchain"), 0o555);
            set_mode(&root.join("opt/boole/native-checker-toolchain/bin"), 0o555);
            let metadata = fs::metadata(&root).expect("test root metadata");
            Self {
                root,
                uid: metadata.uid(),
                gid: metadata.gid(),
            }
        }

        fn verify(&self) -> Result<InstalledToolchainFiles, ToolchainProbeFailure> {
            verify_installed_paths_beneath(&self.root, self.uid, self.gid)
        }

        fn path(&self, relative: &str) -> PathBuf {
            self.root.join(relative)
        }
    }

    impl Drop for InstalledTree {
        fn drop(&mut self) {
            let _ = set_mode_if_present(
                &self.root.join("opt/boole/native-checker-toolchain/bin"),
                0o755,
            );
            let _ =
                set_mode_if_present(&self.root.join("opt/boole/native-checker-toolchain"), 0o755);
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn set_mode(path: &Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .expect("test mode must be settable");
    }

    fn set_mode_if_present(path: &Path, mode: u32) -> std::io::Result<()> {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }

    #[derive(Clone)]
    enum Outcome {
        Output(&'static [u8]),
        Failure(ToolchainProbeFailure),
    }

    struct FakeRunner {
        outcomes: VecDeque<Outcome>,
        requests: Vec<ProbeRequest>,
    }

    impl FakeRunner {
        fn happy() -> Self {
            Self {
                outcomes: VecDeque::from([
                    Outcome::Output(RUSTC_OK),
                    Outcome::Output(CARGO_OK),
                    Outcome::Output(b"Python 3.12.11\n"),
                    Outcome::Output(b"cpython\n"),
                ]),
                requests: Vec::new(),
            }
        }
    }

    impl ProbeRunner for FakeRunner {
        fn run(&mut self, request: &ProbeRequest) -> Result<ProbeExecution, ToolchainProbeFailure> {
            self.requests.push(request.clone());
            match self.outcomes.pop_front().expect("one outcome per probe") {
                Outcome::Output(stdout) => Ok(ProbeExecution {
                    stdout: stdout.to_vec(),
                    stderr: Vec::new(),
                    exit_success: true,
                }),
                Outcome::Failure(failure) => Err(failure),
            }
        }
    }

    #[test]
    fn exact_fixed_probe_matrix_runs_in_order_and_withholds_success_on_first_failure() {
        let mut happy = FakeRunner::happy();
        verify_probe_sequence(&mut happy).expect("all exact fixed probes should match");

        assert_eq!(happy.requests.len(), 4);
        assert_eq!(
            happy.requests[0].program,
            "/opt/boole/native-checker-toolchain/bin/rustc"
        );
        assert_eq!(happy.requests[0].arguments, ["-vV"]);
        assert_eq!(
            happy.requests[1].program,
            "/opt/boole/native-checker-toolchain/bin/cargo"
        );
        assert_eq!(happy.requests[1].arguments, ["-Vv"]);
        assert_eq!(happy.requests[2].program, "/usr/bin/python3.12");
        assert_eq!(happy.requests[2].arguments, ["--version"]);
        assert_eq!(happy.requests[3].program, "/usr/bin/python3.12");
        assert_eq!(
            happy.requests[3].arguments,
            [
                "-I",
                "-S",
                "-c",
                "import sys;print(sys.implementation.name)"
            ]
        );
        for request in &happy.requests {
            assert_eq!(request.cwd, "/");
            assert_eq!(request.environment, FIXED_ENVIRONMENT);
            assert_eq!(request.deadline, PROBE_DEADLINE);
            assert_eq!(request.stdout_limit, STREAM_LIMIT);
            assert_eq!(request.stderr_limit, STREAM_LIMIT);
        }

        let failures = [
            ToolchainProbeFailure::OutputMismatch("wrong release".to_string()),
            ToolchainProbeFailure::Timeout,
            ToolchainProbeFailure::StdoutOverflow {
                limit: STREAM_LIMIT,
            },
            ToolchainProbeFailure::StderrOverflow {
                limit: STREAM_LIMIT,
            },
        ];
        for (index, failure) in failures.into_iter().enumerate() {
            let mut runner = FakeRunner::happy();
            runner.outcomes[index] = Outcome::Failure(failure);
            let error =
                verify_probe_sequence(&mut runner).expect_err("failure must withhold proof");
            assert!(matches!(
                error,
                ToolchainCompatibilityError::PostMoveFatal { .. }
            ));
            assert_eq!(runner.requests.len(), index + 1, "no later probe may run");
        }
    }

    #[test]
    fn process_runner_enforces_deadline_and_separate_output_caps() {
        let mut runner = ProcessProbeRunner::unbound_for_tests();
        let environment_request = ProbeRequest {
            program: "/usr/bin/env",
            arguments: Vec::new(),
            cwd: "/",
            environment: FIXED_ENVIRONMENT,
            deadline: std::time::Duration::from_secs(1),
            stdout_limit: STREAM_LIMIT,
            stderr_limit: STREAM_LIMIT,
        };
        let execution = runner
            .run(&environment_request)
            .expect("fixed clean environment should run");
        let mut lines = String::from_utf8(execution.stdout)
            .expect("env output should be UTF-8")
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>();
        lines.sort();
        let mut expected = FIXED_ENVIRONMENT
            .map(|(key, value)| format!("{key}={value}"))
            .to_vec();
        expected.sort();
        assert_eq!(lines, expected);
        assert!(execution.stderr.is_empty());
        assert!(execution.exit_success);

        for (script, expected_failure) in [
            ("while :; do :; done", ToolchainProbeFailure::Timeout),
            (
                "i=0; while [ \"$i\" -lt 17 ]; do printf x; i=$((i+1)); done",
                ToolchainProbeFailure::StdoutOverflow { limit: 16 },
            ),
            (
                "i=0; while [ \"$i\" -lt 17 ]; do printf x >&2; i=$((i+1)); done",
                ToolchainProbeFailure::StderrOverflow { limit: 16 },
            ),
        ] {
            let request = ProbeRequest {
                program: "/bin/sh",
                arguments: vec!["-c", script],
                cwd: "/",
                environment: FIXED_ENVIRONMENT,
                deadline: std::time::Duration::from_millis(50),
                stdout_limit: 16,
                stderr_limit: 16,
            };
            let failure = runner.run(&request).expect_err("limit must fail closed");
            assert_eq!(failure.to_string(), expected_failure.to_string());
        }

        let inherited_pipe_request = ProbeRequest {
            program: "/bin/sh",
            arguments: vec!["-c", "(while :; do sleep 1; done) & exit 0"],
            cwd: "/",
            environment: FIXED_ENVIRONMENT,
            deadline: std::time::Duration::from_millis(50),
            stdout_limit: 16,
            stderr_limit: 16,
        };
        assert!(matches!(
            runner.run(&inherited_pipe_request),
            Err(ToolchainProbeFailure::Timeout)
        ));
    }

    #[test]
    fn installed_paths_reject_writable_or_symlinked_components() {
        let tree = InstalledTree::new();
        let original = tree
            .verify()
            .expect("exact installed path shape should pass");

        let bin = tree.path("opt/boole/native-checker-toolchain/bin");
        set_mode(&bin, 0o755);
        assert!(matches!(
            tree.verify(),
            Err(ToolchainProbeFailure::UnsafeInstalledPath(_))
        ));
        set_mode(&bin, 0o555);

        let rustc = tree.path("opt/boole/native-checker-toolchain/bin/rustc");
        set_mode(&rustc, 0o775);
        assert!(matches!(
            tree.verify(),
            Err(ToolchainProbeFailure::UnsafeInstalledPath(_))
        ));
        set_mode(&rustc, 0o755);

        set_mode(&bin, 0o755);
        fs::remove_file(&rustc).expect("replace rustc fixture inode");
        fs::write(&rustc, b"replacement executable").expect("write replacement fixture");
        set_mode(&rustc, 0o755);
        set_mode(&bin, 0o555);
        let replacement = tree
            .verify()
            .expect("replacement still has a safe path shape");
        assert_ne!(
            original.rustc, replacement.rustc,
            "the executable snapshot must detect an inode replacement"
        );

        let python = tree.path("usr/bin/python3.12");
        fs::remove_file(&python).expect("replace Python fixture");
        symlink("rustc", &python).expect("create final-component symlink");
        assert!(matches!(
            tree.verify(),
            Err(ToolchainProbeFailure::UnsafeInstalledPath(_))
        ));
    }

    #[test]
    fn execution_time_toolchain_recheck_rejects_any_identity_drift() {
        let tree = InstalledTree::new();
        let startup = tree.verify().expect("startup file identities");
        require_same_installed_toolchain_files(startup, startup)
            .expect("unchanged identities remain valid");

        let rustc = tree.path("opt/boole/native-checker-toolchain/bin/rustc");
        let bin = tree.path("opt/boole/native-checker-toolchain/bin");
        set_mode(&bin, 0o755);
        fs::remove_file(&rustc).expect("replace rustc fixture inode");
        fs::write(&rustc, b"replacement executable").expect("write replacement fixture");
        set_mode(&rustc, 0o755);
        set_mode(&bin, 0o555);
        let execution = tree.verify().expect("safe replacement path shape");
        assert!(matches!(
            require_same_installed_toolchain_files(startup, execution),
            Err(ToolchainProbeFailure::UnsafeInstalledPath(_))
        ));
    }

    #[test]
    fn fixed_probe_constants_match_the_tracked_identity_manifest() {
        let manifest: serde_json::Value =
            serde_json::from_slice(boole_native_shadow_protocol::TRACKED_TOOLCHAIN_IDENTITY_BYTES)
                .expect("tracked toolchain identity is valid JSON");
        let rust = &manifest["rust"];
        assert_eq!(rust["rustcRelease"], RUSTC_RELEASE);
        assert_eq!(rust["rustcCommitHash"], RUSTC_COMMIT);
        assert_eq!(rust["cargoRelease"], CARGO_RELEASE);
        assert_eq!(rust["cargoCommitHash"], CARGO_COMMIT);
        assert_eq!(
            rust["rustcProbe"],
            serde_json::json!([FIXED_PROBES[0].program, FIXED_PROBES[0].arguments[0]])
        );
        assert_eq!(
            rust["cargoProbe"],
            serde_json::json!([FIXED_PROBES[1].program, FIXED_PROBES[1].arguments[0]])
        );
        let python = &manifest["python"];
        assert_eq!(python["requiredVersionPrefix"], PYTHON_VERSION_PREFIX);
        assert_eq!(
            python["requiredImplementationOutput"],
            PYTHON_IMPLEMENTATION
        );
        assert_eq!(
            python["versionProbe"],
            serde_json::json!([FIXED_PROBES[2].program, FIXED_PROBES[2].arguments[0]])
        );
        assert_eq!(
            python["implementationProbe"],
            serde_json::json!([
                FIXED_PROBES[3].program,
                FIXED_PROBES[3].arguments[0],
                FIXED_PROBES[3].arguments[1],
                FIXED_PROBES[3].arguments[2],
                FIXED_PROBES[3].arguments[3]
            ])
        );
        let environment = &manifest["runtimeVerification"]["environment"];
        for (key, value) in FIXED_ENVIRONMENT {
            assert_eq!(environment[key], value);
        }
    }

    #[test]
    fn version_parser_rejects_partial_crlf_trailing_and_patchless_outputs() {
        let rust_expectation = ProbeExpectation::RustVersion {
            tool: "rustc",
            release: RUSTC_RELEASE,
            commit: RUSTC_COMMIT,
        };
        for output in [
            format!("release: {RUSTC_RELEASE}\ncommit-hash: {RUSTC_COMMIT}\n"),
            String::from_utf8(RUSTC_OK.to_vec())
                .expect("fixture UTF-8")
                .replace('\n', "\r\n"),
            format!(
                "{}attacker: yes\n",
                String::from_utf8(RUSTC_OK.to_vec()).expect("fixture UTF-8")
            ),
        ] {
            assert!(matches!(
                validate_execution(
                    ProbeExecution {
                        stdout: output.into_bytes(),
                        stderr: Vec::new(),
                        exit_success: true,
                    },
                    rust_expectation,
                ),
                Err(ToolchainProbeFailure::OutputMismatch(_))
            ));
        }

        for output in [
            b"Python 3.12.\n".as_slice(),
            b"Python 3.12.11".as_slice(),
            b"Python 3.12.1..2\n".as_slice(),
        ] {
            assert!(matches!(
                validate_execution(
                    ProbeExecution {
                        stdout: output.to_vec(),
                        stderr: Vec::new(),
                        exit_success: true,
                    },
                    ProbeExpectation::PythonVersionPrefix(PYTHON_VERSION_PREFIX),
                ),
                Err(ToolchainProbeFailure::OutputMismatch(_))
            ));
        }
    }
}
