//! Collision-shaped component controls, not claims of two OPEN preimages with
//! equal SHA-256 hashes. The genesis-derived copied-COMMIT case is in reachable.
use super::*;

#[test]
fn t82_alias_compensation_and_admissions_are_per_problem() -> Result<()> {
    let (base, c, mut l1, primary, record) = seed()?;
    let foreign = ([244; 32], primary.1);
    let mut grants = 0;
    for outcomes in [
        [Outcome::Accept, Outcome::Accept],
        [Outcome::Accept, Outcome::Reject],
        [Outcome::Reject, Outcome::Accept],
        [Outcome::Reject, Outcome::Reject],
    ] {
        let (mut before, mut ledger) = initialize(
            base.clone(),
            primary,
            Status::Rejected,
            true,
            false,
            &record,
            &c,
        );
        let mut problem = before.problems[&primary.0].clone();
        problem.id = foreign.0;
        before.problems.insert(foreign.0, problem);
        before
            .candidates
            .insert(foreign, before.candidates[&primary].clone());
        let h = before.height() + 1;
        let m = before.m3.as_mut().unwrap();
        m.results.insert(foreign, m.results[&primary].clone());
        let mut records = vec![];
        for (index, (key, outcome)) in [primary, foreign].into_iter().zip(outcomes).enumerate() {
            m.queue.push(Job {
                queue_index: index as u64,
                key,
                kind: RecordKind::Recheck,
                avail: h - c.m3.as_ref().unwrap().k,
                epoch_override: Some(0),
            });
            m.pending_rechecks.insert(key);
            let mut v = record.clone();
            v.kind = RecordKind::Recheck;
            v.outcome = outcome;
            v.reason = if outcome == Outcome::Accept {
                0
            } else {
                KERNEL_FAULT
            };
            v.fuel = 0;
            v.gate_epoch = 0;
            records.push(IndexedVerdict {
                queue_index: index as u64,
                record: v,
            });
        }
        l1.number = h;
        let forks = ForkTable::genesis(&c)?;
        let out = before.step_with_verdicts(&c, &l1, &[], &records, &forks)?;
        let mut prefix = vec![
            boole_derivation::system::l1_info(&l1)?,
            boole_derivation::system::m2_root_deposit(&l1, out.state.root)?,
        ];
        prefix.extend(verdict_deposits(&c, &l1, &records)?);
        for payment in &out.payments {
            prefix.push(reward_deposit(&c, &l1, payment)?);
        }
        let eligible = expected_compensation(&before, &c, &l1, &prefix, None)?;
        let accepted: BTreeSet<Key> = [primary, foreign]
            .into_iter()
            .zip(outcomes)
            .filter_map(|(key, outcome)| (outcome == Outcome::Accept).then_some(key))
            .collect();
        assert_eq!(
            eligible, accepted,
            "a verdict borrowed the other problem's result"
        );
        ledger.observe(
            &before,
            &out.state,
            &c,
            &l1,
            &out.events,
            &out.payments,
            &prefix,
            &vec![true; prefix.len()],
        )?;
        assert_eq!(ledger.rewards.len(), accepted.len());
        assert_eq!(ledger.admissions.len(), accepted.len());
        assert_eq!(
            ledger
                .rewards
                .values()
                .map(|r| r.key)
                .collect::<BTreeSet<_>>(),
            accepted
        );
        assert_eq!(
            ledger
                .admissions
                .values()
                .map(|(k, _)| *k)
                .collect::<BTreeSet<_>>(),
            accepted
        );
        assert_eq!(ledger.totals(), reward_totals(&out.state, &c));
        grants += accepted.len();
    }
    assert_eq!(grants, 4);
    eprintln!("FROZEN-ALIASES compensation_words=4 grants=4 findings=0");
    Ok(())
}

#[test]
fn t82_alias_two_checking_quarantine_targets_remain_ambiguous() -> Result<()> {
    let (base, c, mut l1, key, record) = seed()?;
    let (mut before, _) = initialize(base, key, Status::Checking, false, false, &record, &c);
    let foreign = ([244; 32], key.1);
    let mut p = before.problems[&key.0].clone();
    p.id = foreign.0;
    before.problems.insert(foreign.0, p);
    before
        .candidates
        .insert(foreign, before.candidates[&key].clone());
    let height = before.height();
    let m = before.m3.as_mut().unwrap();
    for (index, key) in [key, foreign].into_iter().enumerate() {
        m.queue.push(Job {
            queue_index: index as u64,
            key,
            kind: RecordKind::Verdict,
            avail: height,
            epoch_override: None,
        });
    }
    let row = ForkRow {
        epoch: 1,
        height: before.height() + 1,
        gate_rule_digest: [90; 32],
        kind: ForkKind::Quarantine,
        list: vec![ForkItem {
            commit_hash: key.1,
            disposition: Disposition::Recheck,
        }],
    };
    let error =
        check_quarantine_targets(&before, Some(&row)).expect_err("ambiguous selector accepted");
    assert_eq!(error.to_string(), "ambiguous quarantine target");
    let mut forks = ForkTable::genesis(&c)?;
    forks.rows.push(row);
    l1.number = before.height() + 1;
    assert!(before
        .step_with_verdicts(&c, &l1, &[], &[], &forks)
        .is_err());
    Ok(())
}
