use anyhow::{Context, Result};
use boole_derivation::{
    attributes::hex_bytes,
    m2::{Config, Input, State},
    m3::{decode_verdict_deposits, ForkTable},
    L1Block,
};
use boole_devnet_audit::ledger::Ledger;
use serde_json::Value;
fn frames_from(tape_json: &str) -> Result<Vec<Frame>> {
    let tape: Value = serde_json::from_str(tape_json)?;
    let c: Config = serde_json::from_str(tape["config_json"].as_str().context("config")?)?;
    let f: ForkTable = serde_json::from_value(tape["forks"].clone())?;
    let mut s = State::new(&c, 30_000_000)?;
    let mut out = vec![];
    for row in tape["heights"].as_array().unwrap() {
        let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
        let txs: Vec<String> = serde_json::from_value(row["result"]["transactions"].clone())?;
        let mut deposits = vec![];
        for tx in txs.iter().skip(2).take_while(|tx| tx.starts_with("0x7e")) {
            let raw = hex_bytes(tx, "tx")?;
            let fields = boole_derivation::rlp::decode(&raw[1..])
                .map_err(anyhow::Error::msg)?
                .children
                .unwrap();
            if fields[7].bytes.starts_with(b"m3vd") {
                deposits.push(tx.clone());
            }
        }
        let verdicts = decode_verdict_deposits(&c, &l1, &deposits)?;
        let logs: Vec<boole_derivation::InboxLog> = serde_json::from_value(row["logs"].clone())?;
        let mut inputs = vec![];
        for log in logs {
            let bytes = hex_bytes(&log.data, "item")?;
            if let Ok((kind, body)) = boole_derivation::inbox::item(&bytes) {
                if (2..=15).contains(&kind) {
                    inputs.push(Input {
                        kind,
                        body: body.to_vec(),
                        transaction_index: log.transaction_index,
                        log_index: log.log_index,
                    });
                }
            }
        }
        let step = s.step_with_verdicts(&c, &l1, &inputs, &verdicts, &f)?;
        boole_devnet_audit::verify_fork_events(&f, &s, &step.state, l1.number, &step.events)?;
        let statuses = row["result"]["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["status"] == "0x1")
            .collect();
        out.push((
            s.clone(),
            step.state.clone(),
            c.clone(),
            l1,
            step.events,
            step.payments,
            txs,
            statuses,
        ));
        s = step.state;
    }
    Ok(out)
}
fn frames() -> Result<Vec<Frame>> {
    frames_from(include_str!("../../../fixtures/devnet-m3/round-v1.3.json"))
}
type Frame = (
    State,
    State,
    Config,
    L1Block,
    Vec<u8>,
    Vec<boole_derivation::m3::RewardPayment>,
    Vec<String>,
    Vec<bool>,
);
fn event(events: &[u8], tag: u8) -> Option<Vec<u8>> {
    let mut at = 0;
    while at < events.len() {
        let len = match events[at] {
            1 => 19,
            2 => 66,
            3 => 47,
            4 => 37,
            5 => 89,
            6 => 118,
            7 => 74,
            8 => 70,
            _ => panic!("event format"),
        };
        if events[at] == tag {
            return Some(events[at..at + len].to_vec());
        }
        at += len;
    }
    None
}
fn attack(attack: u8) -> Result<()> {
    let mut ledger = Ledger::default();
    let mut tested = false;
    for (before, after, c, l1, mut events, payments, txs, statuses) in frames()? {
        let reward = event(&events, 6);
        let admission = event(&events, 7);
        if attack == 0 && reward.as_ref().is_some_and(|r| r[117] == 2) {
            events.extend(reward.unwrap());
            assert!(
                ledger
                    .observe(&before, &after, &c, &l1, &events, &payments, &txs, &statuses)
                    .is_err(),
                "reward vested twice accepted"
            );
            tested = true;
            break;
        }
        if attack == 2 && !payments.is_empty() {
            let mut disabled = c.clone();
            disabled.m3.as_mut().unwrap().mint_enabled = false;
            assert!(
                ledger
                    .observe(&before, &after, &disabled, &l1, &events, &payments, &txs, &statuses)
                    .is_err(),
                "disabled mint accepted"
            );
            tested = true;
            break;
        }
        if let Some(admission) = admission.filter(|_| attack == 3) {
            ledger.observe(
                &before, &after, &c, &l1, &events, &payments, &txs, &statuses,
            )?;
            assert!(
                ledger
                    .observe(&after, &after, &c, &l1, &admission, &[], &[], &[])
                    .is_err(),
                "reused admission k accepted"
            );
            tested = true;
            break;
        }
        ledger.observe(
            &before, &after, &c, &l1, &events, &payments, &txs, &statuses,
        )?;
    }
    assert!(tested);
    Ok(())
}
#[test]
fn t82_duplicate_vest_refuses() -> Result<()> {
    attack(0)
}
#[test]
fn t82_cancel_without_stale_refuses() -> Result<()> {
    let mut ledger = Ledger::default();
    for (before, after, c, l1, events, payments, txs, statuses) in frames_from(include_str!(
        "../../../fixtures/devnet-m3/kernel-fix-v1.3.json"
    ))? {
        let cancel = event_slices(&events)
            .into_iter()
            .find(|r| r[0] == 6 && r[117] == 3);
        if let Some(cancel) = cancel {
            let key = &cancel[9..73];
            let attacked: Vec<u8> = event_slices(&events)
                .into_iter()
                .filter(|r| {
                    !(r[0] == 2
                        && &r[1..65] == key
                        && r[65] == boole_derivation::m2::Status::Stale as u8)
                })
                .flatten()
                .copied()
                .collect();
            assert_ne!(
                attacked, events,
                "fixture must include the matching STALE event"
            );
            let mut control = ledger.clone();
            control.observe(
                &before, &after, &c, &l1, &events, &payments, &txs, &statuses,
            )?;
            let error = ledger
                .observe(
                    &before, &after, &c, &l1, &attacked, &payments, &txs, &statuses,
                )
                .expect_err("CANCELLED without STALE accepted");
            assert_eq!(
                error.to_string(),
                "CANCELLED without same-height STALE",
                "test failed for an unrelated invariant"
            );
            return Ok(());
        }
        ledger.observe(
            &before, &after, &c, &l1, &events, &payments, &txs, &statuses,
        )?;
    }
    anyhow::bail!("fixture never cancelled a reward")
}
#[test]
fn t82_disabled_mint_refuses() -> Result<()> {
    attack(2)
}
#[test]
fn t82_reused_k_refuses() -> Result<()> {
    attack(3)
}
#[test]
fn t82_totals_equal_the_i13_recorded_vested_amount() -> Result<()> {
    let mut ledger = Ledger::default();
    for (before, after, c, l1, events, payments, txs, statuses) in frames()? {
        ledger.observe(
            &before, &after, &c, &l1, &events, &payments, &txs, &statuses,
        )?;
    }
    let i13: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m3/miner-client-v1.3.json"
    ))?;
    let amount = i13["rewards"]["rewards"][0]["amount"].as_str().unwrap();
    let totals = ledger.totals();
    assert_eq!(totals["vested"], amount);
    assert_eq!(totals["minted"], amount);
    assert_eq!(totals["locked"], "0");
    Ok(())
}

