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
