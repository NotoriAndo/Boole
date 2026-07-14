//! SC.9b / ADR-0016 (a-2, "the pin identifies the executable toolchain") —
//! the toolchain identity recorded in evidence must be the one the checker
//! PROCESS actually runs under, not whatever `lean`/`lake` the ambient
//! environment happens to resolve first.
//!
//! Concretely: elan's shims dispatch `lean`/`lake` by the `lean-toolchain`
//! file of the CURRENT directory. The checker child runs with
//! `cwd = package_dir`, so it gets the package-pinned toolchain; a bare
//! `lean --version` from the test-runner's cwd can resolve a DIFFERENT
//! toolchain (any host whose default differs from the pin demonstrates
//! this). Evidence computed from the ambient path is a PATH-resolution
//! TOCTOU: it records an identity no proof was ever checked under.

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

fn effective_output(dir: &Path, cmd: &str, args: &[&str]) -> String {
    let output = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|err| panic!("run {cmd} {args:?} in {}: {err}", dir.display()));
    assert!(
        output.status.success(),
        "{cmd} {args:?} failed in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn effective_toolchain_evidence_matches_checker_process() {
    if !lake_and_lean_available() {
        eprintln!("skipping effective toolchain test: lake/lean unavailable");
        return;
    }
    let dir = canonical_checker_dir();

    // What the checker process actually sees: `lake env lean` dispatched
    // from the package dir (the same cwd the runner gives the child).
    let effective_lean = effective_output(&dir, "lake", &["env", "lean", "--version"]);
    let effective_lake = effective_output(&dir, "lake", &["--version"]);

    let runner = LeanRunner::new(
        LeanRunnerConfig::new("effective-toolchain-test").with_package_dir(dir.clone()),
    );
    let evidence = runner.evidence().expect("evidence");

    assert_eq!(
        evidence.lean_version, effective_lean,
        "evidence must record the lean identity the checker process runs \
         under (package-pinned toolchain), not the ambient PATH's lean"
    );
    assert_eq!(
        evidence.lake_version, effective_lake,
        "evidence must record the lake identity resolved from the package \
         dir, not the ambient cwd's lake"
    );
}