#[test]
fn fork_and_quarantine_recordings_recompute_the_independent_ledger() -> Result<()> {
    for tape in [
        include_str!("../../../fixtures/devnet-m3/kernel-fix-v1.3.json"),
        include_str!("../../../fixtures/devnet-m3/quarantine-recheck-v1.3.json"),
        include_str!("../../../fixtures/devnet-m3/quarantine-void-v1.3.json"),
    ] {
        let mut ledger = Ledger::default();
        let rows = frames_from(tape)?;
        assert_eq!(rows.len(), 100);
        for (before, after, c, l1, events, payments, txs, statuses) in rows {
            ledger.observe(
                &before, &after, &c, &l1, &events, &payments, &txs, &statuses,
            )?;
            if let Some(mut activation) = event(&events, 8) {
                activation[2] ^= 1;
                let table: Value = serde_json::from_str(tape)?;
                let forks: ForkTable = serde_json::from_value(table["forks"].clone())?;
                assert!(boole_devnet_audit::verify_fork_events(
                    &forks,
                    &before,
                    &after,
                    l1.number,
                    &activation
                )
                .is_err());
            }
        }
    }
    Ok(())
}

fn event_slices(mut bytes: &[u8]) -> Vec<&[u8]> {
    let mut rows = vec![];
    while !bytes.is_empty() {
        let n = match bytes[0] {
            1 => 19,
            2 => 66,
            3 => 47,
            4 => 37,
            5 => 89,
            6 => 118,
            7 => 74,
            8 => 70,
            _ => panic!("event"),
        };
        rows.push(&bytes[..n]);
        bytes = &bytes[n..];
    }
    rows
}
#[test]
fn l4_every_qualifying_recheck_requires_compensation_even_if_fold_and_events_omit_it() -> Result<()>
{
    use boole_derivation::{
        m2::{Candidate, Status},
        m3::{verdict_deposits, IndexedVerdict, Outcome, RecordKind, KERNEL_FAULT},
    };
    let mut ledger = Ledger::default();
    for (mut before, after, c, l1, events, payments, txs, statuses) in frames()? {
        let accepted = after
            .m3
            .as_ref()
            .unwrap()
            .results
            .iter()
            .find(|(k, r)| {
                r.outcome == Outcome::Accept && !before.m3.as_ref().unwrap().results.contains_key(k)
            })
            .map(|(k, r)| (*k, r.clone()));
        if let Some((key, mut record)) = accepted {
            // Exercise the independently specified preconditions on the real
            // recorded completed candidate and due job: a prior kernel-fault
            // REJECT and a problem that already has a different winner.
            let mut old = record.clone();
            old.outcome = Outcome::Reject;
            old.reason = KERNEL_FAULT;
            if let Candidate::Live(candidate) = before.candidates.get_mut(&key).unwrap() {
                candidate.status = Status::Rejected;
            } else {
                panic!("retained candidate");
            }
            let height = before.height();
            let problem = before.problems.get_mut(&key.0).unwrap();
            problem.winner = Some([222; 32]);
            problem.closed = Some(height);
            let m = before.m3.as_mut().unwrap();
            m.results.insert(key, old);
            m.pending_rechecks.insert(key);
            let job = m.queue.iter_mut().find(|j| j.key == key).unwrap();
            job.kind = RecordKind::Recheck;
            let index = job.queue_index;
            record.kind = RecordKind::Recheck;
            let records = [IndexedVerdict {
                queue_index: index,
                record,
            }];
            let step =
                before.step_with_verdicts(&c, &l1, &[], &records, &ForkTable::genesis(&c)?)?;
            let compensation = event_slices(&step.events)
                .into_iter()
                .find(|r| r[0] == 6 && r[117] == 4)
                .context("qualifying fold must compensate")?;
            let mut prefix = txs[..2].to_vec();
            prefix[1] = boole_derivation::system::m2_root_deposit(&l1, step.state.root)?;
            prefix.extend(verdict_deposits(&c, &l1, &records)?);
            let statuses = vec![true; prefix.len()];
            let mut control = ledger.clone();
            control.observe(
                &before,
                &step.state,
                &c,
                &l1,
                &step.events,
                &step.payments,
                &prefix,
                &statuses,
            )?;
            let id = u64::from_be_bytes(compensation[1..9].try_into()?);
            let mut after = step.state;
            assert!(after.m3.as_mut().unwrap().rewards.remove(&id).is_some());
            let attacked: Vec<u8> = event_slices(&step.events)
                .into_iter()
                .filter(|r| *r != compensation)
                .flatten()
                .copied()
                .collect();
            let error = ledger
                .observe(
                    &before,
                    &after,
                    &c,
                    &l1,
                    &attacked,
                    &step.payments,
                    &prefix,
                    &statuses,
                )
                .expect_err("omitted compensation escaped independent audit");
            assert!(
                error.to_string().contains("compensation coverage"),
                "{error:#}"
            );
            return Ok(());
        }
        ledger.observe(
            &before, &after, &c, &l1, &events, &payments, &txs, &statuses,
        )?;
    }
    anyhow::bail!("fixture never accepted")
}

