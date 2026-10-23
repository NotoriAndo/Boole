//! T46/T60/T61 through the actual driver, sandbox and authenticated L1 reconstruction.
mod support;
use anyhow::{bail, Result};
use boole_derivation::{m2::Config, m3::ForkTable, ParentHeader};
use boole_devnet_driver::{
    driver::{Driver, StallKind, StepOutcome},
    engine::Engine,
};
use boole_gate_supervisor::{
    codec::{digest, Codec, Scratch},
    process::{Envelope, Worker},
    stub::Descriptor,
    supervisor::{install_devnet_artifacts, Host, Mode, Supervisor},
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path};
use support::*;
#[derive(Clone)]
struct Client {
    head: ParentHeader,
    history: BTreeMap<u64, ParentHeader>,
    txs: BTreeMap<u64, Vec<String>>,
    attrs: Value,
    payload: Value,
    calls: Vec<String>,
    mutate: bool,
}
impl Client {
    fn new() -> Self {
        let head = support::scenario::parent(0);
        Self {
            history: [(0, head.clone())].into(),
            head,
            txs: BTreeMap::new(),
            attrs: Value::Null,
            payload: Value::Null,
            calls: vec![],
            mutate: false,
        }
    }
}
impl Engine for Client {
    fn head(&mut self) -> Result<ParentHeader> {
        Ok(self.head.clone())
    }
    fn header(&mut self, h: u64) -> Result<ParentHeader> {
        self.history
            .get(&h)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing header"))
    }
    fn transactions(&mut self, h: u64) -> Result<Vec<String>> {
        Ok(self.txs[&h].clone())
    }
    fn call(&mut self, method: &str, p: Value) -> Result<Value> {
        self.calls.push(method.into());
        Ok(match method {
            "engine_forkchoiceUpdatedV3" => {
                if p[1].is_null() {
                    self.head.parent_hash = Some(self.head.hash.clone());
                    self.head.number += 1;
                    self.head.hash = p[0]["headBlockHash"].as_str().unwrap().into();
                    self.head.timestamp = u64::from_str_radix(
                        self.payload["timestamp"]
                            .as_str()
                            .unwrap()
                            .trim_start_matches("0x"),
                        16,
                    )
                    .unwrap();
                    self.head.l1_info = self.payload["transactions"][0].as_str().map(str::to_owned);
                    self.head.m2_root_deposit =
                        self.payload["transactions"][1].as_str().map(str::to_owned);
                    self.history.insert(self.head.number, self.head.clone());
                    self.txs.insert(
                        self.head.number,
                        serde_json::from_value(self.payload["transactions"].clone()).unwrap(),
                    );
                } else {
                    self.attrs = p[1].clone();
                }
                json!({"payloadStatus":{"status":"VALID"},"payloadId":"0x01"})
            }
            "engine_getPayloadV5" => {
                let mut txs = self.attrs["transactions"].clone();
                if self.mutate && txs.as_array().unwrap().len() > 2 {
                    txs.as_array_mut().unwrap().remove(2);
                }
                self.payload = json!({"blockNumber":format!("0x{:x}",self.head.number+1),"parentHash":self.head.hash,"blockHash":boole_derivation::attributes::hex_string(&digest(&serde_json::to_vec(&self.attrs).unwrap())),"timestamp":self.attrs["timestamp"],"gasLimit":self.attrs["gasLimit"],"transactions":txs});
                json!({"executionPayload":self.payload})
            }
            "engine_newPayloadV4" => {
                self.payload = p[0].clone();
                json!({"status":"VALID"})
            }
            _ => bail!("unexpected method"),
        })
    }
}
fn descriptor() -> Descriptor {
    Descriptor::pinned(digest(
        &std::fs::read(env!("CARGO_BIN_EXE_boole-gate-supervisor")).unwrap(),
    ))
}
fn config() -> Config {
    let mut c = support::scenario::m3_cfg();
    c.m3.as_mut().unwrap().genesis_gate_rule_digest = descriptor().digest();
    c
}
fn supervisor(c: &Config, forks: &ForkTable, root: &Path) -> Supervisor {
    let artifact = root.join("artifacts");
    install_devnet_artifacts(c, &artifact).unwrap();
    let binary = std::path::PathBuf::from(env!("CARGO_BIN_EXE_boole-gate-supervisor"));
    let worker = Worker {
        descriptor: descriptor(),
        binary_sha256: digest(&std::fs::read(&binary).unwrap()),
        binary,
    };
    Supervisor::new(
        c,
        forks,
        Mode::Verify,
        vec![worker],
        Some(Codec::portable_stub()),
        &root.join("cache"),
        root,
        &artifact,
        Host::local().unwrap(),
        Envelope::default(),
    )
    .unwrap()
}
fn driver(tag: u8, root: &Path) -> Driver<Archive, Client> {
    let (c, _, _, source) = fixture_with(config(), Some(tag));
    let forks = ForkTable::genesis(&c).unwrap();
    let supervisor = supervisor(&c, &forks, root);
    Driver::with_m3(source, Client::new(), c, 30_000_000, supervisor, forks).unwrap()
}
#[test]
fn t46_t61_verdict_pending_is_resumable_and_matches_the_other_node() {
    let a = Scratch::new(&std::env::temp_dir()).unwrap();
    let b = Scratch::new(&std::env::temp_dir()).unwrap();
    let mut delayed = driver(0, &a.0);
    let mut normal = driver(0, &b.0);
    for h in 1..=18 {
        delayed.step(h).unwrap();
        normal.step(h).unwrap();
    }
    delayed.supervisor.as_mut().unwrap().canary_ok = false;
    let count = delayed.engine.calls.len();
    let parent = delayed.m2_state().root;
    let error = delayed.step(19).unwrap_err();
    assert_eq!(
        error.kind,
        StallKind::VerdictPending {
            height: 19,
            queue_index: 0
        }
    );
    assert!(delayed.last_failure().is_none());
    assert_eq!(delayed.engine.calls.len(), count);
    assert_eq!(delayed.m2_state().root, parent);
    delayed.supervisor.as_mut().unwrap().canary_ok = true;
    let StepOutcome::Derived(expected) = normal.step(19).unwrap() else {
        panic!("waiting")
    };
    let StepOutcome::Derived(actual) = delayed.step(19).unwrap() else {
        panic!("waiting")
    };
    assert_eq!(
        actual.canonical_bytes().unwrap(),
        expected.canonical_bytes().unwrap()
    );
    assert_eq!(delayed.m2_state().root, normal.m2_state().root);
}
#[test]
fn t60_all_defer_stops_before_engine_and_preserves_checking() {
    let root = Scratch::new(&std::env::temp_dir()).unwrap();
    let mut d = driver(6, &root.0);
    for h in 1..=18 {
        d.step(h).unwrap();
    }
    let before = d.engine.calls.len();
    let root = d.m2_state().root;
    let e = d.step(19).unwrap_err();
    assert_eq!(
        e.kind,
        StallKind::VerdictPending {
            height: 19,
            queue_index: 0
        }
    );
    assert_eq!(d.engine.calls.len(), before);
    assert_eq!(d.m2_state().root, root);
    assert!(d.last_failure().is_none());
    assert_eq!(d.m2_state().m3.as_ref().unwrap().next_applied, 0);
}

