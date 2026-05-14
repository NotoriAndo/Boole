use std::io::Write;

use boole_core::PersistedRewardEvent;
use boole_node::FileRewardLedger;

fn fixture_reward_event() -> PersistedRewardEvent {
    let raw = r#"{
        "height": 0,
        "c": "4de4d7cc23ab12195fae90e2778deb07c8f7ebf16b3440f326680a2e3ae7750d",
        "credits": [
            {
                "pk": "1111111111111111111111111111111111111111111111111111111111111111",
                "amount": "1"
            }
        ]
    }"#;
    serde_json::from_str(raw).expect("fixture reward event parses")
}

#[test]
fn reward_ledger_recovers_and_truncates_partial_trailing_line_after_crash() {
    let event = fixture_reward_event();

    let dir = std::env::temp_dir().join(format!(
        "boole-node-reward-partial-recovery-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let reward_path = dir.join("rewardledger.ndjson");

    FileRewardLedger::append(&reward_path, &event).expect("append complete reward event");

    let stable_len = std::fs::metadata(&reward_path).expect("metadata").len();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&reward_path)
        .expect("open ledger")
        .write_all(br#"{"height":1,"c":"truncated""#)
        .expect("write partial trailing line");

    let recovered = FileRewardLedger::recover(&reward_path).expect("recover ignores torn tail");
    assert_eq!(recovered.size(), 1);
    assert_eq!(recovered.last_event(), Some(&event));
    assert_eq!(
        std::fs::metadata(&reward_path)
            .expect("metadata after recovery")
            .len(),
        stable_len,
        "recovery must truncate the torn trailing line so restart is idempotent"
    );

    let recovered_again =
        FileRewardLedger::recover(&reward_path).expect("second recover stays clean");
    assert_eq!(recovered_again.size(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reward_ledger_rejects_complete_corrupt_line_instead_of_truncating_history() {
    let event = fixture_reward_event();

    let dir = std::env::temp_dir().join(format!(
        "boole-node-reward-complete-corrupt-line-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let reward_path = dir.join("rewardledger.ndjson");

    FileRewardLedger::append(&reward_path, &event).expect("append complete reward event");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&reward_path)
        .expect("open ledger")
        .write_all(b"not-json\n")
        .expect("write complete corrupt line");

    let err =
        FileRewardLedger::recover(&reward_path).expect_err("complete corrupt lines must fail");
    assert!(
        err.to_string().contains("invalid JSON"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
