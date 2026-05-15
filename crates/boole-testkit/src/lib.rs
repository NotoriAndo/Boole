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
use std::sync::atomic::{AtomicU64, Ordering};
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
/// tempdirs and per-test state directories. Wall-clock nanos are mixed with
/// a process-local atomic counter so two successive calls inside the same
/// test binary cannot collide even when wall-clock resolution is coarser
/// than nanoseconds (this happened on macOS — see history of
/// `tests/reward_store_divergence.rs`).
///
/// Not cryptographically random; callers that need collision-free names
/// across processes should still pair this with a process-id prefix.
pub fn rand_suffix() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let bump = COUNTER.fetch_add(1, Ordering::Relaxed);
    nanos.wrapping_add(bump.wrapping_mul(0x9E37_79B9_7F4A_7C15))
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
    fn rand_suffix_two_calls_never_collide_in_same_process() {
        // The atomic-counter mix guarantees inequality even when wall-clock
        // resolution would otherwise duplicate the value.
        let a = rand_suffix();
        let b = rand_suffix();
        assert_ne!(a, b, "rand_suffix must not return duplicates: {a} == {b}");
    }
}
