use std::io::Write;

use boole_node::FileBountyEventLedger;
use serde_json::{json, Value};

fn fixture_event() -> Value {
    json!({
        "schemaVersion": 1,
        "kind": "create",
        "workId": "gamma-1",
        "problemHash": "1111111111111111111111111111111111111111111111111111111111111111",
        "verifierKind": "mock-accept",
        "ts": 0_i64,
    })
}

#[test]
fn bounty_event_ledger_recovers_and_truncates_partial_trailing_line_after_crash() {
    let event = fixture_event();

    let dir = std::env::temp_dir().join(format!(
        "boole-node-bounty-events-partial-recovery-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("bountyevents.ndjson");

    FileBountyEventLedger::append(&path, &event).expect("append complete event");

    let stable_len = std::fs::metadata(&path).expect("metadata").len();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open ledger")
        .write_all(br#"{"schemaVersion":1,"kind":"create","workId":"trunc"#)
        .expect("write partial trailing line");

    let recovered = FileBountyEventLedger::recover(&path).expect("recover ignores torn tail");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0]["workId"], "gamma-1");
    assert_eq!(
        std::fs::metadata(&path).expect("post-recover").len(),
        stable_len,
        "recovery must truncate the torn trailing line so restart is idempotent"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bounty_event_ledger_rejects_complete_corrupt_line_instead_of_truncating_history() {
    let event = fixture_event();

    let dir = std::env::temp_dir().join(format!(
        "boole-node-bounty-events-complete-corrupt-line-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("bountyevents.ndjson");

    FileBountyEventLedger::append(&path, &event).expect("append complete event");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open ledger")
        .write_all(b"not-json\n")
        .expect("write complete corrupt line");

    let err = FileBountyEventLedger::recover(&path).expect_err("complete corrupt lines must fail");
    assert!(
        err.to_string().contains("invalid JSON"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
