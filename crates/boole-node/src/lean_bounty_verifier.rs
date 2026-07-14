use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use boole_core::{Bounty, BountyProofVerifier, VerifyOutcome};
use boole_lean_runner::{IsolationMode, LeanCheckResult, LeanRunner, LeanRunnerConfig};
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
    timeout_ms: Option<u64>,
}

impl LeanBountyVerifier {
    pub fn new(checker_dir: impl Into<PathBuf>) -> Self {
        Self {
            checker_dir: checker_dir.into(),
            isolation_mode: IsolationMode::default(),
            timeout_ms: None,
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

    /// SC.9a — containment (wall-clock) tuning only, never a verdict input
    /// (ADR-0016 (a)): expiring this bound surfaces as a retryable
    /// availability error, not as a proof rejection.
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
}

/// SC.9a / ADR-0016 (a-3) — map the runner's three-state verdict into the
/// `BountyProofVerifier` contract: a `retryable_unavailable` result is an
/// availability ERROR (`Err`), so the caller's error path handles it and no
/// bounty rejection is ever recorded from a containment kill; only real
/// verdicts become a `VerifyOutcome`.
fn outcome_from_check_result(result: &LeanCheckResult) -> Result<VerifyOutcome, String> {
    if let boole_lean_runner::LeanVerdict::RetryableUnavailable { reason } = &result.verdict {
        return Err(format!(
            "retryable_unavailable: verifier availability failure ({reason}), not a verdict"
        ));
    }
    // P1.4 — surface the LeanRunner's `checker_artifact_hash` so the bounty
    // audit ledger pins the physical checker identity that adjudicated this
    // proof. `verifierHash` is already covered by slice 19 from the bounty
    // record; here we only add evidence the verifier alone can know.
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
        let artifact = self.effective_artifact(bounty, envelope)?;
        self.verify_artifact_with_evidence(bounty, envelope, &artifact)
    }

    /// SC.2-f1 — the checker elaborates EXACTLY the bytes the route
    /// hashed as the proof identity: `artifact` is passed through to the
    /// temp `Proof.lean` verbatim, so identity and judgement cannot
    /// diverge even if the derivation ever changes.
    fn verify_artifact_with_evidence(
        &self,
        bounty: &Bounty,
        envelope: &Value,
        artifact: &[u8],
    ) -> Result<VerifyOutcome, String> {
        // Early presence check keeps the pre-SC.2-f1 error precedence
        // (missing leanSource surfaces before bounty-metadata errors);
        // the judged bytes themselves are the `artifact`.
        let _ = envelope
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

        // SC.2-f1 — the bytes handed to the checker ARE the artifact the
        // route hashed as the proof identity (passed in verbatim).
        let rendered_module = String::from_utf8(artifact.to_vec())
            .map_err(|_| "lean effective artifact must be UTF-8".to_string())?;

        let tmp_dir = std::env::temp_dir().join(format!(
            "boole-lean-bounty-{}-{}-{}",
            std::process::id(),
            random_suffix(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&tmp_dir).map_err(|err| err.to_string())?;
        let proof_path = tmp_dir.join("Proof.lean");
        std::fs::write(&proof_path, rendered_module).map_err(|err| err.to_string())?;

        let mut config = LeanRunnerConfig::new(verifier_hash)
            .with_package_dir(self.checker_dir.clone())
            .with_isolation_mode(self.isolation_mode);
        if let Some(timeout_ms) = self.timeout_ms {
            config = config.with_timeout_ms(timeout_ms);
        }
        let runner = LeanRunner::new(config);
        let outcome = match runner.check_file(&proof_path) {
            Ok(result) => outcome_from_check_result(&result),
            Err(err) => Err(err.to_string()),
        };
        let _ = std::fs::remove_dir_all(&tmp_dir);
        outcome
    }

    /// SC.2-f1 — the artifact is the rendered module the checker
    /// elaborates: pinned statement (from the bounty record) + the
    /// submitter's proof term after `:=`. The submitter's own theorem
    /// name/statement prefix and any extra envelope fields (salt, ...)
    /// are discarded by construction, so they cannot mint distinct
    /// proof identities for one and the same Lean run.
    fn effective_artifact(&self, bounty: &Bounty, envelope: &Value) -> Result<Vec<u8>, String> {
        let lean_source = envelope
            .get("leanSource")
            .and_then(Value::as_str)
            .ok_or_else(|| "lean envelope missing string leanSource".to_string())?;
        let statement = bounty
            .verifier
            .metadata
            .get("statement")
            .and_then(Value::as_str)
            .ok_or_else(|| "lean bounty missing verifier.metadata.statement".to_string())?;
        let proof_term = extract_proof_term(lean_source)?;
        Ok(render_bounty_lean_module(statement, proof_term).into_bytes())
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
    use boole_core::{bounty_proof_hash_hex, BountyVerifier};
    use serde_json::json;

    fn statement_bounty(statement: &str) -> Bounty {
        let mut metadata = Map::new();
        metadata.insert(
            "statement".to_string(),
            Value::String(statement.to_string()),
        );
        metadata.insert("verifierHash".to_string(), Value::String("22".repeat(32)));
        Bounty {
            id: "b1".to_string(),
            domain: "test.lean".to_string(),
            problem_hash: content_hash_hex(statement),
            verifier: BountyVerifier {
                kind: "lean-checker".to_string(),
                metadata,
            },
            reward: "1".to_string(),
            deadline: 0,
            status: "open".to_string(),
            created_at: 0,
            updated_at: 0,
        }
    }

    // SC.2-f1 — the proof identity commits the verifier-effective
    // artifact (the rendered module the checker elaborates), so
    // submitter fields the verifier discards (`salt`, the theorem
    // name/statement before `:=`) cannot mint distinct proofHashes for
    // one and the same Lean run.
    #[test]
    fn proof_hash_commits_verifier_effective_artifact() {
        let bounty = statement_bounty("1 + 1 = 2");
        let verifier = LeanBountyVerifier::new("lean/checker");

        let a = verifier
            .effective_artifact(
                &bounty,
                &json!({"leanSource": "theorem mine : anything := by decide", "salt": "1"}),
            )
            .expect("artifact a");
        let b = verifier
            .effective_artifact(
                &bounty,
                &json!({"leanSource": "theorem other_name : ignored :=   by decide"}),
            )
            .expect("artifact b");

        assert_eq!(
            a, b,
            "verifier-discarded prefix/salt must not change the artifact"
        );
        assert_eq!(bounty_proof_hash_hex(&a), bounty_proof_hash_hex(&b));
        // Domain separation: the proof identity is not the bare SHA-256
        // of the artifact bytes (nor any envelope-hash surface).
        assert_ne!(
            bounty_proof_hash_hex(&a),
            hex::encode(Sha256::digest(&a)),
            "bounty proof hash must be domain-tagged"
        );
    }

    fn check_result_with_verdict(
        accepted: bool,
        timed_out: bool,
        verdict: boole_lean_runner::LeanVerdict,
    ) -> boole_lean_runner::LeanCheckResult {
        boole_lean_runner::LeanCheckResult {
            accepted,
            exit_code: if accepted { 0 } else { -1 },
            stdout: String::new(),
            stderr: String::new(),
            timed_out,
            output_truncated: false,
            verdict,
            evidence: boole_lean_runner::LeanRunnerEvidence {
                verifier_hash: "vh".to_string(),
                checker: "lake exec boole_check".to_string(),
                checker_exe: "boole_check".to_string(),
                checker_artifact_hash: "ah".to_string(),
                package_dir: String::new(),
                lean_version: String::new(),
                lake_version: String::new(),
                timeout_ms: 10_000,
                memory_limit_mb: 8192,
                output_limit_bytes: 64 * 1024,
                max_heartbeats: 400_000,
                max_rec_depth: 512,
            },
        }
    }

    /// SC.9a / ADR-0016 (a-3) — a containment kill maps to an availability
    /// `Err`, never to a `VerifyOutcome{accepted:false}`: the bounty route's
    /// error path returns without touching the registry or the event
    /// ledger, so a slow verifier can never mint a consensus-visible
    /// rejection for a proof a faster node would have judged.
    #[test]
    fn containment_kill_maps_to_retryable_error_not_a_reject_outcome() {
        let containment = check_result_with_verdict(
            false,
            true,
            boole_lean_runner::LeanVerdict::RetryableUnavailable {
                reason: "containment_wall_clock_kill".to_string(),
            },
        );
        let err = outcome_from_check_result(&containment)
            .expect_err("containment kill must be an availability error, not an outcome");
        assert!(
            err.contains("retryable_unavailable"),
            "availability error must be typed retryable: {err}"
        );

        let deterministic = check_result_with_verdict(
            false,
            false,
            boole_lean_runner::LeanVerdict::DeterministicReject {
                reason: "budget_exceeded".to_string(),
            },
        );
        let outcome = outcome_from_check_result(&deterministic)
            .expect("deterministic reject IS a verdict outcome");
        assert!(
            !outcome.accepted,
            "budget_exceeded is a real reject verdict"
        );

        let accepted =
            check_result_with_verdict(true, false, boole_lean_runner::LeanVerdict::Accepted);
        let outcome = outcome_from_check_result(&accepted).expect("accept outcome");
        assert!(outcome.accepted);
        assert_eq!(
            outcome.evidence.get("checkerArtifactHash"),
            Some(&Value::String("ah".to_string()))
        );
    }

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
