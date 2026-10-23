//! Pure queue application. The caller works on a clone, so all failures are atomic.
use super::*;
use crate::m2::{Candidate, Config, Key, State, Status};

pub(crate) fn status(s: &mut State, key: Key, value: Status, e: &mut Vec<u8>) -> Result<(), Error> {
    let Some(Candidate::Live(l)) = s.candidates.get_mut(&key) else {
        return Err(Error::Format("verdict target is not retained"));
    };
    if l.status != value {
        l.status = value;
        e.push(2);
        e.extend(key.0);
        e.extend(key.1);
        e.push(value as u8);
    }
    Ok(())
}
/// R2 stage 0. Called only on the tentative clone, before ties and verdicts.
pub(crate) fn activate(
    s: &mut State,
    h: u64,
    forks: &ForkTable,
    e: &mut Vec<u8>,
) -> Result<(), Error> {
    let rechecks = forks.recheck_set(s, h)?;
    let m = s.m3.as_mut().ok_or(Error::ProfileRequired)?;
    if m.fork_history.is_empty() {
        m.fork_history.push(forks.rows[0].clone());
    }
    let Some(row) = forks.rows.iter().skip(1).find(|row| row.height == h) else {
        return Ok(());
    };
    e.push(8);
    e.extend(row.activation_record()?);
    match row.kind {
        ForkKind::KernelFix => {
            for key in rechecks {
                let queue_index = u64::try_from(m.queue.len()).map_err(|_| Error::Overflow)?;
                m.queue.push(Job {
                    queue_index,
                    key,
                    kind: RecordKind::Recheck,
                    avail: h,
                    epoch_override: Some(row.epoch),
                });
                m.pending_rechecks.insert(key);
            }
        }
        ForkKind::Quarantine => {
            for item in &row.list {
                let mut matches = m.queue[m.next_applied..].iter_mut().filter(|j| {
                    j.kind == RecordKind::Verdict
                        && j.key.1 == item.commit_hash
                        && s.candidates
                            .get(&j.key)
                            .is_some_and(|c| c.status() == Status::Checking)
                });
                let job = matches
                    .next()
                    .ok_or(Error::Format("quarantine target is not queued CHECKING"))?;
                job.epoch_override = Some(row.epoch);
                if item.disposition == Disposition::Void {
                    m.quarantine_voids.insert(job.key);
                } else {
                    m.quarantine_voids.remove(&job.key);
                }
                if matches.next().is_some() {
                    return Err(Error::Format("ambiguous quarantine target"));
                }
            }
        }
        ForkKind::Policy => {}
        ForkKind::Genesis => return Err(Error::Format("nonzero genesis activation")),
    }
    m.fork_history.push(row.clone());
    Ok(())
}

