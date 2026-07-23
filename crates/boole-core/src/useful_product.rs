//! BF.3 — bound useful-product packet and deterministic receipt (C2+B4).
//!
//! `UsefulProductManifest` binds source tree, build recipe/toolchain pin,
//! implementation, release, runtime vectors (ok + reject), the Lean
//! statement/proof and the checker/verifier/canonicalizer pins into one
//! verdict-bearing `artifact_root`. The C2 anti-self-reference order:
//!
//! 1. `artifact_root` hashes the product's canonical bytes — identity
//!    fields (`submission_id`, `commitment`) are structurally excluded,
//!    and `reward_pk` is deliberately OUTSIDE the root so identical
//!    product bytes share one identity (copy detection) while the payee
//!    is bound at step 2.
//! 2. `submission_id = H(task_id ‖ artifact_root ‖ implementation_digest
//!    ‖ release_digest ‖ reward_pk)` (see `useful_work`).
//! 3. `commitment` (BF.2) hashes over `submission_id`.
//!
//! The audit normalizes its result into a small `VerificationReceipt`
//! that recomputes byte-identically from the same inputs. A wall-clock
//! timeout is an availability outcome — never a reject verdict (B4,
//! "step budget = verdict, wall-clock = containment"). No real
//! Circom/Rust/EVM runners live here (BF.3 non-goal); this is the
//! common contract only, deliberately separate from `FamilyManifest`
//! and `BountyProofVerifier` (kept untouched). Nothing links these
//! types to the v3 block schema yet (BF.3 rollback contract).

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::hash::{h_protocol, Hex32};
use crate::useful_work::{push_field, SubmissionIdentity};

