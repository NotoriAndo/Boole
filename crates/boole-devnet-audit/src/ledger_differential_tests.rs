//! Frozen comparisons of the separate ledger decoder; S14 validation is shared.
use super::*;
use boole_derivation::{
    m2::{Candidate, Input},
    m3::*,
};
// Frozen before execution: 10 symbols, every word of length 0..=3, 8 status
// seeds x 2 winner seeds x 2 prior-compensation seeds. The necessary invariant
// proved in docs/devnet-audit-compensation-seeds-v0.md excludes 15 impossible
// CANCELLED-compensation contexts uniformly before applying any word. Retained:
// 17 contexts x 1,111 words = 18,887 sequences. Component snapshots are not a
// certificate that every field is jointly reachable from genesis.
const ALPHABET: usize = 10;
const MAX_LEN: u32 = 3;
const SEQUENCES: usize = 18_887;
type Trace = BTreeMap<Key, (Status, Option<VerdictRecord>)>;
thread_local! {static TRACE:std::cell::RefCell<Trace>=const {std::cell::RefCell::new(BTreeMap::new())};}
thread_local! {
    static DUPLICATE_CHECKS:std::cell::Cell<usize>=const {std::cell::Cell::new(0)};
}
pub(super) fn admitted(key: Key) {
    // Called by the actual auditor only after decoding a valid admission event.
    TRACE.with_borrow_mut(|trace| {
        if let Some((status, _)) = trace.get_mut(&key) {
            if *status == Status::Passed {
                *status = Status::Accepted;
            }
        }
    });
}
pub(super) fn duplicate_compensation() {
    DUPLICATE_CHECKS.set(DUPLICATE_CHECKS.get() + 1);
}
pub(super) fn reset_trace(before: &State) {
    DUPLICATE_CHECKS.set(0);
    TRACE.with_borrow_mut(|trace| {
        *trace = before
            .candidates
            .iter()
            .map(|(key, candidate)| {
                (
                    *key,
                    (
                        candidate.status(),
                        before.m3.as_ref().unwrap().results.get(key).cloned(),
                    ),
                )
            })
            .collect()
    });
}
pub(super) fn trace(key: Key, status: Status, record: &VerdictRecord) {
    TRACE.with_borrow_mut(|trace| {
        trace.insert(key, (status, Some(record.clone())));
    });
}
fn seed() -> Result<(State, Config, L1Block, Key, VerdictRecord)> {
    let tape: Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m3/round-v1.3.json"))?;
    let c: Config = serde_json::from_str(tape["config_json"].as_str().unwrap())?;
    let f: ForkTable = serde_json::from_value(tape["forks"].clone())?;
    let mut state = State::new(&c, 30_000_000)?;
    for row in tape["heights"].as_array().unwrap() {
        let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
        let txs: Vec<String> = serde_json::from_value(row["result"]["transactions"].clone())?;
        let deposits: Vec<_> = txs
            .iter()
            .skip(2)
            .take_while(|s| s.starts_with("0x7e"))
            .filter(|s| {
                let raw = hex_bytes(s, "tx").unwrap();
                boole_derivation::rlp::decode(&raw[1..])
                    .unwrap()
                    .children
                    .unwrap()[7]
                    .bytes
                    .starts_with(b"m3vd")
            })
            .cloned()
            .collect();
        let records = decode_verdict_deposits(&c, &l1, &deposits)?;
        if let Some(v) = records.first() {
            let job = &state.m3.as_ref().unwrap().queue[v.queue_index as usize];
            ensure!(job.kind == v.record.kind && job.key.1 == v.record.commit_hash);
            let key = job.key;
            return Ok((state, c, l1, key, v.record.clone()));
        }
        let logs: Vec<boole_derivation::InboxLog> = serde_json::from_value(row["logs"].clone())?;
        let inputs: Vec<_> = logs
            .iter()
            .filter_map(|log| {
                let b = hex_bytes(&log.data, "log").unwrap();
                boole_derivation::inbox::item(&b)
                    .ok()
                    .map(|(kind, body)| Input {
                        kind,
                        body: body.to_vec(),
                        transaction_index: log.transaction_index,
                        log_index: log.log_index,
                    })
            })
            .collect();
        state = state
            .step_with_verdicts(&c, &l1, &inputs, &records, &f)?
            .state;
    }
    anyhow::bail!("no completed transition seed")
}
fn initialize(
    mut s: State,
    key: Key,
    status: Status,
    winner: bool,
    prior_compensation: bool,
    record: &VerdictRecord,
    c: &Config,
) -> (State, Ledger) {
    if let Candidate::Live(l) = s.candidates.get_mut(&key).unwrap() {
        l.status = status;
    }
    let height = s.height();
    let p = s.problems.get_mut(&key.0).unwrap();
    p.winner = winner.then_some([222; 32]);
    p.closed = winner.then_some(height);
    let m = s.m3.as_mut().unwrap();
    m.queue.clear();
    m.next_applied = 0;
    m.clock = FuelClock::default();
    m.results.clear();
    m.rewards.clear();
    m.admissions.clear();
    m.pending_rechecks.clear();
    let mut old = record.clone();
    old.outcome = Outcome::Reject;
    old.reason = KERNEL_FAULT;
    m.results.insert(key, old);
    let mut ledger = Ledger::default();
    if prior_compensation {
        let solver = s.candidates[&key]
            .live()
            .unwrap()
            .open
            .as_ref()
            .unwrap()
            .solver;
        let amount = c.m3.as_ref().unwrap().reward[usize::from(s.problems[&key.0].slot)];
        m.rewards.insert(
            0,
            boole_derivation::m3::Reward {
                id: 0,
                key,
                solver,
                amount,
                vest_h: height,
                status: RewardStatus::Cancelled,
                compensation: true,
            },
        );
        ledger.rewards.insert(
            0,
            super::Reward {
                key,
                solver,
                amount,
                vest: height,
                status: 3,
                compensation: true,
            },
        );
        m.admissions.insert(
            1,
            Admission {
                k: 1,
                key,
                retired: true,
            },
        );
        ledger.admissions.insert(1, (key, true));
    }
    if status == Status::Accepted {
        let k = m.admissions.len() as u64 + 1;
        m.admissions.insert(
            k,
            Admission {
                k,
                key,
                retired: false,
            },
        );
        ledger.admissions.insert(k, (key, false));
    }
    (s, ledger)
}
#[test]
fn t82_fix2_unreachable_prior_compensation_seeds_are_rejected() -> Result<()> {
    // The preserved 15 findings start in these three impossible prestates.
    // Empty words prove the seed, rather than a later transition, is refused.
    for status in [Status::Checking, Status::Passed, Status::Rejected] {
        let x = fix2_case(status, true, true, &[])?;
        let error = expected_compensation(&x.before, &x.c, &x.l1, &x.prefix, None)
            .expect_err("auditor accepted unreachable cancelled-compensation seed");
        assert!(
            error.downcast_ref::<AdmissionViolation>().is_some(),
            "{error:#}"
        );
        let error = fix2_observe_input(x).expect_err("public ledger accepted unreachable seed");
        assert!(
            error.downcast_ref::<AdmissionViolation>().is_some(),
            "{error:#}"
        );
    }
    // Necessary-invariant positive control, not a full-history certificate.
    let x = fix2_case(Status::Stale, true, true, &[])?;
    assert!(expected_compensation(&x.before, &x.c, &x.l1, &x.prefix, None)?.is_empty());
    Ok(())
}