#[test]
fn t46_peer_verdict_mutation_is_sticky_before_new_payload() {
    let root = Scratch::new(&std::env::temp_dir()).unwrap();
    let mut d = driver(0, &root.0);
    for h in 1..=18 {
        d.step(h).unwrap();
    }
    let before = d.engine.calls.len();
    let parent = d.m2_state().root;
    d.engine.mutate = true;
    let e = d.step(19).unwrap_err();
    assert_eq!(e.kind, StallKind::Verdict { height: 19 });
    assert_eq!(d.engine.calls.len(), before + 2);
    assert_eq!(d.m2_state().root, parent);
    assert_eq!(d.last_failure().unwrap().kind, e.kind);
    assert_eq!(d.step(19).unwrap_err().kind, e.kind);
    assert_eq!(d.engine.calls.len(), before + 2);
}
#[test]
fn t46_input_errors_stay_sticky_instead_of_becoming_verdict_pending() {
    let root = Scratch::new(&std::env::temp_dir()).unwrap();
    let mut d = driver(0, &root.0);
    for h in 1..=18 {
        d.step(h).unwrap();
    }
    d.l1.rows.get_mut(&19).unwrap().0.mix_hash = "invalid".into();
    let before = d.engine.calls.len();
    let e = d.step(19).unwrap_err();
    assert_eq!(e.kind, StallKind::Other);
    assert!(d.last_failure().is_some());
    assert_eq!(d.engine.calls.len(), before);
    assert_eq!(d.m2_state().height(), 18);
}
#[test]
fn t60_quarantine_void_and_recheck_resume_the_same_original_index() {
    use boole_derivation::{m2::Status, m3::*};
    for disposition in [Disposition::Void, Disposition::Recheck] {
        let root = Scratch::new(&std::env::temp_dir()).unwrap();
        let mut d = driver(6, &root.0);
        for h in 1..=18 {
            d.step(h).unwrap();
        }
        assert!(matches!(
            d.step(19).unwrap_err().kind,
            StallKind::VerdictPending { .. }
        ));
        let key = d.m2_state().m3.as_ref().unwrap().queue[0].key;
        let original = d.m2_state().m3.as_ref().unwrap().queue[0].avail;
        let mut fixed = descriptor();
        fixed.fixed = true;
        let binary = std::path::PathBuf::from(env!("CARGO_BIN_EXE_boole-gate-supervisor"));
        let worker = Worker {
            descriptor: fixed.clone(),
            binary_sha256: digest(&std::fs::read(&binary).unwrap()),
            binary,
        };
        d.supervisor
            .as_mut()
            .unwrap()
            .workers
            .insert(fixed.digest(), worker);
        let c = config();
        let mut forks = ForkTable::genesis(&c).unwrap();
        forks.rows.push(ForkRow {
            epoch: 1,
            height: 19,
            gate_rule_digest: fixed.digest(),
            kind: ForkKind::Quarantine,
            list: vec![ForkItem {
                commit_hash: key.1,
                disposition,
            }],
        });
        d.set_forks(forks).unwrap();
        d.step(19).unwrap();
        let m = d.m2_state().m3.as_ref().unwrap();
        assert_eq!(
            (
                m.queue.len(),
                m.next_applied,
                m.queue[0].queue_index,
                m.queue[0].avail
            ),
            (1, 1, 0, original)
        );
        assert_eq!(m.results[&key].gate_epoch, 1);
        assert_eq!(
            d.m2_state().candidates[&key].status(),
            if disposition == Disposition::Void {
                Status::Void
            } else {
                Status::Accepted
            }
        );
        if disposition == Disposition::Void {
            assert!(m.rewards.is_empty());
            assert!(!d.m2_state().problems[&key.0].voided);
        } else {
            assert_eq!(m.rewards.len(), 1);
        }
    }
}
#[test]
fn g10_restart_replays_every_stored_deposit_with_the_bound_cache() {
    let root = Scratch::new(&std::env::temp_dir()).unwrap();
    let mut continuous = driver(0, &root.0);
    for h in 1..=18 {
        continuous.step(h).unwrap();
    }
    let c = config();
    let forks = ForkTable::genesis(&c).unwrap();
    let supervisor = supervisor(&c, &forks, &root.0);
    let mut restarted = Driver::with_m3(
        continuous.l1.clone(),
        continuous.engine.clone(),
        c,
        30_000_000,
        supervisor,
        forks,
    )
    .unwrap();
    let StepOutcome::Derived(a) = continuous.step(19).unwrap() else {
        panic!("wait")
    };
    let StepOutcome::Derived(b) = restarted.step(19).unwrap() else {
        panic!("wait")
    };
    assert_eq!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
    assert_eq!(continuous.m2_state().root, restarted.m2_state().root);
}
#[test]
fn kernel_fix_rechecks_rebuild_the_original_reveal_after_repose() {
    use boole_derivation::{m2::Status, m3::*};
    let root = Scratch::new(&std::env::temp_dir()).unwrap();
    let mut d = driver(7, &root.0);
    for h in 1..=19 {
        d.step(h).unwrap();
    }
    let key = d.m2_state().m3.as_ref().unwrap().queue[0].key;
    let mut fixed = descriptor();
    fixed.fixed = true;
    let binary = std::path::PathBuf::from(env!("CARGO_BIN_EXE_boole-gate-supervisor"));
    d.supervisor.as_mut().unwrap().workers.insert(
        fixed.digest(),
        Worker {
            descriptor: fixed.clone(),
            binary_sha256: digest(&std::fs::read(&binary).unwrap()),
            binary,
        },
    );
    let c = config();
    let mut forks = ForkTable::genesis(&c).unwrap();
    forks.rows.push(ForkRow {
        epoch: 1,
        height: 20,
        gate_rule_digest: fixed.digest(),
        kind: ForkKind::KernelFix,
        list: vec![],
    });
    d.set_forks(forks).unwrap();
    for h in 20..=24 {
        d.step(h).unwrap();
    }
    assert_eq!(d.m2_state().candidates[&key].status(), Status::Stale);
    let m = d.m2_state().m3.as_ref().unwrap();
    assert_eq!(
        m.rewards[&0].status,
        boole_derivation::m3::RewardStatus::Cancelled
    );
    assert!(m.admissions[&1].retired);
    let reposed = d
        .m2_state()
        .problems
        .values()
        .find(|p| p.epoch == 1)
        .unwrap();
    assert_eq!(reposed.reveal_pos, (5, 0, 0));
    let state = d.m2_state().clone();
    let mut archive = d.l1.clone();
    let rebuilt = boole_gate_supervisor::rebuild::rebuild(
        &state,
        &c,
        key,
        &mut archive,
        Some(&Codec::portable_stub()),
        &root.0,
    )
    .unwrap();
    assert!(archive.reads.contains(&5));
    assert_eq!(
        rebuilt.input.package,
        boole_gate_supervisor::stub::package(10000)
    );
}

