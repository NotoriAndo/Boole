use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use boole_core::validate_bounty_ledger_event;
use serde_json::Value;

/// Node-owned NDJSON file-backed audit log for bounty events.
///
/// Core owns the bounty event contract and validation. This node runtime store
/// owns file creation, append-only persistence, and recovery from local disk.
pub struct FileBountyEventLedger;

impl FileBountyEventLedger {
    pub fn append(path: impl AsRef<Path>, event: &Value) -> anyhow::Result<()> {
        validate_bounty_ledger_event(event)
            .map_err(|err| anyhow::anyhow!("bountyEventLedger: {err}"))?;
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.as_ref())?;
        writeln!(file, "{}", serde_json::to_string(event)?)?;
        Ok(())
    }

    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Vec<Value>> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(path)?;
        let mut events = Vec::new();
        for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
            let event: Value = serde_json::from_str(line).map_err(|err| {
                anyhow::anyhow!("bountyEventLedger: line {} invalid JSON: {}", i + 1, err)
            })?;
            validate_bounty_ledger_event(&event).map_err(|err| {
                anyhow::anyhow!("bountyEventLedger: line {} schema invalid: {}", i + 1, err)
            })?;
            events.push(event);
        }
        Ok(events)
    }
}
