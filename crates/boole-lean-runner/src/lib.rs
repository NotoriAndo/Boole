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
//!   post-elaboration process (`BooleCheck/Audit.lean`) computes the
//!   accepted file's full axiom-dependency closure and rejects it unless
//!   that closure is a subset of `{propext, Classical.choice, Quot.sound}`.
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
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
    pub checker_exe: String,
    /// Wall-clock bound. SC.9a / ADR-0016 (a) — containment ONLY, never a
    /// verdict input: expiring it yields `retryable_unavailable`, not a
    /// reject. The verdict-bearing bound is the step budget below.
    pub timeout_ms: u64,
    pub memory_limit_mb: u64,
    pub output_limit_bytes: usize,
    pub isolation_mode: IsolationMode,
    /// SC.9a / ADR-0016 (a)(b) — the committed step budget, forwarded to
    /// the checker as `lean -D maxHeartbeats=<n>` (Lean counts this option
    /// in thousands of raw heartbeats). This IS the verdict input: the same
    /// proof bytes under the same budget exhaust it identically on every
    /// node. The default mirrors `boole_core::BASE_LANE_MAX_HEARTBEATS`
    /// (Tier-2 rule constant); the family lane overrides it from the
    /// consensus-committed `FamilyManifest.resource_limits`.
    pub max_heartbeats: u64,
    /// Companion verdict-bearing counter, forwarded as
    /// `lean -D maxRecDepth=<n>`. Default mirrors
    /// `boole_core::BASE_LANE_MAX_REC_DEPTH` (ADR-0016 (b-1)).
    pub max_rec_depth: u64,
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
            isolation_mode: IsolationMode::default(),
            max_heartbeats: 400_000,
            max_rec_depth: 512,
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

    pub fn with_isolation_mode(mut self, isolation_mode: IsolationMode) -> Self {
        self.isolation_mode = isolation_mode;
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

        // SC.9a / ADR-0016 (a)(b) — the committed step budget rides along
        // as explicit checker args so the verdict never inherits Lean's own
        // (uncommitted) defaults. `BooleCheck.Main` forwards them to the
        // inner `lean` invocation as `-D maxHeartbeats=<n> -D maxRecDepth=<n>`.
        let mut primary_command = Command::new("lake");
        primary_command
            .arg("exec")
            .arg(&self.config.checker_exe)
            .arg(proof_path)
            .arg(self.config.max_heartbeats.to_string())
            .arg(self.config.max_rec_depth.to_string())
            .current_dir(&self.config.package_dir);
        let primary = self.run_sandboxed(primary_command).with_context(|| {
            format!(
                "failed to run lake exec {} in {}",
                self.config.checker_exe,
                self.config.package_dir.display()
            )
        })?;

        if !primary.success {
            let verdict = classify_failed_run(&primary);
            return Ok(LeanCheckResult {
                accepted: false,
                exit_code: primary.exit_code,
                stdout: primary.stdout,
                stderr: primary.stderr,
                timed_out: primary.timed_out,
                output_truncated: primary.output_truncated,
                verdict,
                evidence,
            });
        }

        // TB.1 / ADR-0013 — the primary checker accepted the file; now run
        // the PRIMARY soundness boundary, the post-elaboration axiom-closure
        // audit, as its own fresh `lake env lean --run` process. See
        // `enforce_axiom_allowlist`'s doc comment for the isolation argument
        // and `BooleCheck/Audit.lean`'s header for why this is a SEPARATE
        // process rather than a check folded into `BooleCheck.Main`.
        let mut audit_command = Command::new("lake");
        audit_command
            .arg("env")
            .arg("lean")
            .arg("--run")
            .arg(AXIOM_AUDIT_SCRIPT)
            .arg(proof_path)
            .arg(self.config.max_heartbeats.to_string())
            .arg(self.config.max_rec_depth.to_string())
            .current_dir(&self.config.package_dir);
        let audit = self.run_sandboxed(audit_command).with_context(|| {
            format!(
                "failed to run axiom audit in {}",
                self.config.package_dir.display()
            )
        })?;

        let timed_out = primary.timed_out || audit.timed_out;
        let output_truncated = primary.output_truncated || audit.output_truncated;
        match enforce_axiom_allowlist(&audit) {
            Ok(()) => Ok(LeanCheckResult {
                accepted: true,
                exit_code: primary.exit_code,
                stdout: primary.stdout,
                stderr: primary.stderr,
                timed_out,
                output_truncated,
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
                    verdict,
                    evidence,
                })
            }
        }
    }

    /// Runs `command` inside the sandboxed child-process harness shared by
    /// the primary checker invocation and the TB.1 axiom audit: its own
    /// process group (killed as a whole on timeout), rlimits, a scrubbed
    /// environment, and byte-capped drain threads so the child can never
    /// stall the timeout poll loop on a full pipe. `command`'s program and
    /// args must already be set; stdio/env/sandbox are configured here.
    fn run_sandboxed(&self, mut command: Command) -> Result<SandboxedRunOutcome> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        configure_child_environment(&mut command);
        configure_child_sandbox(&mut command, &self.config);

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
        })
    }

    pub fn evidence(&self) -> Result<LeanRunnerEvidence> {
        // SC.9b / ADR-0016 (a-2) — record the toolchain the checker
        // PROCESS actually runs under (package-dir dispatch), never the
        // ambient PATH's lean/lake: an identity no proof was checked
        // under is evidence of nothing.
        let toolchain = effective_toolchain_identity(&self.config.package_dir)?;
        Ok(LeanRunnerEvidence {
            verifier_hash: self.config.verifier_hash.clone(),
            checker: format!("lake exec {}", self.config.checker_exe),
            checker_exe: self.config.checker_exe.clone(),
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

/// The result of running one sandboxed child process to completion (or to
/// timeout). Both the primary checker invocation and the TB.1 axiom audit
/// produce one of these via [`LeanRunner::run_sandboxed`].
struct SandboxedRunOutcome {
    success: bool,
    exit_code: i32,
    stdout: String,
    stderr: String,
    timed_out: bool,
    output_truncated: bool,
}

/// The three axioms Lean's core library trusts as sound by long-standing
/// convention: `propext` (propositional extensionality), `Classical.choice`
/// (excluded middle via choice), and `Quot.sound` (quotient soundness). Any
/// other axiom in a submitted proof's closure means either the proof itself
/// declared a new axiom (directly via `axiom`, or indirectly via
/// `Lean.addDecl` from inside a custom `elab`), or it depends on a
/// Lean-internal axiom whose name contains no blacklisted token (e.g.
/// `Lean.trustCompiler`) — the blacklist alone cannot catch that case, which
/// is exactly why the audit below exists as the primary boundary.
const ALLOWED_AXIOMS: &[&str] = &["propext", "Classical.choice", "Quot.sound"];

/// Relative path (from the checker package root) to the dedicated axiom
/// audit entrypoint. See `BooleCheck/Audit.lean`'s own header comment for
/// why this MUST be a separate `lake env lean --run` process rather than a
/// check folded into `BooleCheck.Main`.
const AXIOM_AUDIT_SCRIPT: &str = "BooleCheck/Audit.lean";

/// Line prefix `BooleCheck/Audit.lean` prints once per axiom in the closure,
/// e.g. `BOOLE_AXIOM propext`.
const AXIOM_AUDIT_LINE_PREFIX: &str = "BOOLE_AXIOM ";

/// Sentinel line `BooleCheck/Audit.lean` prints only after it has finished
/// walking the full axiom closure. Its absence (crash, timeout, SIGKILL)
/// must be treated as rejection, never as silent acceptance.
const AXIOM_AUDIT_DONE_SENTINEL: &str = "BOOLE_AXIOM_AUDIT_DONE";

/// TB.1 / ADR-0013 — the PRIMARY soundness boundary. `outcome` is the result
/// of running `BooleCheck/Audit.lean` in its own process, AFTER the primary
/// checker has already accepted the submission.
///
/// Mechanization / isolation argument (mirrors the header comment in
/// `BooleCheck/Audit.lean`): the audit script re-parses and re-elaborates
/// the submitted file from scratch into a brand-new `Environment` that the
/// submitted file's own commands never touch, then computes the transitive
/// axiom closure of every declaration the file newly introduced by calling
/// `Lean.CollectAxioms.collect` — the same machinery backing `#print axioms`
/// — and prints it on stdout. That reference is resolved against the audit
/// script's OWN compiled code, not looked up dynamically through the
/// elaborated environment, so nothing the submitted source does (not even
/// `Lean.addDecl` invoked from inside a custom `elab`) can redirect what the
/// audit itself runs: the submission can only influence what ends up IN the
/// environment, and the audit inspects that environment from the outside,
/// in a fresh OS process separate from the primary checker's own process.
///
/// A submission is accepted only if every printed axiom is in
/// [`ALLOWED_AXIOMS`] AND the [`AXIOM_AUDIT_DONE_SENTINEL`] line is present;
/// a missing sentinel (crash, timeout, kill) is rejection, never silent
/// acceptance.
fn enforce_axiom_allowlist(
    outcome: &SandboxedRunOutcome,
) -> std::result::Result<(), (LeanVerdict, String)> {
    // SC.9a / ADR-0016 (a-3) — a containment kill of the audit process is an
    // availability failure, never a verdict; everything below this guard is
    // deterministic (same bytes + same budget reproduce it on every node).
    if outcome.timed_out {
        return Err((
            LeanVerdict::RetryableUnavailable {
                reason: "containment_wall_clock_kill".to_string(),
            },
            "axiom audit timed out".to_string(),
        ));
    }
    if outcome.exit_code < 0 {
        return Err((
            LeanVerdict::RetryableUnavailable {
                reason: "containment_killed".to_string(),
            },
            "axiom audit killed before completion".to_string(),
        ));
    }
    // SC.9a / ADR-0016 (a-2) layer 2 — the audit scans the raw source for
    // budget-bearing option tokens and refuses with this typed marker.
    if let Some(line) = combined_output(outcome)
        .lines()
        .find(|line| line.starts_with(BUDGET_OVERRIDE_MARKER_PREFIX))
    {
        return Err((
            LeanVerdict::DeterministicReject {
                reason: REJECT_BUDGET_OVERRIDE_FORBIDDEN.to_string(),
            },
            line.to_string(),
        ));
    }
    if lean_output_reports_budget_exhaustion(&combined_output(outcome)) {
        return Err((
            LeanVerdict::DeterministicReject {
                reason: REJECT_BUDGET_EXCEEDED.to_string(),
            },
            "axiom audit exhausted the committed step budget".to_string(),
        ));
    }
    if !outcome.success {
        return Err((
            LeanVerdict::DeterministicReject {
                reason: REJECT_AXIOM_AUDIT.to_string(),
            },
            format!(
                "axiom audit process exited non-zero (exit_code={}): {}",
                outcome.exit_code, outcome.stderr
            ),
        ));
    }
    let mut saw_sentinel = false;
    let mut offending: Vec<String> = Vec::new();
    for line in outcome.stdout.lines() {
        if line == AXIOM_AUDIT_DONE_SENTINEL {
            saw_sentinel = true;
            continue;
        }
        if let Some(axiom) = line.strip_prefix(AXIOM_AUDIT_LINE_PREFIX) {
            if !ALLOWED_AXIOMS.contains(&axiom) {
                offending.push(axiom.to_string());
            }
        }
    }
    if !saw_sentinel {
        return Err((
            LeanVerdict::DeterministicReject {
                reason: REJECT_AXIOM_AUDIT.to_string(),
            },
            "axiom audit did not reach completion (missing sentinel)".to_string(),
        ));
    }
    if !offending.is_empty() {
        return Err((
            LeanVerdict::DeterministicReject {
                reason: REJECT_AXIOM_AUDIT.to_string(),
            },
            format!(
                "proof depends on non-allowlisted axiom(s): {}",
                offending.join(", ")
            ),
        ));
    }
    Ok(())
}

/// Typed marker line `BooleCheck/Audit.lean` prints (and exits non-zero on)
/// when the submitted source contains a budget-bearing option token —
/// ADR-0016 (a-2) layer 2, independent of the Rust-side intake scan.
const BUDGET_OVERRIDE_MARKER_PREFIX: &str = "BOOLE_BUDGET_OVERRIDE";

fn combined_output(outcome: &SandboxedRunOutcome) -> String {
    let mut combined = outcome.stdout.clone();
    if !combined.is_empty() && !combined.ends_with('\n') {
        combined.push('\n');
    }
    combined.push_str(&outcome.stderr);
    combined
}

/// Lean's own diagnostics when a `-D maxHeartbeats`/`-D maxRecDepth` budget
/// runs out. These strings are produced by the pinned toolchain, so they are
/// as stable as the checker artifact itself (the pin covers `lean-toolchain`).
fn lean_output_reports_budget_exhaustion(output: &str) -> bool {
    output.contains("maximum number of heartbeats")
        || output.contains("maximum recursion depth has been reached")
}

/// SC.9a / ADR-0016 (a-3) — classify a failed primary checker run into the
/// three-state verdict contract: containment (wall-clock kill or signal
/// death) is `retryable_unavailable`; everything else the checker itself
/// reported is deterministic, with committed-budget exhaustion typed as
/// `budget_exceeded`.
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
    if lean_output_reports_budget_exhaustion(&combined_output(outcome)) {
        return LeanVerdict::DeterministicReject {
            reason: REJECT_BUDGET_EXCEEDED.to_string(),
        };
    }
    LeanVerdict::DeterministicReject {
        reason: REJECT_LEAN_REJECTED.to_string(),
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

/// How a forbidden token's boundaries are checked (see
/// [`contains_forbidden_token`] vs [`contains_forbidden_prefix`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenBoundary {
    /// Both the byte before AND after the match must be non-word characters
    /// (or absent). Correct for identifier-shaped tokens like `sorry`/`axiom`
    /// where `my_axiom_lemma` must NOT be flagged.
    Word,
    /// Only the byte BEFORE the match must be a non-word character (or
    /// absent). Needed for tokens like `debug.` whose next byte is always a
    /// word character (the option name, e.g. `skipKernelTC`) — a `Word`
    /// check could never match `debug.` at all.
    PrefixOnly,
}

/// P1.9 / TB.1 (ADR-0013) — tokens that make a Lean proof unsound, or that
/// let a submission escape the checker's intended trust boundary, and must
/// be rejected before the proof is ever handed to the checker:
///
/// - `sorry` admits any goal without proof;
/// - `axiom` introduces an unverified postulate the kernel trusts blindly;
/// - `native_decide` discharges a goal via native compiled code, outside
///   the trusted kernel;
/// - `#eval` runs arbitrary IO during checking (see below);
/// - `addDecl` lets a custom `elab`/`macro` command register an axiom (or
///   any declaration) directly into the environment, bypassing the `axiom`
///   keyword scan entirely;
/// - `elab`/`macro` let a submission run arbitrary `IO`/`MetaM`/`TermElabM`
///   code *during elaboration*, before the post-elaboration axiom audit
///   (see `enforce_axiom_allowlist`) ever starts;
/// - `initialize` runs IO at import/elaboration time via the same escape;
/// - `debug.` (matched as a prefix, not a whole word — see
///   `TokenBoundary::PrefixOnly`) blocks every `set_option debug.*`, in
///   particular `debug.skipKernelTC`, which disables kernel typechecking
///   entirely. No `debug.*` option has a legitimate use in a submitted proof.
///
/// This blacklist is defense-in-depth, fail-fast hardening, NOT the primary
/// soundness boundary — a blacklist can never enumerate every escape (e.g.
/// a proof term that merely names `Lean.trustCompiler` uses no keyword
/// here). The post-elaboration axiom-closure audit in `check_file` is the
/// boundary that actually decides soundness.
///
/// Each `Word` token is matched on a word boundary (after line comments are
/// stripped), so identifiers that merely contain the substring
/// (`my_axiom_lemma`, `native_decide_helper`) are never flagged.
const FORBIDDEN_TOKENS: &[(&[u8], &str, TokenBoundary)] = &[
    (b"sorry", "sorry", TokenBoundary::Word),
    (b"axiom", "axiom", TokenBoundary::Word),
    (b"native_decide", "native_decide", TokenBoundary::Word),
    // N0-pre.1 — `#eval` executes arbitrary IO (`IO.Process.run`/
    // `IO.FS.readFile`) with node privileges and Lean compiles it as a
    // side-effecting command (not an error), so a hostile proof could run
    // code during checking. Reject it pre-spawn like the other unsound tokens.
    (b"#eval", "#eval", TokenBoundary::Word),
    (b"addDecl", "addDecl", TokenBoundary::Word),
    (b"elab", "elab", TokenBoundary::Word),
    (b"macro", "macro", TokenBoundary::Word),
    (b"initialize", "initialize", TokenBoundary::Word),
    (b"debug.", "debug.", TokenBoundary::PrefixOnly),
    // SC.9a / ADR-0016 (a-2) layer 1 — the committed step budget is a
    // ceiling the source cannot raise: `set_option maxHeartbeats <M>`
    // (including `0` = unlimited) or `set_option maxRecDepth <M>` would
    // override the runner's `-D` defaults and make the consensus budget
    // advisory. Matching the bare option name (word boundary) rejects every
    // spelling that could reach the option without flagging identifiers
    // that merely contain the substring. Layer 2 is the raw-text scan in
    // `BooleCheck/Audit.lean` (`BOOLE_BUDGET_OVERRIDE`).
    (b"maxHeartbeats", "maxHeartbeats", TokenBoundary::Word),
    (b"maxRecDepth", "maxRecDepth", TokenBoundary::Word),
];

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
    if module.is_empty() || ALLOWED_IMPORTS.contains(&module) {
        None
    } else {
        Some(module.to_string())
    }
}

/// Returns the first forbidden token (or disallowed import) found in `path`
/// together with its 1-based line number, or `None` if the proof is free of
/// all of them.
fn scan_for_forbidden_tokens(path: &Path) -> Result<Option<(String, usize)>> {
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
        for &(needle, name, boundary) in FORBIDDEN_TOKENS {
            let hit = match boundary {
                TokenBoundary::Word => contains_forbidden_token(line, needle),
                TokenBoundary::PrefixOnly => contains_forbidden_prefix(line, needle),
            };
            if hit {
                return Ok(Some((name.to_string(), idx + 1)));
            }
        }
        if let Some(module) = disallowed_import_on_line(line) {
            return Ok(Some((format!("import {module}"), idx + 1)));
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

/// Like [`contains_forbidden_token`] but only checks the byte BEFORE the
/// match, not after — for tokens such as `debug.` where the byte after the
/// match is always a word character (the option name) so a whole-word check
/// could never fire. See [`TokenBoundary::PrefixOnly`].
fn contains_forbidden_prefix(line: &str, needle: &[u8]) -> bool {
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
        let is_word_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        if before.map(is_word_char).unwrap_or(false) {
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
    install_kernel_isolation(command, config);
}

// ADR-0008 — kernel-layer isolation on top of the pgroup/rlimit/env-scrub
// baseline above. Scope for this landing slice is exactly the three
// characterization guards named in the ADR: deny network egress, deny
// filesystem writes outside the checker's own package dir, deny `exec` of
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
    vec![config.package_dir.clone()]
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
    // `lake exec <checker_exe>` runs a binary built under the package dir
    // (e.g. `.lake/build/bin/boole_check`), so the package dir itself must
    // be exec-allowed alongside the toolchain's own PATH/elan directories.
    dirs.push(config.package_dir.clone());
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
    let write_subpaths: String = scratch_dirs(config)
        .iter()
        .map(|dir| {
            format!(
                "(subpath {:?})",
                canonical_or_self(dir).display().to_string()
            )
        })
        .collect::<Vec<_>>()
        .join("\n      ");
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
    format!(
        r#"(version 1)
(allow default)
(deny network*)
(deny file-write*
  (require-not
    (require-any
      {write_subpaths})))
(deny process-exec
  (require-not
    (require-any
      {exec_subpaths})))
"#
    )
}

#[cfg(target_os = "macos")]
fn install_kernel_isolation(command: &mut Command, config: &LeanRunnerConfig) {
    use std::os::unix::process::CommandExt;
    if config.isolation_mode != IsolationMode::Enforce {
        // Log mode: no non-blocking Seatbelt level exists, so install
        // nothing (see module comment above).
        return;
    }
    let profile = seatbelt_profile(config);
    let c_profile = match std::ffi::CString::new(profile) {
        Ok(c) => c,
        Err(_) => return, // NUL byte in a path; fail open rather than panic.
    };
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
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn install_kernel_isolation(_command: &mut Command, _config: &LeanRunnerConfig) {
    // Other Unix targets (e.g. BSD) are not part of Boole's supported dev/CI
    // matrix; they keep the portable pgroup/rlimit/env-scrub baseline only.
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
fn build_seccomp_program(mode: IsolationMode) -> anyhow::Result<seccompiler::BpfProgram> {
    use seccompiler::{SeccompAction, SeccompFilter, TargetArch};

    let match_action = match mode {
        IsolationMode::Log => SeccompAction::Log,
        IsolationMode::Enforce => SeccompAction::Errno(libc::EACCES as u32),
    };
    let rules = network_egress_syscalls()
        .into_iter()
        .map(|sysno| (sysno, vec![]))
        .collect();
    let arch = TargetArch::try_from(std::env::consts::ARCH).map_err(|_| {
        anyhow!(
            "unsupported seccomp target arch: {}",
            std::env::consts::ARCH
        )
    })?;
    let filter = SeccompFilter::new(rules, SeccompAction::Allow, match_action, arch)?;
    let program = seccompiler::BpfProgram::try_from(filter)?;
    Ok(program)
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

    let abi = ABI::V1;
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
fn install_kernel_isolation(command: &mut Command, config: &LeanRunnerConfig) {
    use std::os::unix::process::CommandExt;

    let mode = config.isolation_mode;
    let program = match build_seccomp_program(mode) {
        Ok(p) => p,
        Err(_) => return, // Fail open: never break the checker over a filter-build bug.
    };
    // Landlock has no non-blocking level (like macOS Seatbelt, unlike
    // seccomp's RET_LOG); only build/apply the ruleset in Enforce mode so Log
    // mode's filesystem behavior stays identical to the pre-ADR-0008
    // baseline, matching the same reasoning as the macOS implementation.
    let ruleset = if mode == IsolationMode::Enforce {
        build_landlock_ruleset(config).ok()
    } else {
        None
    };
    let mut ruleset = ruleset;
    unsafe {
        command.pre_exec(move || {
            seccompiler::apply_filter(&program)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            if let Some(rs) = ruleset.take() {
                rs.restrict_self()
                    .map_err(|e| std::io::Error::other(e.to_string()))?;
            }
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

/// SC.9b / ADR-0016 (a-2) — the executable toolchain identity of the
/// checker PROCESS: `lean`/`lake` resolved exactly the way the sandboxed
/// child resolves them (scrubbed environment, `cwd = package_dir`, elan
/// dispatch by the package's `lean-toolchain` file). A bare
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
static EFFECTIVE_TOOLCHAIN_CACHE: Mutex<Option<HashMap<PathBuf, EffectiveToolchain>>> =
    Mutex::new(None);

pub fn effective_toolchain_identity(package_dir: &Path) -> Result<EffectiveToolchain> {
    let key = package_dir
        .canonicalize()
        .unwrap_or_else(|_| package_dir.to_path_buf());
    if let Ok(guard) = EFFECTIVE_TOOLCHAIN_CACHE.lock() {
        if let Some(cached) = guard.as_ref().and_then(|map| map.get(&key)) {
            return Ok(cached.clone());
        }
    }
    let lean_version = effective_command_output(package_dir, &["env", "lean", "--version"])?;
    let lean_githash = parse_between(&lean_version, "commit ", ",")
        .ok_or_else(|| {
            anyhow!("could not parse a commit githash out of lean version line: {lean_version}")
        })?
        .to_string();
    let lake_version = effective_command_output(package_dir, &["--version"])?;
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
fn effective_command_output(package_dir: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("lake");
    command
        .args(args)
        .current_dir(package_dir)
        .stdin(Stdio::null());
    configure_child_environment(&mut command);
    let output = command.output().with_context(|| {
        format!(
            "failed to execute `lake {}` in {}",
            args.join(" "),
            package_dir.display()
        )
    })?;
    if !output.status.success() {
        return Err(anyhow!(
            "`lake {}` failed in {}: {}",
            args.join(" "),
            package_dir.display(),
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
        assert_eq!(
            cfg.isolation_mode,
            IsolationMode::Enforce,
            "ADR-0008 decision 4: N3.2 (network ingress opens) flips the \
             default to Enforce in the same change; Log is opt-out only"
        );
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
        let config = LeanRunnerConfig::new("test-group-kill");
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg("sleep 60 & echo \"$!\"; exec sleep 60")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_child_sandbox(&mut cmd, &config);
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

        kill_child_group(&mut child);
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
        let config = LeanRunnerConfig::new("test-cpu-rlimit");
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
        configure_child_sandbox(&mut cmd, &config);
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
        configure_child_sandbox(&mut cmd, &config);
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
        configure_child_sandbox(&mut cmd, &config);
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
        configure_child_sandbox(&mut cmd, &config);
        let output = cmd.output().expect("spawn sandbox_probe");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("RESULT=ALLOWED"),
            "a write inside the configured package_dir must still succeed; got: {stdout}"
        );

        let _ = std::fs::remove_dir_all(&scratch);
        let _ = std::fs::remove_dir_all(&outside);
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
        configure_child_sandbox(&mut cmd, &config);
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
        configure_child_sandbox(&mut cmd, &config);
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
        configure_child_sandbox(&mut cmd, &config);
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
        configure_child_sandbox(&mut cmd, &config);
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
        configure_child_sandbox(&mut cmd, &config);
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
