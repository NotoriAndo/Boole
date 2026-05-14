use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
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
        let raw_bytes = fs::read(path)?;
        let stable_len = stable_jsonl_prefix_len(&raw_bytes);
        if stable_len < raw_bytes.len() {
            OpenOptions::new()
                .write(true)
                .open(path)?
                .set_len(stable_len as u64)?;
        }
        let raw = String::from_utf8(raw_bytes[..stable_len].to_vec())?;
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
        let path = path.as_ref();
        let is_new_file = !path.exists();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{}", serde_json::to_string(receipt)?)?;
        file.flush()?;
        file.sync_all()?;
        if is_new_file {
            fsync_parent_dir(path)?;
        }
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

fn stable_jsonl_prefix_len(bytes: &[u8]) -> usize {
    if bytes.is_empty() || bytes.last() == Some(&b'\n') {
        return bytes.len();
    }
    bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .unwrap_or(0)
}

#[cfg(unix)]
fn fsync_parent_dir(path: &Path) -> anyhow::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let dir_file = File::open(dir)?;
    dir_file.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn fsync_parent_dir(_path: &Path) -> anyhow::Result<()> {
    Ok(())
}
