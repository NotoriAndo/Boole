use std::io::Write;

use boole_core::{PersistedBlock, PersistedRewardEvent};
use boole_node::block_store::FileBlockStore;
use boole_node::reward_store::FileRewardLedger;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    blocks: Vec<PersistedBlock>,
    #[serde(rename = "rewardEvents")]
    reward_events: Vec<PersistedRewardEvent>,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    latest_c: String,
    height: u64,
    balances: std::collections::BTreeMap<String, String>,
}

#[test]
fn stores_recover_typescript_fixture_lines() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");

    let dir = std::env::temp_dir().join(format!("boole-node-store-fixture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blockstore.ndjson");
    let reward_path = dir.join("rewardledger.ndjson");

    for block in &fixture.blocks {
        FileBlockStore::append(&block_path, block).expect("append block");
    }
    for event in &fixture.reward_events {
        FileRewardLedger::append(&reward_path, event).expect("append reward event");
    }

    let block_store = FileBlockStore::recover(&block_path).expect("recover block store");
    let reward_ledger = FileRewardLedger::recover(&reward_path).expect("recover reward ledger");

    assert_eq!(block_store.size() as u64, fixture.expected.height);
    assert_eq!(
        block_store.latest().expect("latest").c,
        fixture.expected.latest_c
    );
    let got_balances = reward_ledger
        .balances()
        .iter()
        .map(|(pk, amount)| (pk.clone(), amount.to_string()))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(got_balances, fixture.expected.balances);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn block_store_recovers_and_truncates_partial_trailing_line_after_crash() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");
    let first = fixture.blocks.first().expect("fixture has block");

    let dir = std::env::temp_dir().join(format!(
        "boole-node-blockstore-partial-recovery-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blockstore.ndjson");
    FileBlockStore::append(&block_path, first).expect("append complete block");

    let stable_len = std::fs::metadata(&block_path).expect("metadata").len();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&block_path)
        .expect("open blockstore")
        .write_all(br#"{"height":1,"prevC":"truncated""#)
        .expect("write partial trailing line");

    let recovered = FileBlockStore::recover(&block_path).expect("recover ignores torn tail");
    assert_eq!(recovered.size(), 1);
    assert_eq!(recovered.latest(), Some(first));
    assert_eq!(
        std::fs::metadata(&block_path)
            .expect("metadata after recovery")
            .len(),
        stable_len,
        "recovery must truncate the torn trailing line so restart is idempotent"
    );

    let recovered_again = FileBlockStore::recover(&block_path).expect("second recover stays clean");
    assert_eq!(recovered_again.size(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn block_store_rejects_complete_corrupt_line_instead_of_truncating_history() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");
    let first = fixture.blocks.first().expect("fixture has block");

    let dir = std::env::temp_dir().join(format!(
        "boole-node-blockstore-complete-corrupt-line-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blockstore.ndjson");
    FileBlockStore::append(&block_path, first).expect("append complete block");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&block_path)
        .expect("open blockstore")
        .write_all(b"not-json\n")
        .expect("write complete corrupt line");

    let err = FileBlockStore::recover(&block_path).expect_err("complete corrupt lines must fail");
    assert!(
        err.to_string().contains("invalid JSON"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
