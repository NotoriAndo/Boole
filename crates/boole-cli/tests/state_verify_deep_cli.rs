//! P1.4 closing sub-slice — `boole state verify --deep`.
//!
//! With slices 19 and 20 the bounty audit ledger now carries the full
//! `{leanSource, verifierHash, checkerArtifactHash}` triad on every
//! accepted-lean proof event. This CLI surfaces that data: it streams a
//! `bounty-events.ndjson`, validates each line against the v1 ledger
//! schema, classifies events by kind, and reports how many
//! accepted-lean proof events are eligible for offline Lean re-execution.
//!
//! This slice pins the CLI shell + read-only inventory contract. The
//! actual Lean re-run integration (which would shell out to lake/lean
//! and compare `checker_artifact_hash` against the recorded value) lands
//! in a follow-up sub-slice once the surface is stable; today, without
//! a `--lean-checker-dir`, every lean proof event lands in
//! `leanProofsSkipped` rather than `leanProofsReverified`.
//!
//! Contract this test pins:
//!   * `boole state verify --deep --bounty-events <ndjson> --json`
//!     emits an envelope with the deterministic shape
//!     `{ok, eventsScanned, leanProofsAccepted, leanProofsReverified,
//!       leanProofsSkipped, divergences}`.
//!   * Empty / non-lean events are scanned but never counted as lean.
//!   * A schema-invalid line aborts the run with exit code 3 and a
//!     typed `{ok:false, reason:"ledger_invalid", ...}` envelope on
//!     stderr (mirrors the existing `replay_mismatch` shape from
//!     `state verify`).
//!   * A missing bounty-events file exits with code 2 and reason
//!     `bounty_events_unreadable` so automation can distinguish
//!     operator typos from durable-state corruption.

use std::io::Write as _;
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeepVerifyOutput {
    ok: bool,
    events_scanned: u64,
    lean_proofs_accepted: u64,
    lean_proofs_reverified: u64,
    lean_proofs_skipped: u64,
    divergences: Vec<serde_json::Value>,
}

fn write_ndjson(path: &std::path::Path, lines: &[serde_json::Value]) {
    let mut file = std::fs::File::create(path).expect("create ndjson");
    for line in lines {
        writeln!(file, "{}", serde_json::to_string(line).expect("line json")).expect("write line");
    }
}

