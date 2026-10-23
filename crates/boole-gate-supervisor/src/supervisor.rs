use crate::{
    cache::{Cache, CacheKey},
    codec::{digest, private_file, read_bounded, Codec},
    process::{host_profile, Envelope, Worker},
    rebuild::{self, History},
    GateInput, GateReply, GateResult,
};
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    m2::{Config, Hash, State},
    m3::*,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Instant,
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Verify,
    Follow { from: u64, through: u64 },
}
impl Mode {
    pub fn follows(&self, h: u64) -> bool {
        matches!(self,Self::Follow{from,through} if *from<=h && h<=*through)
    }
}
#[derive(Clone, Debug)]
pub struct Host {
    pub profile: String,
    pub ram_mib: u64,
    pub cpus: usize,
}
impl Host {
    pub fn local() -> Result<Self> {
        #[cfg(target_os = "macos")]
        let ram_mib = {
            let out = std::process::Command::new("/usr/sbin/sysctl")
                .args(["-n", "hw.memsize"])
                .output()?;
            ensure!(out.status.success(), "host RAM probe");
            String::from_utf8(out.stdout)?.trim().parse::<u64>()? / 1024 / 1024
        };
        #[cfg(target_os = "linux")]
        let ram_mib = std::fs::read_to_string("/proc/meminfo")?
            .lines()
            .find_map(|l| l.strip_prefix("MemTotal:"))
            .context("host RAM probe")?
            .split_whitespace()
            .next()
            .context("RAM units")?
            .parse::<u64>()?
            / 1024;
        Ok(Self {
            profile: host_profile(),
            ram_mib,
            cpus: std::thread::available_parallelism()?.get(),
        })
    }
}
#[derive(Default, Debug)]
pub struct Retry {
    pub attempts: u32,
    pub next_at: u64,
    pub third_authorized: bool,
}
impl Retry {
    pub fn deferred(&mut self, now: u64, escalate: bool) {
        self.attempts += 1;
        self.third_authorized |= escalate;
        self.next_at = now.saturating_add(
            if self.attempts == 1 || self.attempts == 2 && self.third_authorized {
                60
            } else {
                3600
            },
        );
    }
}
pub struct Supervisor {
    pub mode: Mode,
    pub envelope: Envelope,
    pub workers: BTreeMap<Hash, Worker>,
    pub codec: Option<Codec>,
    cache: Cache,
    scratch: PathBuf,
    artifacts: PathBuf,
    start: Instant,
    pub retries: BTreeMap<(CacheKey, u32), Retry>,
    pub alerts: Vec<String>,
    pub evidence: Vec<serde_json::Value>,
    pub canary_ok: bool,
    pub class_disagreement: bool,
    verified_table: Option<(Hash, Vec<ForkRow>)>,
}
/// Preview R2 stage-0 job metadata without changing the consensus state.
pub fn jobs_at_height(s: &State, h: u64, forks: &ForkTable) -> Result<Vec<(Job, bool)>> {
    let m = s.m3.as_ref().context("M3 state")?;
    let mut queue = m.queue.clone();
    if let Some(row) = forks
        .rows
        .iter()
        .find(|r| r.height == h && r.kind == ForkKind::KernelFix)
    {
        for key in forks.recheck_set(s, h)? {
            queue.push(Job {
                queue_index: queue.len() as u64,
                key,
                kind: RecordKind::Recheck,
                avail: h,
                epoch_override: Some(row.epoch),
            });
        }
    }
    Ok(queue
        .into_iter()
        .map(|mut job| {
            let mut forced = m.quarantine_voids.contains(&job.key);
            if let Some(row) = forks
                .rows
                .iter()
                .find(|r| r.height == h && r.kind == ForkKind::Quarantine)
            {
                if let Some(item) = row.list.iter().find(|i| i.commit_hash == job.key.1) {
                    job.epoch_override = Some(row.epoch);
                    forced = item.disposition == Disposition::Void;
                }
            }
            (job, forced)
        })
        .collect())
}
// CacheKey ordering is the exact G10 tuple, independent of local paths.
impl PartialOrd for CacheKey {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for CacheKey {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (self.gate, self.queue_index, self.commit_hash, self.kind).cmp(&(
            o.gate,
            o.queue_index,
            o.commit_hash,
            o.kind,
        ))
    }
}
impl Supervisor {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        c: &Config,
        forks: &ForkTable,
        mode: Mode,
        workers: Vec<Worker>,
        codec: Option<Codec>,
        cache: &Path,
        scratch: &Path,
        artifacts: &Path,
        host: Host,
        envelope: Envelope,
    ) -> Result<Self> {
        if let Mode::Follow { from, through } = &mode {
            ensure!(*from > 0 && from <= through, "explicit follow range");
        }
        let workers = workers
            .into_iter()
            .map(|w| (w.descriptor.digest(), w))
            .collect::<BTreeMap<_, _>>();
        let mut out = Self {
            mode,
            envelope,
            workers,
            codec,
            cache: Cache::new(cache)?,
            scratch: scratch.to_owned(),
            artifacts: artifacts.to_owned(),
            start: Instant::now(),
            retries: BTreeMap::new(),
            alerts: vec![],
            evidence: vec![],
            canary_ok: true,
            class_disagreement: false,
            verified_table: None,
        };
        out.validate_startup(c, forks, &host)?;
        Ok(out)
    }
    pub fn validate_startup(&mut self, c: &Config, forks: &ForkTable, host: &Host) -> Result<()> {
        forks
            .validate(c)
            .map_err(|s| anyhow::anyhow!("S{s}: fork table"))?;
        self.verified_table = None;
        if matches!(self.mode, Mode::Follow { .. }) {
            return Ok(());
        }
        self.validate_verify_startup(c, forks, host)
    }
    fn validate_verify_startup(
        &mut self,
        c: &Config,
        forks: &ForkTable,
        host: &Host,
    ) -> Result<()> {
        ensure!(
            self.envelope.memory_mib > 0 && self.envelope.memory_mib <= host.ram_mib,
            "S16: job RAM exceeds host"
        );
        if c.chain_id != 720926 {
            ensure!(
                host.ram_mib >= self.envelope.memory_mib.saturating_add(3072) && host.cpus >= 4,
                "S16: testnet minimum host"
            );
        }
        for row in &forks.rows {
            let worker = self
                .workers
                .get(&row.gate_rule_digest)
                .context("S16: missing gate epoch")?;
            worker.descriptor.validate(c.chain_id, &host.profile)?;
            worker.validate()?;
            ensure!(
                worker.canary(&self.envelope, &self.scratch),
                "S16: gate canary/sandbox unavailable"
            );
        }
        for id in required_artifacts(c) {
            self.artifact(id).context("S16: environment artifact")?;
        }
        self.codec
            .as_ref()
            .context("S16: pinned codec missing")?
            .verify()?;
        self.verified_table = Some((c.digest(), forks.rows.clone()));
        Ok(())
    }
    fn artifact(&self, id: Hash) -> Result<Vec<u8>> {
        let bytes = read_bounded(
            &self
                .artifacts
                .join(boole_derivation::attributes::hex_string(&id)),
            256 * 1024 * 1024,
        )?;
        ensure!(digest(&bytes) == id, "environment artifact digest mismatch");
        Ok(bytes)
    }
    pub fn collect(
        &mut self,
        s: &State,
        c: &Config,
        h: u64,
        forks: &ForkTable,
        history: &mut dyn History,
    ) -> Result<Vec<IndexedVerdict>> {
        self.collect_at(s, c, h, forks, history, self.start.elapsed().as_secs())
    }
    pub fn collect_at(
        &mut self,
        s: &State,
        c: &Config,
        h: u64,
        forks: &ForkTable,
        history: &mut dyn History,
        now: u64,
    ) -> Result<Vec<IndexedVerdict>> {
        ensure!(
            !self.mode.follows(h),
            "follow mode must use canonical records"
        );
        if !self
            .verified_table
            .as_ref()
            .is_some_and(|(config, rows)| *config == c.digest() && rows == &forks.rows)
        {
            self.validate_verify_startup(c, forks, &Host::local()?)?;
        }
        let m = s.m3.as_ref().context("M3 state")?;
        let p = c.m3.as_ref().context("M3 profile")?;
        let queue = jobs_at_height(s, h, forks)?;
        let mut records = vec![];
        for (job, forced_void) in &queue[m.next_applied..] {
            let epoch = job
                .epoch_override
                .map(Ok)
                .unwrap_or_else(|| forks.job_epoch(job.avail, p.k))?;
            let row = forks.rows.get(epoch as usize).context("job epoch")?;
            let cache_key = CacheKey {
                gate: row.gate_rule_digest,
                queue_index: job.queue_index,
                commit_hash: job.key.1,
                kind: job.kind as u8,
            };
            let retry_key = (cache_key.clone(), epoch);
            if !self.canary_ok {
                self.alerts
                    .push("d-node: canary unavailable; attempt not counted".into());
                break;
            }
            if self
                .retries
                .get(&retry_key)
                .is_some_and(|r| now < r.next_at)
            {
                break;
            }
            let rebuilt = match rebuild::rebuild(
                s,
                c,
                job.key,
                history,
                self.codec.as_ref(),
                &self.scratch,
            ) {
                Ok(value) => value,
                Err(e) if e.is::<rebuild::Availability>() || e.is::<rebuild::NodeUnavailable>() => {
                    self.alerts.push("L1 availability; no verdict".into());
                    break;
                }
                Err(e) => return Err(e),
            };
            let mut input = rebuilt.input;
            for id in rebuilt.components {
                match self.artifact(id) {
                    Ok(bytes) => input.artifacts.push(bytes),
                    Err(_) => {
                        self.alerts
                            .push("d-node: environment unavailable; attempt not counted".into());
                        return Ok(records);
                    }
                }
            }
            if *forced_void {
                input.early = Some((3, QUARANTINE_VOID));
            }
            let Some(worker) = self.workers.get(&row.gate_rule_digest) else {
                self.alerts.push("d-node: epoch unavailable".into());
                break;
            };
            if worker.validate().is_err() {
                self.alerts
                    .push("d-node: binary unavailable; attempt not counted".into());
                break;
            }
            if self
                .retries
                .get(&retry_key)
                .is_some_and(|r| r.attempts >= 2)
                && !worker.canary(&self.envelope, &self.scratch)
            {
                let retry = self.retries.entry(retry_key.clone()).or_default();
                retry.third_authorized = true;
                retry.next_at = now.saturating_add(60);
                self.alerts
                    .push("canary failed; no submitted job attempt counted".into());
                break;
            }
            let cached = self.cache.load(&cache_key, &input, epoch);
            let (reply, hit) = match cached {
                Some(value) => (GateReply::Result { value }, true),
                None => (worker.run(&input, &self.envelope, &self.scratch), false),
            };
            match reply {
                GateReply::Result { value } => {
                    let record =
                        match checked_record(&input, &worker.descriptor, job, epoch, &value) {
                            Ok(v) => v,
                            Err(_) => {
                                self.alerts.push("d-node: malformed adapter result".into());
                                break;
                            }
                        };
                    if !hit && self.cache.save(&cache_key, &input, epoch, &value).is_err() {
                        self.alerts
                            .push("cache write unavailable; computed result retained".into());
                    }
                    self.retries.remove(&retry_key);
                    self.evidence.push(serde_json::json!({"queue_index":job.queue_index,"epoch":epoch,"cache_hit":hit,"classification":"computed","r21":rebuilt.contributions.iter().map(|r|serde_json::json!({"height":r.height,"poster":boole_derivation::attributes::hex_string(&r.poster),"item_len":r.item_len,"gas":21000u128+48*r.item_len as u128,"base_fee":r.base_fee})).collect::<Vec<_>>()}));
                    records.push(IndexedVerdict {
                        queue_index: job.queue_index,
                        record,
                    });
                }
                GateReply::Defer { class } => {
                    self.alerts.push(format!("DEFER({class})"));
                    if class != "d-node" {
                        self.retries
                            .entry(retry_key)
                            .or_default()
                            .deferred(now, self.class_disagreement);
                    }
                    break;
                }
            }
        }
        Ok(records)
    }
}
pub fn checked_record(
    input: &GateInput,
    d: &crate::stub::Descriptor,
    job: &Job,
    epoch: u32,
    v: &GateResult,
) -> Result<VerdictRecord> {
    let outcome = match v.outcome {
        1 => Outcome::Accept,
        2 => Outcome::Reject,
        3 => Outcome::Void,
        _ => anyhow::bail!("unknown typed outcome"),
    };
    ensure!(
        v.reason == 0
            || (0x0101..=0x0109).contains(&v.reason)
            || v.reason == 0x0201
            || (0x0301..=0x0306).contains(&v.reason)
            || (0x0801..=0x0804).contains(&v.reason),
        "descriptor reason enum"
    );
    ensure!(v.fuel <= input.fuel_slot, "gate fuel exceeds slot");
    let prefix = b"BOOLE-M3-STUB-PACKAGE/v1";
    if input.package.starts_with(prefix) && input.package.len() == prefix.len() + 40 {
        let cap = u64::from_be_bytes(input.package[prefix.len()..prefix.len() + 8].try_into()?);
        ensure!(v.fuel <= cap, "gate fuel exceeds leaf cap");
    } else {
        ensure!(v.fuel == 0, "malformed package consumes no kernel fuel");
    }
    let statement = VerdictStatement {
        chain_id: input.chain_id,
        problem_id: input.problem_id,
        commit_hash: input.commit_hash,
        solver: input.solver,
        env_id: input.env_id,
        export_digest: input.export_digest,
        statement_id: v.statement_id,
        gate_rule_digest: d.digest(),
        kernel_set_digest: d.kernel_set_digest,
        outcome,
        reason: v.reason,
        ext: v.ext.clone(),
    };
    ensure!(
        job.key == (input.problem_id, input.commit_hash),
        "adapter identity"
    );
    let record = VerdictRecord {
        kind: job.kind,
        commit_hash: input.commit_hash,
        outcome,
        reason: v.reason,
        gate_epoch: epoch,
        fuel: v.fuel,
        vstmt_digest: statement.digest(input.lane)?,
    };
    record.encode()?;
    Ok(record)
}
fn required_artifacts(c: &Config) -> std::collections::BTreeSet<Hash> {
    std::iter::once(c.base_root)
        .chain(c.env_allowed.iter().flatten().copied())
        .collect()
}
pub fn install_devnet_artifacts(c: &Config, root: &Path) -> Result<()> {
    ensure!(
        c.chain_id == 720926,
        "placeholder artifacts are devnet-only"
    );
    std::fs::create_dir_all(root)?;
    for bytes in [
        b"BOOLE-DEVNET/BASE/v1".as_slice(),
        b"BOOLE-DEVNET/EVM/v1",
        b"BOOLE-DEVNET/RISCV/v1",
    ] {
        let path = root.join(boole_derivation::attributes::hex_string(&digest(bytes)));
        if !path.exists() {
            private_file(&path, bytes)?;
        }
    }
    Ok(())
}
