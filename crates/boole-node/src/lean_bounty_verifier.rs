use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use boole_core::{Bounty, BountyProofVerifier};
use boole_lean_runner::{LeanRunner, LeanRunnerConfig};
use serde_json::Value;

/// Adapter that wires the existing `LeanRunner` shell-out into the
/// `BountyProofVerifier` trait. Envelope shape: `{leanSource: "<utf8>"}`.
/// The bounty's `verifier.metadata.verifierHash` is bound into the runner
/// config so checker artifact identity matches the bounty record.
#[derive(Debug, Clone)]
pub struct LeanBountyVerifier {
    checker_dir: PathBuf,
}

impl LeanBountyVerifier {
    pub fn new(checker_dir: impl Into<PathBuf>) -> Self {
        Self {
            checker_dir: checker_dir.into(),
        }
    }
}

impl BountyProofVerifier for LeanBountyVerifier {
    fn verify(&self, bounty: &Bounty, envelope: &Value) -> Result<bool, String> {
        let lean_source = envelope
            .get("leanSource")
            .and_then(Value::as_str)
            .ok_or_else(|| "lean envelope missing string leanSource".to_string())?;

        let verifier_hash = bounty
            .verifier
            .metadata
            .get("verifierHash")
            .and_then(Value::as_str)
            .ok_or_else(|| "lean bounty missing verifier.metadata.verifierHash".to_string())?;

        let tmp_dir = std::env::temp_dir().join(format!(
            "boole-lean-bounty-{}-{}-{}",
            std::process::id(),
            random_suffix(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&tmp_dir).map_err(|err| err.to_string())?;
        let proof_path = tmp_dir.join("Proof.lean");
        std::fs::write(&proof_path, lean_source).map_err(|err| err.to_string())?;

        let runner = LeanRunner::new(
            LeanRunnerConfig::new(verifier_hash).with_package_dir(self.checker_dir.clone()),
        );
        let outcome = match runner.check_file(&proof_path) {
            Ok(result) => Ok(result.accepted),
            Err(err) => Err(err.to_string()),
        };
        let _ = std::fs::remove_dir_all(&tmp_dir);
        outcome
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn random_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}
