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
    /// Per-pk balances. Folds in BOTH base-lane proposer/share credits
    /// (one unit per credit row) AND bounty-lane promoted credits
    /// (`PromotedBountyCredit.amount` for each row in the block).
    pub balances: BTreeMap<String, u128>,
    /// S23b — per-family bounty credit totals across all replayed
    /// blocks. Empty for chains with no promoted credits.
    /// `verify_ledger_matches_replay` cross-checks this against the
    /// node runtime store.
    pub bounty_credit_by_family: BTreeMap<String, u128>,
}

pub fn compute_block_credits(
    proposer_pk: &str,
    share_owners: &[String],
) -> anyhow::Result<Vec<PersistedCredit>> {
    compute_block_credits_for_reward_pks(proposer_pk, share_owners)
}

pub fn compute_block_credits_for_reward_pks(
    proposer_reward_pk: &str,
    share_reward_pks: &[String],
) -> anyhow::Result<Vec<PersistedCredit>> {
    if share_reward_pks.is_empty() {
        anyhow::bail!("computeBlockCredits: share owners list must not be empty");
    }
    Hex32::from_hex(proposer_reward_pk)?;
    let mut totals: BTreeMap<String, u128> = BTreeMap::new();
    for pk in share_reward_pks {
        Hex32::from_hex(pk)?;
        *totals.entry(pk.clone()).or_insert(0) += 1;
    }
    *totals.entry(proposer_reward_pk.to_string()).or_insert(0) += 1;
    Ok(totals
        .into_iter()
        .map(|(pk, amount)| PersistedCredit {
            pk,
            amount: amount.to_string(),
        })
        .collect())
}

pub fn compute_block_reward_credits(
    block: &PersistedBlock,
) -> anyhow::Result<Vec<PersistedCredit>> {
    let proposer_reward_pk = if block.proposer_reward_pk.is_empty() {
        block.proposer_pk.as_str()
    } else {
        block.proposer_reward_pk.as_str()
    };
    let share_reward_pks = if block.selected_share_reward_pks.is_empty() {
        &block.selected_share_pks
    } else {
        &block.selected_share_reward_pks
    };
    compute_block_credits_for_reward_pks(proposer_reward_pk, share_reward_pks)
}

/// N1.3 (G2) — retarget-aware replay. Folds
/// `validate_retargeted_difficulty` (every persisted block's `t_block` /
/// `difficulty_epoch` / `difficulty_weight` must match what the policy
/// computes for its height) ahead of the standard share-evidence + linkage
/// replay. Any path that claims `difficultyMode = epoch-retarget-v0` MUST
/// use this; `replay_blocks` stays the non-retarget-aware entry point.
pub fn replay_blocks_with_retarget(
    blocks: &[PersistedBlock],
    initial_t_block: &str,
    policy: &crate::DifficultyRetargetPolicy,
) -> anyhow::Result<ReplayResult> {
    crate::validate_retargeted_difficulty(blocks, initial_t_block, policy)?;
    replay_blocks(blocks)
}

pub fn replay_blocks(blocks: &[PersistedBlock]) -> anyhow::Result<ReplayResult> {
    let mut latest_c =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    let mut balances: BTreeMap<String, u128> = BTreeMap::new();
    let mut bounty_credit_by_family: BTreeMap<String, u128> = BTreeMap::new();

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

        for credit in compute_block_reward_credits(block)? {
            let amount: u128 = credit.amount.parse()?;
            *balances.entry(credit.pk).or_insert(0) += amount;
        }
        for credit in &block.promoted_bounty_credits {
            let amount: u128 = credit.amount.parse()?;
            *balances.entry(credit.prover.clone()).or_insert(0) += amount;
            *bounty_credit_by_family
                .entry(credit.family_id.clone())
                .or_insert(0) += amount;
        }
        latest_c = block.c.clone();
    }

    Ok(ReplayResult {
        latest_c,
        height: blocks.len() as u64,
        balances,
        bounty_credit_by_family,
    })
}
