//! Rust wrapper seam for Lean verifier execution.
//!
//! Lean remains the proof/checking truth source. This crate provides a small,
//! deterministic three-stage boundary: a request-private trusted-helper
//! compile, a source-reading primary checker, and an artifact-only kernel
//! auditor. It returns an evidence envelope that Boole runtime code can record.
//!
//! Hardening:
//! - Each child runs in its own process group; on timeout or normal direct
//!   exit the whole group is sent SIGKILL so `lake`'s spawned compiler and any
//!   submitted-command descendants cannot survive the request boundary.
//! - Stdout/stderr are drained on dedicated threads with a per-stream byte
//!   cap, so the child can never block on a full pipe (default 64 KiB) and
//!   stall the timeout poll loop.
//! - On Unix, RLIMIT_AS / RLIMIT_CPU / RLIMIT_FSIZE / RLIMIT_NOFILE are
//!   applied via `pre_exec` so `memory_limit_mb` is a real constraint, not a
//!   recorded-but-unenforced number.
//! - The child environment is wiped (`env_clear`) and a minimal PATH/HOME is
//!   restored so parent secrets do not leak into the untrusted Lean process.
//! - Proof files containing an unsound escape token (`sorry`, `axiom`,
//!   `native_decide`) or an arbitrary-IO command (`#eval`) are rejected before
//!   the checker runs: Lean compiles `sorry` as a mere warning (returning
//!   success), trusts `axiom` blindly, `native_decide` discharges goals outside
//!   the trusted kernel, and `#eval` runs arbitrary IO (`IO.Process.run`/
//!   `IO.FS.readFile`) with node privileges during checking.
//! - Passing that pre-scan is not sufficient for soundness: a proof could
//!   still declare a custom `elab`/`macro` command that runs arbitrary code
//!   during elaboration (e.g. shelling out via `IO.Process.output`) or call
//!   `Lean.addDecl` directly to inject an axiom without ever writing the
//!   literal word `axiom`, and `set_option debug.skipKernelTC true` disables
//!   kernel typechecking entirely. TB.1 (ADR-0013) closes this: the token
//!   blacklist is extended to also reject `addDecl`/`elab`/`macro`/
//!   `initialize`/`debug.` and any `import` outside the reviewed helper
//!   surface (defense-in-depth, fail fast), and — as the PRIMARY boundary,
//!   since a blacklist can never enumerate every escape — a dedicated
//!   primary checker serializes the accepted environment and a fresh
//!   post-elaboration process (`BooleCheck/Audit.lean`) receives only that
//!   artifact, replays it through Lean's kernel, computes its full axiom
//!   dependency closure, and rejects it unless that closure is a subset of
//!   `{propext, Classical.choice, Quot.sound}`. Both stages see the trusted
//!   checker package as read-only; only the primary can write the request
//!   artifact.
//!   See `enforce_axiom_allowlist` for the isolation argument.

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
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// ADR-0008 — how strictly the kernel-layer isolation extensions (Linux
/// seccomp-bpf + Landlock, macOS Seatbelt) enforce their policy on top of the
/// portable baseline (pgroup + rlimits + env-scrub, always enforced).
///
/// Ratified decision 4 (phased enforcement): the isolation slice landed with
/// `Log` as the default so a not-yet-tuned allowlist observed a would-be
/// violation instead of killing the checker outright. N3.2 — the change that
/// opened share-gossip network ingress — flipped the default to `Enforce` in
/// the same commit and added the operator opt-out
/// (`boole-node --allow-isolation-log-mode`); that pairing is deliberate so
/// the trust-boundary change and the enforcement change cannot drift apart.
///
/// Platform asymmetry, documented rather than silently smoothed over: Linux
/// seccomp has a genuine non-blocking "log this syscall, still allow it"
/// action (`SECCOMP_RET_LOG`), so `Log` mode installs the real filter with
/// that action. Landlock and Seatbelt have no such primitive — once either
/// is engaged the kernel actually blocks the denied operation, there is no
/// "observe only" level. For those two mechanisms `Log` mode therefore means
/// the ruleset/profile is not installed at all, which is behaviorally
/// identical to "logged, never blocked" (nothing changes vs. today's
/// baseline) and carries zero risk of an under-tuned allowlist breaking the
/// checker. `Enforce` mode installs and actually applies all layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IsolationMode {
    /// Observe-only: never fails the checker. See type docs for the
    /// per-mechanism meaning of "observe" (seccomp logs; Landlock/Seatbelt
    /// are simply not installed). Opt-out only since N3.2.
    Log,
    /// Kernel-layer checks are installed and actually deny violations.
    /// The default since N3.2 opened network ingress (ADR-0008 decision 4).
    #[default]
    Enforce,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeanRunnerConfig {
    pub verifier_hash: String,
    pub package_dir: PathBuf,
    /// Wall-clock bound. SC.9a / ADR-0016 (a) — containment ONLY, never a
    /// verdict input: expiring it yields `retryable_unavailable`, not a
    /// reject. The verdict-bearing bound is the step budget below.
    pub timeout_ms: u64,
    pub memory_limit_mb: u64,
    pub output_limit_bytes: usize,
    pub isolation_mode: IsolationMode,
    /// Optional genesis or operator pin; the selected package must also be a known release.
    pub expected_checker_artifact_hash: Option<String>,
    checker_artifact_allowlist: Option<Vec<String>>,
    /// SC.9a / ADR-0016 (a)(b) — the committed step budget, forwarded to
    /// the checker as an explicit frontend option (Lean counts this option
    /// in thousands of raw heartbeats). This IS the verdict input: the same
    /// proof bytes under the same budget exhaust it identically on every
    /// node. The default mirrors `boole_core::BASE_LANE_MAX_HEARTBEATS`
    /// (Tier-2 rule constant); the family lane overrides it from the
    /// consensus-committed `FamilyManifest.resource_limits`.
    pub max_heartbeats: u64,
    /// Companion verdict-bearing frontend recursion counter. Default mirrors
    /// `boole_core::BASE_LANE_MAX_REC_DEPTH` (ADR-0016 (b-1)).
    pub max_rec_depth: u64,
    // Request-local directory where the primary checker may serialize the
    // environment consumed by the artifact-only audit. It is never supplied
    // by callers; `check_file` creates and removes it for each request.
    artifact_scratch_dir: Option<PathBuf>,
    // Historical sandbox tests use `package_dir` as their owned scratch
    // directory. Real verification overrides this to false: trusted checker
    // sources/build outputs are read-only to both child stages.
    package_dir_writable: bool,
    // Trusted executable/search-path values resolved through `lake env`
    // before the submitted source is handed to the direct Lean process.
    lean_sysroot: Option<OsString>,
    lean_path: Option<OsString>,
    // The source-reading primary is a single in-process elaborator. Denying
    // process creation prevents submitted elaborator code from leaving a
    // detached descendant that can race artifact promotion.
    deny_process_spawn: bool,
    release_v1_capture: bool,
}

impl LeanRunnerConfig {
    pub fn new(verifier_hash: impl Into<String>) -> Self {
        Self {
            verifier_hash: verifier_hash.into(),
            package_dir: PathBuf::from("."),
            timeout_ms: 10_000,
            memory_limit_mb: 8192,
            output_limit_bytes: 64 * 1024,
            isolation_mode: IsolationMode::default(),
            expected_checker_artifact_hash: None,
            checker_artifact_allowlist: None,
            max_heartbeats: 400_000,
            max_rec_depth: 512,
            artifact_scratch_dir: None,
            package_dir_writable: true,
            lean_sysroot: None,
            lean_path: None,
            deny_process_spawn: false,
            release_v1_capture: false,
        }
    }

    pub fn with_package_dir(mut self, package_dir: impl Into<PathBuf>) -> Self {
        self.package_dir = package_dir.into();
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

    pub fn with_isolation_mode(mut self, isolation_mode: IsolationMode) -> Self {
        self.isolation_mode = isolation_mode;
        self
    }

    pub fn with_checker_artifact_hash(mut self, hash: impl Into<String>) -> Self {
        self.expected_checker_artifact_hash = Some(hash.into());
        self
    }

    pub fn with_max_heartbeats(mut self, max_heartbeats: u64) -> Self {
        self.max_heartbeats = max_heartbeats;
        self
    }

    pub fn with_max_rec_depth(mut self, max_rec_depth: u64) -> Self {
        self.max_rec_depth = max_rec_depth;
        self
    }
}

/// SC.9a / ADR-0016 (a)(a-3) — the three-state verdict contract. The
/// verdict is a pure function of (proof bytes, pinned checker, committed
/// step budget); wall-clock and rlimits are containment and may only ever
/// surface as `RetryableUnavailable`, never as an accept or a reject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LeanVerdict {
    /// The checker elaborated the proof within the committed budget and the
    /// axiom audit passed.
    Accepted,
    /// Every node reaches this same reject from the same bytes: Lean
    /// rejected the proof, the committed step budget ran out
    /// (`budget_exceeded`), the source tried to redefine the budget
    /// (`budget_override_forbidden`), or the axiom audit refused it.
    DeterministicReject { reason: String },
    /// Availability failure (wall-clock containment kill, signal death,
    /// resource-limit kill). NOT a verdict: it must never advance a head or
    /// checkpoint, and must never be translated into a consensus reject.
    RetryableUnavailable { reason: String },
}

impl LeanVerdict {
    pub fn error_group(&self) -> Option<&'static str> {
        match self {
            Self::RetryableUnavailable { reason } => Some(match reason.as_str() {
                "checker_release_mismatch"
                | "toolchain_unavailable"
                | "trusted_helper_compile_unavailable" => "D-node",
                "containment_killed"
                | "primary_checker_unavailable"
                | "axiom_audit_unavailable" => "I",
                "replay_unavailable" => "D-input",
                value if value.starts_with("checker package changed") => "D-node",
                _ => "T",
            }),
            _ => None,
        }
    }

    pub fn is_retryable_unavailable(&self) -> bool {
        matches!(self, LeanVerdict::RetryableUnavailable { .. })
    }
}

/// Typed reason for a deterministic reject caused by exhausting the
/// committed step budget (`maxHeartbeats`/`maxRecDepth`).
pub const REJECT_BUDGET_EXCEEDED: &str = "budget_exceeded";
/// Typed reason when the submitted source tries to (re)define the committed
/// budget (`set_option maxHeartbeats ...`) — ADR-0016 (a-2).
pub const REJECT_BUDGET_OVERRIDE_FORBIDDEN: &str = "budget_override_forbidden";
/// Typed reason for an ordinary Lean elaboration failure.
pub const REJECT_LEAN_REJECTED: &str = "lean_rejected";
/// Typed reason when the ADR-0013 axiom audit refuses the proof.
pub const REJECT_AXIOM_AUDIT: &str = "axiom_audit_rejected";

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
    /// SC.9a — the committed step budget the checker actually ran under.
    /// `#[serde(default)]` keeps pre-SC.9 recorded evidence deserializable.
    #[serde(default)]
    pub max_heartbeats: u64,
    #[serde(default)]
    pub max_rec_depth: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeanCheckResult {
    pub accepted: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub output_truncated: bool,
    #[serde(default)]
    pub stdout_bytes: u64,
    #[serde(default)]
    pub stderr_bytes: u64,
    /// SC.9a / ADR-0016 (a-3) — three-state classification. `accepted`
    /// stays the boolean shorthand (`verdict == Accepted`); consumers that
    /// must not translate availability failures into consensus rejects
    /// (the (a-3) invariant) branch on this field instead.
    pub verdict: LeanVerdict,
    pub evidence: LeanRunnerEvidence,
}

#[derive(Debug, Clone)]
pub struct LeanRunner {
    config: LeanRunnerConfig,
}

static ARTIFACT_WORKSPACE_COUNTER: AtomicU64 = AtomicU64::new(0);
const HELPER_SOURCE_RELATIVE: &str = "Boole/Family/V0Helpers.lean";
const HELPER_OLEAN_RELATIVE: &str = "Boole/Family/V0Helpers.olean";

struct CheckerArtifactWorkspace {
    root: PathBuf,
    submission_source: PathBuf,
    helper_source_root: PathBuf,
    helper_source: PathBuf,
    helper_import_root: PathBuf,
    helper_artifact: PathBuf,
    primary_dir: PathBuf,
    primary_artifact: PathBuf,
    audit_artifact: PathBuf,
}

impl CheckerArtifactWorkspace {
    fn create() -> Result<Self> {
        let parent = std::env::temp_dir();
        for _ in 0..128 {
            let serial = ARTIFACT_WORKSPACE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let root = parent.join(format!(
                "boole-lean-artifact-{}-{serial}",
                std::process::id()
            ));
            let mut builder = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&root) {
                Ok(()) => {
                    let submission_source_root = root.join("submission-source");
                    let helper_source_root = root.join("helper-source");
                    let helper_source_dir = helper_source_root.join("Boole/Family");
                    let helper_import_root = root.join("helper-imports");
                    let helper_import_dir = helper_import_root.join("Boole/Family");
                    let primary_dir = root.join("primary");
                    let audit_dir = root.join("audit");
                    if let Err(error) = create_private_dir(&submission_source_root)
                        .and_then(|()| create_private_dir_all(&helper_source_dir))
                        .and_then(|()| create_private_dir_all(&helper_import_dir))
                        .and_then(|()| create_private_dir(&primary_dir))
                        .and_then(|()| create_private_dir(&audit_dir))
                    {
                        let _ = std::fs::remove_dir_all(&root);
                        return Err(error);
                    }
                    let primary_artifact = primary_dir.join("Submission.olean");
                    let audit_artifact = audit_dir.join("Submission.olean");
                    return Ok(Self {
                        root,
                        submission_source: submission_source_root.join("Submission.lean"),
                        helper_source: helper_source_root.join(HELPER_SOURCE_RELATIVE),
                        helper_source_root,
                        helper_artifact: helper_import_root.join(HELPER_OLEAN_RELATIVE),
                        helper_import_root,
                        primary_dir,
                        primary_artifact,
                        audit_artifact,
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("failed to create artifact workspace {}", root.display())
                    });
                }
            }
        }
        Err(anyhow!(
            "failed to allocate a unique checker artifact workspace"
        ))
    }

    fn snapshot_submission_source(&self, proof_path: &Path) -> Result<SubmissionSourceSnapshot> {
        let mut source = open_regular_nofollow(proof_path)?;
        let metadata = source
            .metadata()
            .with_context(|| format!("failed to inspect proof file {}", proof_path.display()))?;
        if !metadata.file_type().is_file() {
            return Err(anyhow!(
                "Lean proof file is not a regular file: {}",
                proof_path.display()
            ));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        source
            .read_to_end(&mut bytes)
            .with_context(|| format!("failed to read proof file {}", proof_path.display()))?;
        let digest = digest_bytes(&bytes);

        write_private_read_only_file(&self.submission_source, &bytes)
            .context("failed to copy request-private submitted source")?;
        let file = open_regular_nofollow(&self.submission_source)?;
        if digest_open_file(&file)? != digest {
            return Err(anyhow!(
                "request-private submitted source differs from its no-follow caller descriptor"
            ));
        }
        Ok(SubmissionSourceSnapshot {
            bytes,
            digest,
            file,
        })
    }

    fn snapshot_helper_source(&self, package_dir: &Path) -> Result<HelperSourceSnapshot> {
        let source_path = package_dir.join(HELPER_SOURCE_RELATIVE);
        let mut source = open_regular_nofollow(&source_path)?;
        let metadata = source.metadata().with_context(|| {
            format!("failed to inspect helper source {}", source_path.display())
        })?;
        if !metadata.file_type().is_file() || metadata.len() == 0 {
            return Err(anyhow!(
                "helper source is not a non-empty regular file: {}",
                source_path.display()
            ));
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        source
            .read_to_end(&mut bytes)
            .with_context(|| format!("failed to read helper source {}", source_path.display()))?;
        let source_digest = digest_bytes(&bytes);

        write_private_read_only_file(&self.helper_source, &bytes)
            .context("failed to copy private helper source")?;

        let copied = open_regular_nofollow(&self.helper_source)?;
        let copied_digest = digest_open_file(&copied)?;
        if copied_digest != source_digest {
            return Err(anyhow!(
                "private helper source digest differs from its no-follow package descriptor"
            ));
        }
        Ok(HelperSourceSnapshot {
            bytes,
            digest: source_digest,
            file: copied,
        })
    }

    fn seal_helper_artifact(&self) -> Result<SealedCheckerArtifact> {
        let file = open_regular_nofollow(&self.helper_artifact).with_context(|| {
            format!(
                "trusted helper compile did not produce {}",
                self.helper_artifact.display()
            )
        })?;
        let metadata = file.metadata().with_context(|| {
            format!(
                "failed to inspect compiled helper artifact {}",
                self.helper_artifact.display()
            )
        })?;
        if !metadata.file_type().is_file()
            || metadata.len() == 0
            || self
                .helper_artifact
                .extension()
                .and_then(|ext| ext.to_str())
                != Some("olean")
        {
            return Err(anyhow!(
                "compiled helper artifact is not a non-empty .olean regular file: {}",
                self.helper_artifact.display()
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o400))
                .context("failed to make compiled helper artifact read-only")?;
        }
        let digest = digest_open_file(&file)?;
        Ok(SealedCheckerArtifact { file, digest })
    }

    fn promote_for_audit(&self) -> Result<SealedCheckerArtifact> {
        use std::io::{Seek, SeekFrom};

        let mut source = open_regular_nofollow(&self.primary_artifact).with_context(|| {
            format!(
                "primary checker did not produce artifact {}",
                self.primary_artifact.display()
            )
        })?;
        let metadata = source.metadata().with_context(|| {
            format!(
                "failed to inspect primary checker artifact {}",
                self.primary_artifact.display()
            )
        })?;
        if !metadata.file_type().is_file() || metadata.len() == 0 {
            return Err(anyhow!(
                "primary checker artifact is not a non-empty regular file: {}",
                self.primary_artifact.display()
            ));
        }
        let source_digest = digest_open_file(&source)
            .context("failed to digest primary checker artifact before promotion")?;
        source
            .seek(SeekFrom::Start(0))
            .context("failed to rewind primary checker artifact before promotion")?;

        // Copy from the already-open, no-follow descriptor into a fresh inode
        // outside the primary process's write allowlist. A submitted command
        // can keep neither a pathname nor a pre-opened writable descriptor to
        // the exact bytes later consumed by the audit.
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o400);
        }
        let mut destination = options.open(&self.audit_artifact).with_context(|| {
            format!(
                "failed to create audit-owned checker artifact {}",
                self.audit_artifact.display()
            )
        })?;
        std::io::copy(&mut source, &mut destination).with_context(|| {
            format!(
                "failed to promote checker artifact into {}",
                self.audit_artifact.display()
            )
        })?;
        destination
            .flush()
            .context("failed to flush promoted checker artifact")?;
        destination
            .sync_all()
            .context("failed to sync promoted checker artifact")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            destination
                .set_permissions(std::fs::Permissions::from_mode(0o400))
                .with_context(|| {
                    format!(
                        "failed to make audit checker artifact read-only: {}",
                        self.audit_artifact.display()
                    )
                })?;
        }
        drop(destination);

        let file = open_regular_nofollow(&self.audit_artifact)?;
        let promoted = file.metadata().with_context(|| {
            format!(
                "failed to inspect audit checker artifact {}",
                self.audit_artifact.display()
            )
        })?;
        if !promoted.file_type().is_file() || promoted.len() != metadata.len() {
            return Err(anyhow!(
                "promoted checker artifact shape changed: {}",
                self.audit_artifact.display()
            ));
        }
        let digest = digest_open_file(&file)?;
        if digest != source_digest {
            return Err(anyhow!(
                "promoted checker artifact bytes differ from the no-follow primary descriptor"
            ));
        }
        #[cfg(unix)]
        std::fs::remove_file(&self.audit_artifact).with_context(|| {
            format!(
                "failed to unlink the promoted checker artifact pathname before audit: {}",
                self.audit_artifact.display()
            )
        })?;
        Ok(SealedCheckerArtifact { file, digest })
    }
}

struct SubmissionSourceSnapshot {
    bytes: Vec<u8>,
    digest: String,
    file: std::fs::File,
}

impl SubmissionSourceSnapshot {
    fn digest(&self) -> Result<String> {
        digest_open_file(&self.file)
    }

    fn stdin_for_child(&self) -> Result<std::fs::File> {
        clone_rewound_file(&self.file, "request-private submitted source")
    }
}

struct HelperSourceSnapshot {
    bytes: Vec<u8>,
    digest: String,
    file: std::fs::File,
}

impl HelperSourceSnapshot {
    fn digest(&self) -> Result<String> {
        digest_open_file(&self.file)
    }
}

struct SealedCheckerArtifact {
    file: std::fs::File,
    digest: String,
}

impl SealedCheckerArtifact {
    fn digest(&self) -> Result<String> {
        digest_open_file(&self.file)
    }

    fn stdin_for_child(&self) -> Result<std::fs::File> {
        clone_rewound_file(&self.file, "sealed audit artifact")
    }
}

