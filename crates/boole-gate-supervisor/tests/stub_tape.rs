//! The committed v1.3 tape remains authority for the historical stub release.
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::hex_bytes,
    m2::{Config, Input, State},
    m3::*,
    InboxLog, L1Block,
};
use boole_gate_supervisor::{
    codec::Scratch,
    process::{Envelope, Worker},
    real_supervisor::Release,
    rebuild::{self, History},
    stub::Descriptor,
    supervisor::{checked_record, jobs_at_height},
    GateReply,
};
use std::collections::BTreeMap;
struct Tape(BTreeMap<u64, (L1Block, Vec<InboxLog>)>);
impl History for Tape {
    fn origin(&mut self, h: u64) -> Result<(L1Block, Vec<InboxLog>)> {
        self.0.get(&h).cloned().context("tape origin")
    }
}
fn replay(raw: &str, fresh: bool) -> Result<usize> {
    let v: serde_json::Value = serde_json::from_str(raw)?;
    let c: Config = serde_json::from_str(v["config_json"].as_str().unwrap())?;
    let forks: ForkTable = serde_json::from_value(v["forks"].clone())?;
    let descriptors: Vec<Descriptor> = serde_json::from_value(v["gate_descriptors"].clone())?;
    for (d, row) in descriptors.iter().zip(&forks.rows) {
        assert_eq!(d.digest(), row.gate_rule_digest);
        let worker = Worker {
            descriptor: d.clone(),
            binary: "unused".into(),
            binary_sha256: [1; 32],
        };
        assert_eq!(
            serde_json::to_vec(&worker)?,
            serde_json::to_vec(&Release::Stub(worker.clone()))?
        );
    }
    let mut tape = Tape(BTreeMap::new());
    for row in v["heights"].as_array().unwrap() {
        let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
        tape.0.insert(
            l1.number,
            (l1, serde_json::from_value(row["logs"].clone())?),
        );
    }
    let fresh = fresh || std::env::var_os("BOOLE_FRESH_STUB_TAPE").is_some();
    let native = if fresh {
        Some(std::path::PathBuf::from(env!(
            "CARGO_BIN_EXE_boole-gate-supervisor"
        )))
    } else {
        std::env::var_os("BOOLE_STUB_TAPE_BINARY").map(std::path::PathBuf::from)
    };
    let root = Scratch::new(&std::env::temp_dir())?;
    let mut state = State::new(&c, 30_000_000)?;
    let mut checked = 0;
    for row in v["heights"].as_array().unwrap() {
        let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
        let txs: Vec<String> = serde_json::from_value(row["result"]["transactions"].clone())?;
        let deposits: Vec<_> = txs
            .iter()
            .skip(2)
            .take_while(|t| t.starts_with("0x7e"))
            .filter(|t| {
                hex_bytes(t, "tx").is_ok_and(|raw| {
                    boole_derivation::rlp::decode(&raw[1..])
                        .ok()
                        .and_then(|r| r.children)
                        .is_some_and(|f| f[7].bytes.starts_with(b"m3vd"))
                })
            })
            .cloned()
            .collect();
        let records = decode_verdict_deposits(&c, &l1, &deposits)?;
        let queue = jobs_at_height(&state, l1.number, &forks)?;
        for indexed in &records {
            let (job, forced) = queue
                .iter()
                .find(|(j, _)| j.queue_index == indexed.queue_index)
                .context("tape job")?;
            let epoch = job
                .epoch_override
                .map(Ok)
                .unwrap_or_else(|| forks.job_epoch(job.avail, c.m3.as_ref().unwrap().k))?;
            let d = &descriptors[epoch as usize];
            let mut input = rebuild::rebuild(
                &state,
                &c,
                job.key,
                &mut tape,
                Some(&boole_gate_supervisor::codec::Codec::portable_stub()),
                &root.0,
            )?
            .input;
            if *forced {
                input.early = Some((3, QUARANTINE_VOID));
            }
            let reply = if let Some(binary) = &native {
                Worker {
                    descriptor: if fresh {
                        // The recorded release lists darwin-aarch64 only; re-pin the fresh
                        // binary as the sole release for the host that runs it.
                        let profile = boole_gate_supervisor::process::host_profile();
                        let mut pinned = d.clone();
                        pinned.profiles = vec![profile.clone()];
                        pinned.binaries = BTreeMap::from([(
                            profile,
                            boole_gate_supervisor::codec::digest(&std::fs::read(binary)?),
                        )]);
                        pinned
                    } else {
                        d.clone()
                    },
                    binary: binary.clone(),
                    binary_sha256: boole_gate_supervisor::codec::digest(&std::fs::read(binary)?),
                }
                .run(&input, &Envelope::default(), &root.0)
            } else {
                boole_gate_supervisor::stub::evaluate(d, &input)
            };
            let GateReply::Result { value } = reply else {
                anyhow::bail!("historical stub deferred a taped verdict");
            };
            assert_eq!(
                checked_record(&input, d, job, epoch, &value)?.encode()?,
                indexed.record.encode()?
            );
            checked += 1;
        }
        let (_, logs) = tape.origin(l1.number)?;
        let inputs: Vec<_> = logs
            .iter()
            .filter_map(|l| {
                let b = hex_bytes(&l.data, "log").ok()?;
                let (kind, body) = boole_derivation::inbox::item(&b).ok()?;
                Some(Input {
                    kind,
                    body: body.to_vec(),
                    transaction_index: l.transaction_index,
                    log_index: l.log_index,
                })
            })
            .collect();
        let out = state.step_with_verdicts(&c, &l1, &inputs, &records, &forks)?;
        state = out.state;
    }
    eprintln!("STUB-TAPE checked={checked} native={}", native.is_some());
    Ok(checked)
}
#[test]
fn historical_stub_descriptor_serialization_and_every_taped_verdict_stay_byte_identical(
) -> Result<()> {
    let mut count = 0;
    for raw in [
        include_str!("../../../fixtures/devnet-m3/round-v1.3.json"),
        include_str!("../../../fixtures/devnet-m3/kernel-fix-v1.3.json"),
        include_str!("../../../fixtures/devnet-m3/quarantine-recheck-v1.3.json"),
        include_str!("../../../fixtures/devnet-m3/quarantine-void-v1.3.json"),
        include_str!("../../../fixtures/devnet-m3/reorg-v1.3.json"),
    ] {
        count += replay(raw, false)?;
    }
    ensure!(count > 0, "empty stub qualification");
    Ok(())
}

#[test]
fn fresh_binary_source_identity_equals_the_recorded_stub_release() {
    let tape: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m3/round-v1.3.json")).unwrap();
    let d: Descriptor = serde_json::from_value(tape["gate_descriptors"][0].clone()).unwrap();
    assert_eq!(
        boole_gate_supervisor::implementation_digest(),
        d.implementation_sha256
    );
}

#[test]
fn fresh_worker_process_replays_one_recorded_tape() -> Result<()> {
    assert_eq!(
        replay(
            include_str!("../../../fixtures/devnet-m3/round-v1.3.json"),
            true
        )?,
        1
    );
    Ok(())
}
