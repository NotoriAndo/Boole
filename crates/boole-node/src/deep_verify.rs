//! P1.4 closing sub-slice — offline deep verification of the bounty
//! audit ledger.
//!
//! Given the per-event triad `{leanSource, verifierHash, checkerArtifactHash}`
//! that slices 19 and 20 made durable, an operator running
//! `boole state verify --deep` can:
//!   * stream `<state-dir>/bounty-events.ndjson` line by line,
//!   * validate each event against the v1 ledger schema, and
//!   * count how many `proof` events are eligible for offline Lean
//!     re-execution (`verifierKind == "lean"` AND `accepted == true`).
//!
//! When a checker dir is supplied, this module also performs the actual
//! Lean re-execution: it shells out to `LeanRunner` and cross-checks the
//! recorded `checkerArtifactHash`, reporting any mismatch as a
//! `DeepVerifyDivergence`. Without a checker dir (`lean_checker_dir ==
//! None`), it stays a read-only inventory step and every eligible lean
//! proof event is reported under `lean_proofs_skipped` so the caller can
//! tell the deep re-run never ran.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use boole_core::{bounty_proof_hash_hex, validate_bounty_ledger_event};
use boole_lean_runner::{LeanRunner, LeanRunnerConfig, LeanVerdict};
use serde_json::Value;

use crate::ShareEvidenceVerdict;

static REVERIFY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Outcome of a deep verification pass over a bounty event ledger.
#[derive(Debug, Clone, Default)]
pub struct DeepVerifyReport {
    pub events_scanned: u64,
    pub lean_proofs_accepted: u64,
    pub lean_proofs_reverified: u64,
    pub lean_proofs_skipped: u64,
    pub divergences: Vec<DeepVerifyDivergence>,
}

/// A single mismatch found while re-running a Lean proof event. Produced
/// by `reverify_lean_event` on the `Some(checker_dir)` path when a
/// recorded field (e.g. `leanSource`, `checkerArtifactHash`, or the
/// accept decision) does not reproduce under offline re-execution.
#[derive(Debug, Clone)]
pub struct DeepVerifyDivergence {
    pub work_id: String,
    pub proof_hash: String,
    pub field: String,
    pub expected: String,
    pub actual: String,
}

/// Failure modes for the inventory pass. Distinct variants so the CLI
/// can map each one to a stable typed-error envelope.
#[derive(Debug)]
pub enum DeepVerifyError {
    /// The ledger file was not present or could not be opened.
    EventsUnreadable { path: PathBuf, detail: String },
    /// A non-empty line either failed to parse as JSON or did not match
    /// the v1 bounty ledger event schema.
    LedgerInvalid {
        path: PathBuf,
        line_number: u64,
        detail: String,
    },
}

