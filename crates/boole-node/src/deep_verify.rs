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

use boole_core::validate_bounty_ledger_event;
use boole_lean_runner::{LeanRunner, LeanRunnerConfig};
use serde_json::Value;

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

    let lean_source = match event.get("leanSource").and_then(Value::as_str) {
        Some(s) => s,
        None => {
            divergences.push(DeepVerifyDivergence {
                work_id,
                proof_hash,
                field: "leanSource".to_string(),
                expected: "present".to_string(),
                actual: "missing".to_string(),
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
    if let Err(err) = std::fs::write(&proof_path, lean_source) {
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
}
