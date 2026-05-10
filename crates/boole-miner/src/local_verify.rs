// Local Lean verifier — pre-share elaboration check.
//
// Mirror of pof's `localVerify.ts`. Drives the same two-step pipeline:
//
//   1. lake exec verify_emit <seed_hex> <D> <proof_path> <out_path>
//      BooleVerifyMod <profile> [<N>]
//   2. lake env lean -D warningAsError=true -D linter.unusedVariables=false
//      <out_path>
//
// `warningAsError=true` is critical — without it, leftover `sorry` exits 0
// with only a warning, which would let garbage candidates pass.
//
// The miner runs this BEFORE share-grinding so honest miners never burn
// ticket budget on a proof the dispatcher will reject.
//
// `Verifier` is the trait the mining loop consumes. `AcceptingVerifier` /
// `RejectingVerifier` are in-process stubs for tests; `LeanVerifier`
// (feature `lake-verify`) is the production path.
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyReason {
    Accepted,
    EmitFailed,
    ElaborateFailed,
    ElaborateTimeout,
    BinaryNotFound,
}

impl VerifyReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            VerifyReason::Accepted => "accepted",
            VerifyReason::EmitFailed => "emit_failed",
            VerifyReason::ElaborateFailed => "elaborate_failed",
            VerifyReason::ElaborateTimeout => "elaborate_timeout",
            VerifyReason::BinaryNotFound => "binary_not_found",
        }
    }
}

#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub accepted: bool,
    pub reason: VerifyReason,
    pub elapsed: Duration,
    pub stderr_tail: String,
}

pub trait Verifier: Send + Sync {
    fn verify(&self, seed_hex: &str, d: u32, proof_source: &str, n: Option<u32>) -> VerifyResult;
}

/// Always-accept stub. Used by `--mock-verify-accept` and by the integration
/// tests that don't need to exercise Lean.
pub struct AcceptingVerifier;

impl Verifier for AcceptingVerifier {
    fn verify(
        &self,
        _seed_hex: &str,
        _d: u32,
        _proof_source: &str,
        _n: Option<u32>,
    ) -> VerifyResult {
        VerifyResult {
            accepted: true,
            reason: VerifyReason::Accepted,
            elapsed: Duration::ZERO,
            stderr_tail: String::new(),
        }
    }
}

/// Always-reject stub. Lets tests pin a specific failure reason.
pub struct RejectingVerifier {
    pub reason: VerifyReason,
}

impl RejectingVerifier {
    pub fn new(reason: VerifyReason) -> Self {
        Self { reason }
    }
}

impl Verifier for RejectingVerifier {
    fn verify(
        &self,
        _seed_hex: &str,
        _d: u32,
        _proof_source: &str,
        _n: Option<u32>,
    ) -> VerifyResult {
        VerifyResult {
            accepted: false,
            reason: self.reason.clone(),
            elapsed: Duration::ZERO,
            stderr_tail: String::new(),
        }
    }
}

#[cfg(feature = "lake-verify")]
mod lake {
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{Duration, Instant};

    use super::{Verifier, VerifyReason, VerifyResult};

    const STDERR_TAIL_LIMIT: usize = 800;

    fn tail(s: &str, limit: usize) -> String {
        if s.len() > limit {
            s[s.len() - limit..].to_string()
        } else {
            s.to_string()
        }
    }

    pub struct LeanVerifier {
        pub lean_dir: PathBuf,
        pub timeout: Duration,
        pub profile: String,
    }

    impl LeanVerifier {
        pub fn new(lean_dir: PathBuf, profile: impl Into<String>) -> Self {
            Self {
                lean_dir,
                timeout: Duration::from_secs(60),
                profile: profile.into(),
            }
        }
    }