#[test]
fn g1_follow_runs_no_gate_and_waits_for_canonical_fuel_lookahead() {
    let normal_root = Scratch::new(&std::env::temp_dir()).unwrap();
    let follow_root = Scratch::new(&std::env::temp_dir()).unwrap();
    let (c, _, _, source) = fixture_with_fuel(config(), Some(0), 10000);
    let forks = ForkTable::genesis(&c).unwrap();
    let runtime = supervisor(&c, &forks, &normal_root.0);
    let mut normal = Driver::with_m3(
        source.clone(),
        Client::new(),
        c.clone(),
        30_000_000,
        runtime,
        forks.clone(),
    )
    .unwrap();
    for h in 1..=28 {
        normal.step(h).unwrap();
    }
    let runtime = Supervisor::new(
        &c,
        &forks,
        Mode::Follow {
            from: 1,
            through: 28,
        },
        vec![],
        None,
        &follow_root.0.join("cache"),
        &follow_root.0,
        &follow_root.0.join("absent-artifacts"),
        Host::local().unwrap(),
        Envelope::default(),
    )
    .unwrap();
    let mut follow = Driver::with_m3(source, Client::new(), c, 30_000_000, runtime, forks).unwrap();
    let evidence = follow_root.0.join("follow.jsonl");
    follow.set_evidence(&evidence).unwrap();
    for h in 1..=18 {
        follow
            .provide_follow_block(&normal.l1.rows[&h].0, &normal.engine.txs[&h])
            .unwrap();
        follow.step(h).unwrap();
    }
    assert!(matches!(
        follow.step(19).unwrap_err().kind,
        StallKind::VerdictPending { .. }
    ));
    assert!(follow.last_failure().is_none());
    // The canonical chain has no deposit until28: fuel, not wall time, delayed it.
    for h in 19..=28 {
        follow
            .provide_follow_block(&normal.l1.rows[&h].0, &normal.engine.txs[&h])
            .unwrap();
    }
    for h in 19..=28 {
        follow.step(h).unwrap();
        assert_eq!(follow.engine.head.hash, normal.engine.history[&h].hash);
    }
    assert_eq!(follow.m2_state().root, normal.m2_state().root);
    assert!(follow.supervisor.as_ref().unwrap().workers.is_empty());
    for line in std::fs::read_to_string(evidence).unwrap().lines() {
        let record: Value = serde_json::from_str(line).unwrap();
        assert_eq!(record["verification"], "checkpoint-trusted");
    }
}

