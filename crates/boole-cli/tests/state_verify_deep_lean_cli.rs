//! P1.4 closing sub-slice — `boole state verify --deep --lean-checker-dir`
//! actually re-runs Lean over each accepted-lean proof event.
//!
//! Slice 21 fixed the CLI surface: the deep verifier streams the audit
//! ledger, classifies events, and reports an inventory envelope. Today
//! it just counts eligible events under `leanProofsSkipped`. This
//! slice wires the real LeanRunner so that supplying
//! `--lean-checker-dir` flips eligible events into `leanProofsReverified`
//! when the re-run reproduces both:
//!   * `accepted == true`, AND
//!   * `checker_artifact_hash` byte-equal to the value recorded on the
//!     audit event at original-acceptance time.
//!
//! Any mismatch is recorded under `divergences[]` with a `field` of
//! either `accepted` or `checkerArtifactHash`, and the CLI exits 3 so
//! automation can route the failure to operator attention.
//!
//! Both tests are lake/lean-gated (early return with diagnostic when
//! the toolchain is absent), matching the pattern in
//! `crates/boole-node/tests/bounty_lean_verifier.rs`.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::Command;

use boole_core::{Bounty, BountyProofVerifier, BountyVerifier};
use boole_node::LeanBountyVerifier;
use boole_testkit::rand_suffix;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

const PROVER: &str = "1100000000000000000000000000000000000000000000000000000000000000";
// Ledger-event `problemHash` used by the two synthetic ndjson audit
// events below. `deep_verify_bounty_events`/`reverify_lean_event` (the
// actual CLI re-verification path exercised by these tests) never binds
// this value to anything — SC.2-f1: it re-runs the recorded
// `effectiveArtifact` (the rendered module the live verifier judged)
// through `LeanRunner::check_file` directly, after checking the recorded
// `proofHash` equals `bounty_proof_hash_hex(artifact)`.
// `validate_bounty_ledger_event` only requires 32-byte hex.
const PROBLEM_HASH: &str = "9999999999999999999999999999999999999999999999999999999999999999";

/// TB.2 — content hash of a bounty's commissioned `statement`, matching
/// `LeanBountyVerifier`'s own `content_hash_hex` (see
/// `crates/boole-node/src/lean_bounty_verifier.rs` and the same helper in
/// `crates/boole-node/tests/bounty_lean_verifier.rs`).
fn content_hash_hex(statement: &str) -> String {
    hex::encode(Sha256::digest(statement.as_bytes()))
}

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

fn lake_and_lean_available() -> bool {
    let lake_ok = Command::new("lake")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    let lean_ok = Command::new("lean")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    lake_ok && lean_ok
}

fn canonical_checker_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../lean/checker")
        .canonicalize()
        .expect("canonical checker dir")
}

/// Build a bounty whose `problem_hash` is `statement`'s own content hash
/// and whose `verifier.metadata.statement` carries that same statement —
/// TB.2 requires both so `LeanBountyVerifier` can bind the probe's proof
/// term to the bounty's commissioned statement before running Lean.
fn make_lean_bounty(verifier_hash: &str, statement: &str) -> Bounty {
    let mut metadata: Map<String, Value> = Map::new();
    metadata.insert(
        "verifierHash".to_string(),
        Value::String(verifier_hash.to_string()),
    );
    metadata.insert(
        "statement".to_string(),
        Value::String(statement.to_string()),
    );
    Bounty {
        id: "lean-1".to_string(),
        domain: "lean.test".to_string(),
        problem_hash: content_hash_hex(statement),
        verifier: BountyVerifier {
            kind: "lean".to_string(),
            metadata,
        },
        reward: "1".to_string(),
        deadline: 1900000000000,
        status: "open".to_string(),
        created_at: 1800000000000,
        updated_at: 1800000000000,
    }
}

fn write_ndjson(path: &std::path::Path, lines: &[Value]) {
    let mut file = std::fs::File::create(path).expect("create ndjson");
    for line in lines {
        writeln!(file, "{}", serde_json::to_string(line).expect("line json")).expect("write line");
    }
}

/// Probe the real `LeanBountyVerifier` once so the recorded
/// `checkerArtifactHash` in the synthetic audit event matches the
/// runner's computed value byte-for-byte. This avoids burning a
/// hardcoded hash that drifts every time the checker package is
/// rebuilt or relocated.
fn probe_checker_artifact_hash(verifier_hash: &str, statement: &str, lean_source: &str) -> String {
    let bounty = make_lean_bounty(verifier_hash, statement);
    let envelope = json!({ "leanSource": lean_source });
    let outcome = LeanBountyVerifier::new(canonical_checker_dir())
        .verify_with_evidence(&bounty, &envelope)
        .expect("lean verifier returns Ok during probe");
    assert!(outcome.accepted, "probe proof must accept");
    outcome
        .evidence
        .get("checkerArtifactHash")
        .and_then(Value::as_str)
        .expect("probe surfaces checkerArtifactHash")
        .to_string()
}

/// SC.2-f1 — derive the verifier-effective artifact exactly the way the
/// live route does, so the synthetic ledger events carry the same bytes
/// (and artifact-derived `proofHash`) a real node would have recorded.
fn probe_effective_artifact(verifier_hash: &str, statement: &str, lean_source: &str) -> String {
    let bounty = make_lean_bounty(verifier_hash, statement);
    let envelope = json!({ "leanSource": lean_source });
    let artifact = LeanBountyVerifier::new(canonical_checker_dir())
        .effective_artifact(&bounty, &envelope)
        .expect("lean verifier derives artifact");
    String::from_utf8(artifact).expect("rendered module is UTF-8")
}