#[test]
fn deep_verify_counts_lean_proofs_and_skips_when_no_checker_dir_provided() {
    let dir = std::env::temp_dir().join(format!("boole-cli-state-deep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("bounty-events.ndjson");

    // Mix of events so we exercise validation + classification:
    //   * one `create` event (non-proof)
    //   * one accepted lean proof event (eligible for deep re-verify)
    //   * one accepted non-lean proof event (not eligible)
    //   * one rejected lean proof event (not eligible; deep verify only
    //     re-runs events the node already accepted)
    let events = vec![
        serde_json::json!({
            "schemaVersion": 1,
            "kind": "create",
            "workId": "lean-1",
            "problemHash": "9999000000000000000000000000000000000000000000000000000000000000",
            "verifierKind": "lean",
            "ts": 1_800_000_000_000_i64,
        }),
        serde_json::json!({
            "schemaVersion": 1,
            "kind": "proof",
            "workId": "lean-1",
            "problemHash": "9999000000000000000000000000000000000000000000000000000000000000",
            "verifierKind": "lean",
            "ts": 1_800_000_001_000_i64,
            "proofHash": "aaaa000000000000000000000000000000000000000000000000000000000000",
            "solverPk": "1100000000000000000000000000000000000000000000000000000000000000",
            "accepted": true,
            "leanSource": "theorem t : 1 + 1 = 2 := by decide\n",
            "verifierHash": "abcd000000000000000000000000000000000000000000000000000000000000",
            "checkerArtifactHash": "fedc000000000000000000000000000000000000000000000000000000000000",
        }),
        serde_json::json!({
            "schemaVersion": 1,
            "kind": "proof",
            "workId": "echo-1",
            "problemHash": "8888000000000000000000000000000000000000000000000000000000000000",
            "verifierKind": "echo",
            "ts": 1_800_000_002_000_i64,
            "proofHash": "bbbb000000000000000000000000000000000000000000000000000000000000",
            "solverPk": "2200000000000000000000000000000000000000000000000000000000000000",
            "accepted": true,
        }),
        serde_json::json!({
            "schemaVersion": 1,
            "kind": "proof",
            "workId": "lean-2",
            "problemHash": "7777000000000000000000000000000000000000000000000000000000000000",
            "verifierKind": "lean",
            "ts": 1_800_000_003_000_i64,
            "proofHash": "cccc000000000000000000000000000000000000000000000000000000000000",
            "solverPk": "3300000000000000000000000000000000000000000000000000000000000000",
            "accepted": false,
        }),
    ];
    write_ndjson(&path, &events);

    // `--allow-skips`: strict deep verify (SC.10-i) refuses a skipped
    // eligible proof by default; this test pins the best-effort inventory
    // path that tolerates skips when no checker dir is available.
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "state",
            "verify",
            "--deep",
            "--bounty-events",
            path.to_str().expect("utf8 path"),
            "--allow-skips",
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert!(
        output.status.success(),
        "deep verify must succeed on a clean ledger; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: DeepVerifyOutput =
        serde_json::from_slice(&output.stdout).expect("json envelope on stdout");
    assert!(parsed.ok, "ok=true expected on clean ledger");
    assert_eq!(
        parsed.events_scanned, 4,
        "all four ndjson lines must be scanned"
    );
    assert_eq!(
        parsed.lean_proofs_accepted, 1,
        "only the lean-1 accepted proof should count"
    );
    // Without a Lean checker dir, every accepted-lean proof lands in
    // the skipped bucket so callers can tell the deep re-run never ran.
    assert_eq!(parsed.lean_proofs_reverified, 0);
    assert_eq!(parsed.lean_proofs_skipped, 1);
    assert!(parsed.divergences.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

/// SC.10-i / ADR-0016 (c) — "verification is the product": by default a
/// deep verify that could not re-run an eligible Lean proof (no checker
/// dir → `leanProofsSkipped > 0`) is a HARD failure, not a silent `ok:true`
/// inventory. The strict refusal exits 3 with the skip count on stderr so
/// an operator/CI cannot mistake "never re-ran" for "re-verified".
#[test]
fn deep_verify_strict_refuses_a_skipped_lean_proof() {
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-state-deep-strict-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("bounty-events.ndjson");

    // A single accepted-lean proof event: eligible for offline re-verify,
    // so without a checker dir it lands in `leanProofsSkipped`.
    let events = vec![serde_json::json!({
        "schemaVersion": 1,
        "kind": "proof",
        "workId": "lean-1",
        "problemHash": "9999000000000000000000000000000000000000000000000000000000000000",
        "verifierKind": "lean",
        "ts": 1_800_000_001_000_i64,
        "proofHash": "aaaa000000000000000000000000000000000000000000000000000000000000",
        "solverPk": "1100000000000000000000000000000000000000000000000000000000000000",
        "accepted": true,
        "leanSource": "theorem t : 1 + 1 = 2 := by decide\n",
        "verifierHash": "abcd000000000000000000000000000000000000000000000000000000000000",
        "checkerArtifactHash": "fedc000000000000000000000000000000000000000000000000000000000000",
    })];
    write_ndjson(&path, &events);

    // No `--lean-checker-dir` and no `--allow-skips`: strict default.
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "state",
            "verify",
            "--deep",
            "--bounty-events",
            path.to_str().expect("utf8 path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert_eq!(
        output.status.code(),
        Some(3),
        "strict deep verify must exit 3 when an eligible Lean proof is skipped; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let envelope: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stderr).trim())
            .expect("strict-skip envelope JSON on stderr");
    assert_eq!(envelope.get("ok"), Some(&serde_json::Value::Bool(false)));
    assert_eq!(
        envelope.get("leanProofsSkipped").and_then(|v| v.as_u64()),
        Some(1),
        "the refused envelope must still report the skip count"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deep_verify_rejects_a_schema_invalid_ledger_line_with_exit_3() {
    let dir = std::env::temp_dir().join(format!("boole-cli-state-deep-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("bounty-events.ndjson");

    // schemaVersion=2 is rejected by `validate_bounty_ledger_event`.
    let events = vec![serde_json::json!({
        "schemaVersion": 2,
        "kind": "create",
        "workId": "lean-1",
        "problemHash": "9999000000000000000000000000000000000000000000000000000000000000",
        "verifierKind": "lean",
        "ts": 1_800_000_000_000_i64,
    })];
    write_ndjson(&path, &events);

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "state",
            "verify",
            "--deep",
            "--bounty-events",
            path.to_str().expect("utf8 path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert_eq!(
        output.status.code(),
        Some(3),
        "schema-invalid ledger must exit with code 3; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let envelope: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr is JSON envelope");
    assert_eq!(envelope.get("ok"), Some(&serde_json::Value::Bool(false)));
    assert_eq!(
        envelope.get("reason").and_then(|v| v.as_str()),
        Some("ledger_invalid")
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deep_verify_rejects_missing_bounty_events_with_exit_2() {
    let missing = std::env::temp_dir().join(format!(
        "boole-cli-state-deep-missing-{}.ndjson",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&missing);

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "state",
            "verify",
            "--deep",
            "--bounty-events",
            missing.to_str().expect("utf8 path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert_eq!(
        output.status.code(),
        Some(2),
        "missing bounty-events file must exit 2; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let envelope: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr JSON envelope");
    assert_eq!(envelope.get("ok"), Some(&serde_json::Value::Bool(false)));
    assert_eq!(
        envelope.get("reason").and_then(|v| v.as_str()),
        Some("bounty_events_unreadable")
    );
}
