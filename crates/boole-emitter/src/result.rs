//! The emitter result envelope and its XOR invariant
//! (EMITTER-CONTRACT-v1_2-final §1 / §10).
//!
//! One census anchor converts to exactly one [`EmitterResultV1`]: either a
//! non-empty `tasks[]` **or** a single deterministic [`ZeroReason`], never both
//! and never neither ([`EmitterResultV1::validate_xor`]). Stage A classification
//! ([`StageA`]) is a separate census-side judgement carried alongside the result,
//! not a `zero_reason` code.

use serde::Serialize;

use crate::digest::Digest;

/// The `code` set of `zero_reason` (§10). Stage A states are NOT members.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ZeroReasonCode {
    #[serde(rename = "NO-RUNNABLE-TASK")]
    NoRunnableTask,
    #[serde(rename = "ORACLE-ABSENT")]
    OracleAbsent,
    /// The anchor references a written spec, but no runnable task contract has
    /// been materialized from it. Carries `missing` (§10).
    #[serde(rename = "TASK-CONTRACT-UNMATERIALIZED")]
    TaskContractUnmaterialized,
    #[serde(rename = "EXECUTION-CASE-UNLINKED")]
    ExecutionCaseUnlinked,
    #[serde(rename = "ANCHOR-RETIRED")]
    AnchorRetired,
}

/// The absent parts of a runnable task contract (§10, `TASK-CONTRACT-UNMATERIALIZED`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum MissingPart {
    #[serde(rename = "statement")]
    Statement,
    #[serde(rename = "oracle")]
    Oracle,
    #[serde(rename = "work_product_contract")]
    WorkProductContract,
}

/// Stage A classification (§10). Never serialized as a `zero_reason` code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum StageA {
    #[serde(rename = "RUNNABLE-TASK-EMITTER-PRESENT")]
    RunnableTaskEmitterPresent,
    #[serde(rename = "CANDIDATE-ENUMERATOR-ONLY")]
    CandidateEnumeratorOnly,
    /// The anchor's only blocker is an unmaterialized runnable task contract.
    /// MUST NOT be promoted to `RunnableTaskEmitterPresent`.
    #[serde(rename = "TASK-CONTRACT-MISSING")]
    TaskContractMissing,
    #[serde(rename = "EMITTER-MISSING")]
    EmitterMissing,
}

/// The deterministic zero result carried when an anchor yields no tasks (§10).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ZeroReason {
    pub code: ZeroReasonCode,
    /// SHA-256 over the canonical evidence proving the zero result.
    pub evidence_digest: Digest,
    /// Human-scoped description of what the evidence covers.
    pub evidence_scope: String,
    /// The absent contract parts. Present iff `code == TaskContractUnmaterialized`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing: Option<Vec<MissingPart>>,
}

/// Which anchor this result was produced from (§1 `anchor_ref`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AnchorRef {
    pub domain: String,
    pub identity_kind: String,
    pub identity_value: String,
}

/// The anchor→source binding (§1 `anchor_binding`; digest per §3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AnchorBinding {
    pub anchor_binding_digest: Digest,
}

/// A per-task record (§11). Only the two identity-bearing digests are modeled in
/// this slice — the Rust representative anchor emits none, so the full record is
/// deferred to a task-producing converter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EmitterOutputV1 {
    pub emitter_task_key: Digest,
    pub problem_digest: Digest,
}

/// A violation of the §1 XOR invariant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XorViolation {
    /// Both `tasks` non-empty and `zero_reason` present.
    Both,
    /// Neither `tasks` nor `zero_reason` present.
    Neither,
}

/// The emitter result envelope (§1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EmitterResultV1 {
    pub schema: &'static str,
    pub anchor_ref: AnchorRef,
    pub anchor_binding: AnchorBinding,
    pub tasks: Vec<EmitterOutputV1>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zero_reason: Option<ZeroReason>,
}

/// The canonical `schema` string of an [`EmitterResultV1`].
pub const EMITTER_RESULT_SCHEMA: &str = "boole.emitter-result.v1";

impl EmitterResultV1 {
    /// Enforce the §1 XOR invariant: exactly one of `tasks` non-empty **or**
    /// `zero_reason` present.
    pub fn validate_xor(&self) -> Result<(), XorViolation> {
        let has_tasks = !self.tasks.is_empty();
        let has_zero = self.zero_reason.is_some();
        match (has_tasks, has_zero) {
            (true, true) => Err(XorViolation::Both),
            (false, false) => Err(XorViolation::Neither),
            _ => Ok(()),
        }
    }
}
