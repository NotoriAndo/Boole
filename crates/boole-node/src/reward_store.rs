use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use boole_core::{FileBountyEventLedger, PersistedCredit, PersistedRewardEvent};

#[derive(Debug, Default)]
pub struct FileRewardLedger {
    events: Vec<PersistedRewardEvent>,
    balances: BTreeMap<String, u128>,
}

/// Compare an in-memory `FileRewardLedger` against the balance map produced
/// by replaying the on-disk block log. Returns the first divergent pk as a
/// typed error so the runtime boot path can refuse to come up under
/// inconsistent state.
///
/// S23d — when `bounty_event_ledger_path` is supplied, also walk the
/// bounty event ledger and verify per-family credit totals match the
/// replay engine's `bounty_credit_by_family`. The bounty ledger is the
/// audit log; the reward ledger is the balance source of truth; replay
/// is the deterministic recomputation. Any pair diverging fails boot.
pub fn verify_ledger_matches_replay(
    ledger: &FileRewardLedger,
    replay_balances: &BTreeMap<String, u128>,
    bounty_event_ledger_path: Option<&Path>,
    replay_bounty_credit_by_family: &BTreeMap<String, u128>,
) -> anyhow::Result<()> {
    let ledger_balances = ledger.balances();
    for (pk, replay_amount) in replay_balances {
        let ledger_amount = ledger_balances.get(pk).copied().unwrap_or(0);
        if ledger_amount != *replay_amount {
            anyhow::bail!(
                "reward ledger divergence at pk {}: ledger={} replay={}",
                pk,
                ledger_amount,
                replay_amount
            );
        }
    }
    for (pk, ledger_amount) in ledger_balances {
        if !replay_balances.contains_key(pk) {
            anyhow::bail!(
                "reward ledger divergence at pk {}: ledger={} replay=0",
                pk,
                ledger_amount
            );
        }
    }

    if let Some(path) = bounty_event_ledger_path {
        if !path.exists() {
            // No bounty ledger written yet → replay's per-family map must
            // also be empty; otherwise we have credits from blocks with no
            // matching audit trail.
            if !replay_bounty_credit_by_family.is_empty() {
                anyhow::bail!(
                    "bounty event ledger missing but replay has {} family credit entries",
                    replay_bounty_credit_by_family.len()
                );
            }
            return Ok(());
        }
        let events = FileBountyEventLedger::recover(path)?;
        let mut ledger_credit_by_family: BTreeMap<String, u128> = BTreeMap::new();
        for event in &events {
            let kind = event.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if kind != "credit" {
                continue;
            }
            let family_id = event
                .get("familyId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("bountyLedger credit event missing familyId"))?;
            let amount: u128 = event
                .get("amount")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("bountyLedger credit event missing amount"))?
                .parse()?;
            *ledger_credit_by_family
                .entry(family_id.to_string())
                .or_insert(0) += amount;
        }
        for (family_id, replay_amount) in replay_bounty_credit_by_family {
            let ledger_amount = ledger_credit_by_family.get(family_id).copied().unwrap_or(0);
            if ledger_amount != *replay_amount {
                anyhow::bail!(
                    "bounty ledger family-credit divergence at {}: ledger={} replay={}",
                    family_id,
                    ledger_amount,
                    replay_amount
                );
            }
        }
        for (family_id, ledger_amount) in &ledger_credit_by_family {
            if !replay_bounty_credit_by_family.contains_key(family_id) {
                anyhow::bail!(
                    "bounty ledger family-credit divergence at {}: ledger={} replay=0",
                    family_id,
                    ledger_amount
                );
            }
        }
    }
    Ok(())
}

impl FileRewardLedger {
    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path)?;
        let mut ledger = Self::default();
        for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
            let event: PersistedRewardEvent = serde_json::from_str(line).map_err(|err| {
                anyhow::anyhow!("rewardLedger: line {} invalid JSON: {}", i + 1, err)
            })?;
            if event.height != i as u64 {
                anyhow::bail!(
                    "rewardLedger: line {} has height {}, expected {}",
                    i + 1,
                    event.height,
                    i
                );
            }
            ledger.apply(event)?;
        }
        Ok(ledger)
    }

    pub fn append(path: impl AsRef<Path>, event: &PersistedRewardEvent) -> anyhow::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{}", serde_json::to_string(event)?)?;
        Ok(())
    }

    /// Apply an event against the in-memory state. Used both during recover
    /// and after a successful disk append so callers don't have to re-read
    /// the file. Validates `credits` is non-empty and amounts are positive.
    pub fn apply(&mut self, event: PersistedRewardEvent) -> anyhow::Result<()> {
        if event.credits.is_empty() {
            anyhow::bail!("rewardLedger: credits must not be empty");
        }
        for PersistedCredit { pk, amount } in &event.credits {
            let amount: u128 = amount.parse()?;
            if amount == 0 {
                anyhow::bail!("rewardLedger: credit amount must be positive");
            }
            *self.balances.entry(pk.clone()).or_insert(0) += amount;
        }
        self.events.push(event);
        Ok(())
    }

    pub fn events(&self) -> &[PersistedRewardEvent] {
        &self.events
    }

    pub fn last_event(&self) -> Option<&PersistedRewardEvent> {
        self.events.last()
    }

    pub fn balance_of(&self, pk: &str) -> u128 {
        self.balances.get(pk).copied().unwrap_or(0)
    }

    pub fn balances(&self) -> &BTreeMap<String, u128> {
        &self.balances
    }

    pub fn size(&self) -> usize {
        self.events.len()
    }
}
