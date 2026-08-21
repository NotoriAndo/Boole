//! A-ROOTED-NATIVE-MINING-E2E-V1 (msg 4206 section 4) -- closed-local,
//! non-consensus pipeline wiring for exactly one real end-to-end
//! native-mining pass over the frozen `TUPLE-STRUCT-PROJECT` family
//! (local-docs/a-rooted-native-mining-e2e-v1-2026-08-21/FREEZE.json).
//!
//! This module adds no new family and no new checker judgment logic. The
//! actual accept/reject verdict on a candidate's Rust code always comes from
//! the already-sealed, byte-for-byte-reused native checker (the pinned
//! toolchain compiling and running the candidate under `gates.Checker`),
//! supplied here through the [`NativeChecker`] trait so this module never
//! re-implements that judgment. What this module adds is the wiring: proof
//! intake (family-specific extraction of the candidate module text -- a new
//! extraction rule because this family's wire format ("one ACTION line, then
//! one fenced Rust block") differs from Lean's, but a rule that never
//! touches the consensus-path `crate::proof_intake::ProofIntakeV1`), a
//! binding layer that plays the "node verifier" role by independently
//! re-checking the candidate is actually bound to the committed
//! template/challenge/checker/policy rather than trusting a self-reported
//! verdict, a canonical receipt binding all eight fields the operator
//! required, and a local, non-consensus share ledger. No block, reward, or
//! consensus state (`boole_core::block`, `SharePool`, any persisted-block or
//! reward store) is read or written anywhere in this module.

use sha2::{Digest, Sha256};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

/// The frozen identity and digests a candidate must be bound to. Built once
/// from the wave's `FREEZE.json`; never mutated during a pipeline run.
#[derive(Debug, Clone)]
pub struct NativeTaskContext {
    pub family_version: String,
    pub template_id: String,
    pub anchor_sha256: String,
    pub challenge_sha256: String,
    pub epoch: u64,
    pub checker_digest: String,
    pub policy_digest: String,
}

