//! SC.10-ii-a — the single Lean-bound share verifier entry.
//!
//! Before this slice the only place that re-derived a persisted share's
//! canonical Lean source, recomputed its canon and re-ran the pinned
//! checker lived inline in `deep_verify_block` (the offline `--deep`
//! audit). ADR-0016 (c-2) requires that admission, ingest re-verify and
//! reorg re-verify all reach the SAME accept / reject / unavailable
//! decision from the SAME bytes, committed budget and pinned checker —
//! "one shared verifier entry point". This module is that entry.
//!
//! The offline audit is migrated onto it here; the consensus paths
//! (SC.10-ii-b/c/d) converge on it in the following slices. The
//! three-state Lean result is `boole_lean_runner::LeanVerdict` (SC.9a) —
//! reused verbatim, never re-invented: a containment/availability failure
//! is `RetryableUnavailable` and must never become a consensus reject
//! (ADR-0016 (a-3)).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use boole_core::{PersistedBlock, SelectedShareEvidence};
use boole_lean_runner::{LeanRunner, LeanRunnerConfig, LeanVerdict};

static REVERIFY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Classification of one Lean-bound share, produced by the single verifier
/// entry. Every consumer (offline audit and the consensus paths) maps this
/// onto its own outcome so the accept/reject/unavailable decision is made
/// once, in one place.
#[derive(Debug, Clone)]
pub enum ShareEvidenceVerdict {
    /// `seed_hex` is empty — this is not a Lean-bound share (legacy /
    /// placeholder). The caller decides whether that is acceptable on its
    /// path; the entry makes no verdict.
    NotLeanBound,
    /// The stored `seed_hex` did not re-derive to a valid family instance,
    /// so the source the canon claims to bind cannot exist. Deterministic
    /// reject (no Lean process needed).
    SourceRederiveFailed { detail: String },
    /// The canon recomputed from the re-derived source + pinned checker +
    /// verifier hash did not match the stored `proof_package`. A pure
    /// file-hash binding failure — deterministic reject, no Lean process.
    CanonMismatch { expected: String, actual: String },
    /// Canon matched and the caller asked NOT to run the Lean process
    /// (`run_lean == false`, the audit path when lake is unavailable).
    /// Never returned when `run_lean` is true.
    LeanSkipped,
    /// Canon matched and the pinned checker ran: its three-state verdict
    /// (`Accepted` / `DeterministicReject` / `RetryableUnavailable`).
    Lean(LeanVerdict),
}

/// The single verifier entry for one persisted Lean-bound share.
///
/// Steps, in order (identical to the logic previously inline in
/// `deep_verify_block`):
///   1. empty `seed_hex` ⇒ `NotLeanBound`;
///   2. re-derive the family instance from `seed_hex` — never trust a
///      persisted source string — failing ⇒ `SourceRederiveFailed`;
///   3. recompute the canon bound to `(verifier_hash, checker_artifact_hash,
///      re-derived source)` and compare byte-for-byte to `proof_package` —
///      mismatch ⇒ `CanonMismatch`;
///   4. if `run_lean` is false ⇒ `LeanSkipped`;
///   5. otherwise wrap the re-derived proof exactly as the verifier does and
///      run the pinned checker under the committed step budget
///      (`max_heartbeats`/`max_rec_depth`), returning `Lean(verdict)`. A
///      runner launch failure is `RetryableUnavailable` (availability, never
///      a reject).
///
/// `checker_dir` is the node's pinned checker directory; `checker_artifact_hash`
/// must be the hash of that same directory (the caller computes it once).
/// `max_heartbeats`/`max_rec_depth` are the committed base-lane budget the
/// checker must run under, so every path (audit / ingest / reorg / admission)
/// grinds the same proof under the same ceiling (ADR-0016 (a-2), (c-2)).
#[allow(clippy::too_many_arguments)]
pub fn verify_lean_bound_share_evidence(
    block_c: &str,
    share: &SelectedShareEvidence,
    checker_dir: &Path,
    checker_artifact_hash: &str,
    verifier_hash: &str,
    run_lean: bool,
    max_heartbeats: u64,
    max_rec_depth: u64,
) -> ShareEvidenceVerdict {
    if share.seed_hex.is_empty() {
        return ShareEvidenceVerdict::NotLeanBound;
    }

    // Re-derive the canonical Lean source from the seed — never trust a
    // persisted source string.
    let instance = match boole_core::family_v1_lenbound::generate_from_hex(&share.seed_hex) {
        Ok(inst) => inst,
        Err(err) => {
            return ShareEvidenceVerdict::SourceRederiveFailed {
                detail: err.to_string(),
            };
        }
    };
    let lean_source = boole_core::family_v1_lenbound::render_canonical_proof(&instance);

    // Canon recompute (pure): the stored proofPackage must equal the canon
    // bound to the re-derived source + checker + verifier.
    let recomputed =
        boole_core::lean_bound_canon_package(verifier_hash, checker_artifact_hash, &lean_source);
    let recomputed_hex = hex::encode(&recomputed);
    if recomputed_hex != share.proof_package {
        return ShareEvidenceVerdict::CanonMismatch {
            expected: share.proof_package.clone(),
            actual: recomputed_hex,
        };
    }

    if !run_lean {
        return ShareEvidenceVerdict::LeanSkipped;
    }

    // Lean re-elaboration: the canon binds the bare proof body, but the
    // checker elaborates a full module — wrap the re-derived proof exactly
    // as the verifier does before running.
    let module = boole_core::family_v1_lenbound::lean_module(&instance, &lean_source);
    ShareEvidenceVerdict::Lean(run_pinned_checker(
        block_c,
        &module,
        checker_dir,
        max_heartbeats,
        max_rec_depth,
    ))
}