pub(crate) fn apply_due(
    s: &mut State,
    c: &Config,
    h: u64,
    records: &[IndexedVerdict],
    forks: &ForkTable,
    e: &mut Vec<u8>,
) -> Result<Vec<IndexedVerdict>, Error> {
    let p = c.m3.as_ref().ok_or(Error::ProfileRequired)?;
    let mut m = s.m3.take().ok_or(Error::ProfileRequired)?;
    let cap = c
        .q_max
        .checked_mul(c.v_per_deposit)
        .ok_or(Error::Overflow)?;
    let mut clock = m.clock.clone();
    let mut applied = Vec::new();
    for (offset, v) in records.iter().enumerate() {
        let at = m.next_applied.checked_add(offset).ok_or(Error::Overflow)?;
        let job = m
            .queue
            .get(at)
            .ok_or(Error::Format("extra verdict"))?
            .clone();
        if v.queue_index != job.queue_index
            || v.record.commit_hash != job.key.1
            || v.record.kind != job.kind
        {
            return Err(Error::Format("verdict queue identity"));
        }
        v.record.encode()?;
        if m.quarantine_voids.contains(&job.key)
            && (v.record.outcome != Outcome::Void || v.record.reason != QUARANTINE_VOID)
        {
            return Err(Error::Format("quarantine requires VOID(0802)"));
        }
        let epoch = job
            .epoch_override
            .map(Ok)
            .unwrap_or_else(|| forks.job_epoch(job.avail, p.k))?;
        if v.record.gate_epoch != epoch {
            return Err(Error::Format("verdict gate epoch"));
        }
        let slot = usize::from(
            s.problems
                .get(&job.key.0)
                .ok_or(Error::Format("job problem"))?
                .slot,
        );
        if v.record.fuel > p.fuel_slot[slot] {
            return Err(Error::Format("verdict fuel exceeds slot"));
        }
        let due = clock.push(p, cap, job.avail, v.record.fuel)?;
        if due < h {
            return Err(Error::Format("overdue verdict"));
        }
        if due == h {
            e.push(5);
            e.extend(v.queue_index.to_be_bytes());
            e.extend(v.record.encode()?);
            let old = s
                .candidates
                .get(&job.key)
                .ok_or(Error::Format("verdict target absent"))?
                .status();
            let kernel_fault = old == Status::Rejected
                && m.results
                    .get(&job.key)
                    .is_some_and(|r| r.reason == KERNEL_FAULT);
            let compensation = kernel_fault
                && v.record.kind == RecordKind::Recheck
                && v.record.outcome == Outcome::Accept
                && s.problems[&job.key.0].winner.is_some();
            let new = match (v.record.kind, old, v.record.outcome) {
                (RecordKind::Verdict, Status::Checking, Outcome::Accept) => Status::Passed,
                (RecordKind::Verdict, Status::Checking, Outcome::Reject) => Status::Rejected,
                (RecordKind::Verdict, Status::Checking, Outcome::Void) => Status::Void,
                (RecordKind::Verdict, Status::Superseded | Status::Void, _) => old,
                (RecordKind::Recheck, Status::Accepted, Outcome::Reject) => Status::Stale,
                (RecordKind::Recheck, Status::Accepted | Status::Passed, Outcome::Accept) => old,
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
                _ => return Err(Error::Format("invalid verdict target status")),
            };
            status(s, job.key, new, e)?;
            if compensation {
                let solver = s.candidates[&job.key]
                    .live()
                    .and_then(|l| l.open.as_ref())
                    .ok_or(Error::Format("compensation OPEN"))?
                    .solver;
                grant_to(&mut m, p, slot, job.key, solver, h, true, e)?;
            }
            if new == Status::Stale {
                for reward in m
                    .rewards
                    .values_mut()
                    .filter(|r| r.key == job.key && r.status == RewardStatus::Locked)
                {
                    reward.status = RewardStatus::Cancelled;
                    reward_event(reward, 3, e);
                }
                for admission in m
                    .admissions
                    .values_mut()
                    .filter(|a| a.key == job.key && !a.retired)
                {
                    admission.retired = true;
                    admission_event(admission, 2, e);
                }
                s.enqueue_repose(job.key.0);
                round(s, job.key.0, 0xc7, e);
            }
            if old == Status::Checking
                && new == Status::Void
                && [0x0801, 0x0803, 0x0804].contains(&v.record.reason)
            {
                void_problem(s, c, job.key.0, h, e)?;
            }
            m.results.insert(job.key, v.record.clone());
            m.quarantine_voids.remove(&job.key);
            if job.kind == RecordKind::Recheck {
                m.pending_rechecks.remove(&job.key);
            }
            m.clock = clock.clone();
            applied.push(v.clone());
        }
    }
    let examined = m
        .next_applied
        .checked_add(records.len())
        .ok_or(Error::Overflow)?;
    let future = records.len() > applied.len();
    if !future {
        if let Some(job) = m.queue.get(examined) {
            if job.avail.checked_add(p.k).ok_or(Error::Overflow)? <= h {
                return Err(Error::Pending {
                    queue_index: job.queue_index,
                    height: h,
                });
            }
        }
    }
    m.next_applied = m
        .next_applied
        .checked_add(applied.len())
        .ok_or(Error::Overflow)?;
    s.m3 = Some(m);
    Ok(applied)
}

