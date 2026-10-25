//! Genesis-derived histories; wire encoders do not seed or mutate fold state.
use super::*;
use boole_derivation::m2::{Hash, SlotParams};
mod wire {
    use boole_derivation::{m2::*, L1Block};
    use k256::ecdsa::SigningKey;
    use sha2::{Digest, Sha256};
    mod hex {
        pub fn encode(bytes: impl AsRef<[u8]>) -> String {
            boole_derivation::attributes::hex_string(bytes.as_ref())[2..].into()
        }
    }
    pub fn hash(b: &[u8]) -> Hash {
        Sha256::digest(b).into()
    }
    pub fn key() -> SigningKey {
        SigningKey::from_bytes(&hash(b"boole-devnet-m2-public-test-pool-v1").into()).unwrap()
    }
    pub fn block(h: u64) -> L1Block {
        L1Block {
            number: h,
            hash: format!("0x{}", hex::encode(hash(&h.to_be_bytes()))),
            parent_hash: format!(
                "0x{}",
                hex::encode(hash(&h.saturating_sub(1).to_be_bytes()))
            ),
            timestamp: h,
            mix_hash: format!(
                "0x{}",
                hex::encode(hash(&[h.to_be_bytes().as_slice(), b"randao"].concat()))
            ),
            base_fee_per_gas: "0x1".into(),
            parent_beacon_block_root: format!("0x{}", hex::encode([0; 32])),
        }
    }
    pub fn input(kind: u8, body: Vec<u8>, i: u64) -> Input {
        Input {
            transaction_index: i / 2,
            log_index: i,
            kind,
            body,
        }
    }
    pub fn pool_leaf(chain: u64, kind: u8, req: &[Hash], package: &[u8], salt: &Hash) -> Hash {
        boole_derivation::m2::pool_leaf(chain, kind, req, 0, package.len() as u32, package, salt)
    }

