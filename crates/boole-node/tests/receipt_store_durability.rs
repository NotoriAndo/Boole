use std::io::Write;

use boole_core::{ReceiptCommitment, ReceiptCommitmentInput};
use boole_node::FileReceiptStore;

fn fixture_receipt() -> ReceiptCommitment {
    ReceiptCommitment::new(ReceiptCommitmentInput {
        agent_pk: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
        family_id: "v1-lenbound".to_string(),
        verifier_id: "lean-runner-v01".to_string(),
        verifier_hash_version: "v0".to_string(),
        artifact_hash: "2222222222222222222222222222222222222222222222222222222222222222"
            .to_string(),
        request_hash: "3333333333333333333333333333333333333333333333333333333333333333"
            .to_string(),
        result: "accepted".to_string(),
        fee_charged: "1".to_string(),
        reward_recipient: "4444444444444444444444444444444444444444444444444444444444444444"
            .to_string(),
    })
    .expect("valid fixture receipt")
}

#[test]
fn receipt_store_recovers_and_truncates_partial_trailing_line_after_crash() {
    let receipt = fixture_receipt();

    let dir = std::env::temp_dir().join(format!(
        "boole-node-receipt-partial-recovery-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("receipts.ndjson");

    FileReceiptStore::append(&path, &receipt).expect("append complete receipt");

    let stable_len = std::fs::metadata(&path).expect("metadata").len();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open receipt store")
        .write_all(br#"{"receiptId":"truncated","agentPk":"#)
        .expect("write partial trailing line");

    let recovered = FileReceiptStore::recover(&path).expect("recover ignores torn tail");
    assert_eq!(recovered.size(), 1);
    assert_eq!(recovered.get(&receipt.receipt_id), Some(&receipt));
    assert_eq!(
        std::fs::metadata(&path)
            .expect("metadata after recovery")
            .len(),
        stable_len,
        "recovery must truncate the torn trailing line so restart is idempotent"
    );

    let recovered_again = FileReceiptStore::recover(&path).expect("second recover stays clean");
    assert_eq!(recovered_again.size(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn receipt_store_rejects_complete_corrupt_line_instead_of_truncating_history() {
    let receipt = fixture_receipt();

    let dir = std::env::temp_dir().join(format!(
        "boole-node-receipt-complete-corrupt-line-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("receipts.ndjson");

    FileReceiptStore::append(&path, &receipt).expect("append complete receipt");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open receipt store")
        .write_all(b"not-json\n")
        .expect("write complete corrupt line");

    let err = FileReceiptStore::recover(&path).expect_err("complete corrupt lines must fail");
    assert!(
        err.to_string().contains("invalid JSON"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