#[test]
fn t60_quarantine_void_is_not_delayed_by_the_old_epochs_retry_clock() {
    use boole_derivation::{m2::Status, m3::*};
    let root = Scratch::new(&std::env::temp_dir()).unwrap();
    let mut d = driver(6, &root.0);
    for h in 1..=18 {
        d.step(h).unwrap();
    }
    assert!(d.step(19).is_err());
    assert!(!d.supervisor.as_ref().unwrap().retries.is_empty());
    let key = d.m2_state().m3.as_ref().unwrap().queue[0].key;
    let c = config();
    let mut table = ForkTable::genesis(&c).unwrap();
    // VOID needs no new kernel binary; the activating epoch is still a new job input.
    table.rows.push(ForkRow {
        epoch: 1,
        height: 19,
        gate_rule_digest: descriptor().digest(),
        kind: ForkKind::Quarantine,
        list: vec![ForkItem {
            commit_hash: key.1,
            disposition: Disposition::Void,
        }],
    });
    d.set_forks(table).unwrap();
    d.step(19).unwrap();
    assert_eq!(d.m2_state().candidates[&key].status(), Status::Void);
    assert_eq!(d.m2_state().m3.as_ref().unwrap().next_applied, 1);
}

#[test]
fn s16_driver_revalidates_a_supervisor_against_its_actual_configuration() {
    let root = Scratch::new(&std::env::temp_dir()).unwrap();
    let (c, _, _, source) = fixture_with(config(), Some(0));
    let forks = ForkTable::genesis(&c).unwrap();
    let supervisor = supervisor(&c, &forks, &root.0);
    let mut other = c;
    other.chain_id = 720927;
    assert!(
        Driver::with_m3(source, Client::new(), other, 30_000_000, supervisor, forks).is_err(),
        "devnet-validated stub entered a different chain through the driver constructor"
    );
}

