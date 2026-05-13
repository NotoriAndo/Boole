use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use boole_core::{agent_passport_events_for_receipt, AgentPassportEvent, Hex32, ReceiptCommitment};

#[derive(Debug, Default)]
pub struct FileReceiptStore {
    receipts: BTreeMap<String, ReceiptCommitment>,
}

impl FileReceiptStore {
    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path)?;
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
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{}", serde_json::to_string(receipt)?)?;
        Ok(())
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
