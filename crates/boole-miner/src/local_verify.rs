// Local Lean verifier — pre-share elaboration check.
//
// The miner runs this BEFORE share-grinding so honest miners never burn
// ticket budget on a proof the dispatcher will reject.
//
// `Verifier` is the trait the mining loop consumes. `AcceptingVerifier` /
// `RejectingVerifier` are in-process stubs for tests; `LeanVerifier` is
// the production path: it generates the family-v031 instance from the
// target seed, wraps the LLM-supplied proof term in a `BooleVerifyMod`
// module, and shells out to `lake exec boole_check` via boole-lean-runner.
//
// The seam `Verifier::verify(seed_hex, d, proof_source, n)` matches the
// previous (lake-verify-feature-gated) signature byte-for-byte so nothing
// downstream has to change.
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::family_v031::{generate_from_hex, lean_module, Profile};

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

const STDERR_TAIL_LIMIT: usize = 800;

fn tail(s: &str, limit: usize) -> String {
    if s.len() > limit {
        s[s.len() - limit..].to_string()
    } else {
        s.to_string()
    }
}

fn parse_profile(s: &str) -> Option<Profile> {
    match s {
        "v031-lp" => Some(Profile::V031Lp),
        "v031" => Some(Profile::V031),
        _ => None,
    }
}

/// Production verifier. Regenerates the family-v031 instance from the
/// target seed, wraps the LLM-supplied proof term in a BooleVerifyMod
/// module, and runs `lake exec boole_check` via boole-lean-runner.
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

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Verifier for LeanVerifier {
    fn verify(&self, seed_hex: &str, _d: u32, proof_source: &str, _n: Option<u32>) -> VerifyResult {
        let started = Instant::now();
        let Some(profile) = parse_profile(&self.profile) else {
            return VerifyResult {
                accepted: false,
                reason: VerifyReason::EmitFailed,
                elapsed: started.elapsed(),
                stderr_tail: format!(
                    "LeanVerifier does not support profile {:?}; supported: v031-lp, v031",
                    self.profile
                ),
            };
        };
        let instance = match generate_from_hex(seed_hex, profile) {
            Ok(i) => i,
            Err(e) => {
                return VerifyResult {
                    accepted: false,
                    reason: VerifyReason::EmitFailed,
                    elapsed: started.elapsed(),
                    stderr_tail: format!("decode seed_hex failed: {e}"),
                };
            }
        };
        let module_text = lean_module(&instance, proof_source);

        let tmp_dir = std::env::temp_dir().join(format!(
            "boole-verify-{}-{}",
            std::process::id(),
            seed_hex_short(seed_hex),
        ));
        if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
            return VerifyResult {
                accepted: false,
                reason: VerifyReason::EmitFailed,
                elapsed: started.elapsed(),
                stderr_tail: format!("mkdir failed: {e}"),
            };
        }
        let proof_path = tmp_dir.join("VerifyMod.lean");
        if let Err(e) = std::fs::write(&proof_path, &module_text) {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return VerifyResult {
                accepted: false,
                reason: VerifyReason::EmitFailed,
                elapsed: started.elapsed(),
                stderr_tail: format!("write proof failed: {e}"),
            };
        }

        let cfg = boole_lean_runner::LeanRunnerConfig::new("boole-miner-verifier")
            .with_package_dir(self.lean_dir.clone())
            .with_timeout_ms(self.timeout.as_millis() as u64);
        let runner = boole_lean_runner::LeanRunner::new(cfg);

        let result = runner.check_file(&proof_path);
        let _ = std::fs::remove_dir_all(&tmp_dir);

        match result {
            Ok(r) if r.accepted => VerifyResult {
                accepted: true,
                reason: VerifyReason::Accepted,
                elapsed: started.elapsed(),
                stderr_tail: String::new(),
            },
            Ok(r) if r.timed_out => VerifyResult {
                accepted: false,
                reason: VerifyReason::ElaborateTimeout,
                elapsed: started.elapsed(),
                stderr_tail: tail(&r.stderr, STDERR_TAIL_LIMIT),
            },
            Ok(r) => {
                let mut diag = r.stdout;
                if !diag.is_empty() && !diag.ends_with('\n') {
                    diag.push('\n');
                }
                diag.push_str(&r.stderr);
                VerifyResult {
                    accepted: false,
                    reason: VerifyReason::ElaborateFailed,
                    elapsed: started.elapsed(),
                    stderr_tail: tail(&diag, STDERR_TAIL_LIMIT),
                }
            }
            Err(e) => {
                let msg = e.to_string();
                let reason = if msg.contains("does not exist")
                    || msg.contains("No such file")
                    || msg.contains("failed to run lake")
                {
                    VerifyReason::BinaryNotFound
                } else {
                    VerifyReason::ElaborateFailed
                };
                VerifyResult {
                    accepted: false,
                    reason,
                    elapsed: started.elapsed(),
                    stderr_tail: tail(&msg, STDERR_TAIL_LIMIT),
                }
            }
        }
    }
}