#[test]
fn g10_full_sized_rewards_persist_and_restart_without_narrowing_u128() {
    let first = Scratch::new(&std::env::temp_dir()).unwrap();
    let second = Scratch::new(&std::env::temp_dir()).unwrap();
    let mut c = config();
    c.m3.as_mut().unwrap().reward.fill(1000 * 10u128.pow(18));
    let (c, _, _, source) = fixture_with(c, Some(0));
    let forks = ForkTable::genesis(&c).unwrap();
    let journal = first.0.join("checkpoint.json");
    let mut d = Driver::with_m3(
        source,
        Client::new(),
        c.clone(),
        30_000_000,
        supervisor(&c, &forks, &first.0),
        forks.clone(),
    )
    .unwrap();
    d.set_journal(&journal).unwrap();
    d.step(1).unwrap();
    let mut replay = Driver::with_m3(
        d.l1.clone(),
        d.engine.clone(),
        c.clone(),
        30_000_000,
        supervisor(&c, &forks, &second.0),
        forks,
    )
    .unwrap();
    replay.set_journal(&journal).unwrap();
    let expected = d.step(2).unwrap();
    let actual = replay.step(2).unwrap();
    let (StepOutcome::Derived(a), StepOutcome::Derived(b)) = (expected, actual) else {
        panic!("missing block")
    };
    assert_eq!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
    assert_eq!(d.m2_state().root, replay.m2_state().root);
}
