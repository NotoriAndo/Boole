use std::io::Write;

use boole_node::{FileReputationLedger, PersistedReputationEvent, REPUTATION_EVENT_SCHEMA};

fn fixture_event() -> PersistedReputationEvent {
    PersistedReputationEvent {
        schema: REPUTATION_EVENT_SCHEMA.to_string(),
        agent_pk: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
        accepted_submits: 1,
        verified_reward_amount: "100".to_string(),
        source: "submit".to_string(),
    }
}

#[test]
fn reputation_ledger_recovers_and_truncates_partial_trailing_line_after_crash() {
    let event = fixture_event();

    let dir = std::env::temp_dir().join(format!(
        "boole-node-reputation-partial-recovery-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("reputationledger.ndjson");

    FileReputationLedger::append(&path, &event).expect("append complete event");

    let stable_len = std::fs::metadata(&path).expect("metadata").len();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open ledger")
        .write_all(br#"{"schema":"boole.reputation.event.v1","agentPk":"trunc"#)
        .expect("write partial trailing line");

    let recovered = FileReputationLedger::recover(&path).expect("recover ignores torn tail");
    assert_eq!(recovered.size(), 1);
    assert_eq!(recovered.events()[0], event);
    assert_eq!(
        std::fs::metadata(&path).expect("post-recover").len(),
        stable_len,
        "recovery must truncate the torn trailing line so restart is idempotent"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reputation_ledger_rejects_complete_corrupt_line_instead_of_truncating_history() {
    let event = fixture_event();

    let dir = std::env::temp_dir().join(format!(
        "boole-node-reputation-complete-corrupt-line-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("reputationledger.ndjson");

    FileReputationLedger::append(&path, &event).expect("append complete event");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open ledger")
        .write_all(b"not-json\n")
        .expect("write complete corrupt line");

    let err = FileReputationLedger::recover(&path).expect_err("complete corrupt lines must fail");
    assert!(
        err.to_string().contains("invalid JSON"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
