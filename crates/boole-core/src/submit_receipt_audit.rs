use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    canonical_payload_hash_hex, compute_block_reward_credits, Hex32, PersistedBlock,
    PersistedCredit, SignedEnvelope, SIGNED_ENVELOPE_SCHEMA,
};

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
    pub evidence: SubmitReceiptAuditEvidence,
    pub settlement: SubmitReceiptSettlementReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitReceiptSettlementReport {
    pub reward_credits: Vec<PersistedCredit>,
    pub reputation_deltas: Vec<SubmitReceiptReputationDelta>,
    pub checks: SubmitReceiptSettlementChecks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitReceiptReputationDelta {
    pub agent_pk: String,
    pub accepted_submits: u64,
    pub verified_reward_amount: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitReceiptSettlementChecks {
    pub reward_credits_replay_bound: bool,
    pub reputation_bound_to_submitted_by: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitReceiptAuditEvidence {
    pub block_heights: Vec<u64>,
    pub reward_recipients: Vec<String>,
    pub request_hashes: Vec<String>,
    pub signed_work_checked: u64,
    pub checks: SubmitReceiptAuditChecks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitReceiptAuditChecks {
    pub block_chain_continuity: bool,
    pub receipt_shape: bool,
    pub block_binding: bool,
    pub selected_share_binding: bool,
    pub reward_credit_binding: bool,
    pub signed_work_lineage: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitReceiptLineage {
    pub receipt: SubmitReceipt,
    pub signed_work: SignedEnvelope,
}

pub fn audit_submit_receipt_lineages(
    blocks: &[PersistedBlock],
    lineages: &[SubmitReceiptLineage],
) -> anyhow::Result<SubmitReceiptAuditReport> {
    for (idx, lineage) in lineages.iter().enumerate() {
        validate_signed_work_lineage(idx, lineage)?;
    }
    let receipts: Vec<SubmitReceipt> = lineages
        .iter()
        .map(|lineage| lineage.receipt.clone())
        .collect();
    audit_submit_receipts_with_signed_work_count(blocks, &receipts, lineages.len() as u64)
}

pub fn audit_submit_receipts(
    blocks: &[PersistedBlock],
    receipts: &[SubmitReceipt],
) -> anyhow::Result<SubmitReceiptAuditReport> {
    audit_submit_receipts_with_signed_work_count(blocks, receipts, 0)
}

fn audit_submit_receipts_with_signed_work_count(
    blocks: &[PersistedBlock],
    receipts: &[SubmitReceipt],
    signed_work_checked: u64,
) -> anyhow::Result<SubmitReceiptAuditReport> {
    let blocks_by_height: BTreeMap<u64, &PersistedBlock> =
        blocks.iter().map(|block| (block.height, block)).collect();
    validate_block_chain(&blocks_by_height)?;

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
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "receipt {} rewardRecipient not credited by replay: {}",
                    idx,
                    receipt.reward_recipient
                )
            })?
            .amount;
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

    let settlement = settlement_report(receipts)?;

    Ok(SubmitReceiptAuditReport {
        ok: true,
        blocks_checked: blocks.len() as u64,
        receipts_checked: receipts.len() as u64,
        evidence: SubmitReceiptAuditEvidence {
            block_heights: blocks_by_height.keys().copied().collect(),
            reward_recipients: unique_sorted(
                receipts
                    .iter()
                    .map(|receipt| receipt.reward_recipient.clone()),
            ),
            request_hashes: unique_sorted(
                receipts.iter().map(|receipt| receipt.request_hash.clone()),
            ),
            signed_work_checked,
            checks: SubmitReceiptAuditChecks {
                block_chain_continuity: true,
                receipt_shape: true,
                block_binding: true,
                selected_share_binding: true,
                reward_credit_binding: true,
                signed_work_lineage: signed_work_checked > 0,
            },
        },
        settlement,
    })
}

fn settlement_report(receipts: &[SubmitReceipt]) -> anyhow::Result<SubmitReceiptSettlementReport> {
    let mut rewards: BTreeMap<String, u128> = BTreeMap::new();
    let mut reputation: BTreeMap<String, (u64, u128)> = BTreeMap::new();
    for receipt in receipts {
        let amount: u128 = receipt.reward_amount.parse()?;
        *rewards.entry(receipt.reward_recipient.clone()).or_insert(0) += amount;
        let entry = reputation
            .entry(receipt.submitted_by.clone())
            .or_insert((0, 0));
        entry.0 += 1;
        entry.1 += amount;
    }
    Ok(SubmitReceiptSettlementReport {
        reward_credits: rewards
            .into_iter()
            .map(|(pk, amount)| PersistedCredit {
                pk,
                amount: amount.to_string(),
            })
            .collect(),
        reputation_deltas: reputation
            .into_iter()
            .map(|(agent_pk, (accepted_submits, verified_reward_amount))| {
                SubmitReceiptReputationDelta {
                    agent_pk,
                    accepted_submits,
                    verified_reward_amount: verified_reward_amount.to_string(),
                }
            })
            .collect(),
        checks: SubmitReceiptSettlementChecks {
            reward_credits_replay_bound: true,
            reputation_bound_to_submitted_by: true,
        },
    })
}

fn unique_sorted(values: impl Iterator<Item = String>) -> Vec<String> {
    values
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn validate_block_chain(blocks_by_height: &BTreeMap<u64, &PersistedBlock>) -> anyhow::Result<()> {
    for (&height, block) in blocks_by_height {
        block
            .validate_shape()
            .map_err(|err| anyhow::anyhow!("block {} shape invalid: {}", height, err))?;
        if height == 0 {
            if block.prev_c != "0000000000000000000000000000000000000000000000000000000000000000" {
                anyhow::bail!(
                    "block c mismatch at genesis height: prevC got {}",
                    block.prev_c
                );
            }
            continue;
        }
        let parent = blocks_by_height.get(&(height - 1)).ok_or_else(|| {
            anyhow::anyhow!("block c mismatch at height {}: parent missing", height)
        })?;
        if block.prev_c != parent.c {
            anyhow::bail!(
                "block c mismatch at height {}: prevC got {}, expected parent c {}",
                height,
                block.prev_c,
                parent.c
            );
        }
    }
    Ok(())
}

fn validate_signed_work_lineage(idx: usize, lineage: &SubmitReceiptLineage) -> anyhow::Result<()> {
    let receipt = &lineage.receipt;
    let signed_work = &lineage.signed_work;

    if signed_work.schema != SIGNED_ENVELOPE_SCHEMA {
        anyhow::bail!(
            "lineage {} signedWork schema mismatch: got {}, expected {}",
            idx,
            signed_work.schema,
            SIGNED_ENVELOPE_SCHEMA
        );
    }
    if signed_work.pk != receipt.submitted_by {
        anyhow::bail!(
            "lineage {} signedWork pk mismatch: got {}, expected submittedBy {}",
            idx,
            signed_work.pk,
            receipt.submitted_by
        );
    }
    if receipt.submitted_by != receipt.session_pk {
        anyhow::bail!(
            "lineage {} submittedBy/sessionPk mismatch: submittedBy {}, sessionPk {}",
            idx,
            receipt.submitted_by,
            receipt.session_pk
        );
    }
    match signed_work.verify() {
        Ok(true) => {}
        Ok(false) => anyhow::bail!("lineage {} signedWork signature invalid", idx),
        Err(err) => anyhow::bail!("lineage {} signedWork verification failed: {}", idx, err),
    }

    let payload = &signed_work.payload;
    require_payload_str(idx, payload, "schema", "boole.signer.work.v2")?;
    require_payload_str(idx, payload, "route", "/submit")?;
    // work.v2 (ADR-0015 (b)) — the signed payload must authorize exactly
    // the reward recipient the receipt credits.
    let signed_reward_recipient = payload_str(idx, payload, "rewardRecipient")?;
    if signed_reward_recipient != receipt.reward_recipient {
        anyhow::bail!(
            "lineage {} rewardRecipient mismatch: signedWork {}, receipt {}",
            idx,
            signed_reward_recipient,
            receipt.reward_recipient
        );
    }
    let nonce = payload_str(idx, payload, "nonce")?;
    if nonce != receipt.nonce {
        anyhow::bail!(
            "lineage {} nonce mismatch: signedWork {}, receipt {}",
            idx,
            nonce,
            receipt.nonce
        );
    }
    let request_hash = payload_str(idx, payload, "requestHash")?;
    if request_hash != receipt.request_hash {
        anyhow::bail!(
            "lineage {} requestHash mismatch: signedWork {}, receipt {}",
            idx,
            request_hash,
            receipt.request_hash
        );
    }
    let work_payload = payload
        .get("workPayload")
        .ok_or_else(|| anyhow::anyhow!("lineage {} missing signedWork payload.workPayload", idx))?;
    let computed_request_hash = canonical_payload_hash_hex(work_payload);
    if computed_request_hash != receipt.request_hash {
        anyhow::bail!(
            "lineage {} workPayload hash mismatch: got {}, expected {}",
            idx,
            computed_request_hash,
            receipt.request_hash
        );
    }
    Ok(())
}

fn require_payload_str(
    idx: usize,
    payload: &Value,
    field: &str,
    expected: &str,
) -> anyhow::Result<()> {
    let got = payload_str(idx, payload, field)?;
    if got != expected {
        anyhow::bail!(
            "lineage {} signedWork payload.{} mismatch: got {}, expected {}",
            idx,
            field,
            got,
            expected
        );
    }
    Ok(())
}

fn payload_str<'a>(idx: usize, payload: &'a Value, field: &str) -> anyhow::Result<&'a str> {
    payload.get(field).and_then(Value::as_str).ok_or_else(|| {
        anyhow::anyhow!(
            "lineage {} signedWork payload.{} must be a string",
            idx,
            field
        )
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
