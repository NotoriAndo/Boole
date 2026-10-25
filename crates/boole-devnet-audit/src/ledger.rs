//! An independent event ledger, checked against the fold and actual deposits.
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::hex_bytes,
    m2::{Config, Key, State, Status},
    m3::{ForkKind, ForkRow, Outcome, RecordKind, RewardPayment, RewardStatus, KERNEL_FAULT},
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
// The spec names unapplied CHECKING first-verdict jobs. Requiring a unique
// match additionally codifies the fold's activation behavior; the spec does not
// state uniqueness. Count the immutable pre-height queue before outcomes.
fn check_quarantine_targets(before: &State, activation: Option<&ForkRow>) -> Result<()> {
    let Some(row) = activation.filter(|row| row.kind == ForkKind::Quarantine) else {
        return Ok(());
    };
    let old = before.m3.as_ref().context("old ledger")?;
    // Allocate counters only for the declared targets, not the whole queue.
    let mut counts: BTreeMap<_, u8> = row.list.iter().map(|item| (item.commit_hash, 0)).collect();
    for job in old.queue.iter().skip(old.next_applied) {
        if job.kind == RecordKind::Verdict
            && before
                .candidates
                .get(&job.key)
                .is_some_and(|c| c.status() == Status::Checking)
        {
            if let Some(n) = counts.get_mut(&job.key.1) {
                // Only zero, one and multiple matter; never overflow.
                *n = n.saturating_add(1).min(2);
            }
        }
    }
    for item in &row.list {
        match counts.get(&item.commit_hash).copied().unwrap_or(0) {
            0 => anyhow::bail!("quarantine target is not queued CHECKING"),
            1 => {}
            _ => anyhow::bail!("ambiguous quarantine target"),
        }
    }
    Ok(())
}
// Rewards/rechecks specification invariant, proved by induction in
// docs/devnet-audit-compensation-seeds-v0.md. This is an input check, not a
// comparison to the fold's output or an exception based on a failed word.
fn check_compensation_context(before: &State) -> Result<()> {
    (|| -> Result<()> {
        let old = before.m3.as_ref().context("old ledger")?;
        for reward in old.rewards.values().filter(|r| r.compensation) {
            let status = before
                .candidates
                .get(&reward.key)
                .context("compensation candidate missing")?
                .status();
            ensure!(
                matches!(status, Status::Accepted | Status::Stale),
                "compensation candidate must be ACCEPTED or STALE"
            );
            ensure!(
                before
                    .problems
                    .get(&reward.key.0)
                    .is_some_and(|p| p.winner.is_some()),
                "compensation requires the original problem winner"
            );
            match reward.status {
                RewardStatus::Cancelled => ensure!(
                    status == Status::Stale,
                    "cancelled compensation requires STALE"
                ),
                RewardStatus::Locked => ensure!(
                    status == Status::Accepted,
                    "locked compensation requires ACCEPTED"
                ),
                RewardStatus::Vested => {}
            }
        }
        Ok(())
    })()
    .context(AdmissionViolation)
}
// V_h is independently computed and checked against this exact system prefix
// by the caller. Decode it independently of the audited fold's rewards/events.
fn expected_compensation(
    before: &State,
    c: &Config,
    l1: &L1Block,
    prefix: &[String],
    activation: Option<&ForkRow>,
) -> Result<BTreeSet<Key>> {
    check_compensation_context(before)?;
    check_quarantine_targets(before, activation)?;
    let mut deposits = vec![];
    for tx in prefix.iter().skip(2).take_while(|t| t.starts_with("0x7e")) {
        let raw = hex_bytes(tx, "system")?;
        let decoded = boole_derivation::rlp::decode(&raw[1..]).map_err(anyhow::Error::msg)?;
        let fields = decoded.children.context("deposit")?;
        ensure!(
            fields.len() == 8 && fields.iter().all(|f| f.children.is_none()),
            "deposit fields"
        );
        if fields[7].bytes.starts_with(b"m3vd") {
            deposits.push(tx.clone());
        }
    }
    let records = boole_derivation::m3::decode_verdict_deposits(c, l1, &deposits)?;
    let old = before.m3.as_ref().context("old ledger")?;
    #[cfg(test)]
    differential::reset_trace(before);
    let mut expected = BTreeSet::new();
    let already_compensated: BTreeSet<_> = old
        .rewards
        .values()
        .filter(|reward| reward.compensation)
        .map(|reward| reward.key)
        .collect();
    let mut targets: BTreeMap<Key, (Status, u16)> = before
        .candidates
        .iter()
        .map(|(key, candidate)| {
            (
                *key,
                (
                    candidate.status(),
                    old.results.get(key).map_or(0, |r| r.reason),
                ),
            )
        })
        .collect();
    // A later record sees the preceding record's status/reason, not a fresh
    // copy of the pre-block state. This ledger remains independent of the
    // audited fold's final rewards and event stream.
    // A verdict identifies the next queue job, not every candidate sharing its
    // hash. COMMIT does not prove the problem binding; OPEN does. Since k > 0,
    // jobs enqueued at this height cannot be due in this height's prefix.
    for (offset, indexed) in records.into_iter().enumerate() {
        let position = old
            .next_applied
            .checked_add(offset)
            .context("verdict queue overflow")?;
        let job = old.queue.get(position).context("verdict queue target")?;
        let record = indexed.record;
        ensure!(
            indexed.queue_index == job.queue_index
                && record.kind == job.kind
                && record.commit_hash == job.key.1,
            "verdict queue identity"
        );
        let key = &job.key;
        let (status, reason) = targets.get_mut(key).context("verdict candidate")?;
        let kernel_fault = *status == Status::Rejected && *reason == KERNEL_FAULT;
        let compensation = kernel_fault
            && record.kind == RecordKind::Recheck
            && record.outcome == Outcome::Accept
            && before
                .problems
                .get(&key.0)
                .is_some_and(|p| p.winner.is_some());
        // Defensive, unreachable already-compensated case: a prior reward
        // requires ACCEPTED/STALE, but this reversal requires REJECTED. See
        // docs/devnet-audit-compensation-seeds-v0.md.
        #[cfg(test)]
        if compensation && already_compensated.contains(key) {
            differential::duplicate_compensation();
        }
        if compensation && !already_compensated.contains(key) {
            expected.insert(*key);
        }
        *status = match (record.kind, *status, record.outcome) {
            (RecordKind::Verdict, Status::Checking, Outcome::Accept) => Status::Passed,
            (RecordKind::Verdict, Status::Checking, Outcome::Reject) => Status::Rejected,
            (RecordKind::Verdict, Status::Checking, Outcome::Void) => Status::Void,
            (RecordKind::Verdict, old @ (Status::Superseded | Status::Void), _) => old,
            (RecordKind::Recheck, Status::Accepted, Outcome::Reject) => Status::Stale,
            (RecordKind::Recheck, old @ (Status::Accepted | Status::Passed), Outcome::Accept) => {
                old
            }
            (RecordKind::Recheck, Status::Passed, Outcome::Reject) => Status::Rejected,
            (RecordKind::Recheck, Status::Rejected, Outcome::Accept) if kernel_fault => {
                if compensation {
                    Status::Accepted
                } else {
                    Status::Passed
                }
            }
            (RecordKind::Recheck, Status::Rejected, Outcome::Reject) if kernel_fault => {
                Status::Rejected
            }
            _ => anyhow::bail!("compensation verdict transition"),
        };
        *reason = record.reason;
        #[cfg(test)]
        differential::trace(*key, *status, &record);
    }
    Ok(expected)
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
        // The caller binds fork-history rows to the canonical FORK_TABLE/event.
        // Eligibility is checked against the pre-height queue, before outcomes.
        let activation = m
            .fork_history
            .iter()
            .find(|r| r.epoch != 0 && r.height == l1.number);
        let expected_compensation = expected_compensation(before, c, l1, prefix, activation)?;
        let mut compensated = BTreeSet::new();
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
                                ensure!(
                                    expected_compensation.contains(&k),
                                    "compensation without qualifying ordered kernel-fault recheck"
                                );
                                ensure!(
                                    !next.rewards.values().any(|r| r.key == k && r.compensation),
                                    "duplicate compensation"
                                );
                                ensure!(compensated.insert(k), "duplicate compensation");
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
                                #[cfg(test)]
                                differential::admitted(identity);
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
            compensated == expected_compensation,
            "compensation coverage differs from qualifying accepted rechecks"
        );
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

#[cfg(test)]
#[path = "ledger_differential_tests.rs"]
mod differential;
