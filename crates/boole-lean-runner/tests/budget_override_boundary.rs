//! SC.9a / ADR-0016 (a-2) — the committed step budget is a CEILING the
//! proof source cannot raise. `lean -D maxHeartbeats=<N>` sets only the
//! *default* value of the option: a submitted source containing
//! `set_option maxHeartbeats <M>` (including `0` = unlimited) or
//! `set_option maxRecDepth <M>` would override the runner's committed
//! budget within its own scope, making the consensus budget advisory.
//!
//! Two independent verdict-bearing layers must both reject such a source:
//!
//!   1. intake — `check_file`'s pre-spawn forbidden-token scan, before any
//!      Lean process runs (same mechanism as the TB.1 blacklist);
//!   2. checker — the ADR-0013 audit pass (`BooleCheck/Audit.lean`) scans
//!      the raw source itself and refuses with a typed
//!      `BOOLE_BUDGET_OVERRIDE` marker, so a source slipping past layer 1
//!      still cannot buy steps.
//!
//! These tests exercise the CANONICAL checker package (`lean/checker/`),
//! not a synthetic fixture, because layer 2 lives in the shipped
//! `Audit.lean`.

use boole_lean_runner::{LeanRunner, LeanRunnerConfig};
use std::path::{Path, PathBuf};
use std::process::Command;

fn canonical_checker_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate has workspace root")
        .join("lean")
        .join("checker")
}

fn lake_and_lean_available() -> bool {
    Command::new("lake")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
        && Command::new("lean")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
}

fn write_proof(name: &str, content: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-budget-override-{}-{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create proof dir");
    let path = dir.join("Proof.lean");
    std::fs::write(&path, content).expect("write proof");
    path
}

fn assert_intake_rejects(proof: &Path, token: &str) {
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("budget-override-boundary")
            .with_package_dir(canonical_checker_dir())
            .with_timeout_ms(120_000),
    );
    match runner.check_file(proof) {
        Err(err) => {
            let msg = err.to_string();
            assert!(
                msg.contains(token),
                "intake rejection must name the budget token `{token}`: {msg}"
            );
        }
        Ok(result) => panic!(
            "source setting `{token}` must be rejected at intake before any \
             Lean process runs, got accepted={} verdict via full checker run",
            result.accepted
        ),
    }
}

/// ADR-0016 (a-2) layer 1 — a proof source that raises `maxHeartbeats`
/// above the committed budget is rejected at intake.
#[test]
fn proof_cannot_override_committed_max_heartbeats() {
    let proof = write_proof(
        "heartbeats",
        "set_option maxHeartbeats 1000000 in\ntheorem boole_override : 1 + 1 = 2 := by decide\n",
    );
    assert_intake_rejects(&proof, "maxHeartbeats");
}

/// ADR-0016 (a-2) layer 1 — same ceiling for the second verdict-bearing
/// counter, `maxRecDepth`.
#[test]
fn proof_cannot_override_committed_max_rec_depth() {
    let proof = write_proof(
        "recdepth",
        "set_option maxRecDepth 100000 in\ntheorem boole_override : 1 + 1 = 2 := by decide\n",
    );
    assert_intake_rejects(&proof, "maxRecDepth");
}

/// ADR-0016 (a-2) — `set_option maxHeartbeats 0` is the UNLIMITED form
/// (Lean treats 0 as "no heartbeat limit"), the most dangerous override.
#[test]
fn unlock_limits_is_forbidden() {
    let proof = write_proof(
        "unlock",
        "set_option maxHeartbeats 0 in\ntheorem boole_unlock : 1 + 1 = 2 := by decide\n",
    );
    assert_intake_rejects(&proof, "maxHeartbeats");
}

/// ADR-0016 (a-2) layer 2 — the audit pass refuses a budget override on its
/// own, independently of the intake scan. Invoke `BooleCheck/Audit.lean`
/// DIRECTLY (bypassing `check_file` and its layer-1 scan) on a source that
/// sets `maxHeartbeats`, and require a non-zero exit with the typed
/// `BOOLE_BUDGET_OVERRIDE` marker. Without this second line, any bypass of
/// the Rust-side scan silently re-opens the budget.
#[test]
fn audit_pass_rejects_budget_override_independently_of_intake() {
    if !lake_and_lean_available() {
        eprintln!("skipping audit budget-override test: lake/lean unavailable");
        return;
    }
    let proof = write_proof(
        "audit-direct",
        "set_option maxHeartbeats 0 in\ntheorem boole_audit_bypass : 1 + 1 = 2 := by decide\n",
    );
    let output = Command::new("lake")
        .arg("env")
        .arg("lean")
        .arg("--run")
        .arg("BooleCheck/Audit.lean")
        .arg(&proof)
        .current_dir(canonical_checker_dir())
        .output()
        .expect("run audit pass directly");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "audit pass must refuse a budget-override source on its own: \
         stdout={stdout} stderr={stderr}"
    );
    assert!(
        format!("{stdout}{stderr}").contains("BOOLE_BUDGET_OVERRIDE"),
        "audit refusal must carry the typed BOOLE_BUDGET_OVERRIDE marker: \
         stdout={stdout} stderr={stderr}"
    );
}