    #[allow(clippy::too_many_arguments)] // Independent encoder keeps wire field order.
    pub fn problem_body(
        slot: u8,
        round: u32,
        index: u32,
        req: &[Hash],
        salt: Hash,
        path: &[Hash],
        package: &[u8],
    ) -> Vec<u8> {
        let mut b = vec![2, slot];
        b.extend(round.to_be_bytes());
        b.extend(index.to_be_bytes());
        b.extend([2, req.len() as u8]);
        for p in req {
            b.extend(p);
        }
        b.extend(salt);
        b.push(0);
        b.extend((package.len() as u32).to_be_bytes());
        b.push(path.len() as u8);
        for p in path {
            b.extend(p);
        }
        b.extend((package.len() as u32).to_be_bytes());
        b.extend(package);
        b
    }
    pub fn body(
        c: &Config,
        p: Hash,
        solver: [u8; 20],
        salt: Hash,
        data: &[u8],
        pkgs: &[Hash],
    ) -> (Vec<u8>, Vec<Vec<u8>>, Vec<Hash>) {
        let chunks: Vec<Vec<u8>> = data.chunks(PART_SIZE).map(|x| x.to_vec()).collect();
        let leaves: Vec<_> = chunks
            .iter()
            .enumerate()
            .map(|(i, d)| part_leaf(&solver, &salt, i as u16, d))
            .collect();
        let mut b = vec![1];
        b.extend(p);
        b.extend(solver);
        b.extend(salt);
        b.push(1);
        b.extend(c.schema_id);
        b.extend(c.base_root);
        b.push(pkgs.len() as u8);
        for p in pkgs {
            b.extend(p);
        }
        b.extend([1, 1]);
        b.extend((data.len() as u64).to_be_bytes());
        b.extend(hash(data));
        b.extend((data.len() as u32).to_be_bytes());
        b.extend((chunks.len() as u16).to_be_bytes());
        b.extend(merkle_root(&leaves));
        b.extend([0, 0, 0, 0]);
        b.extend(40u16.to_be_bytes());
        b.extend([1, 0, 1, 0]);
        b.extend([0; 32]);
        b.extend(0u32.to_be_bytes());
        (b, chunks, leaves)
    }
    pub fn commit(p: Hash, ch: Hash) -> Vec<u8> {
        [vec![1], vec![3; 20], p.to_vec(), ch.to_vec()].concat()
    }
    pub fn open(b: &[u8], cancel: Hash) -> Vec<u8> {
        [vec![1], vec![4; 20], cancel.to_vec(), b.to_vec()].concat()
    }
    pub fn part(ch: Hash, i: u16, path: &[Hash], data: &[u8]) -> Vec<u8> {
        let mut b = vec![1];
        b.extend([5; 20]);
        b.extend(ch);
        b.extend(i.to_be_bytes());
        for p in path {
            b.extend(p);
        }
        b.extend(data);
        b
    }
    pub fn pool_at(c: &Config, slot: u8, root: Hash, count: u32, start: u64, seq: u32) -> Vec<u8> {
        let mut b = vec![2, slot];
        b.extend(seq.to_be_bytes());
        b.extend(root);
        b.extend(count.to_be_bytes());
        b.extend(start.to_be_bytes());
        b.extend([0; 32]);
        let pre = hash(
            &[
                b"BOOLE-M2/POOL/v2".as_slice(),
                &c.chain_id.to_be_bytes(),
                &b,
            ]
            .concat(),
        );
        let (sig, id) = key().sign_prehash_recoverable(&pre).unwrap();
        b.extend(sig.to_bytes());
        b.push(id.to_byte());
        b
    }
}
#[derive(Clone)]
struct History {
    state: State,
    ledger: Ledger,
    c: Config,
    forks: ForkTable,
    trace: Vec<Value>,
}
struct Applied {
    before: State,
    l1: L1Block,
    events: Vec<u8>,
    records: Vec<IndexedVerdict>,
    payments: Vec<RewardPayment>,
    prefix: Vec<String>,
    codes: Vec<u8>,
}
impl History {
    fn new() -> Result<Self> {
        let mut c = Config::devnet();
        c.d = 2;
        let slot = SlotParams {
            lane: 1,
            solve_deadline: 48,
            announce_gap: 4,
            reveal_grace: 4,
        };
        c.slot_table = vec![slot.clone(), slot];
        let public = wire::key().verifying_key().to_encoded_point(false);
        c.pool_key
            .copy_from_slice(&boole_derivation::user_tx::keccak(&public.as_bytes()[1..])[12..]);
        let mut profile = M3Config::devnet(2);
        profile.k = 2;
        profile.rate = 1000;
        profile.cost_fixed = 100;
        profile.maturity = 16;
        profile.notice = 0;
        c.m3 = Some(profile);
        let state = State::new(&c, 30_000_000)?;
        let forks = ForkTable::genesis(&c)?;
        Ok(Self {
            state,
            ledger: Ledger::default(),
            c,
            forks,
            trace: vec![],
        })
    }
    fn advance(
        &mut self,
        inputs: Vec<Input>,
        target: Option<(Key, Outcome, u16)>,
    ) -> Result<Applied> {
        let before = self.state.clone();
        let h = before.height() + 1;
        let l1 = wire::block(h);
        self.forks
            .validate(&self.c)
            .map_err(|code| anyhow::anyhow!("fork input validation {code}"))?;
        let m = before.m3.as_ref().unwrap();
        let p = self.c.m3.as_ref().unwrap();
        // The reference clock constructs legal fixture timing, not an independent clock oracle.
        let mut clock = m.clock.clone();
        let mut records = vec![];
        for job in m.queue.iter().skip(m.next_applied) {
            let due = clock.push(p, self.c.q_max * self.c.v_per_deposit, job.avail, 0)?;
            ensure!(due >= h, "history missed due height");
            if due > h {
                break;
            }
            let (outcome, reason) = target
                .filter(|(key, _, _)| *key == job.key)
                .map(|(_, o, r)| (o, r))
                .unwrap_or((Outcome::Accept, 0));
            let epoch = job
                .epoch_override
                .map(Ok)
                .unwrap_or_else(|| self.forks.job_epoch(job.avail, p.k))?;
            records.push(IndexedVerdict {
                queue_index: job.queue_index,
                record: VerdictRecord {
                    kind: job.kind,
                    commit_hash: job.key.1,
                    outcome,
                    reason,
                    gate_epoch: epoch,
                    fuel: 0,
                    vstmt_digest: wire::hash(b"genesis history injected verdict"),
                },
            });
        }
        let mut prefix = vec![
            boole_derivation::system::l1_info(&l1)?,
            boole_derivation::system::m2_root_deposit(&l1, before.root)?,
        ];
        prefix.extend(verdict_deposits(&self.c, &l1, &records)?);
        let predicted = expected_compensation(
            &before,
            &self.c,
            &l1,
            &prefix,
            self.forks.rows.iter().skip(1).find(|r| r.height == h),
        );
        let out = before
            .step_with_verdicts(&self.c, &l1, &inputs, &records, &self.forks)
            .with_context(|| format!("fold at height {h}"))?;
        crate::verify_fork_events(&self.forks, &before, &out.state, h, &out.events)?;
        prefix[1] = boole_derivation::system::m2_root_deposit(&l1, out.state.root)?;
        prefix.truncate(2);
        prefix.extend(verdict_deposits(&self.c, &l1, &out.verdicts)?);
        for payment in &out.payments {
            prefix.push(reward_deposit(&self.c, &l1, payment)?);
        }
        let observed = self.ledger.observe(
            &before,
            &out.state,
            &self.c,
            &l1,
            &out.events,
            &out.payments,
            &prefix,
            &vec![true; prefix.len()],
        );
        self.trace.push(serde_json::json!({"height":h,"l1":l1,"inputs":inputs.iter().map(|i|serde_json::json!({"kind":i.kind,"body":boole_derivation::attributes::hex_string(&i.body),"transaction_index":i.transaction_index,"log_index":i.log_index})).collect::<Vec<_>>(),"verdicts":records.iter().map(|v|serde_json::json!({"queue_index":v.queue_index,"record":boole_derivation::attributes::hex_string(&v.record.encode().unwrap())})).collect::<Vec<_>>(),"fold_root":boole_derivation::attributes::hex_string(&out.state.root),"codes":out.codes}));
        if predicted.is_err() || observed.is_err() {
            let statuses = |state: &State| {
                state.candidates.iter().map(|(key,candidate)|serde_json::json!({"problem":boole_derivation::attributes::hex_string(&key.0),"commit_hash":boole_derivation::attributes::hex_string(&key.1),"status":format!("{:?}",candidate.status())})).collect::<Vec<_>>()
            };
            let finding = format!(
                "DIVERGENCE reachable {}",
                serde_json::json!({"height":h,"fold":"accepted","audit_input":predicted.as_ref().err().map(|e|format!("{e:#}")),"audit_ledger":observed.as_ref().err().map(|e|format!("{e:#}")),"config_json":serde_json::to_string(&self.c)?,"forks":&self.forks,"gas_limit":30_000_000,"history":&self.trace,"before_candidates":statuses(&before),"after_candidates":statuses(&out.state)})
            );
            eprintln!("{finding}");
        }
        predicted.with_context(|| format!("fold accepted, audit input refused at height {h}"))?;
        observed.with_context(|| format!("fold accepted, audit ledger refused at height {h}"))?;
        self.state = out.state;
        ensure!(
            self.ledger.totals() == reward_totals(&self.state, &self.c),
            "history reward totals"
        );
        Ok(Applied {
            before,
            l1,
            events: out.events,
            records: out.verdicts,
            payments: out.payments,
            prefix,
            codes: out.codes,
        })
    }
    fn send(&mut self, items: Vec<Input>, code: u8) -> Result<()> {
        let count = items.len();
        let kinds: Vec<_> = items.iter().map(|i| i.kind).collect();
        let row = self.advance(items, None)?;
        ensure!(
            row.codes == vec![code; count],
            "genesis inbox refused at {} kinds={kinds:?}: {:?}, expected {code}",
            row.l1.number,
            row.codes
        );
        Ok(())
    }
    fn idle(&self) -> bool {
        let m = self.state.m3.as_ref().unwrap();
        m.next_applied == m.queue.len()
    }
    fn recheck(&mut self, key: Key, outcome: Outcome) -> Result<Vec<Applied>> {
        let epoch = self.forks.rows.len() as u32;
        self.forks.rows.push(ForkRow {
            epoch,
            height: self.state.height() + 1,
            gate_rule_digest: wire::hash(&epoch.to_be_bytes()),
            kind: ForkKind::KernelFix,
            list: vec![],
        });
        let target = Some((
            key,
            outcome,
            if outcome == Outcome::Accept {
                0
            } else {
                KERNEL_FAULT
            },
        ));
        let mut rows = vec![self.advance(vec![], target)?];
        while !self.idle() {
            ensure!(rows.len() < 32, "history drain bound");
            rows.push(self.advance(vec![], target)?);
        }
        Ok(rows)
    }
}
fn opened_history() -> Result<History> {
    let mut h = History::new()?;
    let root = wire::pool_leaf(h.c.chain_id, 2, &[], b"generator-v1", &[9; 32]);
    h.send(
        vec![
            wire::input(8, wire::pool_at(&h.c, 0, root, 1, 3, 1), 0),
            wire::input(
                8,
                wire::pool_at(
                    &h.c,
                    1,
                    wire::pool_leaf(h.c.chain_id, 2, &[], b"generator-v1", &[10; 32]),
                    1,
                    3,
                    2,
                ),
                1,
            ),
        ],
        0x85,
    )?;
    while h.state.height() < 4 {
        h.advance(vec![], None)?;
    }
    h.send(
        vec![
            wire::input(
                9,
                wire::problem_body(0, 1, 0, &[], [9; 32], &[], b"generator-v1"),
                0,
            ),
            wire::input(
                9,
                wire::problem_body(1, 1, 0, &[], [10; 32], &[], b"generator-v1"),
                1,
            ),
        ],
        0x86,
    )?;
    Ok(h)
}
fn problem(h: &History, slot: u8) -> Hash {
    h.state
        .problems
        .values()
        .find(|p| p.slot == slot)
        .unwrap()
        .id
}
fn compensation(h: &History, key: Key) -> &boole_derivation::m3::Reward {
    h.state
        .m3
        .as_ref()
        .unwrap()
        .rewards
        .values()
        .find(|r| r.key == key && r.compensation)
        .unwrap()
}
fn admission_retired(h: &History, key: Key) -> bool {
    h.state
        .m3
        .as_ref()
        .unwrap()
        .admissions
        .values()
        .find(|a| a.key == key)
        .unwrap()
        .retired
}
fn prior_histories() -> Result<(History, [History; 3], Key)> {
    // From genesis: signed POOLs -> reveals -> two COMMIT/OPEN/PART candidates.
    // Inject A=kernel-fault REJECT and B=ACCEPT so B wins. A real KERNEL-FIX
    // activation enqueues rechecks; A=ACCEPT then creates compensation/admission.
    // Empty heights vest it; another activation/reject produces VESTED+STALE.
    // State, rewards, admissions and queues are changed only by the public fold.
    let mut h = opened_history()?;
    let p = problem(&h, 0);
    let (a, aparts, _) = wire::body(&h.c, p, [6; 20], [7; 32], &[42; 7], &[]);
    let (b, bparts, _) = wire::body(&h.c, p, [6; 20], [7; 32], &[42; 8], &[]);
    let ah = boole_derivation::m2::commit_hash(
        h.c.chain_id,
        p,
        boole_derivation::m2::cancel_hash(&[8; 32]),
        boole_derivation::m2::body_digest(&a),
    );
    let bh = boole_derivation::m2::commit_hash(
        h.c.chain_id,
        p,
        boole_derivation::m2::cancel_hash(&[8; 32]),
        boole_derivation::m2::body_digest(&b),
    );
    let key = (p, ah);
    h.send(
        vec![
            wire::input(2, wire::commit(p, ah), 0),
            wire::input(2, wire::commit(p, bh), 1),
        ],
        0x80,
    )?;
    h.advance(vec![], None)?;
    h.send(
        vec![
            wire::input(
                3,
                wire::open(&a, boole_derivation::m2::cancel_hash(&[8; 32])),
                0,
            ),
            wire::input(
                3,
                wire::open(&b, boole_derivation::m2::cancel_hash(&[8; 32])),
                1,
            ),
        ],
        0x81,
    )?;
    h.send(
        vec![
            wire::input(4, wire::part(ah, 0, &[], &aparts[0]), 0),
            wire::input(4, wire::part(bh, 0, &[], &bparts[0]), 1),
        ],
        0x83,
    )?;
    while h.state.candidates[&key].status() == Status::Checking {
        h.advance(vec![], Some((key, Outcome::Reject, KERNEL_FAULT)))?;
    }
    ensure!(
        h.state.candidates[&key].status() == Status::Rejected
            && h.state.problems[&p].winner == Some(bh),
        "kernel-fault loser and winner construction"
    );
    let base = h.clone();
    h.recheck(key, Outcome::Accept)?;
    ensure!(
        h.state.candidates[&key].status() == Status::Accepted
            && !admission_retired(&h, key)
            && compensation(&h, key).status == RewardStatus::Locked,
        "LOCKED seed"
    );
    let locked = h.clone();
    let vest = compensation(&h, key).vest_h;
    while h.state.height() < vest {
        h.advance(vec![], None)?;
    }
    ensure!(
        compensation(&h, key).status == RewardStatus::Vested && !admission_retired(&h, key),
        "VESTED ACCEPTED seed"
    );
    let vested = h.clone();
    h.recheck(key, Outcome::Reject)?;
    ensure!(
        h.state.candidates[&key].status() == Status::Stale
            && admission_retired(&h, key)
            && compensation(&h, key).status == RewardStatus::Vested,
        "VESTED STALE seed"
    );
    Ok((base, [locked, vested, h], key))
}
#[test]
fn t82_reachable_prior_compensation_frozen_cross_height_histories() -> Result<()> {
    // Frozen before execution: three genesis-derived seeds, all binary words
    // of length 0..=3. A/R selects the target's recheck outcome when queued;
    // STALE remains absorbing. Every symbol is a real KERNEL-FIX activation.
    let (base, seeds, key) = prior_histories()?;
    let mut findings = vec![];
    let mut words = 0;
    let mut symbols = 0;
    let mut heights = 0;
    let mut accepts = 0;
    let mut cancels = 0;
    let mut vested_stales = 0;
    let mut retirements = 0;
    let mut rechecks = 0;
    let mut activations = 0;
    for (seed, initial) in seeds.iter().enumerate() {
        for length in 0..=3 {
            for word in 0..2usize.pow(length) {
                let mut h = initial.clone();
                words += 1;
                let mut sequence = vec![];
                let checked = (|| -> Result<()> {
                    for i in 0..length {
                        let outcome = if word & (1usize << i) == 0 {
                            Outcome::Accept
                        } else {
                            Outcome::Reject
                        };
                        sequence.push(outcome);
                        symbols += 1;
                        let old_status = h.state.candidates[&key].status();
                        let old_reward = compensation(&h, key).status;
                        let count = h.ledger.rewards.len();
                        let rows = h.recheck(key, outcome)?;
                        heights += rows.len();
                        for row in &rows {
                            activations += events(&row.events)?
                                .iter()
                                .filter(|(tag, _)| *tag == 8)
                                .count();
                            for v in &row.records {
                                let job =
                                    &row.before.m3.as_ref().unwrap().queue[v.queue_index as usize];
                                ensure!(
                                    v.record.kind == RecordKind::Recheck,
                                    "suffix first verdict"
                                );
                                rechecks += 1;
                                if job.key == key && v.record.outcome == Outcome::Accept {
                                    accepts += 1;
                                }
                            }
                        }
                        ensure!(
                            h.ledger.rewards.len() == count,
                            "cross-height repeated reward"
                        );
                        if old_status == Status::Accepted && outcome == Outcome::Reject {
                            ensure!(
                                h.state.candidates[&key].status() == Status::Stale
                                    && admission_retired(&h, key),
                                "compensated retirement"
                            );
                            retirements += 1;
                            if old_reward == RewardStatus::Locked {
                                ensure!(
                                    compensation(&h, key).status == RewardStatus::Cancelled,
                                    "locked reward survived STALE"
                                );
                                cancels += 1;
                            } else {
                                ensure!(
                                    compensation(&h, key).status == RewardStatus::Vested,
                                    "vested reward changed"
                                );
                                vested_stales += 1;
                            }
                        }
                        if old_status == Status::Accepted && outcome == Outcome::Accept {
                            ensure!(
                                !admission_retired(&h, key)
                                    && h.state.candidates[&key].status() == Status::Accepted,
                                "repeat ACCEPT changed admission"
                            );
                        }
                    }
                    Ok(())
                })();
                if let Err(e) = checked {
                    findings.push(format!("genesis_seed={seed} word={word} length={length} prefix={sequence:?} height={} {e:#}",h.state.height()));
                }
            }
        }
    }
    // A corrupted replay can carry an already observed ledger into an alternate,
    // independently reachable pre-grant history. Forge a fresh event id so the
    // cross-height duplicate guard itself, rather than id/coverage checks, fires.
    let mut alternative = base;
    alternative.advance(vec![], None)?;
    let rows = alternative.recheck(key, Outcome::Accept)?;
    let row = rows.last().unwrap();
    ensure!(row.l1.number > seeds[0].state.height(), "not cross-height");
    let mut raw = row.events.clone();
    let mut changed = 0;
    for (tag, b) in events(&row.events)? {
        if tag == 6 && b[116] == 4 {
            let at = b.as_ptr() as usize - row.events.as_ptr() as usize;
            raw[at..at + 8].copy_from_slice(&(seeds[0].ledger.rewards.len() as u64).to_be_bytes());
            changed += 1;
        }
    }
    ensure!(changed == 1, "missing forged compensation");
    let mut carried = seeds[0].ledger.clone();
    let error = carried
        .observe(
            &row.before,
            &alternative.state,
            &alternative.c,
            &row.l1,
            &raw,
            &row.payments,
            &row.prefix,
            &vec![true; row.prefix.len()],
        )
        .expect_err("cross-height duplicate accepted");
    ensure!(
        error.to_string() == "duplicate compensation",
        "wrong guard: {error:#}"
    );
    let summary = format!("FROZEN-REACHABLE seeds=3 words={words} symbols={symbols} heights={heights} activations={activations} rechecks={rechecks} repeated_accept={accepts} cancelled={cancels} vested_stale={vested_stales} retirements={retirements} duplicate_guard=1 findings={}",findings.len());
    eprintln!("{summary}");
    for finding in &findings {
        eprintln!("DIVERGENCE {finding}");
    }
    ensure!(findings.is_empty(), "reachable fold/audit findings");
    assert_eq!(
        (
            words,
            symbols,
            heights,
            activations,
            rechecks,
            accepts,
            cancels,
            vested_stales,
            retirements
        ),
        (45, 102, 306, 102, 146, 22, 11, 11, 22)
    );
    Ok(())
}
fn alias_checking() -> Result<(History, Key, Key)> {
    let mut h = opened_history()?;
    let p0 = problem(&h, 0);
    let p1 = problem(&h, 1);
    assert_ne!(p0, p1);
    let (body, parts, _) = wire::body(&h.c, p0, [6; 20], [7; 32], &[42; 7], &[]);
    let ch = boole_derivation::m2::commit_hash(
        h.c.chain_id,
        p0,
        boole_derivation::m2::cancel_hash(&[8; 32]),
        boole_derivation::m2::body_digest(&body),
    );
    h.send(
        vec![
            wire::input(2, wire::commit(p0, ch), 0),
            wire::input(2, wire::commit(p1, ch), 1),
        ],
        0x80,
    )?;
    h.advance(vec![], None)?;
    h.send(
        vec![wire::input(
            3,
            wire::open(&body, boole_derivation::m2::cancel_hash(&[8; 32])),
            0,
        )],
        0x81,
    )?;
    h.send(
        vec![wire::input(4, wire::part(ch, 0, &[], &parts[0]), 0)],
        0x83,
    )?;
    assert_eq!(h.state.candidates[&(p0, ch)].status(), Status::Checking);
    assert_eq!(h.state.candidates[&(p1, ch)].status(), Status::Committed);
    Ok((h, (p0, ch), (p1, ch)))
}
fn quarantine_alias(h: &mut History, ch: Hash) -> Result<()> {
    h.forks.rows.push(ForkRow {
        epoch: 1,
        height: h.state.height() + 1,
        gate_rule_digest: [90; 32],
        kind: ForkKind::Quarantine,
        list: vec![ForkItem {
            commit_hash: ch,
            disposition: Disposition::Recheck,
        }],
    });
    h.advance(vec![], None)?;
    Ok(())
}
#[test]
fn t82_reachable_same_hash_under_two_problems_and_due_verdict() -> Result<()> {
    let (mut h, primary, foreign) = alias_checking()?;
    quarantine_alias(&mut h, primary.1)?;
    assert_eq!(
        boole_derivation::attributes::hex_string(&h.state.root),
        "0xf20266fe1bb7bc95d2698774de120bc59f83e8079f0834fb5583c51787134d5f"
    );
    h.advance(vec![], None)
        .context("same commit hash under two reachable problems at due height")?;
    assert_eq!(
        boole_derivation::attributes::hex_string(&h.state.root),
        "0xf46f29f41cd45681506d2983d1c5e14ed4bf83acb4cd16ee6446cb41d37b8566"
    );
    assert_eq!(h.state.candidates[&primary].status(), Status::Accepted);
    assert_eq!(h.state.candidates[&foreign].status(), Status::Committed);
    // The alias stays unrevealed through the original candidate's vesting,
    // without borrowing its result, reward, or admission. COMMITTED records
    // need not change status merely because their problem's window closes.
    let vest = h
        .state
        .m3
        .as_ref()
        .unwrap()
        .rewards
        .values()
        .map(|r| r.vest_h)
        .max()
        .unwrap();
    while h.state.height() < vest {
        h.advance(vec![], None)?;
    }
    assert_eq!(h.state.candidates[&foreign].status(), Status::Committed);
    assert!(!h.state.m3.as_ref().unwrap().results.contains_key(&foreign));
    assert!(h.ledger.rewards.values().all(|r| r.key != foreign));
    assert!(h.ledger.admissions.values().all(|(k, _)| *k != foreign));
    Ok(())
}
#[test]
fn t82_alias_k4_rebuilds_only_the_quarantine_target() -> Result<()> {
    struct Unavailable;
    impl boole_gate_supervisor::rebuild::History for Unavailable {
        fn origin(&mut self, _: u64) -> Result<(L1Block, Vec<boole_derivation::InboxLog>)> {
            anyhow::bail!("retained source unavailable")
        }
    }
    let (mut h, primary, foreign) = alias_checking()?;
    quarantine_alias(&mut h, primary.1)?;
    let scratch = boole_devnet_archive::durability::PrivateTempDir::new("alias-k4")?;
    let failure = crate::k4::check(
        &h.state,
        &h.c,
        Some(&h.forks),
        &mut Unavailable,
        None,
        &BTreeMap::from([(primary, 9), (foreign, 6)]),
        scratch.path(),
    )
    .expect_err("unavailable source accepted");
    assert_eq!(
        failure.coverage.total, 1,
        "foreign COMMITTED hash alias is not a retained quarantine target"
    );
    assert_eq!(failure.height, 9);
    Ok(())
}