fn clone_rewound_file(file: &std::fs::File, label: &str) -> Result<std::fs::File> {
    use std::io::{Seek, SeekFrom};

    let mut file = file
        .try_clone()
        .with_context(|| format!("failed to clone {label} descriptor"))?;
    file.seek(SeekFrom::Start(0))
        .with_context(|| format!("failed to rewind {label} descriptor"))?;
    Ok(file)
}

#[cfg(target_os = "linux")]
fn inherited_stdin_descriptor_path() -> Result<&'static Path> {
    let path = Path::new("/proc/self/fd/0");
    Path::new("/proc/self/fd")
        .is_dir()
        .then_some(path)
        .ok_or_else(|| anyhow!("Linux procfd view is unavailable for the artifact audit"))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn inherited_stdin_descriptor_path() -> Result<&'static Path> {
    let path = Path::new("/dev/fd/0");
    Path::new("/dev/fd")
        .is_dir()
        .then_some(path)
        .ok_or_else(|| anyhow!("Unix fd view is unavailable for the artifact audit"))
}

#[cfg(not(unix))]
fn inherited_stdin_descriptor_path() -> Result<&'static Path> {
    Err(anyhow!(
        "this platform has no supported inherited-fd view for the artifact audit"
    ))
}

fn write_private_read_only_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o400);
    }
    let mut destination = options
        .open(path)
        .with_context(|| format!("failed to create private file {}", path.display()))?;
    destination
        .write_all(bytes)
        .with_context(|| format!("failed to write private file {}", path.display()))?;
    destination
        .sync_all()
        .with_context(|| format!("failed to sync private file {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        destination
            .set_permissions(std::fs::Permissions::from_mode(0o400))
            .with_context(|| {
                format!("failed to make private file read-only: {}", path.display())
            })?;
    }
    Ok(())
}

fn create_private_dir(path: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .with_context(|| format!("failed to create private directory {}", path.display()))
}

fn create_private_dir_all(path: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .with_context(|| format!("failed to create private directory tree {}", path.display()))
}

fn open_regular_nofollow(path: &Path) -> Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // NONBLOCK prevents an attacker-controlled FIFO at the primary
        // output path from hanging the trusted parent outside its deadline.
        // It has no behavioral effect after fstat confirms a regular file.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    options.open(path).with_context(|| {
        format!(
            "failed to open regular file without following links: {}",
            path.display()
        )
    })
}

fn digest_open_file(file: &std::fs::File) -> Result<String> {
    use std::io::{Seek, SeekFrom};

    let mut file = file
        .try_clone()
        .context("failed to clone artifact descriptor")?;
    file.seek(SeekFrom::Start(0))
        .context("failed to rewind artifact descriptor")?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buf)
            .context("failed to read checker artifact descriptor")?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn digest_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

impl Drop for CheckerArtifactWorkspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn unavailable_result(evidence: &LeanRunnerEvidence, reason: impl Into<String>) -> LeanCheckResult {
    let reason = reason.into();
    LeanCheckResult {
        accepted: false,
        exit_code: -1,
        stdout: String::new(),
        stderr: reason.clone(),
        timed_out: false,
        output_truncated: false,
        stdout_bytes: 0,
        stderr_bytes: 0,
        verdict: LeanVerdict::RetryableUnavailable { reason },
        evidence: evidence.clone(),
    }
}

fn unavailable_stage_result(
    evidence: &LeanRunnerEvidence,
    outcome: SandboxedRunOutcome,
    reason: &str,
    context: &str,
) -> LeanCheckResult {
    let mut stderr = outcome.stderr;
    if !stderr.is_empty() && !stderr.ends_with('\n') {
        stderr.push('\n');
    }
    stderr.push_str(context);
    LeanCheckResult {
        accepted: false,
        exit_code: outcome.exit_code,
        stdout: outcome.stdout,
        stderr,
        timed_out: outcome.timed_out,
        output_truncated: outcome.output_truncated,
        stdout_bytes: outcome.stdout_bytes,
        stderr_bytes: outcome.stderr_bytes,
        verdict: LeanVerdict::RetryableUnavailable {
            reason: reason.to_string(),
        },
        evidence: evidence.clone(),
    }
}

fn apply_remaining_timeout(config: &mut LeanRunnerConfig, deadline: Instant) -> bool {
    apply_remaining_timeout_at(config, deadline, Instant::now())
}

fn apply_remaining_timeout_at(
    config: &mut LeanRunnerConfig,
    deadline: Instant,
    now: Instant,
) -> bool {
    let remaining = deadline.saturating_duration_since(now);
    if remaining.is_zero() {
        return false;
    }
    config.timeout_ms = u64::try_from(remaining.as_millis())
        .unwrap_or(u64::MAX)
        .max(1);
    true
}

fn verification_stage_config(
    base: &LeanRunnerConfig,
    runtime: &EffectiveToolchainRuntime,
    lean_path: OsString,
    scratch: Option<PathBuf>,
) -> LeanRunnerConfig {
    let mut config = base.clone();
    config.artifact_scratch_dir = scratch;
    config.package_dir_writable = false;
    config.lean_sysroot = Some(runtime.lean_sysroot.clone());
    config.lean_path = Some(lean_path);
    config.deny_process_spawn = true;
    config
}

impl LeanRunner {
    pub fn new(config: LeanRunnerConfig) -> Self {
        Self { config }
    }

    /// Bind operator allowlist authority before any checker child executes.
    pub fn with_checker_artifact_allowlist(mut self, hashes: Vec<String>) -> Self {
        self.config.checker_artifact_allowlist = Some(hashes);
        self
    }

    pub fn check_file(&self, proof_path: impl AsRef<Path>) -> Result<LeanCheckResult> {
        let proof_path = proof_path.as_ref();
        if !self.config.package_dir.is_dir() {
            return Err(anyhow!(
                "Lean package directory does not exist: {}",
                self.config.package_dir.display()
            ));
        }
        let artifact_workspace = CheckerArtifactWorkspace::create()?;
        let submitted_source = artifact_workspace.snapshot_submission_source(proof_path)?;
        if let Some((token, line)) = scan_for_forbidden_tokens_in_bytes(&submitted_source.bytes) {
            return Err(anyhow!(
                "Lean proof rejected: forbidden `{}` token at {}:{}",
                token,
                proof_path.display(),
                line
            ));
        }

        let actual_hash = checker_artifact_hash(&self.config.package_dir).unwrap_or_default();
        if self
            .config
            .checker_artifact_allowlist
            .as_ref()
            .is_some_and(|hashes| !hashes.contains(&actual_hash))
            || checker_audit_grammar(&actual_hash).is_none()
            || self
                .config
                .expected_checker_artifact_hash
                .as_ref()
                .is_some_and(|pin| pin != &actual_hash)
        {
            let evidence = LeanRunnerEvidence {
                verifier_hash: self.config.verifier_hash.clone(),
                checker: "direct lean source checker + artifact audit".into(),
                checker_exe: "lean".into(),
                checker_artifact_hash: actual_hash,
                package_dir: self.config.package_dir.display().to_string(),
                lean_version: String::new(),
                lake_version: String::new(),
                timeout_ms: self.config.timeout_ms,
                memory_limit_mb: self.config.memory_limit_mb,
                output_limit_bytes: self.config.output_limit_bytes,
                max_heartbeats: self.config.max_heartbeats,
                max_rec_depth: self.config.max_rec_depth,
            };
            return Ok(unavailable_result(&evidence, "checker_release_mismatch"));
        }
        let evidence = match self.evidence() {
            Ok(evidence) => evidence,
            Err(_) => {
                return Ok(unavailable_result(
                    &self.unprobed_evidence(actual_hash),
                    "toolchain_unavailable",
                ))
            }
        };
        if evidence.checker_artifact_hash != actual_hash {
            return Ok(unavailable_result(&evidence, "checker_release_mismatch"));
        }
        let toolchain_runtime =
            match effective_toolchain_runtime(&self.config.package_dir, self.config.isolation_mode)
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    return Ok(unavailable_result(
                        &evidence,
                        format!("failed to resolve direct checker runtime: {err:#}"),
                    ));
                }
            };
        let helper_source =
            match artifact_workspace.snapshot_helper_source(&self.config.package_dir) {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    return Ok(unavailable_result(
                        &evidence,
                        format!("failed to snapshot pinned helper source: {err:#}"),
                    ));
                }
            };
        let snapshot_checker_hash = match checker_artifact_hash_with_helper_source(
            &self.config.package_dir,
            Some(&helper_source.bytes),
        ) {
            Ok(hash) => hash,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };
        if snapshot_checker_hash != evidence.checker_artifact_hash {
            return Ok(unavailable_result(
                &evidence,
                "checker package changed while the helper source was snapshotted",
            ));
        }

        let toolchain_only_lean_path =
            match isolated_lean_path(&[&toolchain_runtime.lean_stdlib_dir]) {
                Ok(path) => path,
                Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
            };
        let request_lean_path = match isolated_lean_path(&[
            &artifact_workspace.helper_import_root,
            &toolchain_runtime.lean_stdlib_dir,
        ]) {
            Ok(path) => path,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };

        // Compile the pinned helper source into a request-private import tree
        // before any child elaborates the already-snapshotted submission. This is a separate sandbox
        // stage: the checker package is read-only, network/process creation
        // are denied, and only the private helper output tree is writable.
        // The package's prebuilt `.lake` tree is deliberately absent from
        // LEAN_PATH, so stale or substituted helper artifacts cannot affect a
        // verdict carrying the source-based checker identity.
        let mut helper_config = self.config.clone();
        helper_config.artifact_scratch_dir = Some(artifact_workspace.helper_import_root.clone());
        helper_config.package_dir_writable = false;
        helper_config.lean_sysroot = Some(toolchain_runtime.lean_sysroot.clone());
        helper_config.lean_path = Some(toolchain_only_lean_path);
        helper_config.deny_process_spawn = true;
        let verification_deadline = Instant::now() + Duration::from_millis(self.config.timeout_ms);
        if !apply_remaining_timeout(&mut helper_config, verification_deadline) {
            return Ok(unavailable_result(
                &evidence,
                "verification wall-clock budget expired before trusted helper compilation",
            ));
        }
        let mut helper_command = Command::new(&toolchain_runtime.lean_executable);
        helper_command
            .arg(format!("-DmaxHeartbeats={TRUSTED_HELPER_MAX_HEARTBEATS}"))
            .arg(format!("-DmaxRecDepth={TRUSTED_HELPER_MAX_REC_DEPTH}"))
            .arg("-o")
            .arg(&artifact_workspace.helper_artifact)
            .arg(HELPER_SOURCE_RELATIVE)
            .current_dir(&artifact_workspace.helper_source_root);
        let helper_compile = match self
            .run_sandboxed_with_config(helper_command, &helper_config)
            .context("failed to compile the request-private trusted helper")
        {
            Ok(outcome) => outcome,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };
        if !helper_compile.success || helper_compile.capture_failed {
            let reason = if helper_compile.timed_out {
                "containment_wall_clock_kill"
            } else if helper_compile.capture_failed {
                "capture_failed"
            } else {
                "trusted_helper_compile_unavailable"
            };
            return Ok(unavailable_stage_result(
                &evidence,
                helper_compile,
                reason,
                "request-private trusted helper compile failed",
            ));
        }
        let checker_hash_after_helper_compile =
            match checker_artifact_hash(&self.config.package_dir) {
                Ok(hash) => hash,
                Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
            };
        if checker_hash_after_helper_compile != evidence.checker_artifact_hash {
            return Ok(unavailable_result(
                &evidence,
                "checker package changed during request-private helper compilation",
            ));
        }
        let helper_source_hash_after_compile = match helper_source.digest() {
            Ok(hash) => hash,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };
        if helper_source_hash_after_compile != helper_source.digest {
            return Ok(unavailable_result(
                &evidence,
                "private helper source changed during helper compilation",
            ));
        }
        let sealed_helper_artifact = match artifact_workspace.seal_helper_artifact() {
            Ok(artifact) => artifact,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };

        let mut run_config = verification_stage_config(
            &self.config,
            &toolchain_runtime,
            request_lean_path.clone(),
            Some(artifact_workspace.primary_dir.clone()),
        );
        let mut audit_config =
            verification_stage_config(&self.config, &toolchain_runtime, request_lean_path, None);
        let release_v1 = actual_hash == CHECKER_RELEASE_HASH_V1;
        run_config.release_v1_capture = release_v1;
        audit_config.release_v1_capture = release_v1;

        // SC.9a / ADR-0016 (a)(b) — the committed step budget rides along
        // as explicit checker args so the verdict never inherits Lean's own
        // (uncommitted) defaults. The package-pinned Lean executable runs the
        // trusted primary source directly, so no prebuilt checker binary or
        // nested `lake` process sits between the sandbox and elaboration.
        let mut primary_command = Command::new(&toolchain_runtime.lean_executable);
        let inherited_descriptor_path = match inherited_stdin_descriptor_path() {
            Ok(path) => path,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };
        primary_command
            .arg(format!(
                "-DmaxHeartbeats={}",
                if release_v1 {
                    self.config.max_heartbeats
                } else {
                    TRUSTED_HELPER_MAX_HEARTBEATS
                }
            ))
            .arg(format!(
                "-DmaxRecDepth={}",
                if release_v1 {
                    self.config.max_rec_depth
                } else {
                    TRUSTED_HELPER_MAX_REC_DEPTH
                }
            ))
            .arg("--run")
            .arg(PRIMARY_CHECKER_SCRIPT)
            .arg(inherited_descriptor_path)
            .arg(self.config.max_heartbeats.to_string())
            .arg(self.config.max_rec_depth.to_string())
            .arg(&artifact_workspace.primary_artifact)
            .current_dir(&self.config.package_dir);
        if !apply_remaining_timeout(&mut run_config, verification_deadline) {
            return Ok(unavailable_result(
                &evidence,
                "verification wall-clock budget expired before primary verification",
            ));
        }
        let primary_stdin = match submitted_source.stdin_for_child() {
            Ok(file) => file,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };
        let primary = match self
            .run_sandboxed_with_config_and_stdin(primary_command, &run_config, Some(primary_stdin))
            .with_context(|| {
                format!(
                    "failed to run direct checker source with {} in {}",
                    toolchain_runtime.lean_executable.display(),
                    self.config.package_dir.display()
                )
            }) {
            Ok(outcome) => outcome,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };

        if !primary.success || primary.capture_failed {
            let verdict = if release_v1 {
                classify_failed_run_v1(&primary)
            } else {
                classify_failed_run(&primary)
            };
            return Ok(LeanCheckResult {
                accepted: false,
                exit_code: primary.exit_code,
                stdout: primary.stdout,
                stderr: primary.stderr,
                timed_out: primary.timed_out,
                output_truncated: primary.output_truncated,
                stdout_bytes: primary.stdout_bytes,
                stderr_bytes: primary.stderr_bytes,
                verdict,
                evidence,
            });
        }

        let checker_hash_after_primary = match checker_artifact_hash(&self.config.package_dir) {
            Ok(hash) => hash,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };
        if checker_hash_after_primary != evidence.checker_artifact_hash {
            return Ok(unavailable_result(
                &evidence,
                "checker package changed during primary verification",
            ));
        }
        let source_hash_after_primary = match submitted_source.digest() {
            Ok(hash) => hash,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };
        if source_hash_after_primary != submitted_source.digest {
            return Ok(unavailable_result(
                &evidence,
                "request-private submitted source changed during primary verification",
            ));
        }
        let helper_hash_after_primary = match sealed_helper_artifact.digest() {
            Ok(hash) => hash,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };
        if helper_hash_after_primary != sealed_helper_artifact.digest {
            return Ok(unavailable_result(
                &evidence,
                "request-private helper artifact changed during primary verification",
            ));
        }
        let sealed_artifact = match artifact_workspace.promote_for_audit() {
            Ok(artifact) => artifact,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };

        // TB.1 / ADR-0013 — the primary checker accepted and serialized the
        // environment. The separate audit gets only the retained artifact
        // descriptor through fd 0; submitted source bytes never enter it.
        let mut audit_command = Command::new(&toolchain_runtime.lean_executable);
        audit_command
            .arg(format!(
                "-DmaxHeartbeats={}",
                if release_v1 {
                    self.config.max_heartbeats
                } else {
                    TRUSTED_HELPER_MAX_HEARTBEATS
                }
            ))
            .arg(format!(
                "-DmaxRecDepth={}",
                if release_v1 {
                    self.config.max_rec_depth
                } else {
                    TRUSTED_HELPER_MAX_REC_DEPTH
                }
            ))
            .arg("--run")
            .arg(AXIOM_AUDIT_SCRIPT)
            .arg(inherited_descriptor_path)
            .current_dir(&self.config.package_dir);
        if !apply_remaining_timeout(&mut audit_config, verification_deadline) {
            return Ok(unavailable_result(
                &evidence,
                "verification wall-clock budget expired before artifact audit",
            ));
        }
        let audit_stdin = match sealed_artifact.stdin_for_child() {
            Ok(file) => file,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };
        let audit = match self
            .run_sandboxed_with_config_and_stdin(audit_command, &audit_config, Some(audit_stdin))
            .with_context(|| {
                format!(
                    "failed to run axiom audit in {}",
                    self.config.package_dir.display()
                )
            }) {
            Ok(outcome) => outcome,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };

        let artifact_hash_after_audit = match sealed_artifact.digest() {
            Ok(hash) => hash,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };
        if artifact_hash_after_audit != sealed_artifact.digest {
            return Ok(unavailable_result(
                &evidence,
                "checker artifact changed during audit",
            ));
        }
        let helper_hash_after_audit = match sealed_helper_artifact.digest() {
            Ok(hash) => hash,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };
        if helper_hash_after_audit != sealed_helper_artifact.digest {
            return Ok(unavailable_result(
                &evidence,
                "request-private helper artifact changed during artifact audit",
            ));
        }
        let checker_hash_after_audit = match checker_artifact_hash(&self.config.package_dir) {
            Ok(hash) => hash,
            Err(err) => return Ok(unavailable_result(&evidence, format!("{err:#}"))),
        };
        if checker_hash_after_audit != evidence.checker_artifact_hash {
            return Ok(unavailable_result(
                &evidence,
                "checker package changed during artifact audit",
            ));
        }

        let timed_out = primary.timed_out || audit.timed_out;
        let output_truncated = primary.output_truncated || audit.output_truncated;
        let stdout_bytes = primary.stdout_bytes + audit.stdout_bytes;
        let stderr_bytes = primary.stderr_bytes + audit.stderr_bytes;
        match match checker_audit_grammar(&evidence.checker_artifact_hash) {
            Some(AuditGrammar::V1) => enforce_axiom_allowlist_v1(&audit),
            Some(AuditGrammar::V2) => enforce_axiom_allowlist(&audit),
            None => unreachable!("release checked before execution"),
        } {
            Ok(()) => Ok(LeanCheckResult {
                accepted: true,
                exit_code: primary.exit_code,
                stdout: primary.stdout,
                stderr: primary.stderr,
                timed_out,
                output_truncated,
                stdout_bytes,
                stderr_bytes,
                verdict: LeanVerdict::Accepted,
                evidence,
            }),
            Err((verdict, reason)) => {
                let mut stderr = primary.stderr;
                if !stderr.is_empty() && !stderr.ends_with('\n') {
                    stderr.push('\n');
                }
                stderr.push_str("axiom audit rejected: ");
                stderr.push_str(&reason);
                Ok(LeanCheckResult {
                    accepted: false,
                    exit_code: primary.exit_code,
                    stdout: primary.stdout,
                    stderr,
                    timed_out,
                    output_truncated,
                    stdout_bytes,
                    stderr_bytes,
                    verdict,
                    evidence,
                })
            }
        }
    }

    /// Runs `command` inside the sandboxed child-process harness shared by
    /// helper compilation, the primary checker, and the TB.1 axiom audit: its own
    /// process group (killed as a whole on timeout), rlimits, a scrubbed
    /// environment, and byte-capped drain threads so the child can never
    /// stall the timeout poll loop on a full pipe. `command`'s program and
    /// args must already be set; stdio/env/sandbox are configured here.
    fn run_sandboxed_with_config(
        &self,
        command: Command,
        sandbox_config: &LeanRunnerConfig,
    ) -> Result<SandboxedRunOutcome> {
        self.run_sandboxed_with_config_and_stdin(command, sandbox_config, None)
    }

    fn run_sandboxed_with_config_and_stdin(
        &self,
        mut command: Command,
        sandbox_config: &LeanRunnerConfig,
        stdin: Option<std::fs::File>,
    ) -> Result<SandboxedRunOutcome> {
        match stdin {
            Some(file) => command.stdin(Stdio::from(file)),
            None => command.stdin(Stdio::null()),
        };
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        configure_child_environment(&mut command);
        if let Some(sysroot) = &sandbox_config.lean_sysroot {
            command.env("LEAN_SYSROOT", sysroot);
        }
        if let Some(path) = &sandbox_config.lean_path {
            command.env("LEAN_PATH", path);
        }
        configure_child_sandbox(&mut command, sandbox_config)
            .context("failed to prepare sandbox isolation before spawn")?;

        let mut child = ChildKillOnDrop::new(
            command
                .spawn()
                .context("failed to spawn sandboxed command")?,
        );

        let output_limit = self.config.output_limit_bytes;
        let stdout_pipe = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("child stdout was not captured"))?;
        let stderr_pipe = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("child stderr was not captured"))?;
        let stdout_buf: Arc<Mutex<DrainBuffer>> = Arc::new(Mutex::new(DrainBuffer::for_release(
            output_limit,
            sandbox_config.release_v1_capture,
        )));
        let stderr_buf: Arc<Mutex<DrainBuffer>> = Arc::new(Mutex::new(DrainBuffer::for_release(
            output_limit,
            sandbox_config.release_v1_capture,
        )));
        let stdout_handle = spawn_drain(stdout_pipe, Arc::clone(&stdout_buf));
        let stderr_handle = spawn_drain(stderr_pipe, Arc::clone(&stderr_buf));

        let deadline = Instant::now() + Duration::from_millis(sandbox_config.timeout_ms);
        let timed_out = loop {
            if child_finished_without_reaping(&mut child)? {
                break false;
            }
            if Instant::now() >= deadline {
                break true;
            }
            thread::sleep(Duration::from_millis(5));
        };

        // Even when the direct child exits normally, a submitted command may
        // have left descendants holding pipes or waiting to mutate the
        // request artifact. End the whole request process group before the
        // artifact is sealed or any drain thread is joined.
        let child_pid = child.id();
        let cleanup_result = kill_child_group(&mut child);
        let output_status = child.wait_and_disarm()?;
        cleanup_result?;
        confirm_process_group_empty(child_pid)?;
        // Drain threads exit when the child closes its pipe ends. After the
        // child is reaped, EOF is delivered and the threads finish.
        let stdout_join_failed = stdout_handle.join().is_err();
        let stderr_join_failed = stderr_handle.join().is_err();

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

        let mut stdout = String::from_utf8_lossy(&stdout_drain.retained()).to_string();
        let mut stderr = String::from_utf8_lossy(&stderr_drain.retained()).to_string();
        let mut stdout_truncated = stdout_drain.truncated;
        let mut stderr_truncated = stderr_drain.truncated;
        if timed_out {
            if !stderr.is_empty() && !stderr.ends_with('\n') {
                stderr.push('\n');
            }
            stderr.push_str(&format!(
                "lean runner timeout after {}ms",
                sandbox_config.timeout_ms
            ));
        }
        // After appending the timeout marker stderr may have grown past the
        // limit; re-truncate to keep the recorded byte cap honest.
        stdout_truncated |= truncate_utf8_to_bytes(&mut stdout, output_limit);
        stderr_truncated |= truncate_utf8_to_bytes(&mut stderr, output_limit);

        Ok(SandboxedRunOutcome {
            success: !timed_out && output_status.success(),
            exit_code: if timed_out {
                -1
            } else {
                output_status.code().unwrap_or(-1)
            },
            stdout,
            stderr,
            timed_out,
            output_truncated: stdout_truncated || stderr_truncated,
            stdout_truncated,
            stdout_bytes: stdout_drain.total,
            stderr_bytes: stderr_drain.total,
            stderr_final_line: stderr_drain.final_line(),
            capture_failed: stdout_drain.capture_failed
                || stderr_drain.capture_failed
                || stdout_join_failed
                || stderr_join_failed,
        })
    }

    fn unprobed_evidence(&self, hash: String) -> LeanRunnerEvidence {
        LeanRunnerEvidence {
            verifier_hash: self.config.verifier_hash.clone(),
            checker: "direct lean source checker + artifact audit".into(),
            checker_exe: "lean".into(),
            checker_artifact_hash: hash,
            package_dir: self.config.package_dir.display().to_string(),
            lean_version: String::new(),
            lake_version: String::new(),
            timeout_ms: self.config.timeout_ms,
            memory_limit_mb: self.config.memory_limit_mb,
            output_limit_bytes: self.config.output_limit_bytes,
            max_heartbeats: self.config.max_heartbeats,
            max_rec_depth: self.config.max_rec_depth,
        }
    }

    pub fn evidence(&self) -> Result<LeanRunnerEvidence> {
        // SC.9b / ADR-0016 (a-2) — record the toolchain the checker
        // PROCESS actually runs under (package-dir dispatch), never the
        // ambient PATH's lean/lake: an identity no proof was checked
        // under is evidence of nothing.
        let toolchain = effective_toolchain_identity_with_isolation(
            &self.config.package_dir,
            self.config.isolation_mode,
        )?;
        Ok(LeanRunnerEvidence {
            verifier_hash: self.config.verifier_hash.clone(),
            checker: "direct lean source checker + artifact audit".to_string(),
            checker_exe: "lean".to_string(),
            checker_artifact_hash: checker_artifact_hash(&self.config.package_dir)?,
            package_dir: self.config.package_dir.display().to_string(),
            lean_version: toolchain.lean_version,
            lake_version: toolchain.lake_version,
            timeout_ms: self.config.timeout_ms,
            memory_limit_mb: self.config.memory_limit_mb,
            output_limit_bytes: self.config.output_limit_bytes,
            max_heartbeats: self.config.max_heartbeats,
            max_rec_depth: self.config.max_rec_depth,
        })
    }
}