struct MissingK4 {
    root: std::path::PathBuf,
    private_scratch_seen: bool,
}
impl boole_gate_supervisor::rebuild::History for MissingK4 {
    fn origin(&mut self, _: u64) -> Result<(L1Block, Vec<boole_derivation::InboxLog>)> {
        self.private_scratch_seen = std::fs::read_dir(&self.root)?
            .any(|e| e.unwrap().file_name().to_string_lossy().starts_with("k4-"));
        anyhow::bail!("retained original unavailable")
    }
}
fn k4_probe() -> Result<(boole_devnet_audit::k4::Failure, u64, bool)> {
    let frames = frames()?;
    let state = &frames.last().unwrap().1;
    let key = *state
        .m3
        .as_ref()
        .unwrap()
        .results
        .keys()
        .next()
        .context("record")?;
    let record_height = frames
        .iter()
        .rfind(|(before, after, ..)| {
            before.m3.as_ref().unwrap().results.get(&key)
                != after.m3.as_ref().unwrap().results.get(&key)
        })
        .unwrap()
        .3
        .number;
    assert!(record_height < state.height());
    let root = boole_devnet_archive::durability::PrivateTempDir::new("audit-private-k4")?;
    let mut history = MissingK4 {
        root: root.path().to_owned(),
        private_scratch_seen: false,
    };
    let failure = boole_devnet_audit::k4::check(
        state,
        &frames[0].2,
        None,
        &mut history,
        Some(&boole_gate_supervisor::codec::Codec::portable_stub()),
        &std::collections::BTreeMap::from([(key, record_height)]),
        root.path(),
    )
    .expect_err("missing retained input accepted");
    Ok((failure, record_height, history.private_scratch_seen))
}
#[test]
fn l5_k4_mismatch_names_the_verdict_record_height() -> Result<()> {
    let (failure, record_height, _) = k4_probe()?;
    assert_eq!(
        failure.height, record_height,
        "K4 mismatch blamed the audit tip"
    );
    Ok(())
}
#[test]
fn l5_k4_scratch_is_inside_private_audit_work() -> Result<()> {
    let (_, _, private_scratch_seen) = k4_probe()?;
    assert!(
        private_scratch_seen,
        "K4 used system temp instead of the private audit work"
    );
    Ok(())
}