fn void_problem(s: &mut State, c: &Config, p: Hash, h: u64, e: &mut Vec<u8>) -> Result<(), Error> {
    if s.problems[&p].voided {
        return Ok(());
    }
    s.problems.get_mut(&p).expect("known problem").voided = true;
    round(s, p, 0xc6, e);
    close_window(s, c, p, h)?;
    let siblings: Vec<_> = s
        .candidates
        .iter()
        .filter(|(k, r)| k.0 == p && r.status() == Status::Checking)
        .map(|(k, _)| *k)
        .collect();
    for key in siblings {
        status(s, key, Status::Void, e)?;
    }
    Ok(())
}

pub(crate) fn complete(s: &mut State, key: Key, h: u64, e: &mut Vec<u8>) -> Result<(), Error> {
    let problem = &s.problems[&key.0];
    if problem.voided {
        return status(s, key, Status::Void, e);
    }
    if problem.winner.is_some() {
        return status(s, key, Status::Superseded, e);
    }
    let m = s.m3.as_mut().ok_or(Error::ProfileRequired)?;
    let index = u64::try_from(m.queue.len()).map_err(|_| Error::Overflow)?;
    if let Some(Candidate::Live(l)) = s.candidates.get_mut(&key) {
        l.open
            .as_mut()
            .ok_or(Error::Format("completion without OPEN"))?
            .queue_index = Some(index);
    }
    m.queue.push(Job {
        queue_index: index,
        key,
        kind: RecordKind::Verdict,
        avail: h,
        epoch_override: None,
    });
    Ok(())
}