#[test]
fn t82_alias_open_cannot_rebind_the_preimage_to_another_problem() -> Result<()> {
    let mut h = opened_history()?;
    let p0 = problem(&h, 0);
    let p1 = problem(&h, 1);
    let cancel = boole_derivation::m2::cancel_hash(&[8; 32]);
    let (body, _, _) = wire::body(&h.c, p0, [6; 20], [7; 32], &[42; 7], &[]);
    let ch = boole_derivation::m2::commit_hash(
        h.c.chain_id,
        p0,
        cancel,
        boole_derivation::m2::body_digest(&body),
    );
    h.send(
        vec![
            wire::input(2, wire::commit(p0, ch), 0),
            wire::input(2, wire::commit(p1, ch), 1),
        ],
        0x80,
    )?;
    h.advance(vec![], None)?;
    let mut wrong = body.clone();
    wrong[1..33].copy_from_slice(&p1);
    assert_ne!(
        boole_derivation::m2::commit_hash(
            h.c.chain_id,
            p1,
            cancel,
            boole_derivation::m2::body_digest(&wrong)
        ),
        ch
    );
    let row = h.advance(
        vec![
            wire::input(3, wire::open(&wrong, cancel), 0),
            wire::input(3, wire::open(&body, cancel), 1),
        ],
        None,
    )?;
    assert_eq!(row.codes[1], 0x81, "valid original OPEN refused");
    assert_eq!(
        row.codes[0], 0x06,
        "wrong problem preimage must name no committed candidate"
    );
    assert_eq!(h.state.candidates[&(p1, ch)].status(), Status::Committed);
    assert_eq!(h.state.candidates[&(p0, ch)].status(), Status::Open);
    Ok(())
}
