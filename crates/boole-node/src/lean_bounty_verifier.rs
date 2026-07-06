use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use boole_core::{Bounty, BountyProofVerifier, VerifyOutcome};
use boole_lean_runner::{IsolationMode, LeanRunner, LeanRunnerConfig};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// Fixed wrapper the commissioned statement is rendered into — mirrors
/// `family_v1_lenbound::lean_module`'s pattern: the theorem *statement* is
/// fixed (here: the bounty's own commissioned statement, sourced from the
/// bounty record — never from the submitter's envelope) and only the proof
/// term after `:=` is attacker-supplied.
const BOUNTY_NAMESPACE: &str = "BooleBountyVerifyMod";
const BOUNTY_THEOREM: &str = "bounty_instance_thm";

/// Render the commissioned `statement` and a submitted `proof_term` into
/// the fixed Lean module the checker elaborates.
fn render_bounty_lean_module(statement: &str, proof_term: &str) -> String {
    format!(
        "namespace {ns}\n\ntheorem {thm} : {statement} :=\n{proof_term}\n\nend {ns}\n",
        ns = BOUNTY_NAMESPACE,
        thm = BOUNTY_THEOREM,
    )
}

/// Split a submitter-authored Lean source blob on its first top-level
/// `:=` and return only the trimmed proof term that follows. Whatever the
/// submitter wrote before `:=` (their own theorem name/statement) is
/// discarded — TB.2 binds the proof to the bounty's own commissioned
/// statement, not to anything the submitter claims.
fn extract_proof_term(lean_source: &str) -> Result<&str, String> {
    let (_, proof_term) = lean_source
        .split_once(":=")
        .ok_or_else(|| "lean envelope leanSource missing ':=' proof term".to_string())?;
    let trimmed = proof_term.trim();
    if trimmed.is_empty() {
        return Err("lean envelope leanSource has an empty proof term after ':='".to_string());
    }
    Ok(trimmed)
}

fn content_hash_hex(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

/// Adapter that wires the existing `LeanRunner` shell-out into the
/// `BountyProofVerifier` trait. Envelope shape: `{leanSource: "<utf8>"}`.
/// The bounty's `verifier.metadata.verifierHash` is bound into the runner
/// config so checker artifact identity matches the bounty record.
#[derive(Debug, Clone)]
pub struct LeanBountyVerifier {
    checker_dir: PathBuf,
    isolation_mode: IsolationMode,
}

impl LeanBountyVerifier {
    pub fn new(checker_dir: impl Into<PathBuf>) -> Self {
        Self {
            checker_dir: checker_dir.into(),
            isolation_mode: IsolationMode::default(),
        }
    }

    /// N3.2 — operator opt-out plumb (ADR-0008 decision 4). The crate
    /// default is `Enforce` from the same change that opened network
    /// ingress; `run-local --allow-isolation-log-mode` routes here to relax
    /// the checker child back to observe-only `Log` mode.
    pub fn with_isolation_mode(mut self, isolation_mode: IsolationMode) -> Self {
        self.isolation_mode = isolation_mode;
        self
    }

    pub fn isolation_mode(&self) -> IsolationMode {
        self.isolation_mode
    }
}

impl BountyProofVerifier for LeanBountyVerifier {
    fn verify(&self, bounty: &Bounty, envelope: &Value) -> Result<bool, String> {
        self.verify_with_evidence(bounty, envelope)
            .map(|outcome| outcome.accepted)
    }

    fn verify_with_evidence(
        &self,
        bounty: &Bounty,
        envelope: &Value,
    ) -> Result<VerifyOutcome, String> {
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

        // TB.2 — bind the proof to the commissioned `problem_hash`: the
        // statement lives in the bounty's own metadata (set at announce
        // time), never in the submitter's envelope. Verify `problem_hash`
        // really is that statement's content hash BEFORE running
        // `check_file`, so a proof of an unrelated statement can no
        // longer claim an open bounty just because it happens to type-check.
        let statement = bounty
            .verifier
            .metadata
            .get("statement")
            .and_then(Value::as_str)
            .ok_or_else(|| "lean bounty missing verifier.metadata.statement".to_string())?;
        if content_hash_hex(statement) != bounty.problem_hash {
            let mut evidence: Map<String, Value> = Map::new();
            evidence.insert(
                "rejectReason".to_string(),
                Value::String("problem_hash_mismatch".to_string()),
            );
            return Ok(VerifyOutcome {
                accepted: false,
                evidence,
            });
        }

        let proof_term = extract_proof_term(lean_source)?;
        let rendered_module = render_bounty_lean_module(statement, proof_term);

        let tmp_dir = std::env::temp_dir().join(format!(
            "boole-lean-bounty-{}-{}-{}",
            std::process::id(),
            random_suffix(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&tmp_dir).map_err(|err| err.to_string())?;
        let proof_path = tmp_dir.join("Proof.lean");
        std::fs::write(&proof_path, rendered_module).map_err(|err| err.to_string())?;

        let runner = LeanRunner::new(
            LeanRunnerConfig::new(verifier_hash)
                .with_package_dir(self.checker_dir.clone())
                .with_isolation_mode(self.isolation_mode),
        );
        let outcome = match runner.check_file(&proof_path) {
            Ok(result) => {
                // P1.4 — surface the LeanRunner's `checker_artifact_hash`
                // so the bounty audit ledger pins the physical checker
                // identity that adjudicated this proof. `verifierHash` is
                // already covered by slice 19 from the bounty record;
                // here we only add evidence the verifier alone can know.
                let mut evidence: Map<String, Value> = Map::new();
                evidence.insert(
                    "checkerArtifactHash".to_string(),
                    Value::String(result.evidence.checker_artifact_hash.clone()),
                );
                Ok(VerifyOutcome {
                    accepted: result.accepted,
                    evidence,
                })
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verifier_defaults_to_enforced_isolation_and_log_is_an_explicit_opt_out() {
        let verifier = LeanBountyVerifier::new("lean/checker");
        assert_eq!(
            verifier.isolation_mode(),
            IsolationMode::Enforce,
            "ADR-0008 decision 4: enforce-by-default from the N3.2 change"
        );
        let relaxed = verifier.with_isolation_mode(IsolationMode::Log);
        assert_eq!(relaxed.isolation_mode(), IsolationMode::Log);
    }
}