fn reward_event(r: &Reward, event: u8, e: &mut Vec<u8>) {
    e.push(6);
    e.extend(r.id.to_be_bytes());
    e.extend(r.key.0);
    e.extend(r.key.1);
    e.extend(r.solver);
    e.extend(r.amount.to_be_bytes());
    e.extend(r.vest_h.to_be_bytes());
    e.push(event);
}
fn admission_event(a: &Admission, event: u8, e: &mut Vec<u8>) {
    e.push(7);
    e.extend(a.k.to_be_bytes());
    e.extend(a.key.0);
    e.extend(a.key.1);
    e.push(event);
}
fn round(s: &State, p: Hash, event: u8, e: &mut Vec<u8>) {
    if let Some((key, r)) = s.rounds.iter().find(|(_, r)| r.problem == Some(p)) {
        State::round_event(e, key.0, key.1, event, r.pool_seq, r.leaf_index, Some(p));
    }
}
fn close_window(s: &mut State, c: &Config, p: Hash, h: u64) -> Result<(), Error> {
    let problem = s
        .problems
        .get_mut(&p)
        .ok_or(Error::Format("missing problem"))?;
    let slot = usize::from(problem.slot);
    let deadline = problem
        .h_v
        .checked_add(c.slot_table[slot].solve_deadline)
        .ok_or(Error::Overflow)?;
    if problem.closed.is_none() && h <= deadline {
        problem.closed = Some(h);
        if let Some((key, r)) = s.rounds.iter_mut().find(|(_, r)| r.problem == Some(p)) {
            r.status = crate::m2::RoundStatus::Closed;
            if s.slots[slot].active == Some(*key) {
                s.slots[slot].active = None;
                s.slots[slot].scheduled = Some(
                    h.checked_add(c.slot_table[slot].announce_gap)
                        .ok_or(Error::Overflow)?,
                );
            }
        }
    }
    Ok(())
}
fn grant(
    s: &mut State,
    c: &Config,
    key: Key,
    h: u64,
    compensation: bool,
    e: &mut Vec<u8>,
) -> Result<(), Error> {
    let p = c.m3.as_ref().ok_or(Error::ProfileRequired)?;
    let slot = usize::from(s.problems[&key.0].slot);
    let solver = s.candidates[&key]
        .live()
        .and_then(|l| l.open.as_ref())
        .ok_or(Error::Format("winner has no frozen OPEN"))?
        .solver;
    let m = s.m3.as_mut().ok_or(Error::ProfileRequired)?;
    grant_to(m, p, slot, key, solver, h, compensation, e)
}
#[allow(clippy::too_many_arguments)]
fn grant_to(
    m: &mut M3State,
    p: &M3Config,
    slot: usize,
    key: Key,
    solver: [u8; 20],
    h: u64,
    compensation: bool,
    e: &mut Vec<u8>,
) -> Result<(), Error> {
    if compensation && m.rewards.values().any(|r| r.key == key && r.compensation) {
        return Ok(());
    }
    let id = u64::try_from(m.rewards.len()).map_err(|_| Error::Overflow)?;
    let r = Reward {
        id,
        key,
        solver,
        amount: p.reward[slot],
        vest_h: h.checked_add(p.maturity).ok_or(Error::Overflow)?,
        status: RewardStatus::Locked,
        compensation,
    };
    reward_event(&r, if compensation { 4 } else { 1 }, e);
    m.rewards.insert(id, r);
    let k = u64::try_from(m.admissions.len())
        .map_err(|_| Error::Overflow)?
        .checked_add(1)
        .ok_or(Error::Overflow)?;
    let a = Admission {
        k,
        key,
        retired: false,
    };
    admission_event(&a, 1, e);
    m.admissions.insert(k, a);
    Ok(())
}
pub(crate) fn settle(s: &mut State, c: &Config, h: u64, e: &mut Vec<u8>) -> Result<(), Error> {
    let ids: Vec<_> = s.problems.keys().copied().collect();
    for p in ids {
        if s.problems[&p].winner.is_some() || s.problems[&p].voided {
            continue;
        }
        let first = s
            .candidates
            .iter()
            .filter_map(|(key, candidate)| {
                let l = candidate.live()?;
                (key.0 == p
                    && matches!(
                        l.status,
                        Status::Committed | Status::Open | Status::Checking | Status::Passed
                    ))
                .then_some((l.h_c, l.t.unwrap_or([0; 32]), key.1, l.status))
            })
            .min_by_key(|x| (x.0, x.1, x.2));
        let Some((_, _, ch, Status::Passed)) = first else {
            continue;
        };
        let key = (p, ch);
        if s.m3
            .as_ref()
            .ok_or(Error::ProfileRequired)?
            .pending_rechecks
            .contains(&key)
        {
            continue;
        }
        status(s, key, Status::Accepted, e)?;
        let rest: Vec<_> = s
            .candidates
            .iter()
            .filter(|(k, r)| {
                k.0 == p && **k != key && matches!(r.status(), Status::Checking | Status::Passed)
            })
            .map(|(k, _)| *k)
            .collect();
        for k in rest {
            status(s, k, Status::Superseded, e)?;
        }
        s.problems.get_mut(&p).expect("known problem").winner = Some(ch);
        round(s, p, 0xc5, e);
        close_window(s, c, p, h)?;
        grant(s, c, key, h, false, e)?;
    }
    Ok(())
}

pub(crate) fn vest(
    s: &mut State,
    c: &Config,
    h: u64,
    e: &mut Vec<u8>,
) -> Result<Vec<RewardPayment>, Error> {
    let p = c.m3.as_ref().ok_or(Error::ProfileRequired)?;
    let m = s.m3.as_mut().ok_or(Error::ProfileRequired)?;
    let mut eligible: Vec<_> = m
        .rewards
        .values()
        .filter(|r| r.status == RewardStatus::Locked && r.vest_h <= h)
        .map(|r| (r.vest_h, r.id))
        .collect();
    eligible.sort();
    let mut payments = Vec::new();
    for (_, id) in eligible
        .into_iter()
        .take(usize::try_from(p.q_reward).unwrap_or(usize::MAX))
    {
        let r = m.rewards.get_mut(&id).expect("eligible reward");
        r.status = RewardStatus::Vested;
        reward_event(r, 2, e);
        if p.mint_enabled {
            payments.push(RewardPayment {
                reward_id: id,
                solver: r.solver,
                amount: r.amount,
            });
        }
    }
    Ok(payments)
}