const ARTIFACT_ROOT_DOMAIN: &[u8] = b"boole.useful-work.artifact-root.v0";
const RECEIPT_DOMAIN: &[u8] = b"boole.useful-work.receipt.v0";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UsefulProductError {
    #[error("useful-product json malformed: {0}")]
    MalformedJson(String),
    #[error("identity fields must not appear inside the packet (C2)")]
    SelfReferentialField,
    #[error("invalid digest in field {field}")]
    InvalidDigest { field: &'static str },
    #[error("empty field {field}")]
    EmptyField { field: &'static str },
}

impl UsefulProductError {
    /// Stable machine-readable label — part of the fixture contract.
    pub fn label(&self) -> &'static str {
        match self {
            UsefulProductError::MalformedJson(_) => "malformed-json",
            UsefulProductError::SelfReferentialField => "self-referential-field",
            UsefulProductError::InvalidDigest { .. } => "invalid-digest",
            UsefulProductError::EmptyField { .. } => "empty-field",
        }
    }
}

/// The packet manifest. Every field except `reward_pk` is verdict-
/// bearing and committed by [`UsefulProductManifest::artifact_root`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsefulProductManifest {
    pub source_tree_root: Hex32,
    pub build_recipe_digest: Hex32,
    pub toolchain_pin: String,
    pub implementation_digest: Hex32,
    pub release_digest: Hex32,
    pub runtime_ok_vector_root: Hex32,
    pub runtime_reject_vector_root: Hex32,
    pub statement_hash: Hex32,
    pub lean_proof_digest: Hex32,
    pub checker_hash: Hex32,
    pub verifier_hash: Hex32,
    pub canonicalizer_hash: Hex32,
    pub reward_pk: Hex32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct UsefulProductManifestRaw {
    source_tree_root: String,
    build_recipe_digest: String,
    toolchain_pin: String,
    implementation_digest: String,
    release_digest: String,
    runtime_ok_vector_root: String,
    runtime_reject_vector_root: String,
    statement_hash: String,
    lean_proof_digest: String,
    checker_hash: String,
    verifier_hash: String,
    canonicalizer_hash: String,
    reward_pk: String,
}

fn digest(value: &str, field: &'static str) -> Result<Hex32, UsefulProductError> {
    Hex32::from_hex(value).map_err(|_| UsefulProductError::InvalidDigest { field })
}

impl UsefulProductManifest {
    pub fn from_json_value(value: &serde_json::Value) -> Result<Self, UsefulProductError> {
        // C2 pre-check with a precise label: a packet that tries to carry
        // its own identity is circular by construction, not merely
        // malformed.
        if let Some(object) = value.as_object() {
            if object.contains_key("submissionId") || object.contains_key("commitment") {
                return Err(UsefulProductError::SelfReferentialField);
            }
        }
        let raw: UsefulProductManifestRaw = serde_json::from_value(value.clone())
            .map_err(|err| UsefulProductError::MalformedJson(err.to_string()))?;
        if raw.toolchain_pin.is_empty() {
            return Err(UsefulProductError::EmptyField {
                field: "toolchainPin",
            });
        }
        Ok(Self {
            source_tree_root: digest(&raw.source_tree_root, "sourceTreeRoot")?,
            build_recipe_digest: digest(&raw.build_recipe_digest, "buildRecipeDigest")?,
            toolchain_pin: raw.toolchain_pin,
            implementation_digest: digest(&raw.implementation_digest, "implementationDigest")?,
            release_digest: digest(&raw.release_digest, "releaseDigest")?,
            runtime_ok_vector_root: digest(&raw.runtime_ok_vector_root, "runtimeOkVectorRoot")?,
            runtime_reject_vector_root: digest(
                &raw.runtime_reject_vector_root,
                "runtimeRejectVectorRoot",
            )?,
            statement_hash: digest(&raw.statement_hash, "statementHash")?,
            lean_proof_digest: digest(&raw.lean_proof_digest, "leanProofDigest")?,
            checker_hash: digest(&raw.checker_hash, "checkerHash")?,
            verifier_hash: digest(&raw.verifier_hash, "verifierHash")?,
            canonicalizer_hash: digest(&raw.canonicalizer_hash, "canonicalizerHash")?,
            reward_pk: digest(&raw.reward_pk, "rewardPk")?,
        })
    }

    pub fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(UsefulProductManifestRaw {
            source_tree_root: self.source_tree_root.to_hex(),
            build_recipe_digest: self.build_recipe_digest.to_hex(),
            toolchain_pin: self.toolchain_pin.clone(),
            implementation_digest: self.implementation_digest.to_hex(),
            release_digest: self.release_digest.to_hex(),
            runtime_ok_vector_root: self.runtime_ok_vector_root.to_hex(),
            runtime_reject_vector_root: self.runtime_reject_vector_root.to_hex(),
            statement_hash: self.statement_hash.to_hex(),
            lean_proof_digest: self.lean_proof_digest.to_hex(),
            checker_hash: self.checker_hash.to_hex(),
            verifier_hash: self.verifier_hash.to_hex(),
            canonicalizer_hash: self.canonicalizer_hash.to_hex(),
            reward_pk: self.reward_pk.to_hex(),
        })
        .expect("manifest serializes")
    }

    /// C2 step 1 — the verdict-bearing product identity. Length-prefixed
    /// canonical bytes in a fixed field order (JSON key order is
    /// irrelevant; moving bytes between fields changes the root).
    /// `reward_pk` and identity fields are excluded by construction.
    pub fn artifact_root(&self) -> Hex32 {
        let mut bytes = Vec::new();
        push_field(&mut bytes, self.source_tree_root.as_bytes());
        push_field(&mut bytes, self.build_recipe_digest.as_bytes());
        push_field(&mut bytes, self.toolchain_pin.as_bytes());
        push_field(&mut bytes, self.implementation_digest.as_bytes());
        push_field(&mut bytes, self.release_digest.as_bytes());
        push_field(&mut bytes, self.runtime_ok_vector_root.as_bytes());
        push_field(&mut bytes, self.runtime_reject_vector_root.as_bytes());
        push_field(&mut bytes, self.statement_hash.as_bytes());
        push_field(&mut bytes, self.lean_proof_digest.as_bytes());
        push_field(&mut bytes, self.checker_hash.as_bytes());
        push_field(&mut bytes, self.verifier_hash.as_bytes());
        push_field(&mut bytes, self.canonicalizer_hash.as_bytes());
        h_protocol(ARTIFACT_ROOT_DOMAIN, &[&bytes])
    }

    /// C2 step 2 — the miner's result identity for this packet.
    pub fn submission_identity(&self, task_id: &Hex32) -> SubmissionIdentity {
        SubmissionIdentity {
            task_id: *task_id,
            source_root: self.source_tree_root,
            implementation_digest: self.implementation_digest,
            release_digest: self.release_digest,
            artifact_root: self.artifact_root(),
            reward_pk: self.reward_pk,
        }
    }
}

/// What the node actually observed while re-deriving the packet: rebuild
/// digests, runtime vector results, the protocol-owned exact theorem
/// hash, proof hygiene flags and the pinned tool identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedVerification {
    pub rebuilt_source_tree_root: Hex32,
    pub rebuilt_release_digest: Hex32,
    pub runtime_ok_vectors_passed: bool,
    pub runtime_reject_vectors_passed: bool,
    pub protocol_statement_hash: Hex32,
    pub proof_has_sorry: bool,
    pub proof_new_axioms: bool,
    pub checker_hash: Hex32,
    pub verifier_hash: Hex32,
    pub canonicalizer_hash: Hex32,
    /// B4 — wall-clock overrun is containment, not a verdict input.
    pub wall_clock_timeout: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptRejectReason {
    RebuildSourceMismatch,
    RebuildReleaseMismatch,
    RuntimeOkVectorsFailed,
    RuntimeRejectVectorsFailed,
    StatementMismatch,
    ProofHasSorry,
    ProofNewAxiom,
    CheckerPinMismatch,
    VerifierPinMismatch,
    CanonicalizerPinMismatch,
}

