use std::io::Write;

use boole_core::PersistedRewardEvent;
use boole_node::FileRewardLedger;

fn fixture_reward_event() -> PersistedRewardEvent {
    let raw = r#"{
        "height": 0,
        "c": "3ac401a345c27e266387c9f084ef1aa94a2f4fb4764edca71f1ade48c1a6ddcc",
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

// SC.4 (GAP-04) — the reward-ledger balance fold uses checked arithmetic: a
// would-be u128 overflow is a typed boundary error, never a silent wrap
// (release) or panic (debug). `apply` is the ledger fold; `recover` is the
// boot/reorg path that replays the same fold over the on-disk events.
fn credit_event(height: u64, pk: &str, amount: &str) -> PersistedRewardEvent {
    let raw = serde_json::json!({
        "height": height,
        "c": "3ac401a345c27e266387c9f084ef1aa94a2f4fb4764edca71f1ade48c1a6ddcc",
        "credits": [{ "pk": pk, "amount": amount }],
    });
    serde_json::from_value(raw).expect("credit event parses")
}

const OVERFLOW_PK: &str = "2222222222222222222222222222222222222222222222222222222222222222";

#[test]
fn reward_ledger_apply_returns_typed_error_on_balance_overflow() {
    let dir = std::env::temp_dir().join(format!(
        "boole-node-reward-apply-overflow-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("empty.ndjson");

    let mut ledger = FileRewardLedger::recover(&path).expect("empty ledger");
    ledger
        .apply(credit_event(0, OVERFLOW_PK, &u128::MAX.to_string()))
        .expect("crediting u128::MAX is fine on its own");
    let err = ledger
        .apply(credit_event(1, OVERFLOW_PK, "1"))
        .expect_err("crediting past u128::MAX must be a typed error, not a wrap/panic");
    assert!(
        err.to_string().contains("consensus accounting overflow"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reward_ledger_recover_returns_typed_error_on_balance_overflow() {
    // The boot path (recover replays the on-disk events through the same fold):
    // two events that overflow the same pk must fail recovery with the typed
    // error, so a corrupt/adversarial ledger can never boot with a wrapped
    // balance.
    let dir = std::env::temp_dir().join(format!(
        "boole-node-reward-recover-overflow-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("rewardledger.ndjson");

    FileRewardLedger::append(&path, &credit_event(0, OVERFLOW_PK, &u128::MAX.to_string()))
        .expect("append max credit");
    FileRewardLedger::append(&path, &credit_event(1, OVERFLOW_PK, "1"))
        .expect("append overflowing credit");

    let err = FileRewardLedger::recover(&path)
        .expect_err("recovering an overflowing ledger must be a typed error");
    assert!(
        err.to_string().contains("consensus accounting overflow"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