fn seed_hex_short(seed_hex: &str) -> &str {
    if seed_hex.len() > 12 {
        &seed_hex[..12]
    } else {
        seed_hex
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepting_verifier_returns_accepted() {
        let v = AcceptingVerifier;
        let r = v.verify("00", 0, "rfl", None);
        assert!(r.accepted);
        assert_eq!(r.reason, VerifyReason::Accepted);
    }

    #[test]
    fn rejecting_verifier_returns_pinned_reason() {
        let v = RejectingVerifier::new(VerifyReason::ElaborateFailed);
        let r = v.verify("00", 0, "rfl", None);
        assert!(!r.accepted);
        assert_eq!(r.reason, VerifyReason::ElaborateFailed);
    }

    #[test]
    fn lean_verifier_rejects_unsupported_profile() {
        let v = LeanVerifier::new(PathBuf::from("/nonexistent"), "v01");
        let r = v.verify(
            "0000000000000000000000000000000000000000000000000000000000000000",
            0,
            "rfl",
            None,
        );
        assert!(!r.accepted);
        assert_eq!(r.reason, VerifyReason::EmitFailed);
        assert!(r.stderr_tail.contains("does not support profile"));
    }

    #[test]
    fn lean_verifier_reports_emit_failed_on_bad_seed() {
        let v = LeanVerifier::new(PathBuf::from("/nonexistent"), "v031-lp");
        let r = v.verify("not-hex", 0, "rfl", None);
        assert!(!r.accepted);
        assert_eq!(r.reason, VerifyReason::EmitFailed);
    }

    /// End-to-end: drive `lake exec boole_check` against a real-life
    /// canonical proof produced by `family_v031`. Validates that the
    /// module text shape, BooleVerifyMod namespace, and family helpers
    /// are all in sync with `Boole.Family.V0Helpers`. Ignored by default
    /// because it requires the Lean toolchain and a built checker.
    /// Run with: `cargo test -p boole-miner --lib -- --ignored
    /// lean_verifier_accepts_canonical_proof`.
    #[test]
    #[ignore]
    fn lean_verifier_accepts_canonical_proof() {
        use crate::family_v031;
        let lean_dir = std::env::var("BOOLE_LEAN_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let manifest = env!("CARGO_MANIFEST_DIR");
                PathBuf::from(manifest).join("../../lean/checker")
            });
        // Use a fixed seed so the test is deterministic.
        let seed_hex = "b606f7037936d8191ded73d7051fb423e72d2b442b0e868da9e3b11e72c7f764";
        for profile_str in ["v031-lp", "v031"] {
            let profile = parse_profile(profile_str).unwrap();
            let inst = family_v031::generate_from_hex(seed_hex, profile).unwrap();
            let canonical = family_v031::render_canonical_proof(&inst);
            let v = LeanVerifier::new(lean_dir.clone(), profile_str)
                .with_timeout(Duration::from_secs(120));
            let r = v.verify(seed_hex, 0, &canonical, None);
            assert!(
                r.accepted,
                "canonical proof rejected for {profile_str}: \
                 reason={:?} stderr={}",
                r.reason, r.stderr_tail
            );
        }
    }
}
