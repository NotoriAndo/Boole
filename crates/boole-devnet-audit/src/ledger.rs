//! An independent event ledger, checked against the fold and actual deposits.
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::hex_bytes,
    m2::{Config, Key, State, Status},
    m3::{Outcome, RecordKind, RewardPayment, RewardStatus, KERNEL_FAULT},
    L1Block,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug)]
pub struct AdmissionViolation;
impl std::fmt::Display for AdmissionViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("admission invariant")
    }
}
impl std::error::Error for AdmissionViolation {}
#[derive(Clone)]
struct Reward {
    key: Key,
    solver: [u8; 20],
    amount: u128,
    vest: u64,
    status: u8,
    compensation: bool,
}
#[derive(Clone, Default)]
pub struct Ledger {
    rewards: BTreeMap<u64, Reward>,
    admissions: BTreeMap<u64, (Key, bool)>,
    minted: u128,
    failed: u128,
}
pub(crate) fn events(bytes: &[u8]) -> Result<Vec<(u8, &[u8])>> {
    let mut out = vec![];
    let mut at = 0;
    while at < bytes.len() {
        let tag = bytes[at];
        let len = match tag {
            1 => 19,
            2 => 66,
            3 => 47,
            4 => 37,
            5 => 89,
            6 => 118,
            7 => 74,
            8 => 70,
            _ => anyhow::bail!("unknown event class"),
        };
        let end = at.checked_add(len).context("event overflow")?;
        out.push((tag, bytes.get(at + 1..end).context("event length")?));
        at = end;
    }
    Ok(out)
}
fn array<const N: usize>(b: &[u8], at: usize) -> Result<[u8; N]> {
    Ok(b.get(at..at + N).context("event field")?.try_into()?)
}
fn key(b: &[u8], at: usize) -> Result<Key> {
    Ok((array(b, at)?, array(b, at + 32)?))
}
fn sum(mut values: impl Iterator<Item = u128>) -> Result<u128> {
    values.try_fold(0u128, |a, b| {
        a.checked_add(b).context("ledger total overflow")
    })
}
impl Ledger {
    #[allow(clippy::too_many_arguments)]
    pub fn observe(
        &mut self,
        before: &State,
        after: &State,
        c: &Config,
        l1: &L1Block,
        raw: &[u8],
        payments: &[RewardPayment],
        prefix: &[String],
        statuses: &[bool],
    ) -> Result<()> {
        let Some(p) = c.m3.as_ref() else {
            ensure!(payments.is_empty(), "mint without M3");
            return Ok(());
        };
        let m = after.m3.as_ref().context("M3 ledger")?;
        let records = events(raw)?;
        let stale: BTreeSet<_> = records
            .iter()
            .filter(|(tag, b)| *tag == 2 && b[64] == Status::Stale as u8)
            .map(|(_, b)| key(b, 0))
            .collect::<Result<_>>()?;
        let mut next = self.clone();
        let mut vested = vec![];
        for (tag, b) in records {
            match tag {
                6 => {
                    let id = u64::from_be_bytes(array(b, 0)?);
                    let k = key(b, 8)?;
                    let solver = array(b, 72)?;
                    let amount = u128::from_be_bytes(array(b, 92)?);
                    let vest = u64::from_be_bytes(array(b, 108)?);
                    let action = b[116];
                    match action {
                        1 | 4 => {
                            ensure!(
                                id == next.rewards.len() as u64 && !next.rewards.contains_key(&id),
                                "reward LOCKED twice or noncontiguous id"
                            );
                            let slot = usize::from(
                                after.problems.get(&k.0).context("reward problem")?.slot,
                            );
                            ensure!(
                                amount == p.reward[slot]
                                    && vest
                                        == l1
                                            .number
                                            .checked_add(p.maturity)
                                            .context("maturity overflow")?,
                                "reward amount/maturity"
                            );
                            if action == 4 {
                                let old = before.m3.as_ref().context("old ledger")?;
                                ensure!(
                                    before
                                        .candidates
                                        .get(&k)
                                        .is_some_and(|v| v.status() == Status::Rejected)
                                        && old
                                            .results
                                            .get(&k)
                                            .is_some_and(|v| v.reason == KERNEL_FAULT),
                                    "compensation without kernel-fault REJECT"
                                );
                                ensure!(
                                    before
                                        .problems
                                        .get(&k.0)
                                        .is_some_and(|v| v.winner.is_some())
                                        && m.results
                                            .get(&k)
                                            .is_some_and(|v| v.kind == RecordKind::Recheck
                                                && v.outcome == Outcome::Accept),
                                    "compensation without accepted recheck after winner"
                                );
                                ensure!(
                                    !next.rewards.values().any(|r| r.key == k && r.compensation),
                                    "duplicate compensation"
                                );
                            }
                            next.rewards.insert(
                                id,
                                Reward {
                                    key: k,
                                    solver,
                                    amount,
                                    vest,
                                    status: 1,
                                    compensation: action == 4,
                                },
                            );
                        }
                        2 | 3 => {
                            let r = next
                                .rewards
                                .get_mut(&id)
                                .context("terminal reward without LOCKED")?;
                            ensure!(
                                r.status == 1
                                    && r.key == k
                                    && r.solver == solver
                                    && r.amount == amount
                                    && r.vest == vest,
                                "duplicate/changed terminal reward"
                            );
                            if action == 2 {
                                ensure!(vest <= l1.number, "early vest");
                                vested.push(id)
                            } else {
                                ensure!(stale.contains(&k), "CANCELLED without same-height STALE");
                            }
                            r.status = action;
                        }
                        _ => anyhow::bail!("reward event action"),
                    }
                }
                7 => {
                    (|| -> Result<()> {
                        let k = u64::from_be_bytes(array(b, 0)?);
                        let identity = key(b, 8)?;
                        match b[72] {
                            1 => {
                                ensure!(
                                    k == next.admissions.len() as u64 + 1
                                        && !next.admissions.contains_key(&k),
                                    "reused or noncontiguous admission k"
                                );
                                next.admissions.insert(k, (identity, false));
                            }
                            2 => {
                                let a =
                                    next.admissions.get_mut(&k).context("unknown retirement")?;
                                ensure!(!a.1 && a.0 == identity, "changed or repeated retirement");
                                a.1 = true;
                            }
                            _ => anyhow::bail!("admission event action"),
                        }
                        Ok(())
                    })()
                    .context(AdmissionViolation)?;
                }
                _ => {}
            }
        }
        ensure!(
            next.rewards.len() == m.rewards.len(),
            "reward event/state coverage"
        );
        for (id, r) in &m.rewards {
            let a = next.rewards.get(id).context("unseen reward")?;
            let status = match r.status {
                RewardStatus::Locked => 1,
                RewardStatus::Vested => 2,
                RewardStatus::Cancelled => 3,
            };
            ensure!(
                a.key == r.key
                    && a.solver == r.solver
                    && a.amount == r.amount
                    && a.vest == r.vest_h
                    && a.status == status
                    && a.compensation == r.compensation,
                "reward event/state mismatch"
            );
        }
        (|| -> Result<()> {
            ensure!(
                next.admissions.len() == m.admissions.len(),
                "admission event/state coverage"
            );
            for (k, a) in &m.admissions {
                ensure!(
                    next.admissions.get(k) == Some(&(a.key, a.retired)) && a.k == *k,
                    "admission event/state mismatch"
                );
            }
            let active: BTreeSet<_> = next
                .admissions
                .values()
                .filter(|(_, r)| !r)
                .map(|(k, _)| *k)
                .collect();
            ensure!(
                active.len() == next.admissions.values().filter(|(_, r)| !r).count(),
                "duplicate active admission"
            );
            let accepted: BTreeSet<_> = after
                .candidates
                .iter()
                .filter(|(_, c)| c.status() == Status::Accepted)
                .map(|(k, _)| *k)
                .collect();
            ensure!(
                active == accepted,
                "active admissions differ from ACCEPTED candidates"
            );
            Ok(())
        })()
        .context(AdmissionViolation)?;
        let mut actual_mints = vec![];
        for (index, tx) in prefix
            .iter()
            .take_while(|tx| tx.starts_with("0x7e"))
            .enumerate()
        {
            let raw = hex_bytes(tx, "system")?;
            let f = boole_derivation::rlp::decode(&raw[1..])
                .map_err(anyhow::Error::msg)?
                .children
                .context("deposit")?;
            ensure!(
                f.len() == 8 && f.iter().all(|f| f.children.is_none()),
                "deposit fields"
            );
            if !f[3].bytes.is_empty() || !f[4].bytes.is_empty() {
                actual_mints.push((index, tx));
            }
        }
        if !p.mint_enabled {
            ensure!(
                payments.is_empty() && actual_mints.is_empty(),
                "mint with MINT_ENABLED=0"
            );
        } else {
            ensure!(
                payments.len() == vested.len() && actual_mints.len() == payments.len(),
                "vest/mint count"
            );
            for ((payment, id), (index, tx)) in payments.iter().zip(vested).zip(actual_mints) {
                let r = &next.rewards[&id];
                ensure!(
                    payment.reward_id == id
                        && payment.solver == r.solver
                        && payment.amount == r.amount,
                    "vest/mint identity"
                );
                boole_derivation::m3::check_reward_deposit(c, l1, payment, tx)?;
                next.minted = next
                    .minted
                    .checked_add(payment.amount)
                    .context("mint overflow")?;
                if !*statuses.get(index).context("REWARD receipt missing")? {
                    next.failed = next
                        .failed
                        .checked_add(payment.amount)
                        .context("failed mint overflow")?;
                }
            }
        }
        sum(next.rewards.values().map(|r| r.amount))?;
        let expected = if p.mint_enabled {
            sum(next
                .rewards
                .values()
                .filter(|r| r.status == 2)
                .map(|r| r.amount))?
        } else {
            0
        };
        ensure!(next.minted == expected, "minted total differs from VESTED");
        *self = next;
        Ok(())
    }
    pub fn failed_mints(&self) -> u128 {
        self.failed
    }
    pub fn totals(&self) -> Value {
        let total = |status| {
            self.rewards
                .values()
                .filter(|r| r.status == status)
                .map(|r| r.amount)
                .sum::<u128>()
                .to_string()
        };
        json!({"locked":total(1),"vested":total(2),"cancelled":total(3),"compensation":self.rewards.values().filter(|r|r.compensation).map(|r|r.amount).sum::<u128>().to_string(),"minted":self.minted.to_string(),"would_be":total(2),"failed_calls":self.failed.to_string()})
    }
    pub fn admission_counts(&self) -> Value {
        json!({"total":self.admissions.len().to_string(),"active":self.admissions.values().filter(|(_,retired)|!*retired).count().to_string(),"retired":self.admissions.values().filter(|(_,retired)|*retired).count().to_string()})
    }
}
