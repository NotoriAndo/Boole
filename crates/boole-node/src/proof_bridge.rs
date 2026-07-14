use boole_lean_runner::{LeanCheckResult, LeanRunner};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

pub const LEAN_CANON_TAG: u8 = 0;

#[derive(Debug, Clone)]
pub struct ProofSubmissionTemplate {
    pub c: String,
    pub pk: String,
    pub n: String,
    pub j: String,
    pub nonce_s: String,
}

#[derive(Debug, Clone)]
pub struct BridgedProofSubmission {
    pub body: Map<String, Value>,
    pub lean: LeanCheckResult,
    pub package_bytes: Vec<u8>,
    pub canon_tag: u8,
}

#[derive(Debug, Clone)]
pub struct ProofBridgeError {
    kind: &'static str,
    lean: Box<LeanCheckResult>,
}

impl ProofBridgeError {
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn lean(&self) -> &LeanCheckResult {
        &self.lean
    }
}

#[derive(Debug)]
pub struct LeanProofBridge {
    runner: LeanRunner,
    policy: LeanProofBridgePolicy,
}

#[derive(Debug, Clone, Default)]
pub struct LeanProofBridgePolicy {
    required_verifier_hash: Option<String>,
    allowed_checker_artifact_hashes: BTreeSet<String>,
}

impl LeanProofBridgePolicy {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn require_verifier_hash(mut self, verifier_hash: impl Into<String>) -> Self {
        self.required_verifier_hash = Some(verifier_hash.into());
        self
    }

    pub fn allow_checker_artifact_hash(mut self, checker_artifact_hash: impl Into<String>) -> Self {
        self.allowed_checker_artifact_hashes
            .insert(checker_artifact_hash.into());
        self
    }

    fn validate_required(&self) -> anyhow::Result<()> {
        if self.required_verifier_hash.is_none() {
            anyhow::bail!("Lean proof bridge policy must set a required verifier hash");
        }
        if self.allowed_checker_artifact_hashes.is_empty() {
            anyhow::bail!(
                "Lean proof bridge policy must set a non-empty checker artifact allowlist"
            );
        }
        Ok(())
    }

    fn validate(&self, lean: &LeanCheckResult) -> Option<&'static str> {
        if self
            .required_verifier_hash
            .as_ref()
            .is_some_and(|expected| lean.evidence.verifier_hash != *expected)
        {
            return Some("lean_verifier_hash_mismatch");
        }
        if !self.allowed_checker_artifact_hashes.is_empty()
            && !self
                .allowed_checker_artifact_hashes
                .contains(&lean.evidence.checker_artifact_hash)
        {
            return Some("lean_artifact_not_allowed");
        }
        None
    }
}

impl LeanProofBridge {
    pub fn try_new_with_policy(
        runner: LeanRunner,
        policy: LeanProofBridgePolicy,
    ) -> anyhow::Result<Self> {
        policy.validate_required()?;
        Ok(Self { runner, policy })
    }

    pub fn new_unchecked_for_tests(runner: LeanRunner) -> Self {
        Self {
            runner,
            policy: LeanProofBridgePolicy::default(),
        }
    }

    pub fn new_with_policy(runner: LeanRunner, policy: LeanProofBridgePolicy) -> Self {
        Self::try_new_with_policy(runner, policy)
            .expect("Lean proof bridge policy must be pinned before use")
    }

    pub fn build_submission_body(
        &self,
        proof_path: impl AsRef<Path>,
        template: &ProofSubmissionTemplate,
    ) -> anyhow::Result<BridgedProofSubmission, ProofBridgeError> {
        let lean = self
            .runner
            .check_file(proof_path.as_ref())
            .map_err(|err| ProofBridgeError {
                kind: "lean_runner_failed",
                lean: Box::new(runner_error_result(err.to_string())),
            })?;
        // SC.9a / ADR-0016 (a-3) — an availability failure (containment
        // kill) is not a judgement of the proof: name it distinctly so an
        // operator retries instead of discarding the proof as rejected.
        if lean.verdict.is_retryable_unavailable() {
            return Err(ProofBridgeError {
                kind: "lean_runner_unavailable",
                lean: Box::new(lean),
            });
        }
        if !lean.accepted {
            return Err(ProofBridgeError {
                kind: "lean_rejected",
                lean: Box::new(lean),
            });
        }
        if let Some(kind) = self.policy.validate(&lean) {
            return Err(ProofBridgeError {
                kind,
                lean: Box::new(lean),
            });
        }

        let proof_source = std::fs::read(proof_path.as_ref()).map_err(|err| ProofBridgeError {
            kind: "proof_source_read_failed",
            lean: Box::new(runner_error_result(err.to_string())),
        })?;
        let package_bytes =
            canonical_pofp_package_from_lean_result_and_source(&lean, &proof_source);
        let mut body = Map::new();
        body.insert("c".to_string(), Value::String(template.c.clone()));
        body.insert("pk".to_string(), Value::String(template.pk.clone()));
        body.insert("n".to_string(), Value::String(template.n.clone()));
        body.insert("j".to_string(), Value::String(template.j.clone()));
        body.insert(
            "nonceS".to_string(),
            Value::String(template.nonce_s.clone()),
        );
        body.insert(
            "bytes".to_string(),
            Value::String(hex::encode(&package_bytes)),
        );

        Ok(BridgedProofSubmission {
            body,
            lean,
            package_bytes,
            canon_tag: LEAN_CANON_TAG,
        })
    }
}