impl ReceiptRejectReason {
    pub fn label(&self) -> &'static str {
        match self {
            ReceiptRejectReason::RebuildSourceMismatch => "rebuild-source-mismatch",
            ReceiptRejectReason::RebuildReleaseMismatch => "rebuild-release-mismatch",
            ReceiptRejectReason::RuntimeOkVectorsFailed => "runtime-ok-vectors-failed",
            ReceiptRejectReason::RuntimeRejectVectorsFailed => "runtime-reject-vectors-failed",
            ReceiptRejectReason::StatementMismatch => "statement-mismatch",
            ReceiptRejectReason::ProofHasSorry => "proof-has-sorry",
            ReceiptRejectReason::ProofNewAxiom => "proof-new-axiom",
            ReceiptRejectReason::CheckerPinMismatch => "checker-pin-mismatch",
            ReceiptRejectReason::VerifierPinMismatch => "verifier-pin-mismatch",
            ReceiptRejectReason::CanonicalizerPinMismatch => "canonicalizer-pin-mismatch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptVerdict {
    Accepted,
    Rejected(ReceiptRejectReason),
}

/// The normalized verification result — small enough to commit, rich
/// enough to audit. Recomputing from the same inputs yields the same
/// canonical bytes and digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationReceipt {
    pub task_id: Hex32,
    pub submission_id: Hex32,
    pub artifact_root: Hex32,
    pub checker_hash: Hex32,
    pub verdict: ReceiptVerdict,
}

impl VerificationReceipt {
    pub fn accepted(&self) -> bool {
        self.verdict == ReceiptVerdict::Accepted
    }

    pub fn reject_label(&self) -> Option<&'static str> {
        match self.verdict {
            ReceiptVerdict::Accepted => None,
            ReceiptVerdict::Rejected(reason) => Some(reason.label()),
        }
    }

    /// Deterministic serialization: fixed field order, length-prefixed.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        push_field(&mut bytes, self.task_id.as_bytes());
        push_field(&mut bytes, self.submission_id.as_bytes());
        push_field(&mut bytes, self.artifact_root.as_bytes());
        push_field(&mut bytes, self.checker_hash.as_bytes());
        match self.verdict {
            ReceiptVerdict::Accepted => push_field(&mut bytes, b"accepted"),
            ReceiptVerdict::Rejected(reason) => {
                push_field(&mut bytes, b"rejected");
                push_field(&mut bytes, reason.label().as_bytes());
            }
        }
        bytes
    }

    pub fn receipt_digest(&self) -> Hex32 {
        h_protocol(RECEIPT_DOMAIN, &[&self.canonical_bytes()])
    }
}

/// B4 — a verdict is deterministic; unavailability is not a verdict. A
/// wall-clock timeout therefore yields `RetryableUnavailable` with NO
/// receipt: the packet may be retried, never punished for slowness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationOutcome {
    Verdict(VerificationReceipt),
    RetryableUnavailable,
}

/// The common audit contract: compare what the packet declares with what
/// the node observed re-deriving it. Checks run in a fixed order so every
/// node reports the same first failure.
pub fn audit_packet(
    manifest: &UsefulProductManifest,
    observed: &ObservedVerification,
    task_id: &Hex32,
) -> VerificationOutcome {
    if observed.wall_clock_timeout {
        return VerificationOutcome::RetryableUnavailable;
    }
    use ReceiptRejectReason::*;
    let reject = if observed.rebuilt_source_tree_root != manifest.source_tree_root {
        Some(RebuildSourceMismatch)
    } else if observed.rebuilt_release_digest != manifest.release_digest {
        Some(RebuildReleaseMismatch)
    } else if !observed.runtime_ok_vectors_passed {
        Some(RuntimeOkVectorsFailed)
    } else if !observed.runtime_reject_vectors_passed {
        Some(RuntimeRejectVectorsFailed)
    } else if observed.protocol_statement_hash != manifest.statement_hash {
        Some(StatementMismatch)
    } else if observed.proof_has_sorry {
        Some(ProofHasSorry)
    } else if observed.proof_new_axioms {
        Some(ProofNewAxiom)
    } else if observed.checker_hash != manifest.checker_hash {
        Some(CheckerPinMismatch)
    } else if observed.verifier_hash != manifest.verifier_hash {
        Some(VerifierPinMismatch)
    } else if observed.canonicalizer_hash != manifest.canonicalizer_hash {
        Some(CanonicalizerPinMismatch)
    } else {
        None
    };
    let verdict = match reject {
        Some(reason) => ReceiptVerdict::Rejected(reason),
        None => ReceiptVerdict::Accepted,
    };
    VerificationOutcome::Verdict(VerificationReceipt {
        task_id: *task_id,
        submission_id: manifest.submission_identity(task_id).submission_id(),
        artifact_root: manifest.artifact_root(),
        checker_hash: manifest.checker_hash,
        verdict,
    })
}
