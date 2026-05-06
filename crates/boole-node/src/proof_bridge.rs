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
    pub fn new(runner: LeanRunner) -> Self {
        Self::new_with_policy(runner, LeanProofBridgePolicy::default())
    }

    pub fn new_with_policy(runner: LeanRunner, policy: LeanProofBridgePolicy) -> Self {
        Self { runner, policy }
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

        let package_bytes = canonical_pofp_package_from_lean_result(&lean);
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

/// Build the canonical POFP-v1 package bytes from a `LeanCheckResult`.
///
/// SECURITY: this package is the input to `canon_hash = sha256(package)`,
/// which the protocol uses (via `share_hash(c, pk, n, j, canon_hash)`)
/// to bind a share to the Lean proof identity. The package format pins
/// two 32-bit slots derived from `stable_u32(result, ...)`, so the
/// *variable* portion of the package across distinct `LeanCheckResult`
/// inputs is at most 64 bits. That puts the effective entropy of
/// `canon_hash` at 64 bits even though sha256 itself outputs 256.
///
/// This is acceptable in the v1 protocol because:
/// 1. `share_hash(c, pk, n, j, canon_hash)` mixes the share identity
///    with `canon_hash`. Two distinct submissions cannot collide on
///    `share_hash` unless `(c, pk, n, j)` also collide.
/// 2. `n` is bound by the **ticket PoW** `ticket(c, pk, n) < T_ticket`
///    (see `boole_core::hash::ticket`). The **submission PoW** is
///    `submission_pow_hash(c, pk, nonceS, canon_hash)` and does NOT
///    bind `n`; only the ticket step does.
/// 3. The share pool dedups by `(pk, n, j)` per chain head `c` (see
///    `boole_core::share_pool::share_key`), NOT by `share_hash`. A
///    canon_hash-only collision therefore cannot replay against the
///    same `(c, pk, n, j)` slot — the second insert is rejected as
///    `Duplicate` regardless of `canon_hash`.
/// 4. Forging an accepted `LeanCheckResult` with a chosen `canon_hash`
///    requires either editing the checker (rejected by
///    `LeanProofBridgePolicy::allow_checker_artifact_hash`, which
///    pins `lean-toolchain`, `lakefile.lean`, `lake-manifest.json`,
///    and the recursive `BooleCheck/**` tree) or breaking the host
///    `lean` binary.
///
/// Widening to a full 256-bit canonical surface is tracked in
/// `docs/adr/0001-pofp-v2-canonical-widening.md`. POFP-v2 changes the
/// wire format and invalidates every previously recorded proof, so it
/// must coincide with a chain reset.
pub fn canonical_pofp_package_from_lean_result(result: &LeanCheckResult) -> Vec<u8> {
    let mut package = Vec::with_capacity(30);
    package.extend_from_slice(b"POFP");
    package.extend_from_slice(&1u32.to_le_bytes());
    package.extend_from_slice(&0u32.to_le_bytes());
    package.extend_from_slice(&0u32.to_le_bytes());
    package.push(0x10);
    package.extend_from_slice(&stable_u32(result, b"type").to_le_bytes());
    package.push(0x10);
    package.extend_from_slice(&stable_u32(result, b"value").to_le_bytes());
    package.extend_from_slice(&0u32.to_le_bytes());
    package
}

fn runner_error_result(error: String) -> LeanCheckResult {
    LeanCheckResult {
        accepted: false,
        exit_code: -1,
        stdout: String::new(),
        stderr: error,
        timed_out: false,
        output_truncated: false,
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
        },
    }
}

fn stable_u32(result: &LeanCheckResult, domain: &[u8]) -> u32 {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(result.evidence.verifier_hash.as_bytes());
    hasher.update(result.evidence.checker_artifact_hash.as_bytes());
    hasher.update(result.evidence.lean_version.as_bytes());
    hasher.update(result.evidence.lake_version.as_bytes());
    hasher.update(result.stdout.as_bytes());
    let digest = hasher.finalize();
    u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]])
}