fn reward_totals(s: &State, c: &Config) -> Value {
    let m = s.m3.as_ref().unwrap();
    let sum = |status| {
        m.rewards
            .values()
            .filter(|r| r.status == status)
            .map(|r| r.amount)
            .sum::<u128>()
            .to_string()
    };
    let vested = sum(RewardStatus::Vested);
    json!({"locked":sum(RewardStatus::Locked),"vested":vested,"cancelled":sum(RewardStatus::Cancelled),"compensation":m.rewards.values().filter(|r|r.compensation).map(|r|r.amount).sum::<u128>().to_string(),"minted":if c.m3.as_ref().unwrap().mint_enabled {vested}else{"0".into()},"would_be":sum(RewardStatus::Vested),"failed_calls":"0"})
}
#[test]
fn t82_frozen_exhaustive_fold_vs_audit_transition_sequences() -> Result<()> {
    let (base, c, mut l1, key, record) = seed()?;
    let statuses = [
        Status::Checking,
        Status::Passed,
        Status::Rejected,
        Status::Rejected,
        Status::Accepted,
        Status::Superseded,
        Status::Void,
        Status::Stale,
    ];
    let mut excluded_contexts = 0;
    let mut retained_contexts = 0;
    let mut count = 0;
    let mut steps = 0;
    let mut invalid = 0;
    let mut compensation_cases = 0;
    let mut quarantine_cases = 0;
    let mut valid_sequences = 0;
    let mut duplicate_checks = 0;
    let mut activation_controls = 0;
    let mut auditor_admission_cases = 0;
    let mut divergences = vec![];
    for (seed_index, status) in statuses.into_iter().enumerate() {
        for winner in [false, true] {
            for prior_compensation in [false, true] {
                // Fixed proof rule, never a whitelist of observed disagreements.
                let (context, _) = initialize(
                    base.clone(),
                    key,
                    status,
                    winner,
                    prior_compensation,
                    &record,
                    &c,
                );
                if prior_compensation && !(status == Status::Stale && winner) {
                    ensure!(
                        check_compensation_context(&context).is_err(),
                        "auditor accepted a proof-excluded seed"
                    );
                    excluded_contexts += 1;
                    continue;
                }
                check_compensation_context(&context)?;
                retained_contexts += 1;
                for length in 0..=MAX_LEN {
                    for word in 0..ALPHABET.pow(length) {
                        count += 1;
                        let (mut before, mut ledger) = initialize(
                            base.clone(),
                            key,
                            status,
                            winner,
                            prior_compensation,
                            &record,
                            &c,
                        );
                        if seed_index == 3 {
                            before
                                .m3
                                .as_mut()
                                .unwrap()
                                .results
                                .get_mut(&key)
                                .unwrap()
                                .reason = 0x0101;
                        }
                        let mut forks = ForkTable::genesis(&c)?;
                        let h = before.height() + 1;
                        l1.number = h;
                        l1.timestamp = 1_790_000_000 + 12 * h;
                        l1.parent_hash = l1.hash.clone();
                        l1.hash = boole_derivation::attributes::hex_string(&[h as u8; 32]);
                        let mut n = word;
                        let mut sequence = vec![];
                        let mut records = vec![];
                        for _ in 0..length {
                            let symbol = n % ALPHABET;
                            n /= ALPHABET;
                            sequence.push(symbol);
                            steps += 1;
                            let kind = if !(4..8).contains(&symbol) {
                                RecordKind::Verdict
                            } else {
                                RecordKind::Recheck
                            };
                            let (outcome, reason) = match symbol % 4 {
                                0 => (Outcome::Accept, 0),
                                1 => (Outcome::Reject, KERNEL_FAULT),
                                2 => (Outcome::Reject, 0x0101),
                                _ => (Outcome::Void, QUARANTINE_VOID),
                            };
                            let (outcome, reason) = if symbol == 9 {
                                (Outcome::Void, QUARANTINE_VOID)
                            } else {
                                (outcome, reason)
                            };
                            let m = before.m3.as_mut().unwrap();
                            let index = m.queue.len() as u64;
                            m.queue.push(Job {
                                queue_index: index,
                                key,
                                kind,
                                avail: h - c.m3.as_ref().unwrap().k,
                                epoch_override: Some(0),
                            });
                            if kind == RecordKind::Recheck {
                                m.pending_rechecks.insert(key);
                            }
                            let mut v = record.clone();
                            v.kind = kind;
                            v.outcome = outcome;
                            v.reason = reason;
                            v.fuel = 0;
                            v.gate_epoch = 0;
                            if symbol >= 8 {
                                let epoch = forks.rows.len() as u32;
                                forks.rows.push(ForkRow {
                                    epoch,
                                    height: h,
                                    gate_rule_digest: [80 + epoch as u8; 32],
                                    kind: ForkKind::Quarantine,
                                    list: vec![ForkItem {
                                        commit_hash: key.1,
                                        disposition: if symbol == 9 {
                                            Disposition::Void
                                        } else {
                                            Disposition::Recheck
                                        },
                                    }],
                                });
                                v.gate_epoch = epoch;
                            }
                            records.push(IndexedVerdict {
                                queue_index: index,
                                record: v,
                            });
                        }
                        let mut prefix = vec![
                            boole_derivation::system::l1_info(&l1)?,
                            boole_derivation::system::m2_root_deposit(&l1, before.root)?,
                        ];
                        prefix.extend(verdict_deposits(&c, &l1, &records)?);
                        // The real fork input validator and independent verdict decoder
                        // both run. No fold-rejected fork word gets an exemption.
                        DUPLICATE_CHECKS.set(0);
                        let predicted = (|| -> Result<_> {
                            forks
                                .validate(&c)
                                .map_err(|_| anyhow::anyhow!("fork input validation"))?;
                            forks.validate_history(&before)?;
                            expected_compensation(
                                &before,
                                &c,
                                &l1,
                                &prefix,
                                forks.rows.iter().skip(1).find(|r| r.height == h),
                            )
                        })();
                        duplicate_checks += DUPLICATE_CHECKS.get();
                        let folded = before.step_with_verdicts(&c, &l1, &[], &records, &forks);
                        let step = match folded {
                            Ok(step) => step,
                            Err(error) => {
                                invalid += 1;
                                if predicted.is_ok() {
                                    divergences.push(format!("seed={seed_index} winner={winner} prior_compensation={prior_compensation} word={word} length={length} prefix={sequence:?} fold={error} audit-input-and-verdict-checks=accepted"));
                                }
                                continue;
                            }
                        };
                        let checked = (|| -> Result<()> {
                            let eligible = predicted?;
                            compensation_cases += usize::from(!eligible.is_empty());
                            quarantine_cases += usize::from(sequence.iter().any(|s| *s >= 8));
                            valid_sequences += 1;
                            ensure!(
                                step.verdicts.len() == records.len(),
                                "not all frozen symbols applied in the same block"
                            );
                            crate::verify_fork_events(
                                &forks,
                                &before,
                                &step.state,
                                h,
                                &step.events,
                            )?;
                            for row in forks.rows.iter().skip(1).filter(|r| r.height == h) {
                                // Verify the real activation bytes, and prove a modified
                                // byte is rejected rather than using an empty event stream.
                                let activation = events(&step.events)?
                                    .into_iter()
                                    .find(|(tag, _)| *tag == 8)
                                    .context("actual activation")?
                                    .1;
                                let at =
                                    activation.as_ptr() as usize - step.events.as_ptr() as usize;
                                let mut changed = step.events.clone();
                                changed[at] ^= 1;
                                ensure!(
                                    crate::verify_fork_events(
                                        &forks,
                                        &before,
                                        &step.state,
                                        h,
                                        &changed
                                    )
                                    .is_err(),
                                    "changed activation accepted"
                                );
                                activation_controls += 1;
                                for item in &row.list {
                                    let old = before.m3.as_ref().unwrap();
                                    let targets: Vec<_> = old.queue[old.next_applied..]
                                        .iter()
                                        .filter(|j| {
                                            j.kind == RecordKind::Verdict
                                                && j.key.1 == item.commit_hash
                                                && before.candidates[&j.key].status()
                                                    == Status::Checking
                                        })
                                        .collect();
                                    ensure!(targets.len() == 1, "quarantine target uniqueness");
                                    let job = targets[0];
                                    ensure!(
                                        step.state.m3.as_ref().unwrap().queue
                                            [job.queue_index as usize]
                                            .epoch_override
                                            == Some(row.epoch),
                                        "quarantine original-index epoch"
                                    );
                                    let applied = step
                                        .verdicts
                                        .iter()
                                        .find(|v| v.queue_index == job.queue_index)
                                        .context("quarantine release applied")?;
                                    ensure!(
                                        applied.record.gate_epoch == row.epoch,
                                        "quarantine release epoch"
                                    );
                                    if item.disposition == Disposition::Void {
                                        ensure!(
                                            applied.record.outcome == Outcome::Void
                                                && applied.record.reason == QUARANTINE_VOID,
                                            "quarantine VOID disposition"
                                        );
                                    }
                                }
                            }

                            let decoded = events(&step.events)?;
                            let mut status = before.candidates[&key].status();
                            let mut decoded_record =
                                before.m3.as_ref().unwrap().results.get(&key).cloned();
                            let mut compensation = BTreeSet::new();
                            for (tag, b) in decoded {
                                if tag == 5 {
                                    decoded_record = Some(VerdictRecord::decode(&b[8..])?);
                                }
                                if tag == 6 && b[116] == 4 {
                                    compensation.insert(super::key(b, 8)?);
                                }
                            }
                            // All status values are parsed from the auditor's own event
                            // slices; enum names are mapped explicitly below, not cast.
                            if let Some((_, b)) = events(&step.events)?
                                .into_iter()
                                .rev()
                                .find(|(tag, b)| *tag == 2 && super::key(b, 0).ok() == Some(key))
                            {
                                status = decode_status(b[64])?;
                            }
                            ensure!(status == step.state.candidates[&key].status(), "status");
                            ensure!(
                                decoded_record
                                    == step.state.m3.as_ref().unwrap().results.get(&key).cloned(),
                                "reason/kind/outcome"
                            );
                            ensure!(eligible == compensation, "compensation eligibility");
                            prefix[1] =
                                boole_derivation::system::m2_root_deposit(&l1, step.state.root)?;
                            prefix.truncate(2);
                            prefix.extend(verdict_deposits(&c, &l1, &step.verdicts)?);
                            for payment in &step.payments {
                                prefix.push(reward_deposit(&c, &l1, payment)?);
                            }
                            ledger.observe(
                                &before,
                                &step.state,
                                &c,
                                &l1,
                                &step.events,
                                &step.payments,
                                &prefix,
                                &vec![true; prefix.len()],
                            )?;
                            let audit_trace = TRACE.with_borrow(Clone::clone);
                            let (predicted_status, predicted_record) = &audit_trace[&key];
                            ensure!(
                                *predicted_status == step.state.candidates[&key].status(),
                                "auditor ordered status/admission differs from fold"
                            );
                            ensure!(
                                *predicted_record
                                    == step.state.m3.as_ref().unwrap().results.get(&key).cloned(),
                                "auditor reason/kind/outcome differs from fold"
                            );
                            // Admissions decode fold-produced events. This checks their
                            // consistency with fold state, preserving the context winner;
                            // it does not independently predict the winner.
                            let new_admission =
                                ledger.admissions.iter().find_map(|(id, (identity, _))| {
                                    (!before.m3.as_ref().unwrap().admissions.contains_key(id)
                                        && identity.0 == key.0)
                                        .then_some(identity.1)
                                });
                            auditor_admission_cases += usize::from(new_admission.is_some());
                            let audited_winner = before.problems[&key.0].winner.or(new_admission);
                            ensure!(
                                audited_winner == step.state.problems[&key.0].winner,
                                "auditor admission winner differs from fold"
                            );
                            ensure!(
                                ledger.totals() == reward_totals(&step.state, &c),
                                "reward ledger effect"
                            );
                            Ok(())
                        })();
                        if let Err(error) = checked {
                            divergences.push(format!("seed={seed_index} winner={winner} prior_compensation={prior_compensation} word={word} length={length} prefix={sequence:?}: {error:#}"));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(excluded_contexts, 15);
    assert_eq!(retained_contexts, 17);
    assert_eq!(count, SEQUENCES);
    assert_eq!(steps, 54_570);
    assert_eq!(
        duplicate_checks, 0,
        "unreachable duplicate-compensation precondition"
    );
    assert_eq!(
        (
            valid_sequences,
            invalid,
            compensation_cases,
            quarantine_cases,
            activation_controls,
            auditor_admission_cases
        ),
        (558, 18_329, 25, 22, 22, 51)
    );
    eprintln!(
        "FROZEN-DIFFERENTIAL excluded_contexts={excluded_contexts} retained_contexts={retained_contexts} sequences={count} steps={steps} fold_rejected={invalid} fold_accepted={valid_sequences} compensation={compensation_cases} quarantine={quarantine_cases} duplicate_checks={duplicate_checks} activation_controls={activation_controls} auditor_admission_cases={auditor_admission_cases} divergences={}",
        divergences.len()
    );
    for finding in &divergences {
        eprintln!("DIVERGENCE {finding}");
    }
    assert!(
        divergences.is_empty(),
        "fold/audit divergence: preserve and review every finding"
    );
    Ok(())
}
fn decode_status(n: u8) -> Result<Status> {
    [
        Status::Committed,
        Status::Open,
        Status::Checking,
        Status::Passed,
        Status::Accepted,
        Status::Rejected,
        Status::Superseded,
        Status::Void,
        Status::Stale,
    ]
    .into_iter()
    .find(|s| *s as u8 == n)
    .context("status event")
}

struct Fix2Case {
    before: State,
    ledger: Ledger,
    c: Config,
    l1: L1Block,
    forks: ForkTable,
    records: Vec<IndexedVerdict>,
    prefix: Vec<String>,
}
fn fix2_case(status: Status, winner: bool, prior: bool, word: &[usize]) -> Result<Fix2Case> {
    let (base, c, mut l1, key, template) = seed()?;
    let (mut before, ledger) = initialize(base, key, status, winner, prior, &template, &c);
    let h = before.height() + 1;
    l1.number = h;
    l1.timestamp = 1_790_000_000 + 12 * h;
    l1.parent_hash = l1.hash.clone();
    l1.hash = boole_derivation::attributes::hex_string(&[h as u8; 32]);
    let mut forks = ForkTable::genesis(&c)?;
    let mut records = vec![];
    for &symbol in word {
        let kind = if (4..8).contains(&symbol) {
            RecordKind::Recheck
        } else {
            RecordKind::Verdict
        };
        let (outcome, reason) = if symbol == 9 {
            (Outcome::Void, QUARANTINE_VOID)
        } else {
            match symbol % 4 {
                0 => (Outcome::Accept, 0),
                1 => (Outcome::Reject, KERNEL_FAULT),
                2 => (Outcome::Reject, 0x0101),
                _ => (Outcome::Void, QUARANTINE_VOID),
            }
        };
        let m = before.m3.as_mut().unwrap();
        let index = m.queue.len() as u64;
        m.queue.push(Job {
            queue_index: index,
            key,
            kind,
            avail: h - c.m3.as_ref().unwrap().k,
            epoch_override: Some(0),
        });
        if kind == RecordKind::Recheck {
            m.pending_rechecks.insert(key);
        }
        let mut record = template.clone();
        record.kind = kind;
        record.outcome = outcome;
        record.reason = reason;
        record.fuel = 0;
        record.gate_epoch = 0;
        if symbol >= 8 {
            let epoch = forks.rows.len() as u32;
            forks.rows.push(ForkRow {
                epoch,
                height: h,
                gate_rule_digest: [80 + epoch as u8; 32],
                kind: ForkKind::Quarantine,
                list: vec![ForkItem {
                    commit_hash: key.1,
                    disposition: if symbol == 9 {
                        Disposition::Void
                    } else {
                        Disposition::Recheck
                    },
                }],
            });
            record.gate_epoch = epoch;
        }
        records.push(IndexedVerdict {
            queue_index: index,
            record,
        });
    }
    let mut prefix = vec![
        boole_derivation::system::l1_info(&l1)?,
        boole_derivation::system::m2_root_deposit(&l1, before.root)?,
    ];
    prefix.extend(verdict_deposits(&c, &l1, &records)?);
    Ok(Fix2Case {
        before,
        ledger,
        c,
        l1,
        forks,
        records,
        prefix,
    })
}
// Exercise the public ledger boundary independently of a successful fold. The
// caller has already authenticated the activation row; no output transition is
// invented here to make an invalid input pass the fold.
fn fix2_observe_input(mut x: Fix2Case) -> Result<()> {
    let mut after = x.before.clone();
    after
        .m3
        .as_mut()
        .unwrap()
        .fork_history
        .extend(x.forks.rows.iter().skip(1).cloned());
    x.ledger.observe(
        &x.before,
        &after,
        &x.c,
        &x.l1,
        &[],
        &[],
        &x.prefix,
        &vec![true; x.prefix.len()],
    )
}
#[test]
fn t82_fix2_quarantine_nonchecking_smallest_preserved_word_is_rejected() -> Result<()> {
    // Preserved seed 5, winner=false, prior=false, word=[8].
    let x = fix2_case(Status::Superseded, false, false, &[8])?;
    let fold = x
        .before
        .step_with_verdicts(&x.c, &x.l1, &[], &x.records, &x.forks)
        .err()
        .context("fold should refuse")?;
    assert!(fold
        .to_string()
        .contains("quarantine target is not queued CHECKING"));
    let error = expected_compensation(&x.before, &x.c, &x.l1, &x.prefix, x.forks.rows.get(1))
        .expect_err("auditor accepted non-CHECKING quarantine target");
    assert!(
        error
            .to_string()
            .contains("quarantine target is not queued CHECKING"),
        "{error:#}"
    );
    let error = fix2_observe_input(x).expect_err("public ledger accepted non-CHECKING target");
    assert!(
        error
            .to_string()
            .contains("quarantine target is not queued CHECKING"),
        "{error:#}"
    );
    Ok(())
}
#[test]
fn t82_fix2_valid_queued_checking_quarantine_stays_accepted() -> Result<()> {
    for symbol in [8, 9] {
        let mut x = fix2_case(Status::Checking, false, false, &[symbol])?;
        expected_compensation(&x.before, &x.c, &x.l1, &x.prefix, x.forks.rows.get(1))?;
        let step = x
            .before
            .step_with_verdicts(&x.c, &x.l1, &[], &x.records, &x.forks)?;
        x.prefix[1] = boole_derivation::system::m2_root_deposit(&x.l1, step.state.root)?;
        for payment in &step.payments {
            x.prefix.push(reward_deposit(&x.c, &x.l1, payment)?);
        }
        x.ledger.observe(
            &x.before,
            &step.state,
            &x.c,
            &x.l1,
            &step.events,
            &step.payments,
            &x.prefix,
            &vec![true; x.prefix.len()],
        )?;
    }
    Ok(())
}

#[test]
fn t82_fix2_quarantine_ambiguous_smallest_preserved_word_is_rejected() -> Result<()> {
    // Preserved seed 0, winner=false, prior=false, word=[9,0].
    let x = fix2_case(Status::Checking, false, false, &[9, 0])?;
    let fold = x
        .before
        .step_with_verdicts(&x.c, &x.l1, &[], &x.records, &x.forks)
        .err()
        .context("fold should refuse")?;
    assert!(fold.to_string().contains("ambiguous quarantine target"));
    let error = expected_compensation(&x.before, &x.c, &x.l1, &x.prefix, x.forks.rows.get(1))
        .expect_err("auditor accepted ambiguous quarantine target");
    assert!(
        error.to_string().contains("ambiguous quarantine target"),
        "{error:#}"
    );
    let error = fix2_observe_input(x).expect_err("public ledger accepted ambiguous target");
    assert!(
        error.to_string().contains("ambiguous quarantine target"),
        "{error:#}"
    );
    Ok(())
}

#[path = "ledger_history_tests.rs"]
mod reachable;

#[path = "ledger_alias_tests.rs"]
mod aliases;
