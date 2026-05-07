use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::block::PersistedBlock;
use crate::replay_evidence::verify_selected_share_evidence;
use crate::{block_hash, Hex32};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedCredit {
    pub pk: String,
    pub amount: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedRewardEvent {
    pub height: u64,
    pub c: String,
    pub credits: Vec<PersistedCredit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayResult {
    pub latest_c: String,
    pub height: u64,
    pub balances: BTreeMap<String, u128>,
}

pub fn compute_block_credits(
    proposer_pk: &str,
    share_owners: &[String],
) -> anyhow::Result<Vec<PersistedCredit>> {
    if share_owners.is_empty() {
        anyhow::bail!("computeBlockCredits: share owners list must not be empty");
    }
    Hex32::from_hex(proposer_pk)?;
    let mut totals: BTreeMap<String, u128> = BTreeMap::new();
    for pk in share_owners {
        Hex32::from_hex(pk)?;
        *totals.entry(pk.clone()).or_insert(0) += 1;
    }
    *totals.entry(proposer_pk.to_string()).or_insert(0) += 1;
    Ok(totals
        .into_iter()
        .map(|(pk, amount)| PersistedCredit {
            pk,
            amount: amount.to_string(),
        })
        .collect())
}

pub fn replay_blocks(blocks: &[PersistedBlock]) -> anyhow::Result<ReplayResult> {
    let mut latest_c =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    let mut balances: BTreeMap<String, u128> = BTreeMap::new();

    for (expected_height, block) in blocks.iter().enumerate() {
        block.validate_shape()?;
        if block.height != expected_height as u64 {
            anyhow::bail!(
                "block height mismatch: got {}, expected {}",
                block.height,
                expected_height
            );
        }
        if block.prev_c != latest_c {
            anyhow::bail!(
                "prevC mismatch: block prevC={} but replay head={}",
                block.prev_c,
                latest_c
            );
        }
        let prev = Hex32::from_hex(&block.prev_c)?;
        let share_hashes = block
            .selected_share_hashes
            .iter()
            .map(|h| Hex32::from_hex(h))
            .collect::<Result<Vec<_>, _>>()?;
        let expected_c = block_hash(&prev, &share_hashes).to_hex();
        if block.c != expected_c {
            anyhow::bail!("block c mismatch: got {}, expected {}", block.c, expected_c);
        }
        verify_selected_share_evidence(block)?;

        for credit in compute_block_credits(&block.proposer_pk, &block.selected_share_pks)? {
            let amount: u128 = credit.amount.parse()?;
            *balances.entry(credit.pk).or_insert(0) += amount;
        }
        latest_c = block.c.clone();
    }

    Ok(ReplayResult {
        latest_c,
        height: blocks.len() as u64,
        balances,
    })
}
