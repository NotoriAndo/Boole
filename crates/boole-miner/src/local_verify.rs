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
use std::time::{Duration, Instant, SystemTime};

use crate::family_v031::{
    generate_from_hex as generate_v031_from_hex, lean_module as lean_v031_module, Profile,
};
use crate::family_v1_lenbound::{
    generate_from_hex as generate_v1_lenbound_from_hex, lean_module as lean_v1_lenbound_module,
};

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
    pub attempt_artifact_path: Option<PathBuf>,
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
            attempt_artifact_path: None,
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
            attempt_artifact_path: None,
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

enum LeanProfile {
    V031(Profile),
    V1Lenbound,
}

fn parse_profile(s: &str) -> Option<LeanProfile> {
    match s {
        "v031-lp" => Some(LeanProfile::V031(Profile::V031Lp)),
        "v031" => Some(LeanProfile::V031(Profile::V031)),
        "v1-lenbound" => Some(LeanProfile::V1Lenbound),
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
                    "LeanVerifier does not support profile {:?}; supported: v031-lp, v031, v1-lenbound",
                    self.profile
                ),
                attempt_artifact_path: None,
            };
        };
        let module_text = match profile {
            LeanProfile::V031(profile) => {
                let instance = match generate_v031_from_hex(seed_hex, profile) {
                    Ok(i) => i,
                    Err(e) => {
                        return VerifyResult {
                            accepted: false,
                            reason: VerifyReason::EmitFailed,
                            elapsed: started.elapsed(),
                            stderr_tail: format!("decode seed_hex failed: {e}"),
                            attempt_artifact_path: None,
                        };
                    }
                };
                lean_v031_module(&instance, proof_source)
            }
            LeanProfile::V1Lenbound => {
                let instance = match generate_v1_lenbound_from_hex(seed_hex) {
                    Ok(i) => i,
                    Err(e) => {
                        return VerifyResult {
                            accepted: false,
                            reason: VerifyReason::EmitFailed,
                            elapsed: started.elapsed(),
                            stderr_tail: format!("decode seed_hex failed: {e}"),
                            attempt_artifact_path: None,
                        };
                    }
                };
                lean_v1_lenbound_module(&instance, proof_source)
            }
        };

        let tmp_dir = std::env::temp_dir().join(format!(
            "boole-verify-{}-{}-{}",
            std::process::id(),
            seed_hex_short(seed_hex),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        ));
        if let Err(e) = std::fs::create_dir_all(&tmp_dir) {
            return VerifyResult {
                accepted: false,
                reason: VerifyReason::EmitFailed,
                elapsed: started.elapsed(),
                stderr_tail: format!("mkdir failed: {e}"),
                attempt_artifact_path: None,
            };
        }
        let proof_path = tmp_dir.join("generated_module.lean");
        if let Err(e) = std::fs::write(tmp_dir.join("extracted_proof.lean"), proof_source) {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return VerifyResult {
                accepted: false,
                reason: VerifyReason::EmitFailed,
                elapsed: started.elapsed(),
                stderr_tail: format!("write extracted proof failed: {e}"),
                attempt_artifact_path: None,
            };
        }
        if let Err(e) = std::fs::write(&proof_path, &module_text) {
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return VerifyResult {
                accepted: false,
                reason: VerifyReason::EmitFailed,
                elapsed: started.elapsed(),
                stderr_tail: format!("write proof failed: {e}"),
                attempt_artifact_path: None,
            };
        }

        let cfg = boole_lean_runner::LeanRunnerConfig::new("boole-miner-verifier")
            .with_package_dir(self.lean_dir.clone())
            .with_timeout_ms(self.timeout.as_millis() as u64);
        let runner = boole_lean_runner::LeanRunner::new(cfg);

        let result = runner.check_file(&proof_path);

        let write_diagnostics =
            |stdout: &str, stderr: &str, reason: VerifyReason, elapsed: Duration| {
                let _ = std::fs::write(tmp_dir.join("lean_stdout.txt"), stdout);
                let _ = std::fs::write(tmp_dir.join("lean_stderr.txt"), stderr);
                let result_json = serde_json::json!({
                    "accepted": false,
                    "reason": reason.as_str(),
                    "elapsedMs": elapsed.as_millis(),
                    "stderrTail": tail(&format!("{stdout}{stderr}"), STDERR_TAIL_LIMIT),
                });
                let _ = std::fs::write(
                    tmp_dir.join("verify_result.json"),
                    serde_json::to_string_pretty(&result_json).unwrap_or_else(|_| "{}".to_string()),
                );
            };

        match result {
            Ok(r) if r.accepted => {
                let _ = std::fs::remove_dir_all(&tmp_dir);
                VerifyResult {
                    accepted: true,
                    reason: VerifyReason::Accepted,
                    elapsed: started.elapsed(),
                    stderr_tail: String::new(),
                    attempt_artifact_path: None,
                }
            }
            Ok(r) if r.timed_out => {
                let elapsed = started.elapsed();
                write_diagnostics(
                    &r.stdout,
                    &r.stderr,
                    VerifyReason::ElaborateTimeout,
                    elapsed,
                );
                VerifyResult {
                    accepted: false,
                    reason: VerifyReason::ElaborateTimeout,
                    elapsed,
                    stderr_tail: tail(&r.stderr, STDERR_TAIL_LIMIT),
                    attempt_artifact_path: Some(tmp_dir.clone()),
                }
            }
            Ok(r) => {
                let mut diag = r.stdout.clone();
                if !diag.is_empty() && !diag.ends_with('\n') {
                    diag.push('\n');
                }
                diag.push_str(&r.stderr);
                let elapsed = started.elapsed();
                write_diagnostics(&r.stdout, &r.stderr, VerifyReason::ElaborateFailed, elapsed);
                VerifyResult {
                    accepted: false,
                    reason: VerifyReason::ElaborateFailed,
                    elapsed,
                    stderr_tail: tail(&diag, STDERR_TAIL_LIMIT),
                    attempt_artifact_path: Some(tmp_dir.clone()),
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
                let elapsed = started.elapsed();
                write_diagnostics("", &msg, reason.clone(), elapsed);
                VerifyResult {
                    accepted: false,
                    reason,
                    elapsed,
                    stderr_tail: tail(&msg, STDERR_TAIL_LIMIT),
                    attempt_artifact_path: Some(tmp_dir.clone()),
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

    #[test]
    fn lean_verifier_routes_v1_lenbound_profile_to_lean_runner() {
        let v = LeanVerifier::new(PathBuf::from("/nonexistent"), "v1-lenbound");
        let r = v.verify(
            "0000000000000000000000000000000000000000000000000000000000000000",
            0,
            "by intro xs; simp",
            None,
        );
        assert!(!r.accepted);
        assert_ne!(r.reason, VerifyReason::EmitFailed);
        assert!(!r.stderr_tail.contains("does not support profile"));
    }

    #[test]
    fn lean_verifier_preserves_rejected_attempt_artifact() {
        let missing_package_dir = std::env::temp_dir().join(format!(
            "boole-miner-missing-lean-package-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&missing_package_dir);
        let proof_term = "by intro xs; exact by simp";
        let v = LeanVerifier::new(missing_package_dir, "v031-lp");
        let r = v.verify(
            "b606f7037936d8191ded73d7051fb423e72d2b442b0e868da9e3b11e72c7f764",
            0,
            proof_term,
            None,
        );

        assert!(!r.accepted);
        let artifact_path = r
            .attempt_artifact_path
            .as_ref()
            .expect("rejected Lean attempts keep artifact directory");
        assert!(
            artifact_path.is_dir(),
            "artifact directory should still exist"
        );
        let generated_module = std::fs::read_to_string(artifact_path.join("generated_module.lean"))
            .expect("read preserved generated module");
        assert!(generated_module.contains("namespace BooleVerifyMod"));
        assert!(generated_module.contains(proof_term));
        let extracted_proof = std::fs::read_to_string(artifact_path.join("extracted_proof.lean"))
            .expect("read preserved extracted proof");
        assert_eq!(extracted_proof, proof_term);
        assert!(artifact_path.join("lean_stdout.txt").is_file());
        assert!(artifact_path.join("lean_stderr.txt").is_file());
        let verify_result = std::fs::read_to_string(artifact_path.join("verify_result.json"))
            .expect("read preserved verify result");
        assert!(
            verify_result.contains("binary_not_found")
                || verify_result.contains("elaborate_failed")
        );

        let _ = std::fs::remove_dir_all(artifact_path);
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
            let profile = match parse_profile(profile_str).unwrap() {
                LeanProfile::V031(profile) => profile,
                LeanProfile::V1Lenbound => unreachable!("v031-only canonical proof test"),
            };
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

    /// End-to-end v1 helper-surface sync: the Rust v1 canonical proof renderer
    /// must compose only lemmas provided by `Boole.Family.V0Helpers`, and the
    /// Lean verifier must accept that proof body in the v1 theorem slot.
    /// Ignored by default because it requires the Lean toolchain and checker.
    /// Run with: `cargo test -p boole-miner --lib -- --ignored
    /// lean_verifier_accepts_v1_lenbound_canonical_proof`.
    #[test]
    #[ignore]
    fn lean_verifier_accepts_v1_lenbound_canonical_proof() {
        use crate::family_v1_lenbound;
        let lean_dir = std::env::var("BOOLE_LEAN_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let manifest = env!("CARGO_MANIFEST_DIR");
                PathBuf::from(manifest).join("../../lean/checker")
            });

        // Sweep several deterministic seeds so the calc-chain fix is exercised
        // across many op shapes (mixed `≤`/`=`, all-`≤`, ≥3-op chains). Each
        // proof is rendered from the same instance the verifier derives, so
        // the theorem rhs and proof body always agree.
        let seed_hexes = [
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
            "ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100",
            "deadbeefcafebabe0123456789abcdef0123456789abcdeffeedfacecafe0001",
        ];
        let mut seen_mixed = false;
        for seed_hex in seed_hexes {
            let inst = family_v1_lenbound::generate_from_hex(seed_hex).unwrap();
            let canonical = family_v1_lenbound::render_canonical_proof(&inst);
            assert!(canonical.starts_with("by\n  intro xs\n  calc"));
            if canonical.contains("Nat.le_of_eq") {
                seen_mixed = true;
            }
            let v = LeanVerifier::new(lean_dir.clone(), "v1-lenbound")
                .with_timeout(Duration::from_secs(120));
            let r = v.verify(seed_hex, 0, &canonical, None);
            assert!(
                r.accepted,
                "v1 canonical proof rejected for seed {seed_hex}: \
                 chain={:?} reason={:?} stderr={}",
                inst.chain, r.reason, r.stderr_tail
            );
        }
        assert!(
            seen_mixed,
            "test sweep did not cover any mixed `=`/`≤` chain — broaden the seed list"
        );
    }
}
