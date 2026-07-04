//! TB.3 — the `proof_bridge` canon digest must be a stable dedup key.
//!
//! `stable_digest` used to hash `lean_version`/`lake_version`/`stdout`
//! (toolchain/runtime values) alongside the raw, unnormalized `proof_source`
//! bytes. That meant a semantically inert blank-line edit to the proof, or a
//! toolchain patch bump, minted a fresh `canon_hash` for an otherwise
//! identical proof — defeating byte-keyed dedup and violating the "one
//! proof = one canon hash" invariant that the future consensus-level dedup
//! (N4-pre.1) will rely on. This mirrors the sibling
//! `boole_core::lean_bound_canon_package`, which documents excluding
//! toolchain/runtime values as a hazard for exactly this reason.

use boole_lean_runner::{LeanCheckResult, LeanRunnerEvidence};
use boole_node::canonical_pofp_package_from_lean_result_and_source;

fn synthetic_lean_result(lean_version: &str, lake_version: &str, stdout: &str) -> LeanCheckResult {
    LeanCheckResult {
        accepted: true,
        exit_code: 0,
        stdout: stdout.to_string(),
        stderr: String::new(),
        timed_out: false,
        output_truncated: false,
        evidence: LeanRunnerEvidence {
            verifier_hash: "bridge-verifier-hash".to_string(),
            checker: "lake exec boole_check".to_string(),
            checker_exe: "lake".to_string(),
            checker_artifact_hash:
                "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            package_dir: "/tmp/boole-check".to_string(),
            lean_version: lean_version.to_string(),
            lake_version: lake_version.to_string(),
            timeout_ms: 5_000,
            memory_limit_mb: 8_192,
            output_limit_bytes: 65_536,
        },
    }
}

#[test]
fn blank_line_edit_yields_same_canon_hash() {
    let result = synthetic_lean_result("Lean 4.29.1", "Lake 5.0.0", "stdout-a");
    let proof_source_a = "theorem boole_canon_stable : 1 = 1 := by\n  rfl\n";
    // Semantically inert edit: an extra blank line inserted mid-proof.
    let proof_source_b = "theorem boole_canon_stable : 1 = 1 := by\n\n  rfl\n";

    let package_a =
        canonical_pofp_package_from_lean_result_and_source(&result, proof_source_a.as_bytes());
    let package_b =
        canonical_pofp_package_from_lean_result_and_source(&result, proof_source_b.as_bytes());

    assert_eq!(
        package_a, package_b,
        "a blank-line-only edit to the proof source must not change the canon package"
    );
}

#[test]
fn toolchain_version_change_does_not_change_canon_hash() {
    let proof_source = "theorem boole_canon_stable : 1 = 1 := by\n  rfl\n";
    let result_a = synthetic_lean_result("Lean 4.29.1", "Lake 5.0.0", "stdout-a");
    let result_b = synthetic_lean_result("Lean 4.30.0", "Lake 5.1.0", "stdout-b");

    let package_a =
        canonical_pofp_package_from_lean_result_and_source(&result_a, proof_source.as_bytes());
    let package_b =
        canonical_pofp_package_from_lean_result_and_source(&result_b, proof_source.as_bytes());

    assert_eq!(
        package_a, package_b,
        "a lean/lake toolchain bump (and its stdout) must not change the canon package"
    );
}