/// Elaborate a re-derived Lean module through the pinned `LeanRunner` under
/// the committed step budget and return its three-state verdict. A runner
/// launch failure maps to `RetryableUnavailable` (ADR-0016 (a-3):
/// availability, never a reject).
fn run_pinned_checker(
    block_c: &str,
    module_text: &str,
    checker_dir: &Path,
    max_heartbeats: u64,
    max_rec_depth: u64,
) -> LeanVerdict {
    let tmp_dir = std::env::temp_dir().join(format!(
        "boole-share-verify-{}-{}",
        std::process::id(),
        REVERIFY_COUNTER.fetch_add(1, Ordering::Relaxed),
    ));
    if let Err(err) = std::fs::create_dir_all(&tmp_dir) {
        return LeanVerdict::RetryableUnavailable {
            reason: format!("tmp dir for {block_c}: {err}"),
        };
    }
    let proof_path = tmp_dir.join("Proof.lean");
    if let Err(err) = std::fs::write(&proof_path, module_text) {
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return LeanVerdict::RetryableUnavailable {
            reason: format!("proof file for {block_c}: {err}"),
        };
    }
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("boole-share-verify")
            .with_package_dir(checker_dir.to_path_buf())
            .with_max_heartbeats(max_heartbeats)
            .with_max_rec_depth(max_rec_depth),
    );
    let verdict = match runner.check_file(&proof_path) {
        Ok(result) => result.verdict,
        Err(err) => LeanVerdict::RetryableUnavailable {
            reason: err.to_string(),
        },
    };
    let _ = std::fs::remove_dir_all(&tmp_dir);
    verdict
}

/// The block-level fold of the single share verifier entry over a peer
/// block's base-lane `selectedShareEvidence` — the gate ingest and reorg
/// (SC.10-ii-b/c) run before adopting a block on a checker-pinned network.
///
/// Only the base lane is re-verified here: promoted bounty shares are NOT
/// peer-re-verified (ADR-0016 (d)), so `promoted_bounty_shares` is untouched.
#[derive(Debug, Clone)]
pub enum BlockReverifyOutcome {
    /// Every base-lane share accepted, was not Lean-bound, or was skipped —
    /// the block clears the Lean re-verify gate.
    Verified,
    /// At least one share is a deterministic failure (source re-derive, canon
    /// mismatch, or Lean `DeterministicReject`). The block is a consensus
    /// reject and must not be adopted.
    DeterministicReject { detail: String },
    /// At least one share hit a containment / availability failure (Lean
    /// `RetryableUnavailable`) and no share deterministically rejected. The
    /// block is deferred — the node cannot adopt it or advance its head, but
    /// it is never a consensus reject and never a fail-open accept
    /// (ADR-0016 (a-3)).
    RetryableUnavailable { detail: String },
}

