//! Staged pure M3 consensus rules. No gate, clock, filesystem or network enters here.
use crate::m2::Hash;
mod wire;
pub use wire::*;
mod identities;
pub use identities::*;
mod forks;
pub use forks::*;
mod fold;
pub(crate) use fold::apply_due;
pub(crate) use fold::complete;
pub(crate) use fold::settle;
pub(crate) use fold::vest;

#[derive(Debug, Clone)]
pub struct Job {
    pub queue_index: u64,
    pub key: crate::m2::Key,
    pub kind: RecordKind,
    pub avail: u64,
    pub epoch_override: Option<u32>,
}
#[derive(Debug, Clone, Default)]
pub struct M3State {
    pub queue: Vec<Job>,
    pub next_applied: usize,
    pub clock: FuelClock,
    pub results: std::collections::BTreeMap<crate::m2::Key, VerdictRecord>,
    pub rewards: std::collections::BTreeMap<u64, Reward>,
    pub admissions: std::collections::BTreeMap<u64, Admission>,
    pub pending_rechecks: std::collections::BTreeSet<crate::m2::Key>,
    pub fork_history: Vec<ForkRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewardStatus {
    Locked,
    Vested,
    Cancelled,
}
#[derive(Debug, Clone)]
pub struct Reward {
    pub id: u64,
    pub key: crate::m2::Key,
    pub solver: [u8; 20],
    pub amount: u128,
    pub vest_h: u64,
    pub status: RewardStatus,
    pub compensation: bool,
}
#[derive(Debug, Clone)]
pub struct Admission {
    pub k: u64,
    pub key: crate::m2::Key,
    pub retired: bool,
}

/// G7's virtual reference worker. Updates are atomic even at arithmetic limits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FuelClock {
    pub fuel: u128,
    pub due: u64,
    pub count: u64,
}
impl FuelClock {
    pub fn push(&mut self, p: &M3Config, cap: u64, avail: u64, fuel: u64) -> Result<u64, Error> {
        if cap == 0 || p.rate == 0 {
            return Err(Error::Format("zero clock capacity"));
        }
        let minimum = avail.checked_add(p.k).ok_or(Error::Overflow)?;
        let rate = u128::from(p.rate);
        let f = self
            .fuel
            .max(u128::from(avail) * rate)
            .checked_add(u128::from(p.cost_fixed))
            .and_then(|n| n.checked_add(u128::from(fuel)))
            .ok_or(Error::Overflow)?;
        let quotient = f / rate + u128::from(f % rate != 0);
        let d = u64::try_from(quotient)
            .map_err(|_| Error::Overflow)?
            .max(minimum)
            .max(self.due);
        let (due, count) = if d == self.due && self.count >= cap {
            (d.checked_add(1).ok_or(Error::Overflow)?, 1)
        } else if d == self.due {
            (d, self.count.checked_add(1).ok_or(Error::Overflow)?)
        } else {
            (d, 1)
        };
        *self = Self {
            fuel: f,
            due,
            count,
        };
        Ok(due)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("M3 fork table changes the already folded history")]
    PastForkChanged,
    #[error("M3 result pending for queue index {queue_index} at height {height}")]
    Pending { queue_index: u64, height: u64 },
    #[error("M3 requires an enabled profile")]
    ProfileRequired,
    #[error("M3 arithmetic overflow")]
    Overflow,
    #[error("invalid M3 input: {0}")]
    Format(&'static str),
    #[error(
        "fork event08 kind bytes and list_digest preimage are unspecified in the supplied design"
    )]
    UnspecifiedForkEncoding,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct M3Config {
    pub k: u64,
    pub rate: u64,
    pub cost_fixed: u64,
    pub fuel_slot: Vec<u64>,
    pub verdict_sink: [u8; 20],
    pub q_reward: u64,
    pub reward_gas: u64,
    pub maturity: u64,
    pub reward: Vec<u128>,
    pub mint_enabled: bool,
    pub notice: u64,
    pub open_licenses: std::collections::BTreeSet<u8>,
    pub bridge: bool,
    pub native_is_reward_unit: bool,
    pub genesis_gate_rule_digest: Hash,
}

impl M3Config {
    pub fn devnet(slots: usize) -> Self {
        Self {
            k: 8,
            rate: 1_000,
            cost_fixed: 100,
            fuel_slot: vec![10_000; slots],
            verdict_sink: [
                0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xb3,
            ],
            q_reward: 16,
            reward_gas: 100_000,
            maturity: 64,
            reward: vec![1_000_000_000_000_000_000_000; slots],
            mint_enabled: true,
            notice: 0,
            open_licenses: [1].into(),
            bridge: false,
            native_is_reward_unit: false,
            genesis_gate_rule_digest: crate::m2::body_digest(b"BOOLE-M3/STUB/v1"),
        }
    }
    /// Staged parameters only: fuel calibration and real gate activation are later work.
    pub fn lean_testnet(slots: usize) -> Self {
        Self {
            k: 300,
            maturity: 50_400,
            notice: 7_200,
            mint_enabled: false,
            ..Self::devnet(slots)
        }
    }
    pub fn validate(&self, c: &crate::m2::Config, gas_limit: u64) -> Result<(), u8> {
        let floor = c
            .v_per_deposit
            .checked_mul(80)
            .and_then(|n| n.checked_add(24))
            .and_then(|n| n.checked_mul(40))
            .and_then(|n| n.checked_add(21_000));
        if floor.is_none_or(|n| n > c.verdict_gas) || self.reward_gas < 21_000 {
            return Err(9);
        }
        let maximum = c
            .q_max
            .checked_mul(c.verdict_gas)
            .and_then(|n| {
                self.q_reward
                    .checked_mul(self.reward_gas)
                    .and_then(|w| n.checked_add(w))
            })
            .and_then(|n| n.checked_add(crate::config::L1INFO_GAS))
            .and_then(|n| n.checked_add(crate::config::M2ROOT_GAS))
            .and_then(|n| n.checked_add(16_777_216));
        if maximum.is_none_or(|n| n > gas_limit) {
            return Err(10);
        }
        if self.k == 0
            || self.rate == 0
            || self.cost_fixed == 0
            || self.fuel_slot.len() != c.slot_table.len()
            || self.fuel_slot.contains(&0)
            || self
                .fuel_slot
                .iter()
                .max()
                .and_then(|n| n.checked_add(self.cost_fixed))
                .is_none_or(|n| n > 1u64 << 63)
        {
            return Err(11);
        }
        if self.k.checked_mul(2).is_none_or(|n| self.maturity < n)
            || self
                .notice
                .checked_add(self.k)
                .is_none_or(|n| self.maturity < n)
        {
            return Err(12);
        }
        if self.reward.len() != c.slot_table.len() || self.mint_enabled && self.reward.contains(&0)
        {
            return Err(13);
        }
        if c.terms_table
            .keys()
            .any(|id| !self.open_licenses.contains(id))
        {
            return Err(15);
        }
        if self.mint_enabled && self.bridge && !self.native_is_reward_unit {
            return Err(17);
        }
        Ok(())
    }
}
