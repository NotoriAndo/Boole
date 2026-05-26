//! P2.3 closure (slice 42) — legacy state path migration matrix.
//!
//! The miner's canonical post-P2.3 state lives under
//! `$HOME/.boole/miner/state.json` (with `BOOLE_MINER_HOME` and
//! `BOOLE_HOME` overrides). Operators who upgraded from the older
//! layout still have a `state.json` under `$XDG_CONFIG_HOME/boole-miner`
//! or `$HOME/.config/boole-miner`. The migration helper detects that
//! and copies the file atomically to the modern path, returning a typed
//! outcome so the caller can print a one-line stderr notice exactly
//! once per migration.
//!
//! Test matrix (per §6.5 P2.3):
//!   (a) legacy-only → `Migrated`, modern populated, bytes match.
//!   (b) both present → `BothPresent`, modern bytes unchanged.
//!   (c) neither present → `None`, silent.
//!   (d) re-launch idempotent — a second call after (a) returns
//!       `BothPresent` (not `Migrated`); the migration notice is
//!       therefore printed only once.
//!
//! Tests build a `StateEnv` value directly instead of mutating process
//! env vars, so cargo's parallel test runner does not cause races.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use boole_miner::{
    canonical_state_path_with, legacy_candidates_with, try_migrate_legacy_state_with,
    LegacyMigration, StateEnv,
};

fn temp_root() -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!(
        "boole-miner-legacy-state-{}-{}",
        std::process::id(),
        seq
    ));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).expect("temp root");
    p
}

fn env_with_home(home: &Path) -> StateEnv {
    StateEnv {
        boole_miner_home: None,
        boole_home: None,
        xdg_config_home: None,
        home: home.to_path_buf(),
    }
}

fn legacy_path(home: &Path) -> PathBuf {
    home.join(".config").join("boole-miner").join("state.json")
}

fn modern_path(home: &Path) -> PathBuf {
    home.join(".boole").join("miner").join("state.json")
}

fn seed_legacy(home: &Path, bytes: &[u8]) {
    let p = legacy_path(home);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, bytes).unwrap();
}

fn seed_modern(home: &Path, bytes: &[u8]) {
    let p = modern_path(home);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, bytes).unwrap();
}

#[test]
fn case_a_legacy_only_migrates_and_returns_migrated() {
    let home = temp_root();
    seed_legacy(&home, b"{\"schemaVersion\":1,\"legacy\":true}");
    let env = env_with_home(&home);

    let result = try_migrate_legacy_state_with(&env).expect("migrate ok");
    match result {
        Some(LegacyMigration::Migrated { from, to }) => {
            assert_eq!(from, legacy_path(&home));
            assert_eq!(to, modern_path(&home));
            assert_eq!(to, canonical_state_path_with(&env).expect("canonical"));
        }
        other => panic!("expected Migrated, got {other:?}"),
    }
    let modern = modern_path(&home);
    assert!(modern.exists(), "modern path should be populated");
    let modern_bytes = fs::read(&modern).expect("read modern");
    assert_eq!(modern_bytes, b"{\"schemaVersion\":1,\"legacy\":true}");
    // Legacy file is left intact so the operator can verify the copy
    // before removing the old location by hand.
    assert!(
        legacy_path(&home).exists(),
        "legacy should remain after copy"
    );

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn case_b_both_present_returns_both_present_and_leaves_modern_alone() {
    let home = temp_root();
    seed_legacy(&home, b"{\"schemaVersion\":1,\"side\":\"legacy\"}");
    seed_modern(&home, b"{\"schemaVersion\":1,\"side\":\"modern\"}");
    let env = env_with_home(&home);

    let result = try_migrate_legacy_state_with(&env).expect("ok");
    match result {
        Some(LegacyMigration::BothPresent { legacy, modern }) => {
            assert_eq!(legacy, legacy_path(&home));
            assert_eq!(modern, modern_path(&home));
        }
        other => panic!("expected BothPresent, got {other:?}"),
    }
    let modern_bytes = fs::read(modern_path(&home)).expect("read modern");
    assert_eq!(
        modern_bytes, b"{\"schemaVersion\":1,\"side\":\"modern\"}",
        "modern must be untouched when both present"
    );

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn case_c_neither_present_returns_none_silently() {
    let home = temp_root();
    let env = env_with_home(&home);

    let result = try_migrate_legacy_state_with(&env).expect("ok");
    assert!(result.is_none(), "no legacy and no modern → None");
    assert!(!modern_path(&home).exists(), "no file created");

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn case_d_re_launch_after_migration_returns_both_present_not_migrated() {
    let home = temp_root();
    seed_legacy(&home, b"{\"schemaVersion\":1,\"second\":\"run\"}");
    let env = env_with_home(&home);

    let first = try_migrate_legacy_state_with(&env).expect("first ok");
    assert!(
        matches!(first, Some(LegacyMigration::Migrated { .. })),
        "first call migrates: {first:?}"
    );
    let second = try_migrate_legacy_state_with(&env).expect("second ok");
    assert!(
        matches!(second, Some(LegacyMigration::BothPresent { .. })),
        "second call must NOT report Migrated (notice prints once): {second:?}"
    );

    let _ = fs::remove_dir_all(&home);
}

#[test]
fn legacy_candidates_with_xdg_set_returns_xdg_first() {
    let home = temp_root();
    let xdg = home.join(".xdg");
    let env = StateEnv {
        boole_miner_home: None,
        boole_home: None,
        xdg_config_home: Some(xdg.clone()),
        home: home.clone(),
    };
    let candidates = legacy_candidates_with(&env);
    assert!(
        candidates
            .iter()
            .any(|p| p.starts_with(&xdg) && p.ends_with("state.json")),
        "XDG candidate present: {candidates:?}"
    );
    assert!(
        candidates
            .iter()
            .any(|p| p.starts_with(home.join(".config")) && p.ends_with("state.json")),
        "$HOME/.config candidate present: {candidates:?}"
    );
    let _ = fs::remove_dir_all(&home);
}