#[cfg(unix)]
fn child_finished_without_reaping(child: &mut Child) -> std::io::Result<bool> {
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    let rc = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id() as libc::id_t,
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let info = unsafe { info.assume_init() };
    Ok(unsafe { info.si_pid() } != 0)
}

#[cfg(not(unix))]
fn child_finished_without_reaping(child: &mut Child) -> std::io::Result<bool> {
    child.try_wait().map(|status| status.is_some())
}

/// The result of running one sandboxed child process to completion (or to
/// timeout). Both the primary checker invocation and the TB.1 artifact audit
/// produce one of these via `LeanRunner::run_sandboxed_with_config`.
struct SandboxedRunOutcome {
    success: bool,
    exit_code: i32,
    stdout: String,
    stderr: String,
    timed_out: bool,
    output_truncated: bool,
    stdout_truncated: bool,
    stdout_bytes: u64,
    stderr_bytes: u64,
    stderr_final_line: Option<String>,
    capture_failed: bool,
}

/// Released v2 checker artifact identity (audit grammar v2).
pub const CHECKER_RELEASE_HASH: &str =
    "8174cf67577e7ab2490b6d4c98f75bc131056462fa50d9909dd1eb47311b9ea6";
/// Frozen v1 checker used by existing pinned networks.
pub const CHECKER_RELEASE_HASH_V1: &str =
    "f9da3a1c5bcb605a26c8f778e2661e471398864c69393079aed461dd1453d7b4";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditGrammar {
    V1,
    V2,
}

pub const CHECKER_RELEASES: &[(&str, AuditGrammar)] = &[
    (CHECKER_RELEASE_HASH_V1, AuditGrammar::V1),
    (CHECKER_RELEASE_HASH, AuditGrammar::V2),
];

pub fn checker_audit_grammar(hash: &str) -> Option<AuditGrammar> {
    CHECKER_RELEASES
        .iter()
        .find_map(|(pin, grammar)| (*pin == hash).then_some(*grammar))
}

/// Relative source-reading primary entrypoint within either released package.
const PRIMARY_CHECKER_SCRIPT: &str = "BooleCheck/Main.lean";

/// Relative path (from the checker package root) to the dedicated axiom
/// audit entrypoint. See `BooleCheck/Audit.lean`'s own header comment for
/// why this MUST be a separate `lean --run` process rather than a
/// check folded into `BooleCheck.Main`.
const AXIOM_AUDIT_SCRIPT: &str = "BooleCheck/Audit.lean";

// Compiling pinned setup sources is separate from submitted elaboration.
// V2 applies committed counters inside its source-reading frontend; trusted
// script/helper startup must not turn a small valid submission into availability.
const TRUSTED_HELPER_MAX_HEARTBEATS: u64 = 400_000;
const TRUSTED_HELPER_MAX_REC_DEPTH: u64 = 512;

/// Line prefix `BooleCheck/Audit.lean` prints once per axiom in the closure,
/// e.g. `BOOLE_AXIOM_JSON ["propext"]`.
const AXIOM_AUDIT_LINE_PREFIX: &str = "BOOLE_AXIOM_JSON ";

const AUDIT_SEMANTIC_REJECT_PREFIXES: &[&str] = &[
    "BOOLE_UNSUPPORTED_MODULE_ARTIFACT",
    "BOOLE_UNEXPECTED_IMPORT ",
    "BOOLE_MALFORMED_ARTIFACT ",
    "BOOLE_UNREPLAYABLE_CONSTANT ",
];

/// Sentinel line `BooleCheck/Audit.lean` prints only after it has finished
/// walking the full axiom closure. Its absence (crash, timeout, SIGKILL)
/// must be treated as rejection, never as silent acceptance.
const AXIOM_AUDIT_DONE_SENTINEL: &str = "BOOLE_AXIOM_AUDIT_DONE count=";

fn enforce_axiom_allowlist(
    outcome: &SandboxedRunOutcome,
) -> std::result::Result<(), (LeanVerdict, String)> {
    enforce_axiom_allowlist_with_grammar(outcome, AuditGrammar::V2)
}

fn enforce_axiom_allowlist_v1(
    outcome: &SandboxedRunOutcome,
) -> std::result::Result<(), (LeanVerdict, String)> {
    enforce_axiom_allowlist_with_grammar(outcome, AuditGrammar::V1)
}

/// A complete transcript must match the grammar bound to its released checker.
fn enforce_axiom_allowlist_with_grammar(
    outcome: &SandboxedRunOutcome,
    grammar: AuditGrammar,
) -> std::result::Result<(), (LeanVerdict, String)> {
    let unavailable = |reason: &str| {
        (
            LeanVerdict::RetryableUnavailable {
                reason: reason.into(),
            },
            reason.into(),
        )
    };
    let rejected = || {
        (
            LeanVerdict::DeterministicReject {
                reason: REJECT_AXIOM_AUDIT.into(),
            },
            "audit grammar or axiom allowlist rejected".into(),
        )
    };
    if outcome.timed_out {
        return Err(unavailable("containment_wall_clock_kill"));
    }
    if outcome.exit_code < 0 {
        return Err(unavailable("containment_killed"));
    }
    if outcome.capture_failed {
        return Err(unavailable("capture_failed"));
    }
    if grammar == AuditGrammar::V1 && release_v1_budget(&release_v1_output(outcome)) {
        return Err((
            LeanVerdict::DeterministicReject {
                reason: REJECT_BUDGET_EXCEEDED.into(),
            },
            "audit exhausted committed budget".into(),
        ));
    }
    if !outcome.success {
        if grammar == AuditGrammar::V2
            && outcome
                .stderr_final_line
                .as_deref()
                .is_some_and(is_budget_marker)
        {
            return Err((
                LeanVerdict::DeterministicReject {
                    reason: REJECT_BUDGET_EXCEEDED.into(),
                },
                "audit exhausted committed budget".into(),
            ));
        }
        let semantic_reject = if grammar == AuditGrammar::V1 {
            release_v1_output(outcome).lines().any(|line| {
                AUDIT_SEMANTIC_REJECT_PREFIXES
                    .iter()
                    .any(|prefix| line.starts_with(prefix))
            })
        } else {
            outcome.stderr_final_line.as_ref().is_some_and(|line| {
                AUDIT_SEMANTIC_REJECT_PREFIXES
                    .iter()
                    .any(|prefix| line.starts_with(prefix))
            })
        };
        if semantic_reject {
            let (verdict, _) = rejected();
            return Err((
                verdict,
                format!(
                    "artifact audit rejected (exit_code={}): {}",
                    outcome.exit_code, outcome.stderr
                ),
            ));
        }
        return Err(unavailable("axiom_audit_unavailable"));
    }
    if outcome.stdout_truncated || !outcome.stdout.ends_with('\n') || outcome.stdout.contains('\r')
    {
        return Err(rejected());
    }
    let lines: Vec<_> = outcome.stdout[..outcome.stdout.len() - 1]
        .split('\n')
        .collect();
    if grammar == AuditGrammar::V1 {
        if lines.last() != Some(&"BOOLE_AXIOM_AUDIT_DONE") {
            return Err(rejected());
        }
        for line in &lines[..lines.len() - 1] {
            let Some(name) = line.strip_prefix("BOOLE_AXIOM ") else {
                return Err(rejected());
            };
            if !matches!(name, "propext" | "Classical.choice" | "Quot.sound") {
                return Err(rejected());
            }
        }
        return Ok(());
    }
    let count = lines
        .last()
        .and_then(|line| line.strip_prefix(AXIOM_AUDIT_DONE_SENTINEL))
        .ok_or_else(rejected)?;
    if count.is_empty()
        || !count.bytes().all(|b| b.is_ascii_digit())
        || (count.len() > 1 && count.starts_with('0'))
        || count.parse::<usize>().ok() != Some(lines.len() - 1)
    {
        return Err(rejected());
    }
    for line in &lines[..lines.len() - 1] {
        let json = line
            .strip_prefix(AXIOM_AUDIT_LINE_PREFIX)
            .ok_or_else(rejected)?;
        let components: serde_json::Value = serde_json::from_str(json).map_err(|_| rejected())?;
        if serde_json::to_string(&components).map_err(|_| rejected())? != json {
            return Err(rejected());
        }
        let array = components
            .as_array()
            .filter(|a| !a.is_empty())
            .ok_or_else(rejected)?;
        if !array.iter().all(|v| v.is_string() || v.as_u64().is_some()) {
            return Err(rejected());
        }
        if components != serde_json::json!(["propext"])
            && components != serde_json::json!(["Classical", "choice"])
            && components != serde_json::json!(["Quot", "sound"])
        {
            return Err(rejected());
        }
    }
    Ok(())
}

/// Typed marker line `BooleCheck/Main.lean` prints (and exits non-zero on)
/// when the submitted source contains a budget-bearing option token —
/// ADR-0016 (a-2) layer 2, independent of the Rust-side intake scan.
const BUDGET_OVERRIDE_MARKER_PREFIX: &str = "BOOLE_BUDGET_OVERRIDE ";

/// Printed only when the primary completed normal elaboration and Lean
/// rejected the submitted source. A non-zero exit without this marker is a
/// checker/runtime failure and therefore cannot become a proof verdict.
const PRIMARY_REJECT_MARKER: &str = "BOOLE_PRIMARY_REJECT";

fn is_budget_marker(line: &str) -> bool {
    matches!(
        line,
        "BOOLE_BUDGET_EXCEEDED maxHeartbeats" | "BOOLE_BUDGET_EXCEEDED maxRecDepth"
    )
}

fn release_v1_output(outcome: &SandboxedRunOutcome) -> String {
    let mut output = outcome.stdout.clone();
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(&outcome.stderr);
    output
}

fn release_v1_budget(output: &str) -> bool {
    output.contains("maximum number of heartbeats")
        || output.contains("maximum recursion depth has been reached")
}

fn classify_failed_run_v1(outcome: &SandboxedRunOutcome) -> LeanVerdict {
    if outcome.timed_out || outcome.exit_code < 0 || outcome.capture_failed {
        return classify_failed_run(outcome);
    }
    let output = release_v1_output(outcome);
    let reason = if output
        .lines()
        .any(|line| line.starts_with("BOOLE_BUDGET_OVERRIDE"))
    {
        REJECT_BUDGET_OVERRIDE_FORBIDDEN
    } else if release_v1_budget(&output) {
        REJECT_BUDGET_EXCEEDED
    } else if output.lines().any(|line| line == PRIMARY_REJECT_MARKER) {
        REJECT_LEAN_REJECTED
    } else {
        return LeanVerdict::RetryableUnavailable {
            reason: "primary_checker_unavailable".into(),
        };
    };
    LeanVerdict::DeterministicReject {
        reason: reason.into(),
    }
}

/// SC.9a / ADR-0016 (a-3) — classify a failed primary checker run into the
/// three-state verdict contract: containment (wall-clock kill or signal
/// death) is `retryable_unavailable`; only a typed checker marker can become
/// a deterministic proof reject. Committed-budget exhaustion remains typed
/// as `budget_exceeded`.
fn classify_failed_run(outcome: &SandboxedRunOutcome) -> LeanVerdict {
    if outcome.timed_out {
        return LeanVerdict::RetryableUnavailable {
            reason: "containment_wall_clock_kill".to_string(),
        };
    }
    if outcome.exit_code < 0 {
        // Signal death (RLIMIT_CPU SIGKILL, OOM kill, sandbox kill) carries
        // no exit code; `run_sandboxed` records it as -1.
        return LeanVerdict::RetryableUnavailable {
            reason: "containment_killed".to_string(),
        };
    }
    if outcome.capture_failed {
        return LeanVerdict::RetryableUnavailable {
            reason: "capture_failed".into(),
        };
    }
    if outcome
        .stderr_final_line
        .as_ref()
        .is_some_and(|line| line.starts_with(BUDGET_OVERRIDE_MARKER_PREFIX))
    {
        return LeanVerdict::DeterministicReject {
            reason: REJECT_BUDGET_OVERRIDE_FORBIDDEN.into(),
        };
    }
    if outcome
        .stderr_final_line
        .as_deref()
        .is_some_and(is_budget_marker)
    {
        return LeanVerdict::DeterministicReject {
            reason: REJECT_BUDGET_EXCEEDED.into(),
        };
    }
    if outcome.stderr_final_line.as_deref() == Some(PRIMARY_REJECT_MARKER) {
        return LeanVerdict::DeterministicReject {
            reason: REJECT_LEAN_REJECTED.into(),
        };
    }
    LeanVerdict::RetryableUnavailable {
        reason: "primary_checker_unavailable".to_string(),
    }
}

struct DrainBuffer {
    head: Vec<u8>,
    tail: Vec<u8>,
    limit: usize,
    total: u64,
    truncated: bool,
    capture_failed: bool,
    prefix_only: bool,
}

impl DrainBuffer {
    #[cfg(test)]
    fn new(limit: usize) -> Self {
        Self::for_release(limit, false)
    }
    fn for_release(limit: usize, prefix_only: bool) -> Self {
        Self {
            prefix_only,
            head: Vec::new(),
            tail: Vec::new(),
            limit,
            total: 0,
            truncated: false,
            capture_failed: false,
        }
    }
    fn push(&mut self, chunk: &[u8]) {
        self.total = self.total.saturating_add(chunk.len() as u64);
        self.truncated = self.total > self.limit as u64;
        let head_limit = if self.prefix_only {
            self.limit
        } else {
            self.limit / 2
        };
        let tail_limit = self.limit - head_limit;
        let n = chunk.len().min(head_limit.saturating_sub(self.head.len()));
        self.head.extend_from_slice(&chunk[..n]);
        if tail_limit == 0 {
            return;
        }
        if chunk.len() >= tail_limit {
            self.tail.clear();
            self.tail
                .extend_from_slice(&chunk[chunk.len() - tail_limit..]);
        } else {
            let excess = (self.tail.len() + chunk.len()).saturating_sub(tail_limit);
            self.tail.drain(..excess);
            self.tail.extend_from_slice(chunk);
        }
    }
    fn retained(&self) -> Vec<u8> {
        let overlap = (self.head.len() + self.tail.len())
            .saturating_sub(self.total.min(usize::MAX as u64) as usize);
        let mut out = self.head.clone();
        out.extend_from_slice(&self.tail[overlap..]);
        out
    }
    fn final_line(&self) -> Option<String> {
        let body = self.tail.strip_suffix(b"\n")?;
        let start = match body.iter().rposition(|b| *b == b'\n') {
            Some(position) => position + 1,
            None if self.total == self.tail.len() as u64 => 0,
            None => return None,
        };
        std::str::from_utf8(&body[start..]).ok().map(str::to_owned)
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
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    if let Ok(mut guard) = sink.lock() {
                        guard.capture_failed = true;
                    }
                    break;
                }
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

// Both runners use the same patterns and ASCII word boundaries. The text
// policy scans comments and strings too; the axiom audit remains the authority.
#[derive(Deserialize)]
struct TextPattern {
    text: String,
    label: String,
    boundary: String,
}

#[derive(Deserialize)]
struct AttributeLimits {
    markers: usize,
    brackets: usize,
    reported_lines: usize,
}

#[derive(Clone, Deserialize)]
struct OutputOptions {
    forbidden_roots: Vec<String>,
    name_grammar: String,
}

#[derive(Deserialize)]
struct TextPolicyData {
    patterns: Vec<TextPattern>,
    safe_attributes: Vec<String>,
    ascii_whitespace: String,
    attribute_limits: AttributeLimits,
    output_options: OutputOptions,
    forbidden_name_characters: Vec<String>,
}

#[derive(Deserialize)]
#[serde(try_from = "TextPolicyData")]
struct TextPolicy {
    patterns: Vec<TextPattern>,
    safe_attributes: Vec<String>,
    ascii_whitespace: String,
    attribute_limits: AttributeLimits,
    output_options: OutputOptions,
    option_name: regex_automata::meta::Regex,
    forbidden_name_ranges: Vec<(u32, u32)>,
}

impl TryFrom<TextPolicyData> for TextPolicy {
    type Error = &'static str;

