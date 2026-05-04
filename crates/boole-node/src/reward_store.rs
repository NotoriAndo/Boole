use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use boole_core::{PersistedCredit, PersistedRewardEvent};

#[derive(Debug, Default)]
pub struct FileRewardLedger {
    events: Vec<PersistedRewardEvent>,
    balances: BTreeMap<String, u128>,
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
            ledger.apply_event(event)?;
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

    fn apply_event(&mut self, event: PersistedRewardEvent) -> anyhow::Result<()> {
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
