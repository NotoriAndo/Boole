//! SC.9a / ADR-0016 (a)(a-3)(b) — the verdict is a pure function of
//! (proof bytes, pinned checker, committed step budget). Wall-clock and
//! rlimits are containment only, never a verdict:
//!
//!   accepted | deterministic_reject | retryable_unavailable
//!
//! Running out of the committed step budget (`maxHeartbeats`) is a
//! DETERMINISTIC reject — every node, fast or slow, reaches the same
//! verdict from the same bytes. A wall-clock containment kill is an
//! AVAILABILITY failure — it may never masquerade as a reject (or an
//! accept), because a slow node would then fork from a fast one.

use boole_lean_runner::{LeanRunner, LeanRunnerConfig, LeanVerdict};
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
        "boole-budget-verdict-{}-{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create proof dir");
    let path = dir.join("Proof.lean");
    std::fs::write(&path, content).expect("write proof");
    path
}

/// A proof whose elaboration needs more than the tiny committed budget the
/// test sets, but far less than the generous wall-clock timeout — so the
/// only way it can fail is by exhausting `maxHeartbeats`, and the runner
/// must classify that as a deterministic `budget_exceeded` reject, NOT as
/// a timeout/containment condition.
#[test]
fn verdict_is_budget_exceeded_not_timeout_when_steps_run_out() {
    if !lake_and_lean_available() {
        eprintln!("skipping budget verdict test: lake/lean unavailable");
        return;
    }
    let proof = write_proof(
        "exhaust",
        "theorem boole_budget_burn : (List.range 400).foldl Nat.add 0 = 79800 := by decide\n",
    );
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("budget-verdict")
            .with_package_dir(canonical_checker_dir())
            // Committed budget: 1 (thousand) heartbeats — far below what
            // the `decide` above needs. Wall clock stays generous so the
            // deterministic budget, not containment, is what trips.
            .with_max_heartbeats(1)
            .with_max_rec_depth(512)
            .with_timeout_ms(120_000),
    );
    let result = runner.check_file(&proof).expect("checker returns envelope");
    assert!(
        !result.accepted,
        "budget-exceeded proof must not be accepted"
    );
    assert!(
        !result.timed_out,
        "step-budget exhaustion is not a wall-clock timeout: {result:?}"
    );
    match &result.verdict {
        LeanVerdict::DeterministicReject { reason } => assert_eq!(
            reason, "budget_exceeded",
            "running out of committed steps must be the typed budget_exceeded reject"
        ),
        other => panic!("expected deterministic budget_exceeded reject, got {other:?}"),
    }
}

/// ADR-0016 (a-3) — a wall-clock containment kill maps ONLY to
/// `retryable_unavailable`. It is not a verdict: translating it into a
/// reject (today's behavior: `accepted=false`) lets a slow node reach a
/// different consensus judgement than a fast node for the same bytes.
#[test]
fn wall_clock_containment_kill_is_retryable_unavailable_never_a_verdict() {
    if !lake_and_lean_available() {
        eprintln!("skipping containment verdict test: lake/lean unavailable");
        return;
    }
    let proof = write_proof("containment", "theorem boole_trivial : True := trivial\n");
    let runner = LeanRunner::new(
        LeanRunnerConfig::new("budget-verdict")
            .with_package_dir(canonical_checker_dir())
            // Wall clock far below even `lake` startup cost: guaranteed
            // containment kill on any machine, with a budget that the
            // trivial proof could never exhaust.
            .with_max_heartbeats(400_000)
            .with_max_rec_depth(512)
            .with_timeout_ms(50),
    );
    let result = runner.check_file(&proof).expect("kill returns envelope");
    assert!(result.timed_out, "harness must record the containment kill");
    match &result.verdict {
        LeanVerdict::RetryableUnavailable { reason } => assert!(
            reason.contains("containment"),
            "availability failure should name containment: {reason}"
        ),
        other => {
            panic!("containment kill must be retryable_unavailable, never a verdict: {other:?}")
        }
    }
    assert!(
        !result.accepted,
        "an availability failure must also never fail open into acceptance"
    );
}

/// The committed budget is part of the runner's evidence surface via its
/// config defaults: the runner must always pass an EXPLICIT budget to the
/// checker (never inherit Lean's own defaults), so the default config
/// values are consensus-meaningful and pinned here.
#[test]
fn runner_default_budget_is_explicit_and_pinned() {
    let config = LeanRunnerConfig::new("budget-verdict");
    assert_eq!(
        config.max_heartbeats, 400_000,
        "default step budget must mirror the base-lane Tier-2 constant"
    );
    assert_eq!(
        config.max_rec_depth, 512,
        "default recursion budget must mirror the base-lane Tier-2 constant"
    );
}
