use std::path::Path;

use boole_core::validate_bounty_ledger_event;
use serde_json::Value;

use crate::durability::{
    append_ndjson_line_durable, read_stable_prefix, write_ndjson_lines_atomic,
};

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

    /// N4 — atomically replace the WHOLE ledger with `events` (each validated
    /// first), via a temp-write + rename so a crash cannot leave a partially
    /// rewritten file. Used by the reorg rebuild to swap the block-driven rows
    /// for the newly-adopted chain's while preserving the route-driven rows;
    /// `append` (single-line durable append) cannot express a rewrite.
    pub fn rewrite_atomic(path: impl AsRef<Path>, events: &[Value]) -> anyhow::Result<()> {
        let mut lines = Vec::with_capacity(events.len());
        for event in events {
            validate_bounty_ledger_event(event)
                .map_err(|err| anyhow::anyhow!("bountyEventLedger: {err}"))?;
            lines.push(serde_json::to_string(event)?);
        }
        write_ndjson_lines_atomic(path.as_ref(), &lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const HEX32: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const PK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn rewrite_atomic_replaces_file_and_round_trips() {
        let dir = std::env::temp_dir().join(format!("boole-bounty-rewrite-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("bounty-events.ndjson");

        // Seed one row via append, then wholesale-replace with two different rows.
        let old = json!({
            "schemaVersion": 1, "kind": "create", "workId": "old",
            "problemHash": HEX32, "verifierKind": "lean", "ts": 1
        });
        FileBountyEventLedger::append(&path, &old).expect("seed append");

        let a = json!({
            "schemaVersion": 1, "kind": "create", "workId": "a",
            "problemHash": HEX32, "verifierKind": "lean", "ts": 2
        });
        let b = json!({
            "schemaVersion": 1, "kind": "credit", "height": 3, "c": HEX32,
            "familyId": "fam.a", "bountyId": "a", "prover": PK, "amount": "5"
        });
        FileBountyEventLedger::rewrite_atomic(&path, &[a.clone(), b.clone()])
            .expect("rewrite atomic");

        let recovered = FileBountyEventLedger::recover(&path).expect("recover after rewrite");
        assert_eq!(recovered, vec![a, b], "old row gone, new rows in order");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rewrite_atomic_rejects_invalid_event_and_writes_nothing() {
        let dir =
            std::env::temp_dir().join(format!("boole-bounty-rewrite-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("bounty-events.ndjson");

        // A credit row missing every required field must be rejected up front.
        let bad = json!({ "schemaVersion": 1, "kind": "credit" });
        let err = FileBountyEventLedger::rewrite_atomic(&path, std::slice::from_ref(&bad))
            .expect_err("invalid event must reject");
        assert!(
            err.to_string().contains("bountyEventLedger"),
            "unexpected error: {err}"
        );
        assert!(!path.exists(), "no file written when validation fails");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