    impl Verifier for LeanVerifier {
        fn verify(
            &self,
            seed_hex: &str,
            d: u32,
            proof_source: &str,
            n: Option<u32>,
        ) -> VerifyResult {
            let started = Instant::now();
            let tmp_dir = std::env::temp_dir().join(format!("boole-verify-{}", std::process::id()));
            if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
                return VerifyResult {
                    accepted: false,
                    reason: VerifyReason::EmitFailed,
                    elapsed: started.elapsed(),
                    stderr_tail: format!("mkdir failed: {e}"),
                };
            }
            let proof_path = tmp_dir.join("proof.txt");
            let out_path = tmp_dir.join("VerifyMod.lean");
            if let Err(e) = std::fs::write(&proof_path, proof_source) {
                return VerifyResult {
                    accepted: false,
                    reason: VerifyReason::EmitFailed,
                    elapsed: started.elapsed(),
                    stderr_tail: format!("write proof failed: {e}"),
                };
            }
            let mut emit_cmd = Command::new("lake");
            emit_cmd
                .arg("exec")
                .arg("verify_emit")
                .arg(seed_hex)
                .arg(d.to_string())
                .arg(&proof_path)
                .arg(&out_path)
                .arg("BooleVerifyMod")
                .arg(&self.profile);
            if matches!(self.profile.as_str(), "v03" | "v031" | "v031-lp") {
                emit_cmd.arg(n.unwrap_or(1).to_string());
            }
            emit_cmd.current_dir(&self.lean_dir);
            let emit_out = match emit_cmd.output() {
                Ok(o) => o,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let _ = std::fs::remove_dir_all(&tmp_dir);
                    return VerifyResult {
                        accepted: false,
                        reason: VerifyReason::BinaryNotFound,
                        elapsed: started.elapsed(),
                        stderr_tail: "lake binary not found on PATH".to_string(),
                    };
                }
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&tmp_dir);
                    return VerifyResult {
                        accepted: false,
                        reason: VerifyReason::EmitFailed,
                        elapsed: started.elapsed(),
                        stderr_tail: e.to_string(),
                    };
                }
            };
            if !emit_out.status.success() {
                let stderr = String::from_utf8_lossy(&emit_out.stderr);
                let _ = std::fs::remove_dir_all(&tmp_dir);
                return VerifyResult {
                    accepted: false,
                    reason: VerifyReason::EmitFailed,
                    elapsed: started.elapsed(),
                    stderr_tail: tail(&stderr, STDERR_TAIL_LIMIT),
                };
            }

            let mut elab_cmd = Command::new("lake");
            elab_cmd
                .arg("env")
                .arg("lean")
                .arg("-D")
                .arg("warningAsError=true")
                .arg("-D")
                .arg("linter.unusedVariables=false")
                .arg(&out_path);
            elab_cmd.current_dir(&self.lean_dir);
            let elab_out = match elab_cmd.output() {
                Ok(o) => o,
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&tmp_dir);
                    return VerifyResult {
                        accepted: false,
                        reason: VerifyReason::ElaborateFailed,
                        elapsed: started.elapsed(),
                        stderr_tail: e.to_string(),
                    };
                }
            };
            if !elab_out.status.success() {
                let mut diag = String::new();
                diag.push_str(&String::from_utf8_lossy(&elab_out.stdout));
                diag.push_str(&String::from_utf8_lossy(&elab_out.stderr));
                let _ = std::fs::remove_dir_all(&tmp_dir);
                return VerifyResult {
                    accepted: false,
                    reason: VerifyReason::ElaborateFailed,
                    elapsed: started.elapsed(),
                    stderr_tail: tail(&diag, STDERR_TAIL_LIMIT),
                };
            }
            let _ = std::fs::remove_dir_all(&tmp_dir);
            let _ = self.timeout; // reserved
            VerifyResult {
                accepted: true,
                reason: VerifyReason::Accepted,
                elapsed: started.elapsed(),
                stderr_tail: String::new(),
            }
        }
    }
}

#[cfg(feature = "lake-verify")]
pub use lake::LeanVerifier;
