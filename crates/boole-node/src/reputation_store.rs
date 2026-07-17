use std::collections::BTreeMap;
use std::path::Path;

use boole_core::Hex32;
use serde::{Deserialize, Serialize};

use crate::durability::{append_ndjson_line_durable, read_stable_prefix};

pub const REPUTATION_EVENT_SCHEMA: &str = "boole.reputation.event.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedReputationEvent {
    pub schema: String,
    pub agent_pk: String,
    pub accepted_submits: u64,
    pub verified_reward_amount: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReputationStats {
    pub agent_pk: String,
    pub accepted_submits: u64,
    pub verified_reward_amount: String,
    pub event_count: u64,
}

#[derive(Debug, Default)]
pub struct FileReputationLedger {
    events: Vec<PersistedReputationEvent>,
    stats: BTreeMap<String, ReputationAccumulator>,
}

#[derive(Debug, Default, Clone, Copy)]
struct ReputationAccumulator {
    accepted_submits: u64,
    verified_reward_amount: u128,
    event_count: u64,
}

impl FileReputationLedger {
    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let Some(raw) = read_stable_prefix(path)? else {
            return Ok(Self::default());
        };
        let mut ledger = Self::default();
        for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
            let event: PersistedReputationEvent = serde_json::from_str(line).map_err(|err| {
                anyhow::anyhow!("reputationLedger: line {} invalid JSON: {}", i + 1, err)
            })?;
            ledger.apply(event).map_err(|err| {
                anyhow::anyhow!("reputationLedger: line {} invalid: {}", i + 1, err)
            })?;
        }
        Ok(ledger)
    }

    pub fn append(path: impl AsRef<Path>, event: &PersistedReputationEvent) -> anyhow::Result<()> {
        validate_event(event)?;
        append_ndjson_line_durable(path.as_ref(), &serde_json::to_string(event)?)
    }

    pub fn apply(&mut self, event: PersistedReputationEvent) -> anyhow::Result<()> {
        validate_event(&event)?;
        let amount: u128 = event.verified_reward_amount.parse()?;
        let entry = self.stats.entry(event.agent_pk.clone()).or_default();
        entry.accepted_submits = entry
            .accepted_submits
            .checked_add(event.accepted_submits)
            .ok_or_else(|| anyhow::anyhow!("reputation accepted_submits overflow"))?;
        entry.verified_reward_amount = entry
            .verified_reward_amount
            .checked_add(amount)
            .ok_or_else(|| anyhow::anyhow!("reputation verified_reward_amount overflow"))?;
        entry.event_count = entry
            .event_count
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("reputation event_count overflow"))?;
        self.events.push(event);
        Ok(())
    }

    pub fn stats_for(&self, agent_pk: &str) -> ReputationStats {
        let stats = self.stats.get(agent_pk).copied().unwrap_or_default();
        ReputationStats {
            agent_pk: agent_pk.to_string(),
            accepted_submits: stats.accepted_submits,
            verified_reward_amount: stats.verified_reward_amount.to_string(),
            event_count: stats.event_count,
        }
    }

    pub fn events(&self) -> &[PersistedReputationEvent] {
        &self.events
    }

    pub fn size(&self) -> usize {
        self.events.len()
    }
}

fn validate_event(event: &PersistedReputationEvent) -> anyhow::Result<()> {
    if event.schema != REPUTATION_EVENT_SCHEMA {
        anyhow::bail!(
            "schema mismatch: got {}, expected {}",
            event.schema,
            REPUTATION_EVENT_SCHEMA
        );
    }
    Hex32::from_hex(&event.agent_pk).map_err(|err| anyhow::anyhow!("agentPk invalid: {err}"))?;
    if event.accepted_submits == 0 {
        anyhow::bail!("acceptedSubmits must be positive");
    }
    let amount: u128 = event.verified_reward_amount.parse()?;
    if amount == 0 {
        anyhow::bail!("verifiedRewardAmount must be positive");
    }
    if event.source.trim().is_empty() {
        anyhow::bail!("source must not be empty");
    }
    Ok(())
}
