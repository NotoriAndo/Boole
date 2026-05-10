//! S12 — `FileBountyEventLedger` NDJSON append + recover.
//!
//! Mirrors the `FileRewardLedger` pattern: one event per line, recovery
//! is an idempotent fold over the file. Schema is pof's `BountyEventLedger`
//! v1 (kind="proof" entry shape):
//!
//! ```json
//! {"schemaVersion":1,"kind":"proof","workId":"<id>","problemHash":"<hex32>",
//!  "verifierKind":"<kind>","ts":<ms>,"proofHash":"<hex32>","solverPk":"<hex32>",
//!  "accepted":true,"reward":"<dec>","credit":"<dec>"}
//! ```

use boole_core::FileBountyEventLedger;
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
        "boole-s12-bounty-events-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir.join("events.ndjson")
}

#[test]
fn append_then_recover_round_trips_events_in_order() {
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
    assert_eq!(recovered.len(), 2, "expected 2 events: {recovered:?}");
    assert_eq!(recovered[0], e1, "event 0 byte-equal");
    assert_eq!(recovered[1], e2, "event 1 byte-equal");

    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn recover_missing_file_returns_empty_vec() {
    let path = tmp_path();
    // Do NOT create the file. recover must treat absent as empty (same as
    // FileRewardLedger valve).
    let recovered = FileBountyEventLedger::recover(&path).expect("recover absent");
    assert!(
        recovered.is_empty(),
        "expected empty vec when file absent: {recovered:?}"
    );
    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn append_rejects_event_missing_required_fields() {
    let path = tmp_path();
    let bad = json!({
        "schemaVersion": 1,
        "kind": "proof"
        // missing workId/problemHash/verifierKind/ts/proofHash/solverPk/accepted
    });
    let result = FileBountyEventLedger::append(&path, &bad);
    assert!(
        result.is_err(),
        "expected schema rejection on missing fields"
    );
    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn append_then_recover_round_trips_create_event_byte_equal() {
    // S13b — `kind:"create"` events carry the full `Bounty` shape under a
    // `bounty` sub-object so a restart can rebuild the dynamically-announced
    // bounty registry without any external catalog. The flat workId /
    // problemHash / verifierKind fields still travel alongside so existing
    // proof-event indexing helpers keep working uniformly.
    let path = tmp_path();
    let bounty = json!({
        "id": "new-bounty-1",
        "domain": "code.spec-template",
        "problemHash": "1111111111111111111111111111111111111111111111111111111111111111",
        "verifier": {
            "kind": "mock-accept",
            "metadata": {
                "verifierHash": "2222222222222222222222222222222222222222222222222222222222222222"
            }
        },
        "reward": "100",
        "deadline": 1900000000000_u64,
        "status": "open",
        "createdAt": 1800000300000_u64,
        "updatedAt": 1800000300000_u64,
    });
    let event = json!({
        "schemaVersion": 1,
        "kind": "create",
        "workId": "new-bounty-1",
        "problemHash": "1111111111111111111111111111111111111111111111111111111111111111",
        "verifierKind": "mock-accept",
        "ts": 1800000300000_u64,
        "announcerPk": "abab000000000000000000000000000000000000000000000000000000000000",
        "bounty": bounty,
    });
    FileBountyEventLedger::append(&path, &event).expect("append create");

    let recovered = FileBountyEventLedger::recover(&path).expect("recover");
    assert_eq!(recovered.len(), 1, "expected 1 event: {recovered:?}");
    assert_eq!(
        recovered[0], event,
        "create event must round-trip byte-equal"
    );

    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}

#[test]
fn append_rejects_create_event_when_workid_disagrees_with_embedded_bounty_id() {
    // S13b — flat `workId` is the index key for in-memory recall and the
    // embedded `bounty.id` is the durable record. The two must agree;
    // otherwise an audit log entry could be replayed under the wrong id and
    // silently corrupt the registry. Schema validation catches the mismatch
    // at append time.
    let path = tmp_path();
    let bounty = json!({
        "id": "actual-id",
        "domain": "code.spec-template",
        "problemHash": "1111111111111111111111111111111111111111111111111111111111111111",
        "verifier": {"kind": "mock-accept", "metadata": {}},
        "reward": "100",
        "deadline": 1900000000000_u64,
        "status": "open",
        "createdAt": 1800000300000_u64,
        "updatedAt": 1800000300000_u64,
    });
    let bad = json!({
        "schemaVersion": 1,
        "kind": "create",
        "workId": "lying-id",
        "problemHash": "1111111111111111111111111111111111111111111111111111111111111111",
        "verifierKind": "mock-accept",
        "ts": 1800000300000_u64,
        "bounty": bounty,
    });
    let result = FileBountyEventLedger::append(&path, &bad);
    assert!(
        result.is_err(),
        "expected schema rejection on workId/bounty.id mismatch: {result:?}"
    );
    let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
}
