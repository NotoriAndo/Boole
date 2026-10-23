//! Optional local verification of owned candidates. Other verdicts stay L2-trusted.
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    m2::{Config, Hash, State},
    m3::{ForkTable, IndexedVerdict, QUARANTINE_VOID},
    InboxLog, L1Block,
};
use boole_devnet_driver::{engine::retry_rpc, l1::L1Source};
use boole_gate_supervisor::{
    cache::{Cache, CacheKey},
    codec::{digest, read_bounded, Codec},
    process::{Envelope, Worker},
    rebuild::{self, History},
    stub::Descriptor,
    supervisor::{checked_record, jobs_at_height, Host},
    GateInput, GateReply,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub trait OwnGate {
    fn descriptor(&self) -> &Descriptor;
    fn validate(&self, chain_id: u64, scratch: &Path) -> Result<()>;
    fn check_pin(&self) -> Result<()> {
        Ok(())
    }
    fn run(&self, input: &GateInput, scratch: &Path) -> GateReply;
}
pub struct OwnVerification {
    pub gates: Vec<Box<dyn OwnGate>>,
    pub artifacts: PathBuf,
}

pub struct PinnedOwnGate {
    pub worker: Worker,
    pub envelope: Envelope,
}
impl OwnGate for PinnedOwnGate {
    fn descriptor(&self) -> &Descriptor {
        &self.worker.descriptor
    }
    fn check_pin(&self) -> Result<()> {
        self.worker.validate()
    }
    fn validate(&self, chain_id: u64, scratch: &Path) -> Result<()> {
        let host = Host::local()?;
        self.worker.descriptor.validate(chain_id, &host.profile)?;
        self.worker.validate()?;
        ensure!(
            self.envelope.memory_mib > 0 && self.envelope.memory_mib <= host.ram_mib,
            "verify-own envelope exceeds host RAM"
        );
        ensure!(
            self.worker.canary(&self.envelope, scratch),
            "verify-own sandbox canary unavailable"
        );
        Ok(())
    }
    fn run(&self, input: &GateInput, scratch: &Path) -> GateReply {
        self.worker.run(input, &self.envelope, scratch)
    }
}
pub(crate) struct Verifier {
    gates: BTreeMap<Hash, Box<dyn OwnGate>>,
    artifacts: PathBuf,
    codec: Codec,
    scratch: PathBuf,
    cache: Cache,
}
impl Verifier {
    pub fn new(
        c: &Config,
        forks: &ForkTable,
        settings: OwnVerification,
        codec: Option<Codec>,
        root: &Path,
    ) -> Result<Self> {
        let mut gates = BTreeMap::new();
        for gate in settings.gates {
            gate.validate(c.chain_id, root)?;
            ensure!(
                gates.insert(gate.descriptor().digest(), gate).is_none(),
                "duplicate verify-own gate"
            );
        }
        ensure!(
            forks
                .rows
                .iter()
                .all(|r| gates.contains_key(&r.gate_rule_digest)),
            "verify-own missing epoch gate"
        );
        let codec = codec.unwrap_or_else(Codec::portable_stub);
        codec.verify()?;
        let out = Self {
            gates,
            artifacts: settings.artifacts,
            codec,
            scratch: root.to_owned(),
            cache: Cache::new(&root.join("verify-own-cache"))?,
        };
        let components: BTreeSet<_> = std::iter::once(c.base_root)
            .chain(c.env_allowed.iter().flatten().copied())
            .collect();
        for id in components {
            out.artifact(id)?;
        }
        Ok(out)
    }
    fn artifact(&self, id: Hash) -> Result<Vec<u8>> {
        let raw = read_bounded(
            &self
                .artifacts
                .join(boole_derivation::attributes::hex_string(&id)),
            256 * 1024 * 1024,
        )?;
        ensure!(digest(&raw) == id, "verify-own artifact digest mismatch");
        Ok(raw)
    }
    pub fn verify(
        &self,
        s: &State,
        c: &Config,
        forks: &ForkTable,
        records: &[IndexedVerdict],
        owned: &BTreeSet<Hash>,
        history: &mut dyn History,
    ) -> Result<()> {
        let h = s
            .height()
            .checked_add(1)
            .context("verify-own height overflow")?;
        let jobs = jobs_at_height(s, h, forks)?;
        for record in records {
            let (job, forced) = jobs
                .get(usize::try_from(record.queue_index)?)
                .context("verify-own queue index")?;
            if !owned.contains(&job.key.1) {
                continue;
            }
            let epoch = job
                .epoch_override
                .map(Ok)
                .unwrap_or_else(|| forks.job_epoch(job.avail, c.m3.as_ref().expect("M3").k))?;
            let gate_id = forks.rows[epoch as usize].gate_rule_digest;
            let gate = self
                .gates
                .get(&gate_id)
                .context("verify-own unavailable: epoch gate")?;
            gate.check_pin()
                .map_err(|_| anyhow::anyhow!("verify-own unavailable: gate pin"))?;
            let rebuilt =
                rebuild::rebuild(s, c, job.key, history, Some(&self.codec), &self.scratch)
                    .map_err(|_| {
                        anyhow::anyhow!("verify-own unavailable or inconsistent anchored input")
                    })?;
            let mut input = rebuilt.input;
            for id in rebuilt.components {
                input.artifacts.push(self.artifact(id).map_err(|_| {
                    anyhow::anyhow!("verify-own unavailable: environment artifact")
                })?);
            }
            if *forced {
                input.early = Some((3, QUARANTINE_VOID));
            }
            let key = CacheKey {
                gate: gate_id,
                queue_index: job.queue_index,
                commit_hash: job.key.1,
                kind: job.kind as u8,
            };
            let cached = self.cache.load(&key, &input, epoch);
            let value = if let Some(value) = cached {
                value
            } else {
                let GateReply::Result { value } = gate.run(&input, &self.scratch) else {
                    return Err(boole_devnet_archive::error::availability(
                        h,
                        "gate",
                        "verify-own unavailable: local gate deferred",
                    ));
                };
                value
            };
            let computed = checked_record(&input, gate.descriptor(), job, epoch, &value)
                .map_err(|_| anyhow::anyhow!("verify-own unavailable: malformed adapter reply"))?;
            ensure!(
                computed == record.record,
                "verify-own mismatch at queue index {}",
                job.queue_index
            );
            // Persistence is an optimization. Every hit still rebuilds the input
            // and rechecks the complete locally computed record above.
            let _ = self.cache.save(&key, &input, epoch, &value);
        }
        Ok(())
    }
}
pub(crate) struct AnchoredHistory<'a, L> {
    pub chain: &'a crate::chain::Chain,
    pub l1: &'a mut L,
    pub maximum: u64,
}
impl<L: L1Source> History for AnchoredHistory<'_, L> {
    fn origin(&mut self, h: u64) -> Result<(L1Block, Vec<InboxLog>)> {
        ensure!(
            h <= self.maximum,
            "verify-own input beyond the folded L1 anchor"
        );
        let entry = self
            .chain
            .blocks
            .get(&h)
            .context("verify-own L2 anchor unavailable")?;
        let block = retry_rpc(|| self.l1.block(h))?;
        ensure!(
            block.number == h
                && entry.block.transactions.first()
                    == Some(&boole_derivation::system::l1_info(&block)?),
            "verify-own L1-info anchor changed"
        );
        let logs = retry_rpc(|| self.l1.inbox_logs(&block.hash))?;
        crate::client::validated_inputs(&block, logs.clone())?;
        Ok((block, logs))
    }
}