impl std::fmt::Display for DeepVerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeepVerifyError::EventsUnreadable { path, detail } => {
                write!(
                    f,
                    "bounty events ledger unreadable at {}: {detail}",
                    path.display()
                )
            }
            DeepVerifyError::LedgerInvalid {
                path,
                line_number,
                detail,
            } => write!(
                f,
                "bounty events ledger {} line {line_number} invalid: {detail}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for DeepVerifyError {}

/// Stream the bounty event ledger and classify each event. When
/// `lean_checker_dir` is `Some(dir)`, each accepted-lean proof event is
/// re-run via `reverify_lean_event` (counted under
/// `lean_proofs_reverified`, with mismatches collected as
/// `divergences`). When `None`, eligible lean proofs land under
/// `lean_proofs_skipped` and no re-execution is attempted.
pub fn deep_verify_bounty_events(
    bounty_events_path: &Path,
    lean_checker_dir: Option<&Path>,
) -> Result<DeepVerifyReport, DeepVerifyError> {
    let file = File::open(bounty_events_path).map_err(|err| DeepVerifyError::EventsUnreadable {
        path: bounty_events_path.to_path_buf(),
        detail: err.to_string(),
    })?;

    let reader = BufReader::new(file);
    let mut report = DeepVerifyReport::default();
    for (idx, line) in reader.lines().enumerate() {
        let line = line.map_err(|err| DeepVerifyError::EventsUnreadable {
            path: bounty_events_path.to_path_buf(),
            detail: err.to_string(),
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let event: Value =
            serde_json::from_str(&line).map_err(|err| DeepVerifyError::LedgerInvalid {
                path: bounty_events_path.to_path_buf(),
                line_number: (idx as u64) + 1,
                detail: err.to_string(),
            })?;
        validate_bounty_ledger_event(&event).map_err(|detail| DeepVerifyError::LedgerInvalid {
            path: bounty_events_path.to_path_buf(),
            line_number: (idx as u64) + 1,
            detail,
        })?;
        report.events_scanned += 1;

        if event.get("kind").and_then(Value::as_str) == Some("proof")
            && event.get("verifierKind").and_then(Value::as_str) == Some("lean")
            && event.get("accepted").and_then(Value::as_bool) == Some(true)
        {
            report.lean_proofs_accepted += 1;
            match lean_checker_dir {
                None => report.lean_proofs_skipped += 1,
                Some(dir) => {
                    let event_divergences = reverify_lean_event(&event, dir);
                    if event_divergences.is_empty() {
                        report.lean_proofs_reverified += 1;
                    } else {
                        report.divergences.extend(event_divergences);
                    }
                }
            }
        }
    }
    Ok(report)
}

/// SC.2-f1 — the bytes offline re-execution runs, plus the identity
/// check binding them to the ledger row: `effectiveArtifact` is REQUIRED
/// on every accepted lean proof event, and the recorded `proofHash` must
/// equal `bounty_proof_hash_hex(bytes)`. A mismatch means the row's
/// identity was tampered — surfaced as a `proofHash` divergence without
/// spawning any Lean process. A MISSING artifact is likewise a
/// divergence, not a fallback: silently re-running the raw `leanSource`
/// would (a) execute submitter bytes the live verifier never ran and
/// (b) let an attacker strip the field to skip the identity check
/// entirely (5th-review HIGH, downgrade bypass). There is no legacy
/// ledger to stay compatible with — the §SC W1 reset window discarded
/// every pre-v3 chain, and SC.2-f1 lands before any durable testnet-2
/// ledger exists.
///
/// Errors are `(field, expected, actual)` divergence triples.
fn reverify_execution_source(event: &Value) -> Result<&str, (String, String, String)> {
    let Some(artifact) = event.get("effectiveArtifact").and_then(Value::as_str) else {
        return Err((
            "effectiveArtifact".to_string(),
            "present".to_string(),
            "missing".to_string(),
        ));
    };
    let recorded = event.get("proofHash").and_then(Value::as_str).unwrap_or("");
    let recomputed = bounty_proof_hash_hex(artifact.as_bytes());
    if recorded != recomputed {
        return Err(("proofHash".to_string(), recorded.to_string(), recomputed));
    }
    Ok(artifact)
}

/// Re-run Lean for a single accepted-lean proof event and produce zero
/// or more divergences. An empty return means every recorded field
/// reproduced byte-identically (or, for `accepted`, structurally) under
/// the offline re-execution.
fn reverify_lean_event(event: &Value, checker_dir: &Path) -> Vec<DeepVerifyDivergence> {
    let mut divergences = Vec::new();
    let work_id = event
        .get("workId")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let proof_hash = event
        .get("proofHash")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    // SC.2-f1 — pick the bytes to re-execute and check the recorded
    // proof identity against them BEFORE any Lean process is spawned
    // (see `reverify_execution_source`).
    let execution_source = match reverify_execution_source(event) {
        Ok(source) => source,
        Err((field, expected, actual)) => {
            divergences.push(DeepVerifyDivergence {
                work_id,
                proof_hash,
                field,
                expected,
                actual,
            });
            return divergences;
        }
    };
    let verifier_hash = match event.get("verifierHash").and_then(Value::as_str) {
        Some(s) => s,
        None => {
            divergences.push(DeepVerifyDivergence {
                work_id,
                proof_hash,
                field: "verifierHash".to_string(),
                expected: "present".to_string(),
                actual: "missing".to_string(),
            });
            return divergences;
        }
    };
    let recorded_checker_hash = match event.get("checkerArtifactHash").and_then(Value::as_str) {
        Some(s) => s.to_string(),
        None => {
            divergences.push(DeepVerifyDivergence {
                work_id,
                proof_hash,
                field: "checkerArtifactHash".to_string(),
                expected: "present".to_string(),
                actual: "missing".to_string(),
            });
            return divergences;
        }
    };

    let tmp_dir = std::env::temp_dir().join(format!(
        "boole-deep-verify-{}-{}",
        std::process::id(),
        REVERIFY_COUNTER.fetch_add(1, Ordering::Relaxed),
    ));
    if let Err(err) = std::fs::create_dir_all(&tmp_dir) {
        divergences.push(DeepVerifyDivergence {
            work_id,
            proof_hash,
            field: "tmpDir".to_string(),
            expected: "writable".to_string(),
            actual: err.to_string(),
        });
        return divergences;
    }
    let proof_path = tmp_dir.join("Proof.lean");
    if let Err(err) = std::fs::write(&proof_path, execution_source) {
        divergences.push(DeepVerifyDivergence {
            work_id,
            proof_hash,
            field: "proofFile".to_string(),
            expected: "writable".to_string(),
            actual: err.to_string(),
        });
        let _ = std::fs::remove_dir_all(&tmp_dir);
        return divergences;
    }

    let runner = LeanRunner::new(
        LeanRunnerConfig::new(verifier_hash).with_package_dir(checker_dir.to_path_buf()),
    );
    match runner.check_file(&proof_path) {
        // SC.9a / ADR-0016 (a-3) — an availability failure is not a
        // re-verification verdict: report it as a runner problem the
        // operator retries, never as an "accepted=false" divergence that
        // reads like the proof no longer holds.
        Ok(result) if result.verdict.is_retryable_unavailable() => {
            divergences.push(DeepVerifyDivergence {
                work_id,
                proof_hash,
                field: "runner".to_string(),
                expected: "ok".to_string(),
                actual: format!("retryable_unavailable: {:?}", result.verdict),
            });
        }
        Ok(result) => {
            if !result.accepted {
                divergences.push(DeepVerifyDivergence {
                    work_id: work_id.clone(),
                    proof_hash: proof_hash.clone(),
                    field: "accepted".to_string(),
                    expected: "true".to_string(),
                    actual: "false".to_string(),
                });
            }
            if result.evidence.checker_artifact_hash != recorded_checker_hash {
                divergences.push(DeepVerifyDivergence {
                    work_id,
                    proof_hash,
                    field: "checkerArtifactHash".to_string(),
                    expected: recorded_checker_hash,
                    actual: result.evidence.checker_artifact_hash,
                });
            }
        }
        Err(err) => {
            divergences.push(DeepVerifyDivergence {
                work_id,
                proof_hash,
                field: "runner".to_string(),
                expected: "ok".to_string(),
                actual: err.to_string(),
            });
        }
    }
    let _ = std::fs::remove_dir_all(&tmp_dir);
    divergences
}

/// Outcome of a deep verification pass over a block store's Lean-bound
/// shares (N0.4c).
#[derive(Debug, Clone, Default)]
pub struct DeepVerifyBlockReport {
    pub blocks_scanned: u64,
    /// Shares carrying a `seedHex` (Lean-bound; eligible for re-derivation).
    pub lean_bound_shares: u64,
    /// Shares whose canon was recomputed from the seed and matched.
    pub canon_reverified: u64,
    /// Shares whose Lean proof was re-elaborated and accepted (only when a
    /// checker dir + lake/lean were available).
    pub lean_reverified: u64,
    /// Shares skipped (no `seedHex`, or no checker dir to recompute against).
    pub shares_skipped: u64,
    pub divergences: Vec<DeepVerifyDivergence>,
}

/// Cheap capability probe gating the OPTIONAL Lean re-elaboration step:
/// the canon recompute is pure and always runs, but re-elaborating the
/// proof needs `lake`/`lean` on PATH.
fn lake_and_lean_available() -> bool {
    use std::process::Command;
    let probe = |bin: &str| {
        Command::new(bin)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    probe("lake") && probe("lean")
}

/// N0.4c — deep-verify a block store's live-mined Lean-bound shares.
///
/// For every `selectedShareEvidence` carrying a `seedHex` (Path 2), the
/// share's canonical Lean source is RE-DERIVED from the seed (not trusted
/// from any persisted string), and:
///   * the canon is recomputed via the shared `lean_bound_canon_package`
///     encoder (checker hash read from `lean_checker_dir`, verifier hash
///     from `profile`) and compared byte-for-byte to the stored
///     `proofPackage` — a pure file-hash check that needs no toolchain; and
///   * when `lean_checker_dir` is set AND lake/lean are available, the
///     re-derived proof is re-elaborated through `LeanRunner` and must be
///     accepted.
///
/// Without `lean_checker_dir` the canon cannot be recomputed (no checker
/// hash), so every Lean-bound share is reported under `shares_skipped`.
pub fn deep_verify_block(
    block_path: &Path,
    lean_checker_dir: Option<&Path>,
    profile: &str,
) -> Result<DeepVerifyBlockReport, DeepVerifyError> {
    let blocks = crate::FileBlockStore::recover(block_path).map_err(|err| {
        DeepVerifyError::EventsUnreadable {
            path: block_path.to_path_buf(),
            detail: err.to_string(),
        }
    })?;

    let mut report = DeepVerifyBlockReport::default();

    // Checker identity: recompute once from the node's own canonical checker
    // (a pure file hash; no Lean process). When absent, canon recompute is
    // impossible, so every Lean-bound share is skipped.
    let checker_artifact_hash = match lean_checker_dir {
        Some(dir) => match boole_lean_runner::checker_artifact_hash(dir) {
            Ok(h) => Some(h),
            Err(err) => {
                return Err(DeepVerifyError::EventsUnreadable {
                    path: dir.to_path_buf(),
                    detail: format!("checker artifact hash: {err}"),
                });
            }
        },
        None => None,
    };
    let verifier_hash = boole_core::lean_bound_verifier_hash(profile);
    let lake_ready = lean_checker_dir.is_some() && lake_and_lean_available();

    for block in blocks.blocks() {
        report.blocks_scanned += 1;
        for share in &block.selected_share_evidence {
            if share.seed_hex.is_empty() {
                continue; // not a Lean-bound share (legacy / placeholder)
            }
            report.lean_bound_shares += 1;

            let Some(checker_hash) = checker_artifact_hash.as_deref() else {
                report.shares_skipped += 1;
                continue;
            };

            let checker_dir =
                lean_checker_dir.expect("checker dir present when checker hash present");
            // The single verifier entry (SC.10-ii-a): re-derive the source
            // from the seed, recompute the canon, and — when lake is ready —
            // re-run the pinned checker. The audit maps its classification
            // back onto the report counters; `lake_ready` is the audit's
            // `run_lean` (a lake-less host stays a pure canon check).
            match crate::verify_lean_bound_share_evidence(
                &block.c,
                share,
                checker_dir,
                checker_hash,
                &verifier_hash,
                lake_ready,
            ) {
                // Unreachable: the empty-seed shares are filtered above.
                ShareEvidenceVerdict::NotLeanBound => {}
                ShareEvidenceVerdict::SourceRederiveFailed { detail } => {
                    report.divergences.push(DeepVerifyDivergence {
                        work_id: block.c.clone(),
                        proof_hash: share.canon_hash.clone(),
                        field: "seedHex".to_string(),
                        expected: "valid v1-lenbound seed".to_string(),
                        actual: detail,
                    });
                }
                ShareEvidenceVerdict::CanonMismatch { expected, actual } => {
                    report.divergences.push(DeepVerifyDivergence {
                        work_id: block.c.clone(),
                        proof_hash: share.canon_hash.clone(),
                        field: "proofPackage".to_string(),
                        expected,
                        actual,
                    });
                }
                // Canon matched (all three arms below): count the pure canon
                // reverify exactly as the inline version did before running
                // — or skipping — Lean.
                ShareEvidenceVerdict::LeanSkipped => {
                    report.canon_reverified += 1;
                }
                ShareEvidenceVerdict::Lean(LeanVerdict::Accepted) => {
                    report.canon_reverified += 1;
                    report.lean_reverified += 1;
                }
                // ADR-0016 (a-3) — an availability failure names the runner,
                // never the proof.
                ShareEvidenceVerdict::Lean(verdict) if verdict.is_retryable_unavailable() => {
                    report.canon_reverified += 1;
                    report.divergences.push(DeepVerifyDivergence {
                        work_id: block.c.clone(),
                        proof_hash: share.canon_hash.clone(),
                        field: "runner".to_string(),
                        expected: "ok".to_string(),
                        actual: format!("retryable_unavailable: {verdict:?}"),
                    });
                }
                ShareEvidenceVerdict::Lean(_) => {
                    report.canon_reverified += 1;
                    report.divergences.push(DeepVerifyDivergence {
                        work_id: block.c.clone(),
                        proof_hash: share.canon_hash.clone(),
                        field: "accepted".to_string(),
                        expected: "true".to_string(),
                        actual: "false".to_string(),
                    });
                }
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_ndjson(path: &Path, lines: &[Value]) {
        let mut file = std::fs::File::create(path).expect("create ndjson");
        for line in lines {
            writeln!(file, "{}", serde_json::to_string(line).expect("line json"))
                .expect("write line");
        }
    }

    #[test]
    fn missing_ledger_file_yields_events_unreadable_error() {
        let missing =
            std::env::temp_dir().join(format!("deep-verify-missing-{}.ndjson", std::process::id()));
        let _ = std::fs::remove_file(&missing);
        let err = deep_verify_bounty_events(&missing, None).expect_err("missing file errors");
        assert!(
            matches!(err, DeepVerifyError::EventsUnreadable { .. }),
            "expected EventsUnreadable, got {err:?}"
        );
    }

    #[test]
    fn empty_ledger_scans_zero_events() {
        let dir = std::env::temp_dir().join(format!("deep-verify-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("bounty-events.ndjson");
        std::fs::write(&path, "").expect("write empty");
        let report = deep_verify_bounty_events(&path, None).expect("empty ledger ok");
        assert_eq!(report.events_scanned, 0);
        assert_eq!(report.lean_proofs_accepted, 0);
        assert_eq!(report.lean_proofs_reverified, 0);
        assert_eq!(report.lean_proofs_skipped, 0);
        assert!(report.divergences.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn classifies_accepted_lean_proofs_into_skipped_when_no_checker_dir() {
        let dir = std::env::temp_dir().join(format!("deep-verify-classify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("bounty-events.ndjson");
        let events = vec![
            serde_json::json!({
                "schemaVersion": 1,
                "kind": "proof",
                "workId": "lean-1",
                "problemHash": "9999000000000000000000000000000000000000000000000000000000000000",
                "verifierKind": "lean",
                "ts": 1_800_000_000_000_i64,
                "proofHash": "aaaa000000000000000000000000000000000000000000000000000000000000",
                "solverPk": "1100000000000000000000000000000000000000000000000000000000000000",
                "accepted": true,
            }),
            serde_json::json!({
                "schemaVersion": 1,
                "kind": "proof",
                "workId": "lean-2",
                "problemHash": "8888000000000000000000000000000000000000000000000000000000000000",
                "verifierKind": "lean",
                "ts": 1_800_000_001_000_i64,
                "proofHash": "bbbb000000000000000000000000000000000000000000000000000000000000",
                "solverPk": "2200000000000000000000000000000000000000000000000000000000000000",
                "accepted": false,
            }),
        ];
        write_ndjson(&path, &events);
        let report = deep_verify_bounty_events(&path, None).expect("ok");
        assert_eq!(report.events_scanned, 2);
        assert_eq!(report.lean_proofs_accepted, 1);
        assert_eq!(report.lean_proofs_skipped, 1);
        assert_eq!(report.lean_proofs_reverified, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_schema_line_yields_ledger_invalid_with_one_based_line_number() {
        let dir = std::env::temp_dir().join(format!("deep-verify-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("bounty-events.ndjson");
        let events = vec![serde_json::json!({
            "schemaVersion": 2,
            "kind": "create",
            "workId": "lean-1",
            "problemHash": "9999000000000000000000000000000000000000000000000000000000000000",
            "verifierKind": "lean",
            "ts": 1_800_000_000_000_i64,
        })];
        write_ndjson(&path, &events);
        let err = deep_verify_bounty_events(&path, None).expect_err("invalid schema rejected");
        match err {
            DeepVerifyError::LedgerInvalid { line_number, .. } => {
                assert_eq!(line_number, 1, "one-based line number for first line");
            }
            other => panic!("expected LedgerInvalid, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // SC.2-f1 — offline deep-verify must execute the SAME bytes the live
    // verifier judged: when the ledger row carries `effectiveArtifact`,
    // those bytes (not the raw submitter `leanSource`, whose discarded
    // prefix never ran online) are what the offline TCB re-elaborates.
    #[test]
    fn deep_verify_executes_same_effective_artifact_as_live_verifier() {
        let artifact = "namespace M\n\ntheorem t : 1 + 1 = 2 :=\nby decide\n\nend M\n";
        let event = serde_json::json!({
            "proofHash": boole_core::bounty_proof_hash_hex(artifact.as_bytes()),
            "leanSource": "theorem attacker_prefix : anything := by decide",
            "effectiveArtifact": artifact,
        });
        let source = reverify_execution_source(&event).expect("artifact source resolves");
        assert_eq!(
            source, artifact,
            "offline re-execution must run the recorded effective artifact, not raw leanSource"
        );
    }

    // SC.2-f1 — a tampered ledger `proofHash` (one that does not equal
    // the domain-tagged hash of the recorded artifact) must surface as a
    // divergence BEFORE any Lean process is spawned.
    #[test]
    fn deep_verify_rejects_tampered_artifact_proof_hash() {
        let artifact = "namespace M\n\ntheorem t : 1 + 1 = 2 :=\nby decide\n\nend M\n";
        let tampered = "cccc000000000000000000000000000000000000000000000000000000000000";
        let event = serde_json::json!({
            "proofHash": tampered,
            "leanSource": "theorem t : 1 + 1 = 2 := by decide",
            "effectiveArtifact": artifact,
        });
        let (field, expected, actual) =
            reverify_execution_source(&event).expect_err("tampered proofHash must diverge");
        assert_eq!(field, "proofHash");
        assert_eq!(
            expected, tampered,
            "expected = the (tampered) recorded value"
        );
        assert_eq!(
            actual,
            boole_core::bounty_proof_hash_hex(artifact.as_bytes()),
            "actual = the identity recomputed from the recorded artifact"
        );
    }

    // 5th-review HIGH (downgrade bypass) — a row with the artifact
    // STRIPPED must diverge, never silently fall back to re-running the
    // raw submitter source without an identity check.
    #[test]
    fn deep_verify_rejects_event_with_stripped_effective_artifact() {
        let event = serde_json::json!({
            "proofHash": "aaaa000000000000000000000000000000000000000000000000000000000000",
            "leanSource": "theorem t : 1 + 1 = 2 := by decide",
        });
        let (field, expected, actual) = reverify_execution_source(&event)
            .expect_err("missing effectiveArtifact must diverge, not fall back");
        assert_eq!(field, "effectiveArtifact");
        assert_eq!(expected, "present");
        assert_eq!(actual, "missing");
    }
}
