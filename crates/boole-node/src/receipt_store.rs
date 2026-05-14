use std::collections::BTreeMap;
use std::path::Path;

use boole_core::{agent_passport_events_for_receipt, AgentPassportEvent, Hex32, ReceiptCommitment};

use crate::durability::{append_ndjson_line_durable, read_stable_prefix};

#[derive(Debug, Default)]
pub struct FileReceiptStore {
    receipts: BTreeMap<String, ReceiptCommitment>,
}

impl FileReceiptStore {
    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let Some(raw) = read_stable_prefix(path)? else {
            return Ok(Self::default());
        };
        let mut store = Self::default();
        for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
            let receipt: ReceiptCommitment = serde_json::from_str(line).map_err(|err| {
                anyhow::anyhow!("receiptStore: line {} invalid JSON: {}", i + 1, err)
            })?;
            store
                .apply(receipt)
                .map_err(|err| anyhow::anyhow!("receiptStore: line {} invalid: {}", i + 1, err))?;
        }
        Ok(store)
    }

    pub fn append(path: impl AsRef<Path>, receipt: &ReceiptCommitment) -> anyhow::Result<()> {
        validate_receipt(receipt)?;
        append_ndjson_line_durable(path.as_ref(), &serde_json::to_string(receipt)?)
    }

    pub fn apply(&mut self, receipt: ReceiptCommitment) -> anyhow::Result<()> {
        validate_receipt(&receipt)?;
        self.receipts.insert(receipt.receipt_id.clone(), receipt);
        Ok(())
    }

    pub fn get(&self, receipt_id: &str) -> Option<&ReceiptCommitment> {
        self.receipts.get(receipt_id)
    }

    pub fn size(&self) -> usize {
        self.receipts.len()
    }

    pub fn agent_events(&self) -> Vec<AgentPassportEvent> {
        self.receipts
            .values()
            .flat_map(agent_passport_events_for_receipt)
            .collect()
    }
}

fn validate_receipt(receipt: &ReceiptCommitment) -> anyhow::Result<()> {
    validate_hex32("receiptId", &receipt.receipt_id)?;
    validate_hex32("agentPk", &receipt.agent_pk)?;
    validate_hex32("artifactHash", &receipt.artifact_hash)?;
    validate_hex32("requestHash", &receipt.request_hash)?;
    validate_hex32("rewardRecipient", &receipt.reward_recipient)?;
    if receipt.receipt_id != receipt.compute_id() {
        anyhow::bail!(
            "receiptId mismatch: got {}, expected {}",
            receipt.receipt_id,
            receipt.compute_id()
        );
    }
    Ok(())
}

fn validate_hex32(field: &str, value: &str) -> anyhow::Result<()> {
    Hex32::from_hex(value).map_err(|err| anyhow::anyhow!("{field} invalid: {err}"))?;
    Ok(())
}
