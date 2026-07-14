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

use boole_core::SelectedShareEvidence;
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
///      run the pinned checker, returning `Lean(verdict)`. A runner launch
///      failure is `RetryableUnavailable` (availability, never a reject).
///
/// `checker_dir` is the node's pinned checker directory; `checker_artifact_hash`
/// must be the hash of that same directory (the caller computes it once).
pub fn verify_lean_bound_share_evidence(
    block_c: &str,
    share: &SelectedShareEvidence,
    checker_dir: &Path,
    checker_artifact_hash: &str,
    verifier_hash: &str,
    run_lean: bool,
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
    ShareEvidenceVerdict::Lean(run_pinned_checker(block_c, &module, checker_dir))
}

/// Elaborate a re-derived Lean module through the pinned `LeanRunner` and
/// return its three-state verdict. A runner launch failure maps to
/// `RetryableUnavailable` (ADR-0016 (a-3): availability, never a reject).
fn run_pinned_checker(block_c: &str, module_text: &str, checker_dir: &Path) -> LeanVerdict {
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
        LeanRunnerConfig::new("boole-share-verify").with_package_dir(checker_dir.to_path_buf()),
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
