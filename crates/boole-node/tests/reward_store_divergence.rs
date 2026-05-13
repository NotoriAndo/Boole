//! S9 — reward ledger divergence detection.
//!
//! `verify_ledger_matches_replay` accepts the in-memory `FileRewardLedger`
//! and the post-replay balance map (typically `replay_blocks(blocks).balances`)
//! and bails on the first balance divergence. This is the primitive the
//! runtime boot path uses to decide whether to trust an existing rewards.ndjson
//! against the on-disk block log.

use std::collections::BTreeMap;

use boole_core::{PersistedCredit, PersistedRewardEvent};
use boole_node::{verify_ledger_matches_replay, FileRewardLedger};

const PK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const C_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn seed_ledger_with_two_credits() -> FileRewardLedger {
    let dir = std::env::temp_dir().join(format!(
        "boole-s9-ledger-divergence-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("rewards.ndjson");
    let event = PersistedRewardEvent {
        height: 0,
        c: C_HEX.to_string(),
        credits: vec![
            PersistedCredit {
                pk: PK_A.to_string(),
                amount: "2".to_string(),
            },
            PersistedCredit {
                pk: PK_B.to_string(),
                amount: "1".to_string(),
            },
        ],
    };
    FileRewardLedger::append(&path, &event).expect("append");
    let ledger = FileRewardLedger::recover(&path).expect("recover");
    let _ = std::fs::remove_dir_all(&dir);
    ledger
}

fn rand_suffix() -> u64 {
    // Combine wall-clock nanos with a process-local atomic counter so two
    // tests running concurrently in the same test binary cannot collide on
    // the same temp directory. Nanos alone collide at sub-microsecond
    // intervals on macOS, which manifested as a ~60% flake where two seed
    // calls would share `rewards.ndjson` and corrupt each other's writes.
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let bump = COUNTER.fetch_add(1, Ordering::Relaxed);
    nanos.wrapping_add(bump.wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

#[test]
fn verify_passes_when_ledger_balances_match_replay() {
    let ledger = seed_ledger_with_two_credits();
    let mut replay: BTreeMap<String, u128> = BTreeMap::new();
    replay.insert(PK_A.to_string(), 2);
    replay.insert(PK_B.to_string(), 1);
    verify_ledger_matches_replay(&ledger, &replay, None, &BTreeMap::new()).expect("balances match");
}

#[test]
fn verify_bails_on_balance_mismatch() {
    let ledger = seed_ledger_with_two_credits();
    let mut replay: BTreeMap<String, u128> = BTreeMap::new();
    replay.insert(PK_A.to_string(), 99); // tampered
    replay.insert(PK_B.to_string(), 1);
    let err = verify_ledger_matches_replay(&ledger, &replay, None, &BTreeMap::new())
        .expect_err("must bail on mismatch");
    let msg = err.to_string();
    assert!(
        msg.contains("reward ledger divergence"),
        "expected typed prefix, got: {msg}"
    );
    assert!(
        msg.contains(PK_A),
        "diverging pk must appear in message: {msg}"
    );
}

#[test]
fn verify_bails_when_pk_missing_from_ledger() {
    let ledger = seed_ledger_with_two_credits();
    let mut replay: BTreeMap<String, u128> = BTreeMap::new();
    replay.insert(PK_A.to_string(), 2);
    replay.insert(PK_B.to_string(), 1);
    replay.insert(
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_string(),
        7,
    );
    let err = verify_ledger_matches_replay(&ledger, &replay, None, &BTreeMap::new())
        .expect_err("must bail on missing pk");
    let msg = err.to_string();
    assert!(
        msg.contains("reward ledger divergence"),
        "expected typed prefix, got: {msg}"
    );
}

#[test]
fn verify_bails_when_ledger_carries_pk_absent_from_replay() {
    let ledger = seed_ledger_with_two_credits();
    let mut replay: BTreeMap<String, u128> = BTreeMap::new();
    // Only PK_A present — ledger's PK_B must trigger divergence.
    replay.insert(PK_A.to_string(), 2);
    let err = verify_ledger_matches_replay(&ledger, &replay, None, &BTreeMap::new())
        .expect_err("must bail on extra pk in ledger");
    let msg = err.to_string();
    assert!(
        msg.contains("reward ledger divergence"),
        "expected typed prefix, got: {msg}"
    );
    assert!(msg.contains(PK_B), "extra pk must appear in message: {msg}");
}

// ----- S23d — bounty event ledger cross-check -----

const FAMILY_GAMMA: &str = "test.gamma";
const FAMILY_DELTA: &str = "test.delta";

fn write_credit_event(
    dir: &std::path::Path,
    height: u64,
    family_id: &str,
    bounty_id: &str,
    prover: &str,
    amount: &str,
) -> std::path::PathBuf {
    use serde_json::json;
    let path = dir.join("bounty-events.ndjson");
    let event = json!({
        "schemaVersion": 1,
        "kind": "credit",
        "height": height,
        "c": C_HEX,
        "familyId": family_id,
        "bountyId": bounty_id,
        "prover": prover,
        "amount": amount,
    });
    boole_core::FileBountyEventLedger::append(&path, &event).expect("append credit");
    path
}

fn fresh_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-s23d-divergence-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir
}

#[test]
fn verify_passes_when_bounty_family_credits_match_replay() {
    let ledger = seed_ledger_with_two_credits();
    let mut replay: BTreeMap<String, u128> = BTreeMap::new();
    replay.insert(PK_A.to_string(), 2);
    replay.insert(PK_B.to_string(), 1);

    let dir = fresh_dir();
    let bounty_path = write_credit_event(&dir, 0, FAMILY_GAMMA, "g-1", PK_A, "75");
    let _ = write_credit_event(&dir, 0, FAMILY_DELTA, "d-1", PK_B, "10");

    let mut replay_family: BTreeMap<String, u128> = BTreeMap::new();
    replay_family.insert(FAMILY_GAMMA.to_string(), 75);
    replay_family.insert(FAMILY_DELTA.to_string(), 10);

    verify_ledger_matches_replay(&ledger, &replay, Some(&bounty_path), &replay_family)
        .expect("matching family totals must pass");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_bails_on_per_family_credit_mismatch() {
    let ledger = seed_ledger_with_two_credits();
    let mut replay: BTreeMap<String, u128> = BTreeMap::new();
    replay.insert(PK_A.to_string(), 2);
    replay.insert(PK_B.to_string(), 1);

    let dir = fresh_dir();
    // Bounty ledger says gamma earned 75; replay (block source of truth)
    // says 100. Boot must refuse.
    let bounty_path = write_credit_event(&dir, 0, FAMILY_GAMMA, "g-1", PK_A, "75");

    let mut replay_family: BTreeMap<String, u128> = BTreeMap::new();
    replay_family.insert(FAMILY_GAMMA.to_string(), 100);

    let err = verify_ledger_matches_replay(&ledger, &replay, Some(&bounty_path), &replay_family)
        .expect_err("must bail on family mismatch");
    let msg = err.to_string();
    assert!(
        msg.contains("bounty ledger family-credit divergence"),
        "expected typed prefix, got: {msg}"
    );
    assert!(
        msg.contains(FAMILY_GAMMA),
        "diverging family must appear: {msg}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_bails_when_bounty_ledger_records_family_absent_from_replay() {
    let ledger = seed_ledger_with_two_credits();
    let mut replay: BTreeMap<String, u128> = BTreeMap::new();
    replay.insert(PK_A.to_string(), 2);
    replay.insert(PK_B.to_string(), 1);

    let dir = fresh_dir();
    // Audit log carries credit for delta but replay shows none — silent
    // audit-log forge attempt. Must fail.
    let bounty_path = write_credit_event(&dir, 0, FAMILY_DELTA, "d-1", PK_A, "5");

    let replay_family: BTreeMap<String, u128> = BTreeMap::new();

    let err = verify_ledger_matches_replay(&ledger, &replay, Some(&bounty_path), &replay_family)
        .expect_err("must bail on family in ledger but not in replay");
    let msg = err.to_string();
    assert!(
        msg.contains("bounty ledger family-credit divergence"),
        "expected typed prefix, got: {msg}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_bails_when_replay_has_family_credits_but_no_bounty_ledger_file() {
    let ledger = seed_ledger_with_two_credits();
    let mut replay: BTreeMap<String, u128> = BTreeMap::new();
    replay.insert(PK_A.to_string(), 2);
    replay.insert(PK_B.to_string(), 1);

    let dir = fresh_dir();
    let bounty_path = dir.join("bounty-events.ndjson"); // never written
    assert!(!bounty_path.exists());

    let mut replay_family: BTreeMap<String, u128> = BTreeMap::new();
    replay_family.insert(FAMILY_GAMMA.to_string(), 50);

    let err = verify_ledger_matches_replay(&ledger, &replay, Some(&bounty_path), &replay_family)
        .expect_err("must bail when audit log is missing but credits exist");
    let msg = err.to_string();
    assert!(
        msg.contains("bounty event ledger missing"),
        "expected typed prefix, got: {msg}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_passes_when_bounty_ledger_absent_and_no_credits_in_replay() {
    // Pre-S23 chain shape: no bounty ledger file, no per-family credit.
    // Must remain backward-compatible.
    let ledger = seed_ledger_with_two_credits();
    let mut replay: BTreeMap<String, u128> = BTreeMap::new();
    replay.insert(PK_A.to_string(), 2);
    replay.insert(PK_B.to_string(), 1);

    let dir = fresh_dir();
    let bounty_path = dir.join("bounty-events.ndjson"); // never written
    let replay_family: BTreeMap<String, u128> = BTreeMap::new();

    verify_ledger_matches_replay(&ledger, &replay, Some(&bounty_path), &replay_family)
        .expect("absent ledger + empty replay credits must pass");
    let _ = std::fs::remove_dir_all(&dir);
}