/// Re-run the pinned checker over every base-lane share in `block` under the
/// committed budget and fold the per-share verdicts into one block outcome.
///
/// Deterministic rejects win over retryable-unavailable: a single provably
/// invalid share rejects the whole block regardless of any concurrent
/// availability failure, since the block can never be valid. Only when no
/// share deterministically rejects does an availability failure defer the
/// block (never reject, never fail-open accept).
pub fn reverify_block_selected_shares(
    block: &PersistedBlock,
    checker_dir: &Path,
    checker_artifact_hash: &str,
    verifier_hash: &str,
    max_heartbeats: u64,
    max_rec_depth: u64,
) -> BlockReverifyOutcome {
    let mut retryable: Option<String> = None;
    for (idx, share) in block.selected_share_evidence.iter().enumerate() {
        let verdict = verify_lean_bound_share_evidence(
            &block.c,
            share,
            checker_dir,
            checker_artifact_hash,
            verifier_hash,
            true,
            max_heartbeats,
            max_rec_depth,
        );
        match verdict {
            ShareEvidenceVerdict::NotLeanBound
            | ShareEvidenceVerdict::LeanSkipped
            | ShareEvidenceVerdict::Lean(LeanVerdict::Accepted) => {}
            ShareEvidenceVerdict::SourceRederiveFailed { detail } => {
                return BlockReverifyOutcome::DeterministicReject {
                    detail: format!("share[{idx}] source re-derive failed: {detail}"),
                };
            }
            ShareEvidenceVerdict::CanonMismatch { expected, actual } => {
                return BlockReverifyOutcome::DeterministicReject {
                    detail: format!(
                        "share[{idx}] canon mismatch: expected {expected}, recomputed {actual}"
                    ),
                };
            }
            ShareEvidenceVerdict::Lean(LeanVerdict::DeterministicReject { reason }) => {
                return BlockReverifyOutcome::DeterministicReject {
                    detail: format!("share[{idx}] Lean reject: {reason}"),
                };
            }
            ShareEvidenceVerdict::Lean(LeanVerdict::RetryableUnavailable { reason }) => {
                // Remember the first availability failure but keep scanning:
                // a later deterministic reject still wins.
                if retryable.is_none() {
                    retryable = Some(format!("share[{idx}] unavailable: {reason}"));
                }
            }
        }
    }
    match retryable {
        Some(detail) => BlockReverifyOutcome::RetryableUnavailable { detail },
        None => BlockReverifyOutcome::Verified,
    }
}

/// The chain-level fold of the block re-verify gate over a peer's FULL
/// competing chain — the gate reorg (SC.10-ii-c) runs before adopting a
/// candidate chain on a checker-pinned network.
///
/// Each block's base-lane evidence is re-verified by
/// [`reverify_block_selected_shares`] under the SAME committed budget and
/// pinned checker, so ingest and reorg reach the same accept / reject /
/// unavailable decision from the same bytes (ADR-0016 (c-2)). The per-block
/// outcomes fold with the same precedence as the per-share fold: a
/// deterministic reject anywhere wins immediately (the chain can never be
/// valid), otherwise the first availability failure defers the whole chain,
/// otherwise the chain is `Verified`.
///
/// This re-verifies the full candidate from genesis; skipping an
/// already-verified prefix is deferred to the SC.10-iii verified-prefix
/// checkpoint store, which is the mechanism that records which blocks a node
/// has already cleared.
pub fn reverify_candidate_chain_selected_shares(
    blocks: &[PersistedBlock],
    checker_dir: &Path,
    checker_artifact_hash: &str,
    verifier_hash: &str,
    max_heartbeats: u64,
    max_rec_depth: u64,
) -> BlockReverifyOutcome {
    let mut retryable: Option<String> = None;
    for (idx, block) in blocks.iter().enumerate() {
        match reverify_block_selected_shares(
            block,
            checker_dir,
            checker_artifact_hash,
            verifier_hash,
            max_heartbeats,
            max_rec_depth,
        ) {
            BlockReverifyOutcome::Verified => {}
            BlockReverifyOutcome::DeterministicReject { detail } => {
                return BlockReverifyOutcome::DeterministicReject {
                    detail: format!("block[{idx}] {detail}"),
                };
            }
            BlockReverifyOutcome::RetryableUnavailable { detail } => {
                // Remember the first availability failure but keep scanning:
                // a later deterministic reject still wins.
                if retryable.is_none() {
                    retryable = Some(format!("block[{idx}] {detail}"));
                }
            }
        }
    }
    match retryable {
        Some(detail) => BlockReverifyOutcome::RetryableUnavailable { detail },
        None => BlockReverifyOutcome::Verified,
    }
}
