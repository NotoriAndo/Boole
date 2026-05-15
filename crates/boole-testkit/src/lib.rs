//! Shared test helpers for the Boole workspace.
//!
//! P0.1a — minimal first slice. Exposes three helpers that the master plan
//! L10 contract names and that are duplicated across 30+ test files today:
//! `rand_suffix`, `repo_root`, `lake_and_lean_available`. Later P0.1 slices
//! add `TempStateDir`, `start_node`, `FixtureCatalog`, `MockBountyVerifier`,
//! `MockSubmitter`, `MockChainHead`.
//!
//! Production crates must not depend on this crate. It is a `dev-dependencies`
//! target only (via `[dev-dependencies] boole-testkit = { path = ... }` in
//! each consuming crate). Keeping it out of the production dep graph means
//! a release build does not link the mock surface.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Return the workspace root as an absolute path.
///
/// The crate lives at `<root>/crates/boole-testkit`, so the workspace root
/// is two parents up from `CARGO_MANIFEST_DIR`. This avoids `canonicalize`
/// to keep the path stable when test directories live on symlinked volumes.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("boole-testkit lives at crates/boole-testkit; workspace root is two parents up")
        .to_path_buf()
}

/// Return a nanosecond-resolution monotonic-ish suffix suitable for naming
/// tempdirs and per-test state directories. Not cryptographically random;
/// callers that need collision-free names under parallel execution should
/// pair this with `tempfile::TempDir` or a process-id prefix.
///
/// With `RUST_TEST_THREADS=1` (enforced by `scripts/self-test.sh`) two
/// successive calls inside a single test cannot collide.
pub fn rand_suffix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// True iff both `lake` and `lean` are on `PATH` and respond to `--version`.
///
/// Used by Lean-bridge tests to gate themselves: if the toolchain is missing
/// the test must be `#[ignore = "needs-lean"]`-style annotated; the early
/// `if !lake_and_lean_available() { return; }` pattern is being phased out
/// (see master plan L10).
pub fn lake_and_lean_available() -> bool {
    let lake_ok = Command::new("lake")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    let lean_ok = Command::new("lean")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    lake_ok && lean_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_root_points_at_workspace() {
        let root = repo_root();
        assert!(
            root.join("Cargo.toml").is_file(),
            "repo_root() should resolve to a directory containing the workspace Cargo.toml, got {}",
            root.display()
        );
        assert!(
            root.join("crates").join("boole-testkit").is_dir(),
            "repo_root() should contain crates/boole-testkit, got {}",
            root.display()
        );
    }

    #[test]
    fn rand_suffix_is_monotonic_per_call() {
        let a = rand_suffix();
        // sleep one ns equivalent; consecutive calls on a fast machine can
        // collide at nanosecond resolution. Assert non-decreasing rather
        // than strictly greater so the test is stable on every host.
        let b = rand_suffix();
        assert!(b >= a, "rand_suffix should be non-decreasing: {a} then {b}");
    }
}
