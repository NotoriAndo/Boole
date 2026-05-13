use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{compute_block_reward_credits, Hex32, PersistedBlock};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitReceipt {
    pub schema: String,
    pub accepted: bool,
    pub route: String,
    pub session_pk: String,
    pub submitted_by: String,
    pub nonce: String,
    pub request_hash: String,
    pub block_height: u64,
    pub block_c: String,
    pub share_hash: String,
    pub proposer_pk: String,
    pub reward_recipient: String,
    pub reward_amount: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitReceiptAuditReport {
    pub ok: bool,
    pub blocks_checked: u64,
    pub receipts_checked: u64,
}

pub fn audit_submit_receipts(
    blocks: &[PersistedBlock],
    receipts: &[SubmitReceipt],
) -> anyhow::Result<SubmitReceiptAuditReport> {
    let blocks_by_height: BTreeMap<u64, &PersistedBlock> =
        blocks.iter().map(|block| (block.height, block)).collect();

    for (idx, receipt) in receipts.iter().enumerate() {
        validate_receipt_shape(idx, receipt)?;
        let block = blocks_by_height.get(&receipt.block_height).ok_or_else(|| {
            anyhow::anyhow!(
                "receipt {} blockHeight {} not found in replayed blocks",
                idx,
                receipt.block_height
            )
        })?;
        if block.c != receipt.block_c {
            anyhow::bail!(
                "receipt {} blockC mismatch: got {}, expected {}",
                idx,
                receipt.block_c,
                block.c
            );
        }
        if block.proposer_pk != receipt.proposer_pk {
            anyhow::bail!(
                "receipt {} proposerPk mismatch: got {}, expected {}",
                idx,
                receipt.proposer_pk,
                block.proposer_pk
            );
        }
        if !block.selected_share_hashes.contains(&receipt.share_hash) {
            anyhow::bail!(
                "receipt {} shareHash not selected in block {}: {}",
                idx,
                receipt.block_height,
                receipt.share_hash
            );
        }
        let expected_amount = compute_block_reward_credits(block)?
            .into_iter()
            .find(|credit| credit.pk == receipt.reward_recipient)
            .map(|credit| credit.amount)
            .unwrap_or_else(|| "0".to_string());
        if receipt.reward_amount != expected_amount {
            anyhow::bail!(
                "receipt {} rewardAmount mismatch for rewardRecipient {}: got {}, expected {}",
                idx,
                receipt.reward_recipient,
                receipt.reward_amount,
                expected_amount
            );
        }
    }

    Ok(SubmitReceiptAuditReport {
        ok: true,
        blocks_checked: blocks.len() as u64,
        receipts_checked: receipts.len() as u64,
    })
}

fn validate_receipt_shape(idx: usize, receipt: &SubmitReceipt) -> anyhow::Result<()> {
    if receipt.schema != "boole.submit.receipt.v1" {
        anyhow::bail!(
            "receipt {} schema mismatch: got {}, expected boole.submit.receipt.v1",
            idx,
            receipt.schema
        );
    }
    if !receipt.accepted {
        anyhow::bail!("receipt {} must be accepted=true", idx);
    }
    if receipt.route != "/submit" {
        anyhow::bail!("receipt {} route mismatch: got {}", idx, receipt.route);
    }
    Hex32::from_hex(&receipt.session_pk)?;
    Hex32::from_hex(&receipt.submitted_by)?;
    Hex32::from_hex(&receipt.request_hash)?;
    Hex32::from_hex(&receipt.block_c)?;
    Hex32::from_hex(&receipt.share_hash)?;
    Hex32::from_hex(&receipt.proposer_pk)?;
    Hex32::from_hex(&receipt.reward_recipient)?;
    let _: u128 = receipt.reward_amount.parse()?;
    Ok(())
}