/// What a driver turn produced, plus what task it claims to answer. The
/// `claimed_*` fields are exactly what a confused or malicious submission
/// could misstate; [`run_native_mining_e2e`] never trusts them without
/// checking each against [`NativeTaskContext`].
#[derive(Debug, Clone)]
pub struct NativeCandidate {
    pub raw_model_response: String,
    pub claimed_template_id: String,
    pub claimed_challenge_sha256: String,
    pub claimed_checker_digest: String,
    pub claimed_policy_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeIntakeReject {
    EmptyResponse,
    NoFencedBlock,
}

/// Family-specific proof intake. This family's task contract asks for "one
/// ACTION line and one fenced Rust block containing the complete module"
/// (STEP1-FREEZE.json `prompt_template`), not Lean's fenced-proof contract,
/// so this is a new, small, family-specific extraction -- never a
/// modification of `crate::proof_intake::ProofIntakeV1`, which this module
/// does not import and does not alter.
pub struct NativeProofIntake;

impl NativeProofIntake {
    pub fn extract(raw: &str) -> Result<String, NativeIntakeReject> {
        if raw.trim().is_empty() {
            return Err(NativeIntakeReject::EmptyResponse);
        }
        let start = raw.find("```").ok_or(NativeIntakeReject::NoFencedBlock)?;
        let after_open = &raw[start + 3..];
        let after_lang = after_open.strip_prefix("rust").unwrap_or(after_open);
        let body_start = after_lang
            .char_indices()
            .find(|(_, c)| !c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(after_lang.len());
        let after_ws = &after_lang[body_start..];
        let close_rel = after_ws
            .find("```")
            .ok_or(NativeIntakeReject::NoFencedBlock)?;
        let body = after_ws[..close_rel].trim();
        if body.is_empty() {
            return Err(NativeIntakeReject::NoFencedBlock);
        }
        Ok(body.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeBindingReject {
    TemplateMismatch { expected: String, actual: String },
    ChallengeMismatch { expected: String, actual: String },
    CheckerDigestMismatch { expected: String, actual: String },
    PolicyDigestMismatch { expected: String, actual: String },
}

fn check_binding(
    candidate: &NativeCandidate,
    ctx: &NativeTaskContext,
) -> Result<(), NativeBindingReject> {
    if candidate.claimed_template_id != ctx.template_id {
        return Err(NativeBindingReject::TemplateMismatch {
            expected: ctx.template_id.clone(),
            actual: candidate.claimed_template_id.clone(),
        });
    }
    if candidate.claimed_challenge_sha256 != ctx.challenge_sha256 {
        return Err(NativeBindingReject::ChallengeMismatch {
            expected: ctx.challenge_sha256.clone(),
            actual: candidate.claimed_challenge_sha256.clone(),
        });
    }
    if candidate.claimed_checker_digest != ctx.checker_digest {
        return Err(NativeBindingReject::CheckerDigestMismatch {
            expected: ctx.checker_digest.clone(),
            actual: candidate.claimed_checker_digest.clone(),
        });
    }
    if candidate.claimed_policy_digest != ctx.policy_digest {
        return Err(NativeBindingReject::PolicyDigestMismatch {
            expected: ctx.policy_digest.clone(),
            actual: candidate.claimed_policy_digest.clone(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeCheckerVerdict {
    Accept,
    Reject { reason: String },
}

/// The judge. Never implemented inside this module -- msg 4206 section 4
/// forbids new checker logic. The one implementation used for the single
/// real closed-local pass is a thin adapter that carries the verdict already
/// produced by the sealed, byte-for-byte-reused Python checker (which itself
/// invokes the pinned rustc/cargo toolchain, outside this process); this
/// trait exists so the wiring below is independently testable without that
/// external, closed-local-only process.
pub trait NativeChecker {
    fn run(&self, candidate_text: &str, ctx: &NativeTaskContext) -> NativeCheckerVerdict;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeVerdict {
    Accept,
    Reject { stage: &'static str, reason: String },
}

#[derive(Debug, Clone)]
pub struct ChallengeEpoch {
    pub challenge_sha256: String,
    pub epoch: u64,
}

/// The canonical receipt msg 4206 section 4 required: family/version,
/// template_id, anchor digest, challenge/epoch, candidate digest, checker
/// digest, policy digest, verdict -- all eight bound in one place.
#[derive(Debug, Clone)]
pub struct NativeReceipt {
    pub family_version: String,
    pub template_id: String,
    pub anchor_digest: String,
    pub challenge_epoch: ChallengeEpoch,
    pub candidate_digest: String,
    pub checker_digest: String,
    pub policy_digest: String,
    pub verdict: NativeVerdict,
}

/// The full run report. `proof_intake_accepted`/`proof_intake_rejected` and
/// `verify_accepted` are kept as distinct fields, never folded into one
/// "solved" flag, matching this project's terminology discipline
/// (docs/proof-intake-contract.md).
#[derive(Debug, Clone)]
pub struct NativeE2EReport {
    pub proof_intake_accepted: bool,
    pub proof_intake_reject_reason: Option<String>,
    pub binding_accepted: bool,
    pub binding_reject_reason: Option<String>,
    pub checker_accepted: bool,
    pub checker_reject_reason: Option<String>,
    /// The node-verifier role: independently re-checked, not a pass-through
    /// of the checker's own opinion. True only when intake, binding, and the
    /// checker all agree.
    pub verify_accepted: bool,
    pub receipt: NativeReceipt,
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn run_native_mining_e2e(
    candidate: &NativeCandidate,
    ctx: &NativeTaskContext,
    checker: &dyn NativeChecker,
) -> NativeE2EReport {
    let base_receipt = |candidate_digest: String, verdict: NativeVerdict| NativeReceipt {
        family_version: ctx.family_version.clone(),
        template_id: ctx.template_id.clone(),
        anchor_digest: ctx.anchor_sha256.clone(),
        challenge_epoch: ChallengeEpoch {
            challenge_sha256: ctx.challenge_sha256.clone(),
            epoch: ctx.epoch,
        },
        candidate_digest,
        checker_digest: ctx.checker_digest.clone(),
        policy_digest: ctx.policy_digest.clone(),
        verdict,
    };

    // Stage 1: family-specific proof intake.
    let extracted = match NativeProofIntake::extract(&candidate.raw_model_response) {
        Ok(text) => text,
        Err(reject) => {
            let reason = format!("{reject:?}");
            return NativeE2EReport {
                proof_intake_accepted: false,
                proof_intake_reject_reason: Some(reason.clone()),
                binding_accepted: false,
                binding_reject_reason: None,
                checker_accepted: false,
                checker_reject_reason: None,
                verify_accepted: false,
                receipt: base_receipt(
                    sha256_hex(candidate.raw_model_response.as_bytes()),
                    NativeVerdict::Reject {
                        stage: "proof_intake",
                        reason,
                    },
                ),
            };
        }
    };
    // Bind the digest to the full raw model response, not just the
    // post-extraction substring -- this is the whole point of this receipt
    // versus `LeanBoundCanonicalizer::canonicalize`, which discards the
    // model's actual answer and rebinds a re-derived canonical render
    // instead. A tamper anywhere in what the model actually sent (fenced
    // body or surrounding text) must change this digest.
    let candidate_digest = sha256_hex(candidate.raw_model_response.as_bytes());

    // Stage 2: binding -- the node-verifier's independent re-check that this
    // candidate is actually for the committed template/challenge/checker/policy.
    if let Err(reject) = check_binding(candidate, ctx) {
        let reason = format!("{reject:?}");
        return NativeE2EReport {
            proof_intake_accepted: true,
            proof_intake_reject_reason: None,
            binding_accepted: false,
            binding_reject_reason: Some(reason.clone()),
            checker_accepted: false,
            checker_reject_reason: None,
            verify_accepted: false,
            receipt: base_receipt(
                candidate_digest,
                NativeVerdict::Reject {
                    stage: "binding",
                    reason,
                },
            ),
        };
    }

    // Stage 3: the native checker -- the only place a real judgment on the
    // candidate's Rust code is made; never implemented in this module.
    let checker_verdict = checker.run(&extracted, ctx);
    let (checker_accepted, checker_reject_reason, verdict) = match checker_verdict {
        NativeCheckerVerdict::Accept => (true, None, NativeVerdict::Accept),
        NativeCheckerVerdict::Reject { reason } => (
            false,
            Some(reason.clone()),
            NativeVerdict::Reject {
                stage: "checker",
                reason,
            },
        ),
    };

    NativeE2EReport {
        proof_intake_accepted: true,
        proof_intake_reject_reason: None,
        binding_accepted: true,
        binding_reject_reason: None,
        checker_accepted,
        checker_reject_reason,
        verify_accepted: checker_accepted,
        receipt: base_receipt(candidate_digest, verdict),
    }
}

/// Local, non-consensus dev share accounting. Never touches
/// `boole_core::block`, `SharePool`, or any persisted-block/reward store --
/// msg 4206 section 6 explicitly excludes those. One accepted native-mining
/// verdict appends exactly one JSON line here.
#[derive(Debug, Clone)]
pub struct ShareLedgerEntry {
    pub template_id: String,
    pub candidate_digest: String,
    pub epoch: u64,
}

pub fn record_share_if_accepted(
    report: &NativeE2EReport,
    ledger_path: &Path,
) -> Option<ShareLedgerEntry> {
    if !report.verify_accepted {
        return None;
    }
    let entry = ShareLedgerEntry {
        template_id: report.receipt.template_id.clone(),
        candidate_digest: report.receipt.candidate_digest.clone(),
        epoch: report.receipt.challenge_epoch.epoch,
    };
    let line = format!(
        "{{\"template_id\":\"{}\",\"candidate_digest\":\"{}\",\"epoch\":{}}}\n",
        entry.template_id, entry.candidate_digest, entry.epoch
    );
    if let Some(parent) = ledger_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(ledger_path)
    {
        let _ = file.write_all(line.as_bytes());
    }
    Some(entry)
}