#[test]
fn deep_verify_with_checker_dir_re_executes_lean_and_increments_reverified() {
    if !lake_and_lean_available() {
        eprintln!("skipping deep_verify_reverified: lake/lean unavailable");
        return;
    }
    let checker_dir = canonical_checker_dir();
    let verifier_hash = format!(
        "deep-verify-reverified-{}-{}",
        std::process::id(),
        rand_suffix()
    );
    let lean_source = "theorem boole_deep_verify_accept : 2 + 2 = 4 := by\n  decide\n";
    let checker_artifact_hash =
        probe_checker_artifact_hash(&verifier_hash, "2 + 2 = 4", lean_source);
    let effective_artifact = probe_effective_artifact(&verifier_hash, "2 + 2 = 4", lean_source);
    let artifact_proof_hash = boole_core::bounty_proof_hash_hex(effective_artifact.as_bytes());

    let dir = std::env::temp_dir().join(format!("boole-deep-verify-ok-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let events_path = dir.join("bounty-events.ndjson");

    let event = json!({
        "schemaVersion": 1,
        "kind": "proof",
        "workId": "lean-1",
        "problemHash": PROBLEM_HASH,
        "verifierKind": "lean",
        "ts": 1_800_000_000_000_i64,
        "proofHash": artifact_proof_hash,
        "solverPk": PROVER,
        "accepted": true,
        "leanSource": lean_source,
        "effectiveArtifact": effective_artifact,
        "verifierHash": verifier_hash,
        "checkerArtifactHash": checker_artifact_hash,
    });
    write_ndjson(&events_path, &[event]);

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "state",
            "verify",
            "--deep",
            "--bounty-events",
            events_path.to_str().expect("utf8 events path"),
            "--lean-checker-dir",
            checker_dir.to_str().expect("utf8 checker dir"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert!(
        output.status.success(),
        "deep verify must succeed when re-run matches recorded hash; \
         stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: DeepVerifyOutput =
        serde_json::from_slice(&output.stdout).expect("json envelope on stdout");
    assert!(parsed.ok, "ok=true expected on clean re-verify");
    assert_eq!(parsed.events_scanned, 1);
    assert_eq!(parsed.lean_proofs_accepted, 1);
    assert_eq!(
        parsed.lean_proofs_reverified, 1,
        "checker-dir must trigger real Lean re-execution"
    );
    assert_eq!(
        parsed.lean_proofs_skipped, 0,
        "skipped bucket must be empty once checker-dir is wired"
    );
    assert!(parsed.divergences.is_empty(), "no divergence expected");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn deep_verify_reports_checker_artifact_hash_divergence_with_exit_3() {
    if !lake_and_lean_available() {
        eprintln!("skipping deep_verify_divergence: lake/lean unavailable");
        return;
    }
    let checker_dir = canonical_checker_dir();
    let verifier_hash = format!(
        "deep-verify-divergence-{}-{}",
        std::process::id(),
        rand_suffix()
    );
    let lean_source = "theorem boole_deep_verify_divergence : 1 + 1 = 2 := by\n  decide\n";
    let effective_artifact = probe_effective_artifact(&verifier_hash, "1 + 1 = 2", lean_source);
    let artifact_proof_hash = boole_core::bounty_proof_hash_hex(effective_artifact.as_bytes());

    // Recorded hash is intentionally bogus; LeanRunner will compute the
    // real one and the comparison must surface a divergence on the
    // `checkerArtifactHash` field.
    let bogus_recorded = "0".repeat(64);
    let dir = std::env::temp_dir().join(format!("boole-deep-verify-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    let events_path = dir.join("bounty-events.ndjson");

    let event = json!({
        "schemaVersion": 1,
        "kind": "proof",
        "workId": "lean-divergent",
        "problemHash": PROBLEM_HASH,
        "verifierKind": "lean",
        "ts": 1_800_000_000_000_i64,
        "proofHash": artifact_proof_hash,
        "solverPk": PROVER,
        "accepted": true,
        "leanSource": lean_source,
        "effectiveArtifact": effective_artifact,
        "verifierHash": verifier_hash,
        "checkerArtifactHash": bogus_recorded,
    });
    write_ndjson(&events_path, &[event]);

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "state",
            "verify",
            "--deep",
            "--bounty-events",
            events_path.to_str().expect("utf8 events path"),
            "--lean-checker-dir",
            checker_dir.to_str().expect("utf8 checker dir"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert_eq!(
        output.status.code(),
        Some(3),
        "checker-hash divergence must exit 3; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let envelope: Value = serde_json::from_str(stderr_text.trim()).expect("stderr envelope JSON");
    assert_eq!(envelope.get("ok"), Some(&Value::Bool(false)));
    let divergences = envelope
        .get("divergences")
        .and_then(Value::as_array)
        .expect("divergences array");
    assert_eq!(divergences.len(), 1, "exactly one divergence expected");
    assert_eq!(
        divergences[0].get("field").and_then(Value::as_str),
        Some("checkerArtifactHash"),
        "divergence field must name the mismatched evidence key"
    );
    assert_eq!(
        divergences[0].get("workId").and_then(Value::as_str),
        Some("lean-divergent")
    );

    let _ = std::fs::remove_dir_all(&dir);
}