fn duplicate_rechecks(second_reject: bool) -> Result<()> {
    use boole_derivation::{
        attributes::hex_string,
        m2::{Candidate, Status},
        m3::{
            verdict_deposits, ForkKind, ForkRow, IndexedVerdict, Outcome, RecordKind, KERNEL_FAULT,
        },
    };
    let mut ledger = Ledger::default();
    for (original, mut before, c, mut l1, events, payments, txs, statuses) in frames()? {
        let accepted = before
            .m3
            .as_ref()
            .unwrap()
            .results
            .iter()
            .find(|(k, r)| {
                r.outcome == Outcome::Accept
                    && !original.m3.as_ref().unwrap().results.contains_key(k)
            })
            .map(|(k, r)| (*k, r.clone()));
        let Some((key, mut record)) = accepted else {
            ledger.observe(
                &original, &before, &c, &l1, &events, &payments, &txs, &statuses,
            )?;
            continue;
        };
        // Retain the recorded completed candidate and applied original queue.
        // Inject the kernel-fault/winner preconditions, then let TWO actual
        // KERNEL-FIX activations enqueue the jobs and select distinct epochs.
        if let Candidate::Live(candidate) = before.candidates.get_mut(&key).unwrap() {
            candidate.status = Status::Rejected;
        } else {
            panic!("retained candidate");
        }
        let height = before.height();
        before.problems.get_mut(&key.0).unwrap().winner = Some([222; 32]);
        before.problems.get_mut(&key.0).unwrap().closed = Some(height);
        record.outcome = Outcome::Reject;
        record.reason = KERNEL_FAULT;
        let m = before.m3.as_mut().unwrap();
        m.results.insert(key, record.clone());
        m.rewards.clear();
        m.admissions.clear();
        let mut table = ForkTable::genesis(&c)?;
        for epoch in 1..=2 {
            table.rows.push(ForkRow {
                epoch,
                height: height + u64::from(epoch),
                gate_rule_digest: [20 + epoch as u8; 32],
                kind: ForkKind::KernelFix,
                list: vec![],
            });
        }
        table.validate(&c).map_err(|s| anyhow::anyhow!("S{s}"))?;
        let mut records = vec![];
        let mut due = None;
        for h in height + 1..height + 40 {
            l1.parent_hash = l1.hash.clone();
            l1.hash = hex_string(&[h as u8; 32]);
            l1.number = h;
            l1.timestamp += 12;
            if h == height + 3 {
                let m = before.m3.as_ref().unwrap();
                let jobs = &m.queue[m.next_applied..];
                assert_eq!(jobs.len(), 2);
                assert_eq!(jobs[0].key, jobs[1].key);
                assert_eq!(
                    (jobs[0].epoch_override, jobs[1].epoch_override),
                    (Some(1), Some(2))
                );
                record.kind = RecordKind::Recheck;
                record.outcome = Outcome::Accept;
                record.reason = 0;
                record.gate_epoch = 1;
                record.fuel = c.m3.as_ref().unwrap().fuel_slot[0];
                let mut second = record.clone();
                second.gate_epoch = 2;
                second.fuel = 0;
                if second_reject {
                    second.outcome = Outcome::Reject;
                    second.reason = KERNEL_FAULT;
                }
                records = vec![
                    IndexedVerdict {
                        queue_index: jobs[0].queue_index,
                        record: record.clone(),
                    },
                    IndexedVerdict {
                        queue_index: jobs[1].queue_index,
                        record: second,
                    },
                ];
                let mut clock = m.clock.clone();
                let p = c.m3.as_ref().unwrap();
                let cap = c.q_max * c.v_per_deposit;
                let first = clock.push(p, cap, jobs[0].avail, records[0].record.fuel)?;
                let second = clock.push(p, cap, jobs[1].avail, records[1].record.fuel)?;
                assert_eq!(
                    first, second,
                    "fixture did not put both records in one block"
                );
                due = Some(first);
            }
            let step = before.step_with_verdicts(&c, &l1, &[], &records, &table)?;
            boole_devnet_audit::verify_fork_events(&table, &before, &step.state, h, &step.events)?;
            let mut prefix = vec![
                boole_derivation::system::l1_info(&l1)?,
                boole_derivation::system::m2_root_deposit(&l1, step.state.root)?,
            ];
            prefix.extend(verdict_deposits(&c, &l1, &step.verdicts)?);
            let statuses = vec![true; prefix.len()];
            if due == Some(h) {
                assert_eq!(step.verdicts.len(), 2);
                assert_eq!(
                    event_slices(&step.events)
                        .iter()
                        .filter(|r| r[0] == 6 && r[117] == 4)
                        .count(),
                    1
                );
                assert_eq!(
                    step.state.candidates[&key].status(),
                    if second_reject {
                        Status::Stale
                    } else {
                        Status::Accepted
                    }
                );
                ledger.observe(
                    &before,
                    &step.state,
                    &c,
                    &l1,
                    &step.events,
                    &step.payments,
                    &prefix,
                    &statuses,
                )?;
                return Ok(());
            }
            ledger.observe(
                &before,
                &step.state,
                &c,
                &l1,
                &step.events,
                &step.payments,
                &prefix,
                &statuses,
            )?;
            before = step.state;
        }
        anyhow::bail!("recheck fixture never reached its common due height");
    }
    anyhow::bail!("fixture never accepted")
}

#[test]
fn f5_two_same_block_accept_rechecks_compensate_once_in_fold_order() -> Result<()> {
    duplicate_rechecks(false)
}
#[test]
fn f5_same_block_accept_then_reject_keeps_compensation_and_cancellation_order() -> Result<()> {
    duplicate_rechecks(true)
}
