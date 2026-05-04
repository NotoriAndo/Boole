use std::collections::BTreeMap;

use boole_core::{compute_block_credits, replay_blocks, PersistedBlock, PersistedRewardEvent};
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
    balances: BTreeMap<String, String>,
}

#[test]
fn replay_matches_typescript_golden_fixture() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");

    for (block, event) in fixture.blocks.iter().zip(fixture.reward_events.iter()) {
        let credits = compute_block_credits(&block.proposer_pk, &block.selected_share_pks)
            .expect("credits compute");
        assert_eq!(
            credits, event.credits,
            "reward event height {}",
            event.height
        );
        assert_eq!(block.height, event.height);
        assert_eq!(block.c, event.c);
    }

    let replay = replay_blocks(&fixture.blocks).expect("replay passes");
    assert_eq!(replay.latest_c, fixture.expected.latest_c);
    assert_eq!(replay.height, fixture.expected.height);
    let got_balances = replay
        .balances
        .into_iter()
        .map(|(pk, amount)| (pk, amount.to_string()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(got_balances, fixture.expected.balances);
}