    fn try_from(data: TextPolicyData) -> std::result::Result<Self, Self::Error> {
        if !data.ascii_whitespace.is_ascii() {
            return Err("attribute whitespace must be ASCII only");
        }
        for rule in &data.patterns {
            let bytes = rule.text.as_bytes();
            if (matches!(rule.boundary.as_str(), "word" | "prefix")
                && !bytes.first().is_some_and(|byte| ascii_word(*byte)))
                || (matches!(rule.boundary.as_str(), "word" | "suffix")
                    && !bytes.last().is_some_and(|byte| ascii_word(*byte)))
            {
                return Err("bounded pattern edges must be ASCII word characters");
            }
        }
        let option_name =
            regex_automata::meta::Regex::new(&format!("^(?:{})", data.output_options.name_grammar))
                .map_err(|_| "invalid output option name grammar")?;
        let mut forbidden_name_ranges = Vec::new();
        for range in &data.forbidden_name_characters {
            let parts: Vec<_> = range.split('-').collect();
            if parts.len() > 2 {
                return Err("invalid character range");
            }
            let parse = |value: &str| {
                u32::from_str_radix(
                    value.strip_prefix("U+").ok_or("invalid character range")?,
                    16,
                )
                .map_err(|_| "invalid character range")
            };
            let lo = parse(parts[0])?;
            let hi = parse(parts[parts.len() - 1])?;
            if lo > hi || hi > 0x10ffff {
                return Err("invalid character range");
            }
            forbidden_name_ranges.push((lo, hi));
        }
        Ok(Self {
            output_options: data.output_options,
            option_name,
            forbidden_name_ranges,
            patterns: data.patterns,
            safe_attributes: data.safe_attributes,
            ascii_whitespace: data.ascii_whitespace,
            attribute_limits: data.attribute_limits,
        })
    }
}

fn text_policy() -> &'static TextPolicy {
    static POLICY: OnceLock<TextPolicy> = OnceLock::new();
    POLICY.get_or_init(|| {
        // This internal workspace crate is built from the repository and is
        // never packaged with `cargo package`; the shared authority stays here.
        serde_json::from_str(include_str!(
            "../../../scripts/zk_registry/data/submission_policy_v5.json"
        ))
        .expect("valid shared text policy")
    })
}

fn ascii_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn ascii_space(byte: u8) -> bool {
    text_policy().ascii_whitespace.as_bytes().contains(&byte)
}

fn trim_ascii_space(bytes: &[u8]) -> &[u8] {
    &bytes[bytes
        .iter()
        .position(|byte| !ascii_space(*byte))
        .unwrap_or(bytes.len())..]
}

fn text_pattern_hit(text: &str, rule: &TextPattern) -> Option<usize> {
    let bytes = text.as_bytes();
    for (start, _) in text.match_indices(rule.text.as_str()) {
        let end = start + rule.text.len();
        if matches!(rule.boundary.as_str(), "word" | "prefix")
            && start > 0
            && ascii_word(bytes[start - 1])
        {
            continue;
        }
        if matches!(rule.boundary.as_str(), "word" | "suffix")
            && bytes.get(end).is_some_and(|byte| ascii_word(*byte))
        {
            continue;
        }
        return Some(start);
    }
    None
}

// Storage is bounded by the shared nesting and reported-line limits. Plain
// delimiters outside attributes need no stack. An overflow is already INVALID;
// count its brackets without retaining additional frames until it ends.
fn forbidden_attribute(text: &str) -> (Vec<usize>, bool) {
    struct Frame {
        depth: usize,
        line: usize,
        entry: usize,
    }
    fn allowed_entry(text: &[u8]) -> bool {
        let trimmed = trim_ascii_space(text);
        let trimmed = [b"local".as_slice(), b"scoped".as_slice()]
            .iter()
            .find_map(|prefix| {
                trimmed
                    .strip_prefix(*prefix)
                    .filter(|rest| rest.first().is_some_and(|byte| ascii_space(*byte)))
            })
            .unwrap_or(trimmed);
        let trimmed = trim_ascii_space(trimmed);
        let trimmed = trimmed.strip_prefix(b"-").unwrap_or(trimmed);
        let end = trimmed
            .iter()
            .position(|byte| ascii_space(*byte) || b",()[]{}".contains(byte))
            .unwrap_or(trimmed.len());
        text_policy()
            .safe_attributes
            .iter()
            .any(|name| name.as_bytes() == &trimmed[..end])
    }
    let bytes = text.as_bytes();
    let limits = &text_policy().attribute_limits;
    let mut closers: Vec<u8> = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();
    let mut bad = std::collections::BTreeSet::new();
    let mut more = false;
    let mut overflow = 0usize;
    let mut line = 1;
    let mut reject = |marker_line: usize| {
        if bad.contains(&marker_line) {
            return;
        }
        if bad.len() < limits.reported_lines {
            bad.insert(marker_line);
        } else {
            more = true;
            if marker_line < *bad.last().unwrap() {
                bad.pop_last();
                bad.insert(marker_line);
            }
        }
    };
    for (index, byte) in bytes.iter().copied().enumerate() {
        if byte == b'\n' {
            line += 1;
        }
        let marker_line = if byte == b'[' {
            let mut before = index;
            let mut start_line = line;
            while before > 0 && ascii_space(bytes[before - 1]) {
                before -= 1;
                if bytes[before] == b'\n' {
                    start_line -= 1;
                }
            }
            if (before > 0 && bytes[before - 1] == b'@')
                || (before >= 9
                    && &bytes[before - 9..before] == b"attribute"
                    && (before == 9 || !ascii_word(bytes[before - 10])))
            {
                Some(start_line)
            } else {
                None
            }
        } else {
            None
        };
        if overflow > 0 {
            if let Some(marker_line) = marker_line {
                reject(marker_line);
            }
            if b"[({".contains(&byte) {
                overflow += 1;
            } else if b"])}".contains(&byte) {
                overflow -= 1;
            }
            continue;
        }
        match byte {
            b'[' | b'(' | b'{' if !frames.is_empty() || marker_line.is_some() => {
                if closers.len() == limits.brackets
                    || (marker_line.is_some() && frames.len() == limits.markers)
                {
                    reject(marker_line.unwrap_or_else(|| frames.last().unwrap().line));
                    overflow = 1;
                    continue;
                }
                closers.push(match byte {
                    b'[' => b']',
                    b'(' => b')',
                    _ => b'}',
                });
                if let Some(marker_line) = marker_line {
                    frames.push(Frame {
                        depth: closers.len(),
                        line: marker_line,
                        entry: index + 1,
                    });
                }
            }
            b']' | b')' | b'}' => {
                while closers.last().is_some_and(|close| *close != byte) {
                    if frames
                        .last()
                        .is_some_and(|frame| frame.depth == closers.len())
                    {
                        reject(frames.pop().unwrap().line);
                    }
                    closers.pop();
                }
                if !closers.is_empty() {
                    if frames
                        .last()
                        .is_some_and(|frame| frame.depth == closers.len())
                    {
                        let frame = frames.pop().unwrap();
                        if !allowed_entry(&bytes[frame.entry..index]) {
                            reject(frame.line);
                        }
                    }
                    closers.pop();
                }
            }
            b',' => {
                if let Some(frame) = frames
                    .last_mut()
                    .filter(|frame| frame.depth == closers.len())
                {
                    if !allowed_entry(&bytes[frame.entry..index]) {
                        reject(frame.line);
                    }
                    frame.entry = index + 1;
                }
            }
            _ => {}
        }
    }
    for frame in frames {
        reject(frame.line);
    }
    (bad.into_iter().collect(), more)
}

/// Import paths a submitted proof file may reference. ADR-0013's blacklist
/// hardening step: only the shared, human-reviewed helper surface is
/// reachable from a submission — anything else (in particular `import
/// Lean`, which the `elab`/`addDecl` escapes both require) is rejected
/// pre-spawn.
const ALLOWED_IMPORTS: &[&str] = &["Boole.Family.V0Helpers"];

/// Returns the disallowed module name if `line` is an `import` declaration
/// naming something outside [`ALLOWED_IMPORTS`], or `None` if the line is
/// not an import at all, or names an allowed module.
fn disallowed_import_on_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("import")?;
    // `import` must be a whole keyword: the next byte (if any) must not be a
    // word character, else this is an identifier like `importantThing`, not
    // the `import` command.
    let starts_with_word_char = rest
        .as_bytes()
        .first()
        .map(|&b| b.is_ascii_alphanumeric() || b == b'_')
        .unwrap_or(false);
    if starts_with_word_char {
        return None;
    }
    let module = rest.trim();
    if module.is_empty() {
        Some("<missing module>".to_string())
    } else if ALLOWED_IMPORTS.contains(&module) {
        None
    } else {
        Some(module.to_string())
    }
}

/// Locate an output option disallowed by the digest-bound policy.
fn output_option_position(text: &str) -> Option<usize> {
    output_option_position_with_policy(text, text_policy())
}

fn output_option_position_with_policy(text: &str, policy: &TextPolicy) -> Option<usize> {
    let bytes = text.as_bytes();
    for (start, _) in text.match_indices("set_option") {
        let end = start + 10;
        if (start > 0 && ascii_word(bytes[start - 1]))
            || bytes.get(end).is_some_and(|b| ascii_word(*b))
        {
            continue;
        }
        let mut pos = end;
        while bytes.get(pos).is_some_and(|b| ascii_space(*b)) {
            pos += 1;
        }
        if pos == end {
            return Some(start);
        }
        let Some(name) = policy.option_name.find(&text[pos..]) else {
            return Some(start);
        };
        if policy
            .output_options
            .forbidden_roots
            .iter()
            .any(|root| Some(root.as_str()) == text[pos..][name.range()].split('.').next())
        {
            return Some(start);
        }
    }
    None
}
fn forbidden_name_position(text: &str) -> Option<usize> {
    forbidden_name_position_with_policy(text, text_policy())
}

fn forbidden_name_position_with_policy(text: &str, policy: &TextPolicy) -> Option<usize> {
    let mut position = 0;
    while let Some(relative) = text[position..].find('«') {
        let start = position + relative;
        let body = start + '«'.len_utf8();
        let end = text[body..].find('»').map_or(text.len(), |n| body + n);
        if text[body..end].chars().any(|c| {
            policy
                .forbidden_name_ranges
                .iter()
                .any(|(lo, hi)| (*lo..=*hi).contains(&(c as u32)))
        }) {
            return Some(start);
        }
        if end == text.len() {
            break;
        }
        position = end + '»'.len_utf8();
    }
    None
}

fn scan_for_forbidden_tokens_in_bytes(bytes: &[u8]) -> Option<(String, usize)> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Some(("submission-source-encoding".to_string(), 1));
    };
    let line_at = |position| {
        text[..position]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1
    };
    for rule in &text_policy().patterns {
        if let Some(position) = text_pattern_hit(text, rule) {
            let end = position + rule.text.len();
            let word = &text[position..end];
            let reason = if rule.label.contains(word) {
                rule.label.clone()
            } else {
                format!("{} [forbidden word: {word}]", rule.label)
            };
            return Some((reason, line_at(position)));
        }
    }
    for (label, position) in [
        ("output-option", output_option_position(text)),
        ("forbidden-name-character", forbidden_name_position(text)),
    ] {
        if let Some(position) = position {
            return Some((label.into(), line_at(position)));
        }
    }
    let (lines, more) = forbidden_attribute(text);
    if let Some(line) = lines.first() {
        let reason = if more {
            "forbidden-attribute [more offending lines omitted]"
        } else {
            "forbidden-attribute"
        };
        return Some((reason.to_string(), *line));
    }
    for (index, line) in text.lines().enumerate() {
        if let Some(module) = disallowed_import_on_line(line) {
            return Some((format!("import {module}"), index + 1));
        }
    }
    None
}

#[cfg(test)]
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
/// [`scan_for_forbidden_tokens_in_bytes`].
#[cfg(test)]
fn contains_sorry_token(line: &str) -> bool {
    contains_forbidden_token(line, b"sorry")
}

// A minimal PATH covering common locations for `lake`/`lean` on macOS and
// Linux developer machines. Operators that install Lean elsewhere can set
// BOOLE_LEAN_PATH to override. Shared by `configure_child_environment` (what
// PATH the child sees) and, on Linux/macOS, the kernel-isolation exec
// allowlist (ADR-0008) — the toolchain directories the checker is allowed to
// `exec` from are derived from this same value, so the two never disagree
// about "where `lake`/`lean` live".
fn resolved_child_path() -> String {
    std::env::var("BOOLE_LEAN_PATH")
        .ok()
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_else(|| "/usr/local/bin:/usr/bin:/bin".to_string())
}

fn configure_child_environment(command: &mut Command) {
    command.env_clear();
    command.env("PATH", resolved_child_path());
    if let Ok(home) = std::env::var("HOME") {
        command.env("HOME", home);
    }
    command.env("LANG", "C.UTF-8");
}

#[cfg(unix)]
fn configure_child_sandbox(command: &mut Command, config: &LeanRunnerConfig) -> Result<()> {
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
    install_kernel_isolation(command, config)
}

// ADR-0008 — kernel-layer isolation on top of the pgroup/rlimit/env-scrub
// baseline above. Scope for this landing slice is exactly the three
// characterization guards named in the ADR: deny network egress, deny
// filesystem writes outside explicitly configured request scratch, deny `exec` of
// anything outside the Lean toolchain. Read access is intentionally left
// unrestricted on both platforms: enumerating every path the Lean toolchain
// and dynamic linker legitimately read (shared library search paths, dyld's
// shared cache on macOS, locale/timezone data, etc.) needs a real trace of
// `lake`/`lean`'s syscalls that isn't available on this dev machine, and
// getting it wrong would make the checker fail to even start. That tuning
// is exactly what the ADR's log-mode-by-default phase (decision 4) exists
// for, ahead of the N3.2 commit that flips Enforce on for real untrusted
// traffic.
//
// `scratch_dirs`/`exec_allow_dirs` are shared by both platform
// implementations so the Linux and macOS policies describe the same paths.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn scratch_dirs(config: &LeanRunnerConfig) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if config.package_dir_writable {
        dirs.push(config.package_dir.clone());
    }
    if let Some(dir) = &config.artifact_scratch_dir {
        dirs.push(dir.clone());
    }
    dirs
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn exec_allow_dirs(config: &LeanRunnerConfig) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = resolved_child_path()
        .split(':')
        .filter(|entry| !entry.is_empty())
        .map(PathBuf::from)
        .collect();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".elan"));
    }
    // Real verification executes the canonical package-pinned `lean` under
    // `LEAN_SYSROOT/bin`; checker sources in the package are read, never
    // executed as prebuilt binaries. Historical sandbox probes keep their
    // owned package directory executable only when it is also writable.
    if let Some(sysroot) = &config.lean_sysroot {
        dirs.push(PathBuf::from(sysroot).join("bin"));
    }
    if config.package_dir_writable {
        dirs.push(config.package_dir.clone());
    }
    dirs
}

// macOS — Seatbelt (`sandbox_init`), the mechanism Bazel/Chromium build
// sandboxes use in production (ADR-0008 ratified decision 3: macOS is a
// co-equal enforcement tier, not a documented-weaker fallback). `sandbox_init`
// is a private-but-stable libSystem entry point; there is no published Rust
// crate wrapping it, so we declare the FFI signature directly (the same
// approach every macOS build-sandbox tool uses).
//
// Log mode: Seatbelt has no non-blocking "log but allow" level (unlike
// seccomp's `SECCOMP_RET_LOG` on Linux) — once a profile is installed the
// kernel actually enforces it. So `Log` mode simply does not call
// `sandbox_init` at all, which is behaviorally identical to today's
// pre-ADR-0008 baseline and carries zero risk of breaking the checker.
// `Enforce` mode installs a profile built from `scratch_dirs`/
// `exec_allow_dirs` using an "allow default, then deny the three specific
// things" shape: this both matches the ADR's own phrasing ("deny network,
// filesystem allowlist, restrict process-exec") and is far more robust than
// a hand-tuned deny-default allowlist, which easily breaks process startup
// by missing some mach-lookup/sysctl/IPC right the dynamic linker or libSystem
// needs — exactly the class of failure log-mode tuning is meant to avoid.
#[cfg(target_os = "macos")]
extern "C" {
    fn sandbox_init(
        profile: *const std::os::raw::c_char,
        flags: u64,
        errorbuf: *mut *mut std::os::raw::c_char,
    ) -> std::os::raw::c_int;
}

// SBPL `subpath` is compared against the fully resolved (symlink-free) path
// the kernel sees at access time. On macOS both `/tmp` and `/var` — and
// therefore `$TMPDIR`, which every temp-dir-based path (including test
// fixtures) is rooted under — are themselves symlinks into `/private/...`,
// so a profile built from the literal, unresolved path would silently fail
// to match real accesses underneath it. Canonicalize each configured dir
// (falling back to the original path if it does not exist yet, e.g. before
// first use) so the profile always names the same real path the kernel
// enforces against.
#[cfg(target_os = "macos")]
fn canonical_or_self(dir: &Path) -> PathBuf {
    std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf())
}

#[cfg(target_os = "macos")]
fn seatbelt_profile(config: &LeanRunnerConfig) -> String {
    let write_subpaths: Vec<String> = scratch_dirs(config)
        .iter()
        .map(|dir| {
            format!(
                "(subpath {:?})",
                canonical_or_self(dir).display().to_string()
            )
        })
        .collect();
    let write_rule = if write_subpaths.is_empty() {
        "(deny file-write*)".to_string()
    } else {
        format!(
            "(deny file-write*\n  (require-not\n    (require-any\n      {})))",
            write_subpaths.join("\n      ")
        )
    };
    let exec_subpaths: String = exec_allow_dirs(config)
        .iter()
        .map(|dir| {
            format!(
                "(subpath {:?})",
                canonical_or_self(dir).display().to_string()
            )
        })
        .collect::<Vec<_>>()
        .join("\n      ");
    let process_spawn_rule = if config.deny_process_spawn {
        "(deny process-fork)"
    } else {
        ""
    };
    format!(
        r#"(version 1)
(allow default)
(deny network*)
(deny process-info-setcontrol)
{process_spawn_rule}
{write_rule}
(deny process-exec
  (require-not
    (require-any
      {exec_subpaths})))
"#
    )
}

#[cfg(target_os = "macos")]
fn seatbelt_profile_cstring(profile: String) -> Result<std::ffi::CString> {
    std::ffi::CString::new(profile).context("Seatbelt profile contains a NUL byte")
}

