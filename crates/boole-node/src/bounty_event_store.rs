use std::path::Path;

use boole_core::validate_bounty_ledger_event;
use serde_json::Value;

use crate::durability::{append_ndjson_line_durable, read_stable_prefix};

/// Node-owned NDJSON file-backed audit log for bounty events.
///
/// Core owns the bounty event contract and validation. This node runtime store
/// owns file creation, append-only persistence, and recovery from local disk.
pub struct FileBountyEventLedger;

impl FileBountyEventLedger {
    pub fn append(path: impl AsRef<Path>, event: &Value) -> anyhow::Result<()> {
        validate_bounty_ledger_event(event)
            .map_err(|err| anyhow::anyhow!("bountyEventLedger: {err}"))?;
        append_ndjson_line_durable(path.as_ref(), &serde_json::to_string(event)?)
    }

    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Vec<Value>> {
        let path = path.as_ref();
        let Some(raw) = read_stable_prefix(path)? else {
            return Ok(Vec::new());
        };
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
