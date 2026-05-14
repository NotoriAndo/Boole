//! D3.2 — `FileBountyEventLedger` is node-owned runtime persistence.
//!
//! Core owns the event contract and validation; node owns file-backed NDJSON
//! append/recover semantics, including credit events used during runtime boot
//! replay checks.

use boole_node::FileBountyEventLedger;
use serde_json::json;

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn tmp_path() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-d3-2-bounty-event-store-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir.join("events.ndjson")
}

#[test]
fn node_file_bounty_event_ledger_round_trips_proof_events() {
    let path = tmp_path();
    let e1 = json!({
        "schemaVersion": 1,
        "kind": "proof",
        "workId": "gamma-1",
        "problemHash": "1111111111111111111111111111111111111111111111111111111111111111",
        "verifierKind": "mock-accept",
        "ts": 1800000100000_u64,
        "proofHash": "aaaa000000000000000000000000000000000000000000000000000000000000",
        "solverPk": "bbbb000000000000000000000000000000000000000000000000000000000000",
        "accepted": true,
        "reward": "100",
        "credit": "100"
    });
    let e2 = json!({
        "schemaVersion": 1,
        "kind": "proof",
        "workId": "delta-1",
        "problemHash": "3333333333333333333333333333333333333333333333333333333333333333",
        "verifierKind": "mock-reject",
        "ts": 1800000200000_u64,
        "proofHash": "cccc000000000000000000000000000000000000000000000000000000000000",
        "solverPk": "dddd000000000000000000000000000000000000000000000000000000000000",
        "accepted": false
    });

    FileBountyEventLedger::append(&path, &e1).expect("append e1");
    FileBountyEventLedger::append(&path, &e2).expect("append e2");

    let recovered = FileBountyEventLedger::recover(&path).expect("recover");
    assert_eq!(recovered, vec![e1, e2]);

    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn node_file_bounty_event_ledger_treats_missing_file_as_empty() {
    let path = tmp_path();
    let recovered = FileBountyEventLedger::recover(&path).expect("recover absent");
    assert!(recovered.is_empty(), "expected empty vec: {recovered:?}");
    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn node_file_bounty_event_ledger_rejects_invalid_event_before_write() {
    let path = tmp_path();
    let bad = json!({
        "schemaVersion": 1,
        "kind": "proof"
    });

    let result = FileBountyEventLedger::append(&path, &bad);
    assert!(result.is_err(), "expected schema rejection");
    assert!(
        !path.exists(),
        "invalid append must not create the ledger file"
    );

    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn node_file_bounty_event_ledger_accepts_credit_events_without_work_id() {
    let path = tmp_path();
    let credit = json!({
        "schemaVersion": 1,
        "kind": "credit",
        "height": 7,
        "c": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "familyId": "boole.family.v1.lenbound",
        "bountyId": "bounty-1",
        "prover": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "amount": "42"
    });

    FileBountyEventLedger::append(&path, &credit).expect("append credit");
    let recovered = FileBountyEventLedger::recover(&path).expect("recover");
    assert_eq!(recovered, vec![credit]);

    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}