#[cfg(target_os = "macos")]
fn install_kernel_isolation(command: &mut Command, config: &LeanRunnerConfig) -> Result<()> {
    use std::os::unix::process::CommandExt;
    if config.isolation_mode != IsolationMode::Enforce {
        // Log mode: no non-blocking Seatbelt level exists, so install
        // nothing (see module comment above).
        return Ok(());
    }
    let profile = seatbelt_profile(config);
    let c_profile = seatbelt_profile_cstring(profile)?;
    unsafe {
        command.pre_exec(move || {
            let mut errorbuf: *mut std::os::raw::c_char = std::ptr::null_mut();
            let rc = sandbox_init(c_profile.as_ptr(), 0, &mut errorbuf);
            if rc != 0 {
                return Err(std::io::Error::other(
                    "sandbox_init failed while installing the ADR-0008 Seatbelt profile",
                ));
            }
            Ok(())
        });
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn install_kernel_isolation(_command: &mut Command, _config: &LeanRunnerConfig) -> Result<()> {
    // Other Unix targets (e.g. BSD) are not part of Boole's supported dev/CI
    // matrix; they keep the portable pgroup/rlimit/env-scrub baseline only.
    Ok(())
}

// Linux — seccomp-bpf (`seccompiler`, rust-vmm/Firecracker provenance) denies
// network egress; Landlock (`landlock`, the kernel feature author's reference
// binding) denies filesystem writes/exec outside the allowlisted dirs.
// ADR-0008 assigns both "deny network egress" and "deny arbitrary execve" to
// seccomp, but seccomp-bpf can only inspect syscall *integer* arguments, not
// pointee data like an execve path — it cannot express "deny execve of any
// path outside these directories" by itself. Landlock's `AccessFs::Execute`
// right (available since ABI v1 / kernel 5.13, the kernel's purpose-built
// mechanism for exactly this) implements the path-scoped exec restriction
// instead, while seccomp keeps the network-egress-syscall denylist it can
// express natively. Both crates and both scopes are exactly as ratified;
// this is a choice of *which layer implements which specific denial*, not a
// change to scope, dependencies, or cfg-gating.
#[cfg(target_os = "linux")]
fn network_egress_syscalls() -> Vec<i64> {
    vec![
        libc::SYS_socket,
        libc::SYS_socketpair,
        libc::SYS_connect,
        libc::SYS_bind,
        libc::SYS_listen,
        libc::SYS_accept,
        libc::SYS_accept4,
        libc::SYS_sendto,
        libc::SYS_sendmsg,
        libc::SYS_recvfrom,
        libc::SYS_recvmsg,
    ]
}

#[cfg(target_os = "linux")]
fn process_group_escape_syscalls() -> Vec<i64> {
    vec![libc::SYS_setsid, libc::SYS_setpgid]
}

#[cfg(target_os = "linux")]
fn build_seccomp_programs(
    mode: IsolationMode,
    deny_process_spawn: bool,
) -> anyhow::Result<(seccompiler::BpfProgram, Option<seccompiler::BpfProgram>)> {
    use seccompiler::{
        SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
        SeccompRule, TargetArch,
    };

    let match_action = match mode {
        IsolationMode::Log => SeccompAction::Log,
        IsolationMode::Enforce => SeccompAction::Errno(libc::EACCES as u32),
    };
    let mut rules: std::collections::BTreeMap<i64, Vec<SeccompRule>> = network_egress_syscalls()
        .into_iter()
        .chain(process_group_escape_syscalls())
        .map(|sysno| (sysno, vec![]))
        .collect();
    let mut clone3_program = None;
    if deny_process_spawn {
        #[cfg(target_arch = "x86_64")]
        {
            rules.insert(libc::SYS_fork, vec![]);
            rules.insert(libc::SYS_vfork, vec![]);
        }
        let clone_process_rule = SeccompRule::new(vec![SeccompCondition::new(
            0,
            SeccompCmpArgLen::Qword,
            SeccompCmpOp::MaskedEq(libc::CLONE_THREAD as u64),
            0,
        )?])?;
        rules.insert(libc::SYS_clone, vec![clone_process_rule]);

        if mode == IsolationMode::Log {
            rules.insert(libc::SYS_clone3, vec![]);
        } else {
            let clone3_rules = [(libc::SYS_clone3, vec![])].into_iter().collect();
            let clone3_filter = SeccompFilter::new(
                clone3_rules,
                SeccompAction::Allow,
                SeccompAction::Errno(libc::ENOSYS as u32),
                TargetArch::try_from(std::env::consts::ARCH).map_err(|_| {
                    anyhow!(
                        "unsupported seccomp target arch: {}",
                        std::env::consts::ARCH
                    )
                })?,
            )?;
            clone3_program = Some(seccompiler::BpfProgram::try_from(clone3_filter)?);
        }
    }
    let arch = TargetArch::try_from(std::env::consts::ARCH).map_err(|_| {
        anyhow!(
            "unsupported seccomp target arch: {}",
            std::env::consts::ARCH
        )
    })?;
    let filter = SeccompFilter::new(rules, SeccompAction::Allow, match_action, arch)?;
    let program = seccompiler::BpfProgram::try_from(filter)?;
    Ok((program, clone3_program))
}

// Landlock's `Execute` right is checked via the kernel's `open_exec()` path
// (`FMODE_EXEC`-flagged opens), and that path is *also* how `load_elf_binary`
// opens a dynamically linked ELF's own interpreter (`PT_INTERP`, e.g.
// `/lib64/ld-linux-x86-64.so.2`) while the kernel is still processing the
// outer binary's execve — this is a second, distinct FMODE_EXEC open, not
// merely a read. Every `lake`/`lean` binary Boole runs is dynamically
// linked (as is this crate's own `sandbox_probe` test binary), so without an
// exec-allow rule covering the interpreter's own directory, `execve()`
// itself fails with EACCES the instant Landlock is restricted — even when
// the exec'd binary's own path is correctly allowlisted via
// `exec_allow_dirs`. This matches the upstream `landlock` crate's own
// reference sandboxer (`examples/sandboxer.rs`), whose usage example
// allowlists `/lib` and `/usr` alongside `$PATH` for exactly this reason.
//
// The shared libraries the interpreter subsequently loads (libc.so.6 etc.)
// are opened by the interpreter itself via a plain, non-exec `openat()`,
// which this ruleset does not restrict at all: `ReadFile`/`ReadDir` are not
// in `build_landlock_ruleset`'s `handled` set (see its module-level doc
// above), so only the interpreter's own exec-flagged open needs this rule.
//
// The interpreter's resolved real path (after following distro symlinks,
// e.g. Debian/Ubuntu's multiarch layout) varies by distro, so this
// allowlists the standard search locations rather than one exact path;
// each is Execute-only (never write) and a missing entry on a given distro
// is silently skipped by the same `PathFd::new` fallback used for
// `exec_allow_dirs`/`scratch_dirs` below.
#[cfg(target_os = "linux")]
fn dynamic_loader_exec_dirs() -> Vec<PathBuf> {
    [
        "/lib",
        "/lib64",
        "/usr/lib",
        "/usr/lib64",
        "/lib/x86_64-linux-gnu",
        "/usr/lib/x86_64-linux-gnu",
        "/lib/aarch64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

#[cfg(target_os = "linux")]
fn build_landlock_ruleset(config: &LeanRunnerConfig) -> anyhow::Result<landlock::RulesetCreated> {
    use landlock::{AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreatedAttr, ABI};

    // ABI v3 adds AccessFs::Truncate. Without it, path-based truncate(2) and
    // O_RDONLY|O_TRUNC can alter a package that is otherwise outside the
    // write allowlist. For ftruncate(2), Landlock attaches the truncate right
    // when the descriptor is opened; it cannot retroactively restrict a
    // writable descriptor inherited from before sandbox installation. The
    // verifier child receives no such checker-package descriptor.
    let abi = ABI::V3;
    let write_access = AccessFs::from_write(abi);
    let handled = write_access | AccessFs::Execute;
    let mut created = Ruleset::default().handle_access(handled)?.create()?;

    let exec_dirs = exec_allow_dirs(config)
        .into_iter()
        .chain(dynamic_loader_exec_dirs());
    for dir in exec_dirs {
        if let Ok(fd) = PathFd::new(&dir) {
            created = created.add_rule(PathBeneath::new(fd, AccessFs::Execute))?;
        }
    }
    for dir in scratch_dirs(config) {
        if let Ok(fd) = PathFd::new(&dir) {
            created = created.add_rule(PathBeneath::new(fd, write_access))?;
        }
    }
    Ok(created)
}

#[cfg(target_os = "linux")]
fn require_fully_enforced_landlock(
    ruleset: landlock::RulesetStatus,
    no_new_privs: bool,
) -> std::io::Result<()> {
    if matches!(ruleset, landlock::RulesetStatus::FullyEnforced) && no_new_privs {
        return Ok(());
    }
    Err(std::io::Error::other(format!(
        "Landlock Enforce mode was not fully installed: ruleset={ruleset:?}, no_new_privs={no_new_privs}"
    )))
}

#[cfg(target_os = "linux")]
fn install_kernel_isolation(command: &mut Command, config: &LeanRunnerConfig) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let mode = config.isolation_mode;
    let (program, clone3_program) = build_seccomp_programs(mode, config.deny_process_spawn)
        .context("failed to build the seccomp isolation program")?;
    // Landlock has no non-blocking level (like macOS Seatbelt, unlike
    // seccomp's RET_LOG); only build/apply the ruleset in Enforce mode so Log
    // mode's filesystem behavior stays identical to the pre-ADR-0008
    // baseline, matching the same reasoning as the macOS implementation.
    let ruleset = if mode == IsolationMode::Enforce {
        Some(
            build_landlock_ruleset(config)
                .context("failed to build the Landlock isolation ruleset")?,
        )
    } else {
        None
    };
    let mut ruleset = ruleset;
    unsafe {
        command.pre_exec(move || {
            seccompiler::apply_filter(&program)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            if let Some(program) = &clone3_program {
                seccompiler::apply_filter(program)
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
            }
            if let Some(rs) = ruleset.take() {
                let status = rs
                    .restrict_self()
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
                require_fully_enforced_landlock(status.ruleset, status.no_new_privs)?;
            }
            Ok(())
        });
    }
    Ok(())
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
fn configure_child_sandbox(_command: &mut Command, _config: &LeanRunnerConfig) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn kill_child_group(child: &mut Child) -> Result<()> {
    let pid = child.id() as libc::pid_t;
    let group_rc = unsafe { libc::killpg(pid, libc::SIGKILL) };
    let group_error = if group_rc == 0 {
        None
    } else {
        let err = std::io::Error::last_os_error();
        // Darwin reports EPERM when the process group contains only the
        // already-exited (not-yet-reaped) leader. The direct-PID kill and the
        // post-wait group-empty check below still fail closed for any live
        // member, so this zombie-only response is equivalent to ESRCH here.
        (!matches!(err.raw_os_error(), Some(libc::ESRCH | libc::EPERM))).then_some(err)
    };

    // Always address the direct PID too. This remains effective if the child
    // changed its own process group before the sandbox became active, while
    // the process-creation deny prevents it from handing work to a detached
    // descendant.
    let direct_rc = unsafe { libc::kill(pid, libc::SIGKILL) };
    if direct_rc != 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() != Some(libc::ESRCH) {
            return Err(err).context("failed to SIGKILL sandboxed direct child");
        }
    }
    if let Some(err) = group_error {
        return Err(err).context("failed to SIGKILL sandboxed request process group");
    }
    Ok(())
}

#[cfg(unix)]
fn confirm_process_group_empty(pid: u32) -> Result<()> {
    let pgid = pid as libc::pid_t;
    for _ in 0..100 {
        let rc = unsafe { libc::killpg(pgid, 0) };
        if rc != 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ESRCH) {
                return Ok(());
            }
            // Darwin can report EPERM while a group contains only killed,
            // not-yet-reaped members. `kill_child_group` handles the same
            // transient state before the direct-child wait. Do not call it
            // empty immediately, though: keep polling so a live or otherwise
            // uninspectable group still fails closed at the bounded deadline.
            #[cfg(target_os = "macos")]
            if err.raw_os_error() == Some(libc::EPERM) {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
            return Err(err).context("failed to inspect sandboxed request process group");
        }
        thread::sleep(Duration::from_millis(1));
    }
    Err(anyhow!(
        "sandboxed request process group {pgid} still has a live member after SIGKILL"
    ))
}

#[cfg(not(unix))]
fn kill_child_group(child: &mut Child) -> Result<()> {
    child
        .kill()
        .context("failed to terminate sandboxed child process")
}

#[cfg(not(unix))]
fn confirm_process_group_empty(_pid: u32) -> Result<()> {
    Ok(())
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
// `Deref`/`DerefMut` proxy to the inner `Child` for polling and pipe access.
// The normal path explicitly disarms the guard only after group cleanup; an
// early return leaves it armed so Drop kills the group before reaping.
pub(crate) struct ChildKillOnDrop(Option<Child>);

impl ChildKillOnDrop {
    pub(crate) fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn wait_and_disarm(&mut self) -> std::io::Result<std::process::ExitStatus> {
        let mut child = self
            .0
            .take()
            .expect("child already taken from ChildKillOnDrop");
        match child.wait() {
            Ok(status) => Ok(status),
            Err(error) => {
                // A failed wait must not silently disarm the cancellation
                // backstop. Put the child back so Drop still kills the whole
                // request group and makes one final reap attempt.
                self.0 = Some(child);
                Err(error)
            }
        }
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
            // Kill the group BEFORE reaping the direct child. The leader may
            // already be a zombie while a descendant still owns the request
            // pipes or artifact path; `try_wait` would lose that stable group
            // handle before the descendant was terminated.
            let pid = child.id();
            let _ = kill_child_group(&mut child);
            let _ = child.wait();
            let _ = confirm_process_group_empty(pid);
        }
    }
}

/// Files the artifact hash always pins, in order. Consumers protecting these
/// input paths use the same list rather than duplicating the checker contract.
/// The complete artifact also includes the recursive `BooleCheck/**` source
/// walk below. `Boole/Family/V0Helpers.lean` is explicit (D#6): submitted proof
/// files import this helper even though it lives outside `BooleCheck/`.
pub const CHECKER_PINNED_FILES: &[&str] = &[
    "lean-toolchain",
    "lakefile.lean",
    "lake-manifest.json",
    HELPER_SOURCE_RELATIVE,
];

/// SHA-256 over the checker package's pinned files plus every source under
/// `BooleCheck/**`. Public so tests and operator tooling can recompute the
/// hash with the EXACT production formula instead of mirroring it.
pub fn checker_artifact_hash(package_dir: &Path) -> Result<String> {
    checker_artifact_hash_with_helper_source(package_dir, None)
}

fn checker_artifact_hash_with_helper_source(
    package_dir: &Path,
    helper_source: Option<&[u8]>,
) -> Result<String> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for relative in CHECKER_PINNED_FILES {
        let path = package_dir.join(relative);
        let bytes = if *relative == HELPER_SOURCE_RELATIVE {
            match helper_source {
                Some(bytes) => bytes.to_vec(),
                None => std::fs::read(&path).with_context(|| {
                    format!("failed to read checker artifact {}", path.display())
                })?,
            }
        } else {
            std::fs::read(&path)
                .with_context(|| format!("failed to read checker artifact {}", path.display()))?
        };
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

/// SC.9b / ADR-0016 (a-2) — the executable toolchain identity of the
/// checker PROCESS: `lean`/`lake` resolved from the package directory through
/// elan dispatch by the package's `lean-toolchain` file. A bare
/// `lean --version` from an arbitrary cwd can name a DIFFERENT toolchain
/// than the one proofs are actually checked under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveToolchain {
    /// Full `lake env lean --version` line (includes the platform triple).
    pub lean_version: String,
    /// Bare release commit hash parsed from the version line —
    /// platform-independent identity of the Lean executable.
    pub lean_githash: String,
    /// Full `lake --version` line resolved from the package dir.
    pub lake_version: String,
}

impl EffectiveToolchain {
    /// The `X.Y.Z` token from the `lean --version` line.
    pub fn lean_version_token(&self) -> Option<&str> {
        parse_between(&self.lean_version, "version ", ",")
    }

    /// The version token from the `lake --version` line
    /// (e.g. `5.0.0-src+f72c35b`).
    pub fn lake_version_token(&self) -> Option<&str> {
        let rest = self.lake_version.strip_prefix("Lake version ")?;
        Some(rest.split_whitespace().next().unwrap_or(rest))
    }
}

fn parse_between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let idx = text.find(start)? + start.len();
    let rest = &text[idx..];
    let stop = rest.find(end)?;
    Some(&rest[..stop])
}

/// Identity queries are deterministic per package dir for the life of the
/// process; cache them so per-proof `evidence()` calls do not re-spawn
/// `lake` twice per verification.
static EFFECTIVE_TOOLCHAIN_CACHE: Mutex<
    Option<HashMap<(PathBuf, IsolationMode), EffectiveToolchain>>,
> = Mutex::new(None);

#[derive(Debug, Clone)]
struct EffectiveToolchainRuntime {
    lean_executable: PathBuf,
    lean_sysroot: OsString,
    lean_stdlib_dir: PathBuf,
}

static EFFECTIVE_TOOLCHAIN_RUNTIME_CACHE: Mutex<
    Option<HashMap<(PathBuf, IsolationMode), EffectiveToolchainRuntime>>,
> = Mutex::new(None);

fn isolated_lean_path(entries: &[&Path]) -> Result<OsString> {
    std::env::join_paths(entries.iter().copied())
        .map_err(|err| anyhow!("failed to construct isolated LEAN_PATH: {err}"))
}

fn effective_toolchain_runtime(
    package_dir: &Path,
    isolation_mode: IsolationMode,
) -> Result<EffectiveToolchainRuntime> {
    let key = (
        package_dir
            .canonicalize()
            .unwrap_or_else(|_| package_dir.to_path_buf()),
        isolation_mode,
    );
    if let Ok(guard) = EFFECTIVE_TOOLCHAIN_RUNTIME_CACHE.lock() {
        if let Some(cached) = guard.as_ref().and_then(|map| map.get(&key)) {
            return Ok(cached.clone());
        }
    }
    let lean_executable =
        effective_command_output(package_dir, &["env", "printenv", "LEAN"], isolation_mode)?;
    let lean_sysroot = effective_command_output(
        package_dir,
        &["env", "printenv", "LEAN_SYSROOT"],
        isolation_mode,
    )?;
    if lean_executable.is_empty() || lean_sysroot.is_empty() {
        return Err(anyhow!(
            "lake env returned an empty LEAN or LEAN_SYSROOT for {}",
            package_dir.display()
        ));
    }
    let (lean_executable, lean_sysroot, lean_stdlib_dir) =
        validate_toolchain_lean_executable(Path::new(&lean_executable), Path::new(&lean_sysroot))?;
    let runtime = EffectiveToolchainRuntime {
        lean_executable,
        lean_sysroot: lean_sysroot.into_os_string(),
        lean_stdlib_dir,
    };
    if let Ok(mut guard) = EFFECTIVE_TOOLCHAIN_RUNTIME_CACHE.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(key, runtime.clone());
    }
    Ok(runtime)
}

fn validate_toolchain_lean_executable(
    lean: &Path,
    lean_sysroot: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf)> {
    if !lean.is_absolute() || !lean_sysroot.is_absolute() {
        return Err(anyhow!(
            "lake env must return absolute LEAN and LEAN_SYSROOT paths"
        ));
    }
    let canonical_sysroot = lean_sysroot.canonicalize().with_context(|| {
        format!(
            "failed to canonicalize LEAN_SYSROOT {}",
            lean_sysroot.display()
        )
    })?;
    if !canonical_sysroot.is_dir() {
        return Err(anyhow!(
            "canonical LEAN_SYSROOT is not a directory: {}",
            canonical_sysroot.display()
        ));
    }
    let canonical_bin = canonical_sysroot
        .join("bin")
        .canonicalize()
        .with_context(|| {
            format!(
                "failed to canonicalize LEAN_SYSROOT/bin under {}",
                canonical_sysroot.display()
            )
        })?;
    let canonical_lean = lean
        .canonicalize()
        .with_context(|| format!("failed to canonicalize LEAN {}", lean.display()))?;
    let metadata = canonical_lean.metadata().with_context(|| {
        format!(
            "failed to inspect canonical Lean executable {}",
            canonical_lean.display()
        )
    })?;
    if !metadata.file_type().is_file() || !canonical_lean.starts_with(&canonical_bin) {
        return Err(anyhow!(
            "canonical Lean executable {} must be a regular file under {}",
            canonical_lean.display(),
            canonical_bin.display()
        ));
    }
    let canonical_stdlib = canonical_sysroot
        .join("lib/lean")
        .canonicalize()
        .with_context(|| {
            format!(
                "failed to canonicalize LEAN_SYSROOT/lib/lean under {}",
                canonical_sysroot.display()
            )
        })?;
    if !canonical_stdlib.is_dir() {
        return Err(anyhow!(
            "canonical Lean standard-library path is not a directory: {}",
            canonical_stdlib.display()
        ));
    }
    Ok((canonical_lean, canonical_sysroot, canonical_stdlib))
}

pub fn effective_toolchain_identity(package_dir: &Path) -> Result<EffectiveToolchain> {
    // Standalone metadata callers have no selected proof-isolation policy.
    // Preserve their portable baseline (no submitted proof runs), adding
    // rlimits, output/deadline bounds and process-group cleanup. Configured
    // LeanRunner callers use the explicit-policy API below.
    effective_toolchain_identity_with_isolation(package_dir, IsolationMode::Log)
}

/// Bounded discovery with the caller's explicit isolation posture. A Log
/// result never populates the Enforce cache, so an opt-out cannot satisfy a
/// later caller that requires enforcement.
pub fn effective_toolchain_identity_with_isolation(
    package_dir: &Path,
    isolation_mode: IsolationMode,
) -> Result<EffectiveToolchain> {
    let key = (
        package_dir
            .canonicalize()
            .unwrap_or_else(|_| package_dir.to_path_buf()),
        isolation_mode,
    );
    if let Ok(guard) = EFFECTIVE_TOOLCHAIN_CACHE.lock() {
        if let Some(cached) = guard.as_ref().and_then(|map| map.get(&key)) {
            return Ok(cached.clone());
        }
    }
    let lean_version =
        effective_command_output(package_dir, &["env", "lean", "--version"], isolation_mode)?;
    let lean_githash = parse_between(&lean_version, "commit ", ",")
        .ok_or_else(|| {
            anyhow!("could not parse a commit githash out of lean version line: {lean_version}")
        })?
        .to_string();
    let lake_version = effective_command_output(package_dir, &["--version"], isolation_mode)?;
    let toolchain = EffectiveToolchain {
        lean_version,
        lean_githash,
        lake_version,
    };
    if let Ok(mut guard) = EFFECTIVE_TOOLCHAIN_CACHE.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(key, toolchain.clone());
    }
    Ok(toolchain)
}