/// Build the canonical POFP-v2 package bytes from a `LeanCheckResult`.
///
/// SECURITY: this package is the input to `canon_hash = sha256(package)`,
/// which the protocol uses (via `share_hash(c, pk, n, j, canon_hash)`)
/// to bind a share to the Lean proof identity. POFP-v2 encodes two
/// domain-separated 256-bit opaque digest expression slots derived from
/// the Lean runner evidence and the (normalized) proof source, widening the
/// variable canonical surface from POFP-v1's two 32-bit slots to two full
/// SHA-256 slots.
///
/// Each digest binds only data that is reproducible offline from a
/// persisted proof — `verifier_hash`, `checker_artifact_hash`, and the
/// whitespace-normalized `proof_source` — and deliberately NOT toolchain or
/// runtime values (`lean_version`, `lake_version`, `stdout`), so a
/// semantically inert edit (e.g. a blank line) or a toolchain patch bump
/// does not mint a fresh `canon_hash` for the same proof. This mirrors the
/// sibling `boole_core::lean_bound_canon_package`, which documents excluding
/// those same toolchain/runtime values as a hazard.
///
/// POFP-v2 changes the wire format and invalidates every previously
/// recorded POFP-v1 proof package, so deployment must coincide with a
/// chain reset.
pub fn canonical_pofp_package_from_lean_result(result: &LeanCheckResult) -> Vec<u8> {
    canonical_pofp_package_from_lean_result_and_source(result, &[])
}

pub fn canonical_pofp_package_from_lean_result_and_source(
    result: &LeanCheckResult,
    proof_source: &[u8],
) -> Vec<u8> {
    let mut package = Vec::with_capacity(86);
    package.extend_from_slice(b"POFP");
    package.extend_from_slice(&2u32.to_le_bytes());
    package.extend_from_slice(&0u32.to_le_bytes());
    package.extend_from_slice(&0u32.to_le_bytes());
    package.push(0x19);
    package.extend_from_slice(&stable_digest(result, b"pofp-v2:type", proof_source));
    package.push(0x19);
    package.extend_from_slice(&stable_digest(result, b"pofp-v2:value", proof_source));
    package.extend_from_slice(&0u32.to_le_bytes());
    package
}

fn runner_error_result(error: String) -> LeanCheckResult {
    LeanCheckResult {
        accepted: false,
        exit_code: -1,
        stdout: String::new(),
        stderr: error.clone(),
        timed_out: false,
        output_truncated: false,
        // A runner-level error (spawn failure, missing package) is an
        // availability condition, not a judgement of the proof bytes
        // (ADR-0016 (a-3)).
        verdict: boole_lean_runner::LeanVerdict::RetryableUnavailable { reason: error },
        evidence: boole_lean_runner::LeanRunnerEvidence {
            verifier_hash: String::new(),
            checker: "lake exec boole_check".to_string(),
            checker_exe: String::new(),
            checker_artifact_hash: String::new(),
            package_dir: String::new(),
            lean_version: String::new(),
            lake_version: String::new(),
            timeout_ms: 0,
            memory_limit_mb: 0,
            output_limit_bytes: 0,
            max_heartbeats: 0,
            max_rec_depth: 0,
        },
    }
}

/// Whitespace-canonicalize a proof source before it enters the digest: drop
/// blank (whitespace-only) lines and trailing-whitespace on each remaining
/// line. This is the minimum bar so a semantically inert blank-line edit
/// yields the same normalized bytes, and therefore the same `canon_hash`.
fn normalize_proof_source(proof_source: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(proof_source);
    text.lines()
        .map(|line| line.trim_end())
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .into_bytes()
}

fn stable_digest(result: &LeanCheckResult, domain: &[u8], proof_source: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(result.evidence.verifier_hash.as_bytes());
    hasher.update(result.evidence.checker_artifact_hash.as_bytes());
    hasher.update(b"\0proof-source\0");
    hasher.update(normalize_proof_source(proof_source));
    hasher.finalize().into()
}