/// Run `lake <args>` the way the checker child would see it: package dir as
/// cwd and the same scrubbed environment (`resolved_child_path`), so elan
/// dispatches by the package's `lean-toolchain` pin.
fn effective_command_output(
    package_dir: &Path,
    args: &[&str],
    isolation_mode: IsolationMode,
) -> Result<String> {
    let mut command = Command::new("lake");
    command
        .args(args)
        .current_dir(package_dir)
        .stdin(Stdio::null());
    configure_child_environment(&mut command);
    let config = LeanRunnerConfig::new("toolchain-probe")
        .with_package_dir(package_dir)
        .with_isolation_mode(isolation_mode)
        .with_timeout_ms(30_000);
    run_toolchain_probe(command, &config).with_context(|| {
        format!(
            "failed to execute `lake {}` in {}",
            args.join(" "),
            package_dir.display()
        )
    })
}

fn run_toolchain_probe(command: Command, config: &LeanRunnerConfig) -> Result<String> {
    // Toolchain discovery precedes checker execution, so it needs the same
    // containment boundary: process-group cleanup, bounded output, rlimits,
    // and the caller's isolation posture. Default Enforce also prevents an
    // elan download from acquiring network access or mutating its global
    // installation; an explicit Log opt-out retains its documented semantics.
    let output = LeanRunner::new(config.clone())
        .run_sandboxed_with_config(command, config)
        .context("failed to execute contained toolchain probe")?;
    if output.timed_out {
        return Err(anyhow!(
            "toolchain probe timeout after {}ms",
            config.timeout_ms
        ));
    }
    if output.capture_failed {
        return Err(anyhow!("capture_failed"));
    }
    if output.output_truncated {
        return Err(anyhow!("toolchain probe output exceeded its byte limit"));
    }
    if !output.success {
        return Err(anyhow!(
            "toolchain probe failed in {}: {}",
            config.package_dir.display(),
            output.stderr
        ));
    }
    Ok(output.stdout.trim().to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn review_compact_audit_only() {
        for json in [
            " [\"propext\"]",
            "[ \"propext\"]",
            "[\"propext\"] ",
            "[\"Classical\", \"choice\"]",
        ] {
            let stdout = format!("BOOLE_AXIOM_JSON {json}\nBOOLE_AXIOM_AUDIT_DONE count=1\n");
            assert!(
                enforce_axiom_allowlist(&det2_outcome(&stdout, "", true)).is_err(),
                "{json}"
            );
        }
    }
    #[test]
    fn budget_signal_v1_retains_the_release_prefix_bound() {
        let bytes: Vec<u8> = (0..100_000).map(|n| (n % 251) as u8).collect();
        let mut drain = DrainBuffer::for_release(65536, true);
        for chunk in bytes.chunks(4096) {
            drain.push(chunk);
        }
        assert_eq!(drain.retained(), bytes[..65536]);
        assert_eq!(drain.total, 100_000);
        assert!(drain.truncated);
    }
    #[test]
    fn budget_signal_v2_is_exact_and_final() {
        for kind in ["maxHeartbeats", "maxRecDepth"] {
            let marker = format!("BOOLE_BUDGET_EXCEEDED {kind}\n");
            let result = classify_failed_run(&det2_outcome("", &marker, false));
            assert_eq!(
                result,
                LeanVerdict::DeterministicReject {
                    reason: REJECT_BUDGET_EXCEEDED.into()
                }
            );
            for (stdout, stderr) in [
                (marker.as_str(), ""),
                ("", "BOOLE_BUDGET_EXCEEDED maxHeartbeats"),
                ("", "BOOLE_BUDGET_EXCEEDED other\n"),
                ("", "BOOLE_BUDGET_EXCEEDED maxHeartbeats extra\n"),
                ("", "BOOLE_BUDGET_EXCEEDED maxHeartbeats\ntrailing line\n"),
            ] {
                assert!(classify_failed_run(&det2_outcome(stdout, stderr, false))
                    .is_retryable_unavailable());
            }
            assert_eq!(
                enforce_axiom_allowlist(&det2_outcome("", &marker, false))
                    .unwrap_err()
                    .0,
                LeanVerdict::DeterministicReject {
                    reason: REJECT_BUDGET_EXCEEDED.into()
                }
            );
        }
    }
    #[test]
    fn review_budget_labels_ignore_earlier_text() {
        let primary = det2_outcome(
            "maximum number of heartbeats\n",
            "maximum recursion depth has been reached\nBOOLE_PRIMARY_REJECT\n",
            false,
        );
        assert_eq!(
            classify_failed_run(&primary),
            LeanVerdict::DeterministicReject {
                reason: REJECT_LEAN_REJECTED.into()
            }
        );
        let audit = det2_outcome(
            "",
            "maximum number of heartbeats\nruntime unavailable\n",
            false,
        );
        assert!(enforce_axiom_allowlist(&audit)
            .unwrap_err()
            .0
            .is_retryable_unavailable());
    }
    #[test]
    fn review_marker_through_real_pipe() {
        let config = LeanRunnerConfig::new("pipe")
            .with_package_dir(std::env::temp_dir())
            .with_isolation_mode(IsolationMode::Log);
        let runner = LeanRunner::new(config.clone());
        let mut command = Command::new("/usr/bin/python3");
        command.args([
            "-c",
            "import os,sys;os.write(2,b'x'*102400+b'\\nBOOLE_PRIMARY_REJECT\\n');sys.exit(1)",
        ]);
        let outcome = runner.run_sandboxed_with_config(command, &config).unwrap();
        assert_eq!(outcome.stderr_bytes, 102422);
        assert!(outcome.output_truncated);
        assert_eq!(
            classify_failed_run(&outcome),
            LeanVerdict::DeterministicReject {
                reason: REJECT_LEAN_REJECTED.into()
            }
        );
    }
    #[test]
    fn review_capture_read_error_is_retryable() {
        struct Bad;
        impl Read for Bad {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("fixture read error"))
            }
        }
        let drain = Arc::new(Mutex::new(DrainBuffer::new(65536)));
        spawn_drain(Bad, Arc::clone(&drain)).join().unwrap();
        let mut outcome = det2_outcome("", "BOOLE_PRIMARY_REJECT\n", false);
        outcome.capture_failed = drain.lock().unwrap().capture_failed;
        assert_eq!(
            classify_failed_run(&outcome),
            LeanVerdict::RetryableUnavailable {
                reason: "capture_failed".into()
            }
        );
    }
    #[test]
    fn review_operator_pin_rejects_known_other_release_before_probes() {
        let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lean/checker-v1");
        let directory = CheckerArtifactWorkspace::create().unwrap();
        let proof = directory.primary_dir.join("Proof.lean");
        std::fs::write(&proof, "theorem t : True := trivial\n").unwrap();
        let runner = LeanRunner::new(
            LeanRunnerConfig::new("pin")
                .with_package_dir(package)
                .with_checker_artifact_hash(CHECKER_RELEASE_HASH),
        );
        let result = runner.check_file(proof).unwrap();
        assert_eq!(
            result.verdict,
            LeanVerdict::RetryableUnavailable {
                reason: "checker_release_mismatch".into()
            }
        );
        assert!(result.evidence.lean_version.is_empty());
    }
    #[test]
    fn review_frozen_v1_is_preserved() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lean/checker-v1");
        assert_eq!(
            checker_artifact_hash(&dir).unwrap(),
            "f9da3a1c5bcb605a26c8f778e2661e471398864c69393079aed461dd1453d7b4"
        );
    }

    fn det2_outcome(stdout: &str, stderr: &str, success: bool) -> SandboxedRunOutcome {
        SandboxedRunOutcome {
            success,
            exit_code: if success { 0 } else { 1 },
            stdout: stdout.into(),
            stderr: stderr.into(),
            timed_out: false,
            output_truncated: false,
            stdout_truncated: false,
            stdout_bytes: stdout.len() as u64,
            stderr_bytes: stderr.len() as u64,
            stderr_final_line: {
                let mut drain = DrainBuffer::new(65536);
                drain.push(stderr.as_bytes());
                drain.final_line()
            },
            capture_failed: false,
        }
    }
    #[test]
    fn det2_audit_rejection_preserves_bounded_marker_diagnostic() {
        let outcome = det2_outcome(
            "",
            "BOOLE_UNEXPECTED_IMPORT exact-import-set-required\n",
            false,
        );
        let (verdict, diagnostic) =
            enforce_axiom_allowlist(&outcome).expect_err("semantic rejection");
        assert!(matches!(verdict, LeanVerdict::DeterministicReject { .. }));
        assert!(diagnostic.contains("BOOLE_UNEXPECTED_IMPORT"));
    }
    #[test]
    fn det2_unavailability_error_groups() {
        for (reason, group) in [
            ("trusted_helper_compile_unavailable", "D-node"),
            ("checker package changed during verification", "D-node"),
            ("sandbox_unavailable", "T"),
            ("containment_killed", "I"),
            ("replay_unavailable", "D-input"),
            ("future_reason", "T"),
        ] {
            assert_eq!(
                LeanVerdict::RetryableUnavailable {
                    reason: reason.into()
                }
                .error_group(),
                Some(group),
                "{reason}"
            );
        }
        assert_eq!(LeanVerdict::Accepted.error_group(), None);
    }
    #[test]
    fn det2_drain_tail_and_read_error() {
        let mut drain = DrainBuffer::new(65536);
        drain.push(&vec![b'x'; 102400]);
        drain.push(b"\nBOOLE_PRIMARY_REJECT\n");
        assert_eq!(drain.retained().len(), 65536);
        assert_eq!(drain.total, 102422);
        assert_eq!(drain.final_line().as_deref(), Some("BOOLE_PRIMARY_REJECT"));
        let mut outcome = det2_outcome("", "", false);
        outcome.stderr_final_line = drain.final_line();
        assert_eq!(
            classify_failed_run(&outcome),
            LeanVerdict::DeterministicReject {
                reason: REJECT_LEAN_REJECTED.into()
            }
        );
        struct Bad;
        impl Read for Bad {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("read failed"))
            }
        }
        let sink = Arc::new(Mutex::new(DrainBuffer::new(65536)));
        spawn_drain(Bad, Arc::clone(&sink)).join().unwrap();
        assert!(sink.lock().unwrap().capture_failed);
        outcome.capture_failed = true;
        assert_eq!(classify_failed_run(&outcome).error_group(), Some("T"));
        assert!(classify_failed_run(&outcome).is_retryable_unavailable());
        let mut audit = det2_outcome("BOOLE_AXIOM_AUDIT_DONE count=0\n", "", true);
        audit.stdout_truncated = true;
        assert!(matches!(
            enforce_axiom_allowlist(&audit),
            Err((LeanVerdict::DeterministicReject { .. }, _))
        ));
        audit.stdout_truncated = false;
        audit.capture_failed = true;
        assert!(matches!(
            enforce_axiom_allowlist(&audit),
            Err((LeanVerdict::RetryableUnavailable { .. }, _))
        ));
    }
    #[test]
    fn det2_release_mismatch_is_unavailable_before_toolchain() {
        let root = std::env::temp_dir().join(format!("boole-release-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Boole/Family")).unwrap();
        for name in [
            "lean-toolchain",
            "lakefile.lean",
            "lake-manifest.json",
            "Boole/Family/V0Helpers.lean",
        ] {
            std::fs::write(root.join(name), "different release").unwrap();
        }
        let proof = root.join("Proof.lean");
        std::fs::write(&proof, "theorem t : True := trivial\n").unwrap();
        let result =
            LeanRunner::new(LeanRunnerConfig::new("pin-test").with_package_dir(root.clone()))
                .check_file(proof)
                .expect("typed mismatch");
        assert_eq!(
            result.verdict,
            LeanVerdict::RetryableUnavailable {
                reason: "checker_release_mismatch".into()
            }
        );
        assert_eq!(result.verdict.error_group(), Some("D-node"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn det2_final_stderr_marker_contract() {
        for stdout in [
            "BOOLE_PRIMARY_REJECT\n",
            "BOOLE_BUDGET_OVERRIDE maxHeartbeats\n",
            "maximum number of heartbeats\n",
        ] {
            assert!(
                classify_failed_run(&det2_outcome(stdout, "runtime unavailable\n", false))
                    .is_retryable_unavailable()
            );
        }
        for stderr in [
            "BOOLE_PRIMARY_REJECT\nx\n",
            "BOOLE_PRIMARY_REJECT",
            "BOOLE_BUDGET_OVERRIDE\n",
            "maximum number of heartbeats\n",
        ] {
            assert!(
                classify_failed_run(&det2_outcome("", stderr, false)).is_retryable_unavailable()
            );
        }
    }
    #[test]
    fn det2_audit_grammar_v2() {
        for stdout in [
            "malformed audit line\nBOOLE_AXIOM_AUDIT_DONE count=0\n",
            "BOOLE_AXIOM_AUDIT_DONE count=0\nx\n",
            "BOOLE_AXIOM_JSON [\"propext\"]\nBOOLE_AXIOM_JSON [\"Quot\",\"sound\"]\nBOOLE_AXIOM_AUDIT_DONE count=1\n",
            "BOOLE_AXIOM_AUDIT_DONE count=01\n", "BOOLE_AXIOM_AUDIT_DONE count=0\r\n",
            "BOOLE_AXIOM_JSON [\"Classical\",\"choice\",\"x\"]\nBOOLE_AXIOM_AUDIT_DONE count=1\n",
        ] {
            let (verdict, _) = enforce_axiom_allowlist(&det2_outcome(stdout, "", true)).expect_err(stdout);
            assert!(matches!(verdict, LeanVerdict::DeterministicReject { ref reason } if reason == REJECT_AXIOM_AUDIT));
        }
        for stdout in ["BOOLE_AXIOM_AUDIT_DONE count=0\n", "BOOLE_AXIOM_JSON [\"propext\"]\nBOOLE_AXIOM_JSON [\"Classical\",\"choice\"]\nBOOLE_AXIOM_JSON [\"Quot\",\"sound\"]\nBOOLE_AXIOM_AUDIT_DONE count=3\n"] {
            assert!(enforce_axiom_allowlist(&det2_outcome(stdout, "", true)).is_ok());
        }
    }
    #[test]
    fn det2_policy_v5_shared_cases() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/submission-policy/cases-v5.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let source = case["source"].as_str().unwrap();
            let mut actual = Vec::new();
            for rule in &text_policy().patterns {
                let mut cursor = 0;
                while let Some(position) = text_pattern_hit(&source[cursor..], rule) {
                    let offset = cursor + position;
                    actual.push((
                        rule.label.clone(),
                        1 + source[..offset].bytes().filter(|b| *b == b'\n').count(),
                    ));
                    cursor = offset + rule.text.len();
                }
            }
            for (label, matcher, width) in [
                (
                    "output-option",
                    output_option_position as fn(&str) -> Option<usize>,
                    10,
                ),
                (
                    "forbidden-name-character",
                    forbidden_name_position as fn(&str) -> Option<usize>,
                    '«'.len_utf8(),
                ),
            ] {
                let mut cursor = 0;
                while let Some(position) = matcher(&source[cursor..]) {
                    let offset = cursor + position;
                    actual.push((
                        label.into(),
                        1 + source[..offset].bytes().filter(|b| *b == b'\n').count(),
                    ));
                    cursor = offset + width;
                }
            }
            let (lines, _) = forbidden_attribute(source);
            actual.extend(
                lines
                    .into_iter()
                    .map(|line| ("forbidden-attribute".into(), line)),
            );
            let mut expected: Vec<_> = case["hits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| {
                    (
                        h["label"].as_str().unwrap().to_string(),
                        h["line"].as_u64().unwrap() as usize,
                    )
                })
                .collect();
            actual.sort();
            expected.sort();
            assert_eq!(actual, expected, "{case}");
        }
    }

    #[test]
    fn review_rules_bound_to_policy_fields() {
        let mut data: serde_json::Value = serde_json::from_str(include_str!(
            "../../../scripts/zk_registry/data/submission_policy_v5.json"
        ))
        .unwrap();
        data["output_options"]["forbidden_roots"] = serde_json::json!([]);
        data["forbidden_name_characters"] = serde_json::json!([]);
        data["output_options"]["name_grammar"] = serde_json::json!("Z+");
        let policy: TextPolicy = serde_json::from_value(data).unwrap();
        assert_eq!(
            output_option_position_with_policy("set_option ZZZ true", &policy),
            None
        );
        assert_eq!(
            output_option_position_with_policy("set_option pp.foo true", &policy),
            Some(0)
        );
        assert_eq!(
            forbidden_name_position_with_policy("«x\u{2028}y»", &policy),
            None
        );
    }

    #[test]
    fn review_release_grammars_are_separate_and_complete() {
        assert_eq!(
            checker_audit_grammar(CHECKER_RELEASE_HASH_V1),
            Some(AuditGrammar::V1)
        );
        assert_eq!(
            checker_audit_grammar(CHECKER_RELEASE_HASH),
            Some(AuditGrammar::V2)
        );
        assert_eq!(checker_audit_grammar("unknown"), None);
        for text in ["BOOLE_AXIOM_AUDIT_DONE\n","BOOLE_AXIOM propext\nBOOLE_AXIOM Classical.choice\nBOOLE_AXIOM Quot.sound\nBOOLE_AXIOM_AUDIT_DONE\n"] {
            assert!(enforce_axiom_allowlist_v1(&det2_outcome(text,"",true)).is_ok());
            assert!(enforce_axiom_allowlist(&det2_outcome(text,"",true)).is_err());
        }
        for text in [
            "malformed line\nBOOLE_AXIOM_AUDIT_DONE\n",
            "BOOLE_AXIOM_AUDIT_DONE\nextra\n",
            "BOOLE_AXIOM_AUDIT_DONE\nBOOLE_AXIOM_AUDIT_DONE\n",
            "BOOLE_AXIOM propext \nBOOLE_AXIOM_AUDIT_DONE\n",
            "BOOLE_AXIOM_AUDIT_DONE",
            "BOOLE_AXIOM_AUDIT_DONE count=0\n",
        ] {
            assert!(
                enforce_axiom_allowlist_v1(&det2_outcome(text, "", true)).is_err(),
                "{text}"
            );
        }
        let mut truncated = det2_outcome("BOOLE_AXIOM_AUDIT_DONE\n", "", true);
        truncated.stdout_truncated = true;
        assert!(enforce_axiom_allowlist_v1(&truncated).is_err());
    }
    #[test]
    fn policy_loader_rejects_word_patterns_with_nonword_edges() {
        for word in ["#eval", "debug.", "αnative", "nativeα", ""] {
            let mut data: serde_json::Value = serde_json::from_str(include_str!(
                "../../../scripts/zk_registry/data/submission_policy_v5.json"
            ))
            .unwrap();
            data["patterns"][0]["text"] = word.into();
            assert!(
                serde_json::from_str::<TextPolicy>(&data.to_string()).is_err(),
                "{word}"
            );
        }
    }

    #[test]
    fn policy_loader_rejects_non_ascii_whitespace_and_bad_bounded_edges() {
        let original: serde_json::Value = serde_json::from_str(include_str!(
            "../../../scripts/zk_registry/data/submission_policy_v5.json"
        ))
        .unwrap();
        let mut data = original.clone();
        data["ascii_whitespace"] = " \u{a0}".into();
        assert!(serde_json::from_value::<TextPolicy>(data).is_err());
        for (boundary, word) in [
            ("prefix", ".debug"),
            ("prefix", ""),
            ("suffix", "#eval."),
            ("suffix", ""),
        ] {
            let mut data = original.clone();
            data["patterns"][0]["boundary"] = boundary.into();
            data["patterns"][0]["text"] = word.into();
            assert!(
                serde_json::from_value::<TextPolicy>(data).is_err(),
                "{boundary}: {word}"
            );
        }
    }

    #[test]
    fn review_text_policy_shared_cases() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/submission-policy/cases-v4.json"
        ))
        .unwrap();
        for case in corpus["cases"].as_array().unwrap() {
            let source = case["source"].as_str().unwrap();
            let expected_lines: Vec<usize> = case["hits"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|hit| hit["label"] == "forbidden-attribute")
                .map(|hit| hit["line"].as_u64().unwrap() as usize)
                .collect();
            assert_eq!(
                forbidden_attribute(source),
                (
                    expected_lines,
                    case["more_lines"].as_bool().unwrap_or(false)
                ),
                "{case}"
            );
            let expected = case["hits"].as_array().unwrap().first().map(|hit| {
                (
                    {
                        let label = hit["label"].as_str().unwrap();
                        let word = hit["word"].as_str().unwrap();
                        if label.contains(word) || label == "forbidden-attribute" {
                            if case["more_lines"].as_bool().unwrap_or(false) {
                                format!("{label} [more offending lines omitted]")
                            } else {
                                label.to_string()
                            }
                        } else {
                            format!("{label} [forbidden word: {word}]")
                        }
                    },
                    hit["line"].as_u64().unwrap() as usize,
                )
            });
            assert_eq!(
                scan_for_forbidden_tokens_in_bytes(source.as_bytes()),
                expected,
                "{case}"
            );
        }
    }

    #[test]
    fn sixteen_mib_text_scan_finishes_under_ten_seconds() {
        let mut source = "/- nested /- inner -/ text -/ s!\"a{x}b\" f '' s\n".repeat(400_000);
        source.truncate(16 * 1024 * 1024);
        assert_eq!(source.len(), 16 * 1024 * 1024);
        let started = Instant::now();
        assert_eq!(scan_for_forbidden_tokens_in_bytes(source.as_bytes()), None);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn attribute_bracket_memory_worker() {
        let Ok(bracket) = std::env::var("BOOLE_ATTRIBUTE_BRACKET") else {
            return;
        };
        let source = if bracket.starts_with('@') {
            bracket.repeat(16 * 1024 * 1024 / bracket.len())
        } else {
            format!("@[{}", bracket.repeat(16 * 1024 * 1024))
        };
        let peak = || {
            let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
            assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) }, 0);
            let scale = if cfg!(target_os = "macos") {
                1024.0 * 1024.0
            } else {
                1024.0
            };
            usage.ru_maxrss as f64 / scale
        };
        let before = peak();
        let started = Instant::now();
        assert_eq!(
            scan_for_forbidden_tokens_in_bytes(source.as_bytes()),
            if bracket == "@[simp] " {
                None
            } else {
                Some(("forbidden-attribute".into(), 1))
            }
        );
        let seconds = started.elapsed().as_secs_f64();
        let growth = peak() - before;
        println!(
            "{}",
            serde_json::json!({"unit": bracket, "seconds": seconds, "growth_mib": growth})
        );
        assert!(seconds < 10.0, "attribute scan took {seconds}s");
        assert!(growth < 200.0, "attribute scan grew by {growth} MiB");
    }

    /// Keep the worker deadline and its parent timeout aligned.
    #[cfg(unix)]
    fn attribute_scan_time_limit() -> Duration {
        Duration::from_secs(10)
    }

    #[cfg(unix)]
    #[test]
    fn attribute_brackets_use_bounded_memory_and_time() {
        for bracket in ["(", "[", "{", "@[", "@[simp] "] {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tests::attribute_bracket_memory_worker",
                    "--nocapture",
                ])
                .env("BOOLE_ATTRIBUTE_BRACKET", bracket)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let started = Instant::now();
            while child.try_wait().unwrap().is_none()
                && started.elapsed() < attribute_scan_time_limit() + Duration::from_secs(5)
            {
                thread::sleep(Duration::from_millis(20));
            }
            if child.try_wait().unwrap().is_none() {
                child.kill().unwrap();
            }
            let output = child.wait_with_output().unwrap();
            print!("{}", String::from_utf8_lossy(&output.stdout));
            assert!(
                output.status.success(),
                "{bracket}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn review_forbidden_words_are_rejected_before_spawn() {
        for token in [
            "maxHeartbeats",
            "maxRecDepth",
            "decide +native",
            "decide (config := { native := true })",
            "bv_decide",
            "bv_check",
        ] {
            for source in [
                token.to_string(),
                format!("-- {token}"),
                format!("\"{token}\""),
                format!("«{token}»"),
            ] {
                assert!(
                    scan_for_forbidden_tokens_in_bytes(source.as_bytes()).is_some(),
                    "{source}"
                );
            }
        }
        assert!(scan_for_forbidden_tokens_in_bytes(b"native /- hidden -/ := true").is_some());
    }
    use super::*;

    #[cfg(unix)]
    #[test]
    fn toolchain_probe_enforces_its_wall_clock_budget() {
        let config = LeanRunnerConfig::new("probe-budget")
            .with_timeout_ms(50)
            .with_isolation_mode(IsolationMode::Log);
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 0.25; printf 'late version\\n'"]);
        let started = Instant::now();
        let outcome = run_toolchain_probe(command, &config);
        assert!(
            outcome.is_err(),
            "a late version must not establish toolchain identity"
        );
        assert!(format!("{:#}", outcome.unwrap_err()).contains("timeout"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn submitted_source_snapshot_survives_caller_path_swap_after_scan() {
        let workspace = CheckerArtifactWorkspace::create().expect("create artifact workspace");
        let caller_root = make_temp_dir("submitted-source-swap", line!());
        let caller_path = caller_root.join("Submission.lean");
        let safe = b"import Boole.Family.V0Helpers\nexample : True := trivial\n";
        let forbidden = b"axiom substituted : False\n";
        std::fs::write(&caller_path, safe).expect("write safe caller source");

        let snapshot = workspace
            .snapshot_submission_source(&caller_path)
            .expect("snapshot submitted source");
        assert_eq!(
            scan_for_forbidden_tokens_in_bytes(&snapshot.bytes),
            None,
            "the exact snapshotted bytes must pass intake"
        );

        std::fs::remove_file(&caller_path).expect("unlink scanned caller source");
        std::fs::write(&caller_path, forbidden).expect("replace caller source after scan");

        assert_eq!(
            std::fs::read(&workspace.submission_source).expect("read private source"),
            safe,
            "the primary input must remain bound to the bytes that intake scanned"
        );
        #[cfg(unix)]
        {
            let output = Command::new("/bin/cat")
                .arg(inherited_stdin_descriptor_path().expect("platform fd view"))
                .stdin(Stdio::from(
                    snapshot
                        .stdin_for_child()
                        .expect("clone submitted source descriptor for child"),
                ))
                .output()
                .expect("read source through inherited descriptor");
            assert!(output.status.success(), "cat failed: {output:?}");
            assert_eq!(output.stdout, safe);
        }
        assert_eq!(
            snapshot.digest().expect("digest private source"),
            snapshot.digest
        );
        let _ = std::fs::remove_dir_all(caller_root);
    }

    #[cfg(unix)]
    #[test]
    fn audit_child_reads_the_sealed_inode_after_pathname_swap() {
        let workspace = CheckerArtifactWorkspace::create().expect("create artifact workspace");
        let promoted_bytes = b"verified-primary-artifact";
        let replacement_bytes = b"pathname-replacement";
        std::fs::write(&workspace.primary_artifact, promoted_bytes)
            .expect("write primary artifact");

        let sealed = workspace.promote_for_audit().expect("promote artifact");
        assert!(
            !workspace.audit_artifact.exists(),
            "the verified inode must be unlinked before audit so its pathname cannot be swapped"
        );
        std::fs::write(&workspace.audit_artifact, replacement_bytes)
            .expect("create attacker-controlled replacement pathname");
        use std::os::unix::fs::MetadataExt;
        let sealed_metadata = sealed.file.metadata().expect("sealed inode metadata");
        let replacement_metadata =
            std::fs::metadata(&workspace.audit_artifact).expect("replacement inode metadata");
        assert_ne!(
            (sealed_metadata.dev(), sealed_metadata.ino()),
            (replacement_metadata.dev(), replacement_metadata.ino()),
            "the replacement must be a distinct inode"
        );

        let output = Command::new("/bin/cat")
            .arg(inherited_stdin_descriptor_path().expect("platform fd view"))
            .stdin(Stdio::from(
                sealed
                    .stdin_for_child()
                    .expect("clone sealed descriptor for child"),
            ))
            .output()
            .expect("read inherited descriptor through platform fd view");
        assert!(output.status.success(), "cat failed: {output:?}");
        assert_eq!(output.stdout, promoted_bytes);
        assert_ne!(output.stdout, replacement_bytes);
        assert_eq!(sealed.digest().expect("digest sealed inode"), sealed.digest);
    }

    #[test]
    fn config_records_verifier_hash() {
        let cfg = LeanRunnerConfig::new("abc");
        assert_eq!(cfg.verifier_hash, "abc");
        assert_eq!(cfg.timeout_ms, 10_000);
        assert_eq!(cfg.memory_limit_mb, 8192);
        assert_eq!(cfg.output_limit_bytes, 64 * 1024);
        assert_eq!(
            cfg.isolation_mode,
            IsolationMode::Enforce,
            "ADR-0008 decision 4: N3.2 (network ingress opens) flips the \
             default to Enforce in the same change; Log is opt-out only"
        );
    }

    #[test]
    fn direct_lean_must_be_a_regular_file_under_canonical_sysroot_bin() {
        let root = make_temp_dir("toolchain-runtime", line!());
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).expect("create toolchain bin");
        let lean = bin.join("lean");
        std::fs::write(&lean, b"not executed").expect("write fake lean");
        std::fs::create_dir_all(root.join("lib/lean")).expect("create toolchain stdlib");
        let (resolved_lean, resolved_root, resolved_stdlib) =
            validate_toolchain_lean_executable(&lean, &root).expect("valid toolchain layout");
        assert_eq!(resolved_lean, lean.canonicalize().expect("canonical lean"));
        assert_eq!(resolved_root, root.canonicalize().expect("canonical root"));
        assert_eq!(
            resolved_stdlib,
            root.join("lib/lean")
                .canonicalize()
                .expect("canonical stdlib")
        );

        let outside = root.join("outside-lean");
        std::fs::write(&outside, b"not executed").expect("write outside fake lean");
        let outside_error = validate_toolchain_lean_executable(&outside, &root)
            .expect_err("an executable outside LEAN_SYSROOT/bin must fail");
        assert!(
            outside_error.to_string().contains("regular file under"),
            "unexpected outside-path error: {outside_error:#}"
        );

        let directory = bin.join("not-a-file");
        std::fs::create_dir(&directory).expect("create directory masquerading as lean");
        let directory_error = validate_toolchain_lean_executable(&directory, &root)
            .expect_err("a directory under LEAN_SYSROOT/bin must fail");
        assert!(
            directory_error.to_string().contains("regular file under"),
            "unexpected non-file error: {directory_error:#}"
        );
        let _ = std::fs::remove_dir_all(root);
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
    fn rejects_an_empty_import_before_a_continuation_line_can_escape_the_allowlist() {
        assert_eq!(
            scan_for_forbidden_tokens_in_bytes(
                b"import\n Lean\nexample : True := trivial\n",
            ),
            Some(("import <missing module>".to_string(), 1)),
            "an empty import tail must be rejected before Lean can continue the command on the next line",
        );
    }

    #[test]
    fn ignores_sorry_inside_identifiers() {
        assert!(!contains_sorry_token("notSorry"));
        assert!(!contains_sorry_token("sorry_lemma"));
        assert!(!contains_sorry_token("MySorry"));
    }

    #[test]
    fn elaboration_time_command_aliases_are_forbidden() {
        for token in ["run_cmd", "run_elab", "run_meta", "run_tac"] {
            assert!(
                scan_for_forbidden_tokens_in_bytes(token.as_bytes()).is_some(),
                "`{token}` can execute code while the primary checker elaborates"
            );
        }
    }

    #[test]
    fn primary_budget_override_marker_keeps_its_typed_reject() {
        let outcome = det2_outcome("", "BOOLE_BUDGET_OVERRIDE maxHeartbeats\n", false);
        assert_eq!(
            classify_failed_run(&outcome),
            LeanVerdict::DeterministicReject {
                reason: REJECT_BUDGET_OVERRIDE_FORBIDDEN.to_string(),
            }
        );
    }

    #[test]
    fn primary_lean_reject_requires_its_typed_marker() {
        let outcome = det2_outcome("", &(format!("{PRIMARY_REJECT_MARKER}\n")), false);
        assert_eq!(
            classify_failed_run(&outcome),
            LeanVerdict::DeterministicReject {
                reason: REJECT_LEAN_REJECTED.to_string(),
            }
        );
    }

    #[test]
    fn untyped_primary_failure_is_retryable_unavailable() {
        let outcome = det2_outcome("", "failed to initialize checker runtime", false);
        assert_eq!(
            classify_failed_run(&outcome),
            LeanVerdict::RetryableUnavailable {
                reason: "primary_checker_unavailable".to_string(),
            }
        );
    }

    #[test]
    fn audit_internal_failure_is_unavailable_not_a_proof_reject() {
        let outcome = det2_outcome("", "failed to deserialize checker artifact", false);
        let (verdict, _) = enforce_axiom_allowlist(&outcome).expect_err("audit must not accept");
        assert_eq!(
            verdict,
            LeanVerdict::RetryableUnavailable {
                reason: "axiom_audit_unavailable".to_string(),
            }
        );
    }

    #[test]
    fn audit_explicit_semantic_marker_is_a_deterministic_reject() {
        let outcome = det2_outcome("", "BOOLE_UNREPLAYABLE_CONSTANT uncheckedValue\n", false);
        let (verdict, _) = enforce_axiom_allowlist(&outcome).expect_err("audit must reject");
        assert_eq!(
            verdict,
            LeanVerdict::DeterministicReject {
                reason: REJECT_AXIOM_AUDIT.to_string(),
            }
        );
    }

    #[test]
    fn audit_malformed_artifact_marker_is_a_deterministic_reject() {
        let outcome = det2_outcome(
            "",
            "BOOLE_MALFORMED_ARTIFACT constant-count-mismatch\n",
            false,
        );
        let (verdict, _) = enforce_axiom_allowlist(&outcome).expect_err("audit must reject");
        assert_eq!(
            verdict,
            LeanVerdict::DeterministicReject {
                reason: REJECT_AXIOM_AUDIT.to_string(),
            },
            "a stable structural defect in fixed artifact bytes must not retry forever"
        );
    }

    #[test]
    fn helper_primary_and_audit_share_one_wall_clock_deadline() {
        let config = LeanRunnerConfig::new("shared-deadline").with_timeout_ms(500);
        let started = Instant::now();
        let deadline = started + Duration::from_millis(500);

        // Each production stage starts from a clone of the original config.
        // Inject only the clock observation so host scheduling cannot decide
        // whether the test reaches the remainder/no-renewal assertions.
        for (stage, elapsed_ms, expected_ms) in [
            ("helper", 0, 500),
            ("primary", 300, 200),
            ("audit", 450, 50),
        ] {
            let mut stage_config = config.clone();
            assert!(apply_remaining_timeout_at(
                &mut stage_config,
                deadline,
                started + Duration::from_millis(elapsed_ms),
            ));
            assert_eq!(
                stage_config.timeout_ms, expected_ms,
                "{stage} must receive the common deadline remainder, not a fresh budget"
            );
        }
    }

    #[test]
    fn shared_deadline_exhaustion_after_scheduler_pause_blocks_the_next_stage() {
        let config = LeanRunnerConfig::new("exhausted-deadline").with_timeout_ms(500);
        let started = Instant::now();
        let deadline = started + Duration::from_millis(500);
        let first_stage_finished = started + Duration::from_millis(300);
        let resumed_after_pause = first_stage_finished + Duration::from_millis(600);

        // Cover both exact exhaustion and a scheduler pause longer than the
        // remaining budget. Neither path may launch a stage with a new 500ms.
        for observed in [deadline, resumed_after_pause] {
            let mut stage_config = config.clone();
            assert!(!apply_remaining_timeout_at(
                &mut stage_config,
                deadline,
                observed,
            ));
            assert_eq!(stage_config.timeout_ms, config.timeout_ms);
        }

        // Also exercise the production clock wrapper without a timing race:
        // a deadline observed before this call can only be exhausted.
        let expired_deadline = Instant::now();
        let mut stage_config = config;
        assert!(!apply_remaining_timeout(
            &mut stage_config,
            expired_deadline
        ));
    }

    #[cfg(unix)]
    #[test]
    fn shared_deadline_remainder_limits_a_real_subprocess() {
        let runner = LeanRunner::new(
            LeanRunnerConfig::new("shared-deadline-subprocess")
                .with_isolation_mode(IsolationMode::Log)
                .with_timeout_ms(500),
        );
        let started = Instant::now();
        let mut stage_config = runner.config.clone();
        assert!(apply_remaining_timeout_at(
            &mut stage_config,
            started + Duration::from_millis(500),
            started + Duration::from_millis(450),
        ));
        assert_eq!(stage_config.timeout_ms, 50);

        // Real process enforcement remains covered independently from clock
        // arithmetic. Avoid elapsed-time assertions that merely reintroduce
        // the scheduler race this test is meant to remove.
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("sleep 10");
        let outcome = runner
            .run_sandboxed_with_config(command, &stage_config)
            .expect("a stage returns a timeout envelope");
        assert!(
            outcome.timed_out && !outcome.success,
            "the remainder must bound the subprocess: exit={} stderr={}",
            outcome.exit_code,
            outcome.stderr
        );
    }

    #[cfg(unix)]
    #[test]
    fn normal_child_exit_still_kills_pipe_holding_descendants() {
        let runner = LeanRunner::new(
            LeanRunnerConfig::new("normal-exit-descendants")
                .with_isolation_mode(IsolationMode::Log)
                .with_timeout_ms(2_000),
        );
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg("(trap '' HUP; sleep 60) & echo $!");
        let started = Instant::now();
        let outcome = runner
            .run_sandboxed_with_config(command, &runner.config)
            .expect("normal child exit must return after cleaning descendants");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "a descendant holding stdout open must not hang drain joins"
        );
        assert!(
            outcome.success,
            "direct child should still count as success"
        );
        let descendant: libc::pid_t = outcome.stdout.trim().parse().expect("descendant pid");
        let mut alive = true;
        for _ in 0..100 {
            let rc = unsafe { libc::kill(descendant, 0) };
            if rc == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                alive = false;
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !alive,
            "normal-exit descendant {descendant} survived cleanup"
        );
    }

    #[test]
    fn enforce_isolation_setup_errors_are_not_discarded_in_source() {
        let source = include_str!("lib.rs");
        let mac_fail_open = ["Err(_) => return, // NUL byte in a path; ", "fail open"].concat();
        let seccomp_fail_open = ["Err(_) => return, // Fail ", "open"].concat();
        let erased_landlock_error = ["build_landlock_ruleset(config)", ".ok()"].concat();
        assert!(
            !source.contains(&mac_fail_open),
            "macOS isolation setup must not fail open"
        );
        assert!(
            !source.contains(&seccomp_fail_open),
            "Linux seccomp setup must not fail open"
        );
        assert!(
            !source.contains(&erased_landlock_error),
            "Linux Landlock setup errors must not be erased"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_enforce_requires_full_ruleset_and_no_new_privileges() {
        use landlock::RulesetStatus::{FullyEnforced, NotEnforced, PartiallyEnforced};

        assert!(require_fully_enforced_landlock(FullyEnforced, true).is_ok());
        for (ruleset, no_new_privs) in [
            (NotEnforced, true),
            (PartiallyEnforced, true),
            (FullyEnforced, false),
        ] {
            assert!(
                require_fully_enforced_landlock(ruleset, no_new_privs).is_err(),
                "Enforce mode must fail closed unless Landlock and no_new_privs are both fully active"
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn enforce_mode_rejects_invalid_isolation_profile_before_spawn() {
        let error = seatbelt_profile_cstring("(version 1)\0(allow default)".to_string())
            .expect_err("invalid Seatbelt profile input must stop before spawn");
        assert!(
            error.to_string().contains("NUL"),
            "setup error must stay typed and visible: {error:#}"
        );
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

    #[test]
    fn check_file_rejects_eval_before_lake_spawn() {
        // N0-pre.1 — `#eval` runs arbitrary IO (`IO.Process.run`/
        // `IO.FS.readFile`) with node privileges, and Lean compiles it as a
        // side-effecting command rather than rejecting it. The pre-spawn
        // forbidden-token scan must reject it before any `lake` invocation
        // (so this test needs no lean toolchain).
        let dir = std::env::temp_dir().join(format!(
            "boole-fbscan-eval-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).expect("create temp package dir");
        let proof = dir.join("Proof.lean");
        std::fs::write(
            &proof,
            "theorem t : True := trivial\n#eval IO.println \"x\"\n",
        )
        .expect("write proof");
        let runner = LeanRunner::new(LeanRunnerConfig::new("test").with_package_dir(&dir));
        let err = runner
            .check_file(&proof)
            .expect_err("#eval must be rejected");
        assert!(
            err.to_string().contains("#eval"),
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

    // The normal path must explicitly disarm the guard while waiting. This
    // prevents Drop from ever addressing a PID after it has been reaped and
    // potentially reused by an unrelated process.
    #[cfg(unix)]
    #[test]
    fn child_kill_on_drop_disarms_after_wait() {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg("exit 0");
        let child = cmd.spawn().expect("spawn /bin/sh");
        let mut guard = ChildKillOnDrop::new(child);
        let status = guard.wait_and_disarm().expect("wait child");
        assert!(status.success());
        assert!(guard.0.is_none(), "normal wait must disarm Drop cleanup");
    }

    // P1.7 characterization: the verifier runs the checker in its OWN process
    // group (`configure_child_sandbox` -> `setpgid(0, 0)`) so a timeout kill
    // (`kill_child_group` -> `killpg(SIGKILL)`) reaps the WHOLE group, not just
    // the direct child. That is the real `lake -> lean` shape: `lake` forks the
    // `lean` compiler as a grandchild. The existing `child_kill_on_drop` tests
    // only cover a single direct child; this pins that a grandchild does NOT
    // survive the group kill. A regression that replaced `killpg` with a
    // single-pid `child.kill()` would leave a runaway `lean` process alive past
    // the verifier deadline — this test would then fail (grandchild survives).
    #[cfg(unix)]
    #[test]
    fn kill_child_group_reaps_grandchild_not_just_direct_child() {
        // /bin/sh forks a backgrounded `sleep` (the grandchild), echoes its
        // pid, then `exec`s into a long sleep so the direct child stays alive
        // as the group leader until we kill the group. Non-interactive sh has
        // no job control, so the background job stays in sh's process group.
        let config =
            LeanRunnerConfig::new("test-group-kill").with_isolation_mode(IsolationMode::Log);
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg("sleep 60 & echo \"$!\"; exec sleep 60")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_child_sandbox(&mut cmd, &config).expect("configure child sandbox");
        let mut child = cmd.spawn().expect("spawn group-leader child");

        // Read the grandchild pid from the first stdout line.
        let mut out = child.stdout.take().expect("piped stdout");
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match out.read(&mut byte) {
                Ok(0) => break,
                Ok(_) if byte[0] == b'\n' => break,
                Ok(_) => line.push(byte[0]),
                Err(_) => break,
            }
        }
        let grandchild_pid: libc::pid_t = String::from_utf8_lossy(&line)
            .trim()
            .parse()
            .expect("grandchild pid line");
        assert!(grandchild_pid > 0, "grandchild pid must be positive");

        // The grandchild is running before the group kill.
        assert_eq!(
            unsafe { libc::kill(grandchild_pid, 0) },
            0,
            "grandchild should be alive before the group kill"
        );

        let _ = kill_child_group(&mut child);
        let _ = child.wait();

        // killpg must have SIGKILLed the grandchild too; once its parent (the
        // group leader) is reaped, init reaps the grandchild and `kill(pid, 0)`
        // returns ESRCH. Poll ~1s so a regression fails instead of hanging.
        let mut grandchild_alive = true;
        for _ in 0..100 {
            let rc = unsafe { libc::kill(grandchild_pid, 0) };
            if rc == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
                grandchild_alive = false;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !grandchild_alive,
            "kill_child_group must SIGKILL the whole process group; grandchild \
             pid {grandchild_pid} (the lake->lean shape) survived"
        );
    }

    // P1.7 characterization: the verifier scrubs the parent environment before
    // running the checker (`configure_child_environment` -> `env_clear`) so a
    // hostile proof cannot read operator secrets that happen to live in the
    // node's process env; only a minimal allowlist (PATH/HOME/LANG) is
    // restored. A regression that dropped `env_clear()` would let the checker
    // observe the secret — this test would then see it echoed.
    #[cfg(unix)]
    #[test]
    fn child_environment_is_scrubbed_to_minimal_allowlist() {
        // The secret is set as a Command override BEFORE the scrub, NOT on the
        // process env, so this is race-free under cargo's multi-threaded runner.
        let mut cmd = Command::new("/bin/sh");
        cmd.env("BOOLE_OPERATOR_SECRET", "do-not-leak");
        cmd.arg("-c")
            .arg("printf 'SECRET=%s LANG=%s' \"${BOOLE_OPERATOR_SECRET:-<absent>}\" \"${LANG:-<unset>}\"")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_child_environment(&mut cmd);
        let output = cmd.output().expect("run checker-shaped child");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("SECRET=<absent>"),
            "configure_child_environment must env_clear() prior vars so the \
             checker cannot read operator secrets; got: {stdout}"
        );
        assert!(
            stdout.contains("LANG=C.UTF-8"),
            "the minimal allowlist must restore LANG=C.UTF-8; got: {stdout}"
        );
    }

    // P1.7 characterization: the verifier caps the checker's CPU time via
    // `configure_child_sandbox` -> `setrlimit(RLIMIT_CPU, (timeout_ms/1000)+5)`.
    // This is the backstop that bounds a runaway proof on macOS, where
    // `RLIMIT_AS` is a no-op, so the wall-clock timeout is the primary bound and
    // RLIMIT_CPU the defence-in-depth secondary. setrlimit runs in pre_exec, so
    // the exec'd checker inherits the cap; `ulimit -t` reports the soft limit.
    #[cfg(unix)]
    #[test]
    fn configure_child_sandbox_caps_cpu_time() {
        let config =
            LeanRunnerConfig::new("test-cpu-rlimit").with_isolation_mode(IsolationMode::Log);
        // The expected cap is derived from the default timeout: 10_000/1000 + 5.
        assert_eq!(
            config.timeout_ms, 10_000,
            "test assumes the default timeout"
        );
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg("ulimit -t")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_child_sandbox(&mut cmd, &config).expect("configure child sandbox");
        let output = cmd.output().expect("run checker-shaped child");
        let cpu = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(
            cpu, "15",
            "configure_child_sandbox must cap checker CPU time at \
             (timeout_ms/1000)+5 = 15s; got {cpu:?}"
        );
    }

    // ADR-0008 — kernel isolation characterization guards. `make_temp_dir`
    // follows the same dependency-free tempdir idiom as
    // `check_file_rejects_axiom_before_lake_spawn` above (no `tempfile`
    // crate: a unique path under the OS temp dir, cleaned up manually).
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn make_temp_dir(tag: &str, unique: u32) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "boole-isolation-{tag}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp isolation test dir");
        dir
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn probe_bin() -> PathBuf {
        // `CARGO_BIN_EXE_<name>` is only populated for integration-test /
        // bench targets, not for the crate's own `--lib` unit test binary
        // (confirmed empirically: it is `NotPresent` here even though the
        // plain `sandbox_probe` executable is still built as a sibling of
        // this very test binary). `configure_child_sandbox` itself is
        // private, so these guards must live here in `mod tests` rather
        // than in `tests/*.rs` (which only sees the crate's public API) —
        // so instead of the env var, derive the sibling binary's path from
        // this test binary's own path: both land directly under
        // `<target-dir>/<profile>/`, with the test binary one level deeper
        // in `deps/`.
        let mut path = std::env::current_exe().expect("locate current test binary");
        path.pop(); // drop the test binary's own file name
        path.pop(); // drop `deps/`, landing in `<target-dir>/<profile>/`
        path.push("sandbox_probe");
        assert!(
            path.exists(),
            "expected sibling `sandbox_probe` binary at {path:?}; \
             is it still declared as a [[bin]] target in Cargo.toml?"
        );
        path
    }

    // Copies the probe binary into `dir` and returns its new path. Under an
    // Enforce config the exec-allowlist is path-scoped (a whole directory,
    // not a specific binary identity), so to isolate what a single guard
    // actually characterizes — e.g. "is network egress denied" — the probe
    // itself must be exec'd from an *allowed* directory; otherwise the exec
    // restriction itself would fire first and the guard would conflate two
    // different mechanisms.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn probe_in(dir: &Path) -> PathBuf {
        let dest = dir.join("sandbox_probe");
        std::fs::copy(probe_bin(), &dest).expect("copy sandbox_probe into test dir");
        dest
    }

    // Real verification never executes a binary from the read-only checker
    // package: it executes the package-pinned Lean binary from
    // `LEAN_SYSROOT/bin`.  Tests that make `package_dir_writable = false`
    // must preserve that separation as well, otherwise Landlock correctly
    // denies the probe's exec before the test can observe the intended write
    // denial.  Fix the modes explicitly so the assertion does not depend on
    // the host/CI umask.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn read_only_package_probe(
        package: &Path,
        unique: u32,
    ) -> (PathBuf, PathBuf, LeanRunnerConfig) {
        use std::os::unix::fs::PermissionsExt;

        let toolchain_root = make_temp_dir("truncate-toolchain", unique);
        let toolchain_bin = toolchain_root.join("bin");
        std::fs::create_dir_all(&toolchain_bin).expect("create fake toolchain bin");
        std::fs::set_permissions(&toolchain_root, std::fs::Permissions::from_mode(0o755))
            .expect("make fake toolchain root traversable");
        std::fs::set_permissions(&toolchain_bin, std::fs::Permissions::from_mode(0o755))
            .expect("make fake toolchain bin traversable");
        let probe = probe_in(&toolchain_bin);
        std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o555))
            .expect("make fake toolchain probe executable");

        let mut config = LeanRunnerConfig::new("test-package-truncate")
            .with_package_dir(package)
            .with_isolation_mode(IsolationMode::Enforce);
        config.package_dir_writable = false;
        config.lean_sysroot = Some(toolchain_root.as_os_str().to_os_string());
        (toolchain_root, probe, config)
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn read_only_package_probe_uses_the_toolchain_exec_boundary() {
        let package = make_temp_dir("truncate-package-shape", line!());
        let (toolchain_root, probe, config) = read_only_package_probe(&package, line!());
        let exec_dirs = exec_allow_dirs(&config);

        assert!(!probe.starts_with(&package));
        assert!(
            exec_dirs.iter().any(|dir| probe.starts_with(dir)),
            "the truncate probe itself must be executable before the Linux test can measure the package write denial"
        );
        assert!(
            !exec_dirs.iter().any(|dir| package.starts_with(dir)),
            "the read-only checker package must not become executable merely to run the test probe"
        );

        let _ = std::fs::remove_dir_all(&package);
        let _ = std::fs::remove_dir_all(&toolchain_root);
    }

    // The isolation-denial errno differs per mechanism: macOS Seatbelt
    // reports EPERM (empirically confirmed against this profile shape);
    // Linux seccomp (configured with `SeccompAction::Errno(EACCES)`) and
    // Landlock (LSM convention) both report EACCES.
    #[cfg(target_os = "macos")]
    const ISOLATION_DENIED_ERRNO: i32 = libc::EPERM;
    #[cfg(target_os = "linux")]
    const ISOLATION_DENIED_ERRNO: i32 = libc::EACCES;

    // P1.7/ADR-0008 characterization: under an Enforce config, the checker's
    // network egress is denied. Before `install_kernel_isolation` existed,
    // this probe succeeded in reaching the network stack (ECONNREFUSED, a
    // different errno) — RED confirmed locally on macOS prior to
    // implementation; GREEN once the isolation layer denies the syscall
    // itself (a distinct errno) rather than the kernel routing layer.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn configure_child_sandbox_enforce_denies_network_egress() {
        let scratch = make_temp_dir("egress-scratch", line!());
        let config = LeanRunnerConfig::new("test-isolation-egress")
            .with_package_dir(&scratch)
            .with_isolation_mode(IsolationMode::Enforce);
        // The probe must run from an exec-allowed dir so this guard
        // isolates the network-egress check from the (separately covered)
        // exec-allowlist check.
        let mut cmd = Command::new(probe_in(&scratch));
        cmd.arg("network-connect")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_child_sandbox(&mut cmd, &config).expect("configure child sandbox");
        let output = cmd.output().expect("spawn sandbox_probe");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!("errno=Some({ISOLATION_DENIED_ERRNO})")),
            "Enforce mode must deny network egress with the isolation \
             mechanism's own errno ({ISOLATION_DENIED_ERRNO}), not an \
             unrelated network-stack failure like ECONNREFUSED; got: {stdout}"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn process_spawn_probe_has_an_allowing_control() {
        let scratch = make_temp_dir("process-spawn-control", line!());
        let output = Command::new(probe_in(&scratch))
            .arg("process-spawn")
            .output()
            .expect("run unsandboxed process-spawn control");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("RESULT=PROCESS_ALLOWED thread_ok=true"),
            "the probe must demonstrate process creation succeeds before the primary policy denies it: {stdout}"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn primary_policy_denies_process_creation_but_allows_runtime_threads() {
        let scratch = make_temp_dir("process-spawn-scratch", line!());
        let mut config = LeanRunnerConfig::new("test-primary-process-spawn")
            .with_package_dir(&scratch)
            .with_isolation_mode(IsolationMode::Enforce);
        config.deny_process_spawn = true;
        let mut cmd = Command::new(probe_in(&scratch));
        cmd.arg("process-spawn")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_child_sandbox(&mut cmd, &config).expect("configure primary sandbox");
        let output = cmd.output().expect("run process-spawn probe");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!(
                "RESULT=PROCESS_DENIED errno=Some({ISOLATION_DENIED_ERRNO})"
            )),
            "primary process creation must be denied by the OS sandbox: {stdout}"
        );
        assert!(
            stdout.contains("thread_ok=true"),
            "Lean runtime threads must remain available: {stdout}"
        );
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn primary_seatbelt_profile_denies_process_fork() {
        let mut config = LeanRunnerConfig::new("test-primary-process-spawn");
        config.deny_process_spawn = true;
        let profile = seatbelt_profile(&config);
        assert!(
            profile.contains("(deny process-fork)"),
            "the macOS primary profile must deny child-process creation"
        );
    }

    // P1.7/ADR-0008 characterization: under an Enforce config, writes
    // outside the configured `package_dir` (scratch) are denied, while
    // writes inside it still succeed — proving the profile is a targeted
    // allowlist, not a blanket write block. RED confirmed locally on macOS
    // prior to implementation (both writes succeeded).
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn configure_child_sandbox_enforce_denies_write_outside_scratch() {
        let scratch = make_temp_dir("write-scratch", line!());
        let outside = make_temp_dir("write-outside", line!());
        let config = LeanRunnerConfig::new("test-isolation-write")
            .with_package_dir(&scratch)
            .with_isolation_mode(IsolationMode::Enforce);
        // The probe must run from an exec-allowed dir (package_dir itself)
        // so this guard isolates the write-containment check from the
        // (separately covered) exec-allowlist check.
        let probe = probe_in(&scratch);

        let denied_target = outside.join("denied.txt");
        let mut cmd = Command::new(&probe);
        cmd.arg("write")
            .arg(&denied_target)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_child_sandbox(&mut cmd, &config).expect("configure child sandbox");
        let output = cmd.output().expect("spawn sandbox_probe");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!("errno=Some({ISOLATION_DENIED_ERRNO})")),
            "Enforce mode must deny writes outside package_dir; got: {stdout}"
        );
        assert!(
            !denied_target.exists(),
            "a denied write must not have created the file"
        );

        let allowed_target = scratch.join("allowed.txt");
        let mut cmd = Command::new(&probe);
        cmd.arg("write")
            .arg(&allowed_target)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_child_sandbox(&mut cmd, &config).expect("configure child sandbox");
        let output = cmd.output().expect("spawn sandbox_probe");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("RESULT=ALLOWED"),
            "a write inside the configured package_dir must still succeed; got: {stdout}"
        );

        let _ = std::fs::remove_dir_all(&scratch);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn primary_read_only_package_denies_truncate_and_readonly_o_trunc() {
        use std::os::unix::fs::PermissionsExt;

        let package = make_temp_dir("truncate-package", line!());
        std::fs::set_permissions(&package, std::fs::Permissions::from_mode(0o755))
            .expect("make read-only-package fixture traversable");
        let (toolchain_root, probe, config) = read_only_package_probe(&package, line!());

        assert!(
            exec_allow_dirs(&config)
                .iter()
                .any(|dir| probe.starts_with(dir)),
            "the truncate probe itself must be executable before this test can measure the package write denial"
        );

        for operation in ["truncate", "open-truncate-readonly"] {
            let protected = package.join(format!("protected-{operation}.txt"));
            let expected = b"trusted checker package bytes";
            std::fs::write(&protected, expected).expect("write protected package file");
            std::fs::set_permissions(&protected, std::fs::Permissions::from_mode(0o644))
                .expect("keep the package file host-writable before Landlock");

            let mut cmd = Command::new(&probe);
            cmd.arg(operation)
                .arg(&protected)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            configure_child_sandbox(&mut cmd, &config).expect("configure primary sandbox");
            let output = cmd.output().expect("run truncate probe");
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                stdout.contains("RESULT=DENIED errno=Some(13)"),
                "Enforce mode must deny {operation} against the read-only checker package: {stdout}"
            );
            assert_eq!(
                std::fs::read(&protected).expect("reread protected package file"),
                expected,
                "denied {operation} must leave checker package bytes unchanged"
            );
        }

        let protected = package.join("protected-ftruncate-readonly.txt");
        let expected = b"trusted checker package bytes";
        std::fs::write(&protected, expected).expect("write protected package file");
        std::fs::set_permissions(&protected, std::fs::Permissions::from_mode(0o644))
            .expect("keep the package file host-writable before Landlock");
        let mut cmd = Command::new(&probe);
        cmd.arg("ftruncate-readonly")
            .arg(&protected)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_child_sandbox(&mut cmd, &config).expect("configure primary sandbox");
        let output = cmd.output().expect("run ftruncate probe");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("RESULT=DENIED"),
            "a child that opens the package read-only after sandbox installation must not ftruncate it: {stdout}"
        );
        assert_eq!(
            std::fs::read(&protected).expect("reread protected package file"),
            expected,
            "denied ftruncate must leave checker package bytes unchanged"
        );

        let _ = std::fs::remove_dir_all(&package);
        let _ = std::fs::remove_dir_all(&toolchain_root);
    }

    // P1.7/ADR-0008 characterization: under an Enforce config, `exec` of a
    // binary outside the toolchain allowlist (PATH dirs / ~/.elan /
    // package_dir) is denied, while the SAME binary exec'd from inside the
    // allowlisted package_dir still runs — proving the mechanism restricts
    // exec by path, not by binary identity. RED confirmed locally on macOS
    // prior to implementation (both execs succeeded).
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn configure_child_sandbox_enforce_denies_non_toolchain_exec() {
        let scratch = make_temp_dir("exec-scratch", line!());
        let outside = make_temp_dir("exec-outside", line!());
        let config = LeanRunnerConfig::new("test-isolation-exec")
            .with_package_dir(&scratch)
            .with_isolation_mode(IsolationMode::Enforce);

        let outside_probe = probe_in(&outside);
        let mut cmd = Command::new(&outside_probe);
        cmd.arg("noop")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_child_sandbox(&mut cmd, &config).expect("configure child sandbox");
        match cmd.output() {
            Ok(output) => panic!(
                "Enforce mode must deny exec of a binary outside the toolchain \
                 allowlist, but it ran: status={:?} stdout={:?}",
                output.status,
                String::from_utf8_lossy(&output.stdout)
            ),
            Err(e) => assert_eq!(
                e.raw_os_error(),
                Some(ISOLATION_DENIED_ERRNO),
                "expected the isolation mechanism's own exec-denial errno; got {e}"
            ),
        }

        let inside_probe = probe_in(&scratch);
        let mut cmd = Command::new(&inside_probe);
        cmd.arg("noop")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_child_sandbox(&mut cmd, &config).expect("configure child sandbox");
        let output = cmd
            .output()
            .expect("exec inside the allowlisted package_dir must be allowed");
        assert!(
            output.status.success(),
            "exec inside package_dir must succeed; status={:?}",
            output.status
        );

        let _ = std::fs::remove_dir_all(&scratch);
        let _ = std::fs::remove_dir_all(&outside);
    }

    // P1.7/ADR-0008 characterization: the phased-enforcement contract
    // (decision 4) — the explicit `IsolationMode::Log` opt-out (N3.2:
    // `Enforce` became the default, Log is reached only via
    // `--allow-isolation-log-mode`) must never break the checker. None of
    // the three checks above may be blocked in Log mode. Network is
    // asserted more loosely (its errno must simply not be the isolation
    // mechanism's own denial code) because a real connect attempt
    // legitimately fails with ECONNREFUSED/ETIMEDOUT for unrelated reasons;
    // write/exec are asserted as fully successful since both targets are
    // test-owned.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn configure_child_sandbox_log_mode_does_not_block_any_check() {
        let scratch = make_temp_dir("log-scratch", line!());
        let outside = make_temp_dir("log-outside", line!());
        let config = LeanRunnerConfig::new("test-isolation-log-mode")
            .with_package_dir(&scratch)
            .with_isolation_mode(IsolationMode::Log);

        let mut cmd = Command::new(probe_bin());
        cmd.arg("network-connect")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_child_sandbox(&mut cmd, &config).expect("configure child sandbox");
        let output = cmd.output().expect("spawn sandbox_probe");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            !stdout.contains("errno=Some(1)") && !stdout.contains("errno=Some(13)"),
            "Log mode must never block network egress with the isolation \
             mechanism's own errno (EPERM=1 macOS Seatbelt / EACCES=13 Linux \
             seccomp+Landlock); got: {stdout}"
        );

        let outside_target = outside.join("log-mode-write.txt");
        let mut cmd = Command::new(probe_bin());
        cmd.arg("write")
            .arg(&outside_target)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_child_sandbox(&mut cmd, &config).expect("configure child sandbox");
        let output = cmd.output().expect("spawn sandbox_probe");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("RESULT=ALLOWED"),
            "Log mode must not block writes outside package_dir; got: {stdout}"
        );

        let outside_probe = outside.join("sandbox_probe");
        std::fs::copy(probe_bin(), &outside_probe).expect("copy probe binary outside allowlist");
        let mut cmd = Command::new(&outside_probe);
        cmd.arg("noop")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_child_sandbox(&mut cmd, &config).expect("configure child sandbox");
        let output = cmd
            .output()
            .expect("Log mode must not block exec of a non-toolchain binary");
        assert!(
            output.status.success(),
            "Log mode must not block non-toolchain exec; status={:?}",
            output.status
        );

        let _ = std::fs::remove_dir_all(&scratch);
        let _ = std::fs::remove_dir_all(&outside);
    }
}

#[cfg(test)]
mod release_controls;
