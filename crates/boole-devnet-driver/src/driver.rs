use crate::{engine::Engine, l1::L1Source};
use anyhow::{bail, Context, Result};
use boole_derivation::{
    derive_with_m2,
    m2::{Config, State},
    Derived, ParentHeader,
};
use boole_devnet_archive::version::{file_stamp, stamp, FileStamp};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static CHECKPOINT_NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum StepOutcome {
    Derived(Box<Derived>),
    WaitingForFinality,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StallKind {
    M2Root { height: u64 },
    VerdictPending { height: u64, queue_index: u64 },
    Verdict { height: u64 },
    Availability { height: u64 },
    Integrity { height: u64 },
    Other,
}
#[derive(Debug, Clone)]
pub struct Stall {
    pub height: u64,
    pub reason: String,
    pub kind: StallKind,
}
impl std::fmt::Display for Stall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "stalled at {}: {}", self.height, self.reason)
    }
}
impl std::error::Error for Stall {}
#[derive(Debug)]
struct VerdictMismatch {
    height: u64,
}
impl std::fmt::Display for VerdictMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "system verdict prefix mismatch at {}", self.height)
    }
}
impl std::error::Error for VerdictMismatch {}
fn failure_kind(error: &anyhow::Error) -> StallKind {
    if let Some(stall) = error.downcast_ref::<Stall>() {
        return stall.kind.clone();
    }
    if let Some(e) = error.downcast_ref::<boole_devnet_archive::error::Error>() {
        return match e {
            boole_devnet_archive::error::Error::Availability { height, .. } => {
                StallKind::Availability { height: *height }
            }
            boole_devnet_archive::error::Error::Integrity { height, .. } => {
                StallKind::Integrity { height: *height }
            }
            _ => StallKind::Other,
        };
    }
    if crate::engine::rpc_unavailable(error) {
        return StallKind::Availability { height: 0 };
    }

    if let Some(v) = error.downcast_ref::<VerdictMismatch>() {
        return StallKind::Verdict { height: v.height };
    }
    match error.downcast_ref::<boole_derivation::InputError>() {
        Some(boole_derivation::InputError::M2Root { height }) => {
            StallKind::M2Root { height: *height }
        }
        Some(boole_derivation::InputError::M2(boole_derivation::m2::FoldError::M3(
            boole_derivation::m3::Error::Pending {
                height,
                queue_index,
            },
        ))) => StallKind::VerdictPending {
            height: *height,
            queue_index: *queue_index,
        },
        _ => StallKind::Other,
    }
}
struct AnchoredHistory<'a, L, E> {
    l1: &'a mut L,
    engine: &'a mut E,
    maximum: u64,
}
impl<L: L1Source, E: Engine> boole_gate_supervisor::rebuild::History for AnchoredHistory<'_, L, E> {
    fn origin(
        &mut self,
        height: u64,
    ) -> Result<(boole_derivation::L1Block, Vec<boole_derivation::InboxLog>)> {
        anyhow::ensure!(
            height <= self.maximum,
            "rebuild cannot read a future origin"
        );
        let header = self.engine.header(height)?;
        if header.number != height {
            return Err(boole_gate_supervisor::rebuild::Integrity("L2 origin height").into());
        }
        let hash = boole_derivation::system::l1_origin(
            header.l1_info.as_deref().context("rebuild L1-info")?,
            height,
        )?;
        let l1 = self.l1.block_by_hash(&hash)?;
        if l1.number != height || !l1.hash.eq_ignore_ascii_case(&hash) {
            return Err(boole_gate_supervisor::rebuild::Integrity("L1-info hash binding").into());
        }
        let logs = self.l1.inbox_logs(&hash)?;
        Ok((l1, logs))
    }
}

pub struct Driver<L, E> {
    pub l1: L,
    pub engine: E,
    stalled: Option<Stall>,
    pub supervisor: Option<boole_gate_supervisor::supervisor::Supervisor>,
    forks: Option<boole_derivation::m3::ForkTable>,
    evidence: Option<boole_devnet_archive::log::BoundLog>,
    recover_engine_tail: bool,
    tail_replay: Option<Vec<Value>>,
    retry_selected: Option<(u64, Derived)>,
    config: Config,
    l2_gas_limit: u64,
    m2: State,
    pending: Option<State>,
    selected_head: Option<String>,
    journal: Option<PathBuf>,
    checkpoint: Option<Value>,
    checkpoint_stamp: Option<FileStamp>,
    follow_records: std::collections::BTreeMap<u64, (boole_derivation::m3::IndexedVerdict, u64)>,
}
impl<L: L1Source, E: Engine> Driver<L, E> {
    pub fn new(l1: L, engine: E) -> Self {
        Self::construct(l1, engine, Config::devnet(), 30_000_000)
            .expect("valid built-in devnet config")
    }
    pub fn with_config(mut l1: L, engine: E, config: Config, l2_gas_limit: u64) -> Result<Self> {
        l1.validate_m2_inbox()?;
        Self::construct(l1, engine, config, l2_gas_limit)
    }
    fn construct(l1: L, engine: E, config: Config, l2_gas_limit: u64) -> Result<Self> {
        config
            .validate(l2_gas_limit)
            .map_err(|s| anyhow::anyhow!("M2 startup invariant S{s}"))?;
        let m2 = State::new(&config, l2_gas_limit)?;
        Ok(Self {
            l1,
            engine,
            stalled: None,
            supervisor: None,
            forks: None,
            evidence: None,
            recover_engine_tail: false,
            tail_replay: None,
            retry_selected: None,
            config,
            l2_gas_limit,
            m2,
            pending: None,
            selected_head: None,
            journal: None,
            checkpoint: None,
            checkpoint_stamp: None,
            follow_records: std::collections::BTreeMap::new(),
        })
    }
    pub fn with_m3(
        mut l1: L,
        engine: E,
        config: Config,
        l2_gas_limit: u64,
        mut supervisor: boole_gate_supervisor::supervisor::Supervisor,
        forks: boole_derivation::m3::ForkTable,
    ) -> Result<Self> {
        l1.validate_m2_inbox()?;
        anyhow::ensure!(config.m3.is_some(), "M3 profile required");
        forks
            .validate(&config)
            .map_err(|s| anyhow::anyhow!("startup S{s}"))?;
        supervisor.validate_startup(
            &config,
            &forks,
            &boole_gate_supervisor::supervisor::Host::local()?,
        )?;
        let mut driver = Self::construct(l1, engine, config, l2_gas_limit)?;
        driver.supervisor = Some(supervisor);
        driver.forks = Some(forks);
        Ok(driver)
    }
    fn follows(&self, h: u64) -> bool {
        self.supervisor.as_ref().is_some_and(|s| s.mode.follows(h))
    }
    /// Explicitly trusted canonical records, including lookahead for fuel-delayed
    /// jobs. Follow mode never fabricates a result to get past the fuel-clock wait.
    pub fn provide_follow_block(
        &mut self,
        l1: &boole_derivation::L1Block,
        txs: &[String],
    ) -> Result<()> {
        anyhow::ensure!(
            self.follows(l1.number),
            "canonical record outside explicit follow range"
        );
        anyhow::ensure!(
            txs.first() == Some(&boole_derivation::system::l1_info(l1)?),
            "follow L1-info mismatch"
        );
        let records = canonical_verdicts(&self.config, l1, txs)?;
        for record in &records {
            if let Some(previous) = self.follow_records.get(&record.queue_index) {
                anyhow::ensure!(
                    previous == &(record.clone(), l1.number),
                    "changed canonical follow record"
                );
            }
        }
        for record in records {
            self.follow_records
                .insert(record.queue_index, (record, l1.number));
        }
        Ok(())
    }
    pub fn set_forks(&mut self, forks: boole_derivation::m3::ForkTable) -> Result<()> {
        forks
            .validate(&self.config)
            .map_err(|s| anyhow::anyhow!("startup S{s}"))?;
        forks.validate_history(&self.m2)?;
        self.supervisor
            .as_mut()
            .context("M3 supervisor")?
            .validate_startup(
                &self.config,
                &forks,
                &boole_gate_supervisor::supervisor::Host::local()?,
            )?;
        self.forks = Some(forks);
        Ok(())
    }
    fn fold(
        &mut self,
        state: &State,
        l1: &boole_derivation::L1Block,
        logs: &[boole_derivation::InboxLog],
        parent: &ParentHeader,
    ) -> Result<(Derived, State)> {
        if self.config.m3.is_some() {
            let forks = self.forks.as_ref().context("M3 runtime not configured")?;
            let records = if self.follows(l1.number) {
                let m = state.m3.as_ref().context("M3 state")?;
                let queue =
                    boole_gate_supervisor::supervisor::jobs_at_height(state, l1.number, forks)?;
                let mut records = vec![];
                for (job, _) in &queue[m.next_applied..] {
                    let Some((record, source_height)) = self.follow_records.get(&job.queue_index)
                    else {
                        break;
                    };
                    let epoch = job.epoch_override.map(Ok).unwrap_or_else(|| {
                        forks.job_epoch(job.avail, self.config.m3.as_ref().expect("profile").k)
                    })?;
                    if record.record.gate_epoch != epoch {
                        if *source_height <= l1.number {
                            return Err(VerdictMismatch { height: l1.number }.into());
                        }
                        break;
                    }
                    records.push(record.clone());
                }
                records
            } else {
                let mut history = AnchoredHistory {
                    l1: &mut self.l1,
                    engine: &mut self.engine,
                    maximum: state.height(),
                };
                self.supervisor.as_mut().context("M3 supervisor")?.collect(
                    state,
                    &self.config,
                    l1.number,
                    forks,
                    &mut history,
                )?
            };
            Ok(boole_derivation::attributes::derive_with_m3(
                state,
                &self.config,
                l1,
                logs,
                parent,
                &records,
                forks,
            )?)
        } else {
            Ok(derive_with_m2(state, &self.config, l1, logs, parent)?)
        }
    }
    pub fn m2_state(&self) -> &State {
        &self.m2
    }
    pub fn set_journal(&mut self, path: &Path) -> Result<()> {
        let before = file_stamp(path)?;
        self.checkpoint = match boole_devnet_archive::journal::read_bound(path, 65536) {
            Ok((bytes, _)) => Some(serde_json::from_slice(&bytes).context("decode M2 checkpoint")?),
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                None
            }
            Err(e) => return Err(e).context("read M2 checkpoint"),
        };
        if file_stamp(path)? != before {
            return Err(boole_devnet_archive::error::integrity(
                0,
                "checkpoint",
                "checkpoint changed while reading",
            ));
        }
        self.checkpoint_stamp = before;
        self.journal = Some(path.to_owned());
        Ok(())
    }
    fn config_digest(&self) -> Result<String> {
        if self.config.m3.is_some() {
            // Config's canonical serializer preserves full u128 reward amounts.
            // serde_json::Value would narrow them before hashing the checkpoint.
            let mut hash = Sha256::new();
            hash.update(b"BOOLE-M3/DRIVER-CONFIG/v1");
            hash.update(self.config.digest());
            hash.update(self.l2_gas_limit.to_be_bytes());
            return Ok(boole_derivation::attributes::hex_string(&hash.finalize()));
        }
        Ok(boole_derivation::attributes::hex_string(&Sha256::digest(
            serde_json::to_vec(&json!({"config":self.config,"l2_gas_limit":self.l2_gas_limit}))?,
        )))
    }
    fn check_m2_transport(&mut self, logs: &[boole_derivation::InboxLog]) -> Result<()> {
        for log in logs {
            if let Ok(bytes) = boole_derivation::attributes::hex_bytes(&log.data, "log") {
                if let Ok((2..=15, _)) = boole_derivation::inbox::item(&bytes) {
                    self.l1.validate_m2_inbox()?;
                    break;
                }
            }
        }
        Ok(())
    }
    fn prepare_tail_replay(&mut self, parent: &ParentHeader) -> Result<()> {
        if let Some(height) = self.checkpoint.as_ref().and_then(|v| v["height"].as_u64()) {
            if height > parent.number {
                self.tail_replay.get_or_insert_with(Vec::new);
                if self.recover_engine_tail {
                    return Err(boole_devnet_archive::error::integrity(parent.number, "operator-checkpoint-move", "checkpoint move refused: immutable per-height evidence has no rollback epoch; preserve the store and recover in a fresh directory"));
                }
                let journal = self.l1.input_journal().ok_or_else(|| {
                    boole_devnet_archive::error::availability(height, "engine-tail", "own input journal required for automatic recovery; no historical RPC fallback")
                })?;
                journal.borrow().check()?;
                if journal.borrow().tip() < height {
                    return Err(boole_devnet_archive::error::availability(
                        height,
                        "engine-tail",
                        "own journal cannot cover the checkpoint tail; checkpoint preserved",
                    ));
                }
                self.l1.set_journal_replay(true);
            }
        }
        Ok(())
    }
    fn state_for_parent(&mut self, parent: &ParentHeader) -> Result<State> {
        self.prepare_tail_replay(parent)?;
        if let Some(saved) = &self.checkpoint {
            if saved["version"] != 1
                || saved["config_digest"] != self.config_digest()?
                || saved["height"].as_u64().is_none_or(|h| h == 0)
            {
                return Err(boole_devnet_archive::error::integrity(
                    parent.number,
                    "engine-tail",
                    "M2 checkpoint configuration or height mismatch",
                ));
            }
        }
        if self.m2.height() == parent.number {
            if self
                .selected_head
                .as_ref()
                .is_some_and(|hash| hash != &parent.hash)
            {
                bail!("cached L2 parent changed");
            }
            if parent.number > 0 {
                let origin = boole_derivation::system::l1_origin(
                    parent.l1_info.as_deref().context("cached parent L1-info")?,
                    parent.number,
                )?;
                boole_derivation::system::check_m2_root(
                    parent.m2_root_deposit.as_deref(),
                    parent.number,
                    &origin,
                    self.m2.root,
                )?;
            }
            return Ok(self.m2.clone());
        }
        if self.config.m3.is_some() {
            for h in 1..=parent.number {
                if self.follows(h) {
                    let header = self.engine.header(h)?;
                    let origin = boole_derivation::system::l1_origin(
                        header.l1_info.as_deref().context("follow origin")?,
                        h,
                    )?;
                    let l1 = self.l1.block_by_hash(&origin)?;
                    let txs = self.engine.transactions(h)?;
                    self.provide_follow_block(&l1, &txs)?;
                }
            }
        }
        let mut recovered = vec![];
        let mut last_derived = None;
        let mut state = State::new(&self.config, self.l2_gas_limit)?;
        let mut previous = self.engine.header(0).context("L2 genesis for re-fold")?;
        for height in 1..=parent.number {
            let header = if height == parent.number {
                parent.clone()
            } else {
                self.engine
                    .header(height)
                    .context("L2 history for re-fold")?
            };
            if header.number != height
                || header.parent_hash.as_deref() != Some(previous.hash.as_str())
            {
                bail!("L2 re-fold parentHash chain mismatch");
            }
            let deposit = header
                .l1_info
                .as_deref()
                .context("L2 history missing L1-info")?;
            let origin = boole_derivation::system::l1_origin(deposit, height)
                .context("decode re-fold L1-info")?;
            let l1 = self
                .l1
                .block_by_hash(&origin)
                .context("anchored L1 re-fold")?;
            if l1.number != height || !l1.hash.eq_ignore_ascii_case(&origin) {
                bail!("anchored L1 re-fold identity mismatch");
            }
            let logs = self
                .l1
                .inbox_logs(&origin)
                .context("anchored re-fold logs")?;
            self.check_m2_transport(&logs)?;
            let (derived, next) = self
                .fold(&state, &l1, &logs, &previous)
                .context("pure anchored re-fold")?;
            if derived.attributes.transactions.first().map(String::as_str) != Some(deposit) {
                bail!("anchored L1-info bytes mismatch");
            }
            boole_derivation::system::check_m2_root(
                header.m2_root_deposit.as_deref(),
                height,
                &origin,
                next.root,
            )?;
            if self.config.m3.is_some() {
                let txs = self.engine.transactions(height)?;
                check_payload_transactions(&json!({"transactions":txs}), &derived)
                    .map_err(|_| VerdictMismatch { height })?;
            }
            if let Some(saved) = &self.checkpoint {
                if saved["height"] == height
                    && (saved["m2_root"] != boole_derivation::attributes::hex_string(&next.root)
                        || saved["l2_hash"] != header.hash
                        || saved["l1_hash"] != l1.hash)
                {
                    return Err(boole_devnet_archive::error::integrity(
                        height,
                        "checkpoint",
                        "M2 checkpoint disagrees with anchored re-fold",
                    ));
                }
            }
            if self.l1.input_journal().is_some() {
                let record = json!({"height":height,"outcome":"RECOVERED","verification":if self.follows(height){"checkpoint-trusted"}else{"computed"},"block_hash":header.hash,"l1":{"hash":l1.hash},"m2_root":boole_derivation::attributes::hex_string(&next.root),"attribute_digest":boole_derivation::attributes::hex_string(&Sha256::digest(derived.canonical_bytes()?))});
                if let Some(evidence) = &self.evidence {
                    if let Some(saved) = evidence.record_at(height) {
                        anyhow::ensure!(
                            saved["block_hash"] == record["block_hash"]
                                && saved["l1_hash"] == l1.hash
                                && saved["m2_root"] == record["m2_root"]
                                && saved["attribute_digest"] == record["attribute_digest"],
                            "evidence disagrees with verified Engine replay at {height}"
                        );
                    } else {
                        recovered.push(record);
                    }
                }
            }
            last_derived = Some(derived);
            state = next;
            previous = header;
        }
        // Publish recovery evidence only after the complete replay/checkpoint comparison.
        if let Some(pending) = &mut self.tail_replay {
            pending.extend(recovered);
            if self
                .checkpoint
                .as_ref()
                .is_some_and(|v| v["height"].as_u64().is_some_and(|h| h <= parent.number))
            {
                if let Some(file) = &mut self.evidence {
                    for row in pending.iter() {
                        if file
                            .record_at(row["height"].as_u64().expect("replay height"))
                            .is_none()
                        {
                            file.append(row)?;
                        }
                    }
                }
                pending.clear();
            }
        } else {
            for record in &recovered {
                self.evidence
                    .as_mut()
                    .expect("recovery evidence")
                    .append(record)?;
            }
        }
        if self.l1.input_journal().is_some() && parent.number > 0 {
            let origin = boole_derivation::system::l1_origin(
                parent.l1_info.as_deref().context("recovery origin")?,
                parent.number,
            )?;
            self.persist_checkpoint(
                &state,
                &json!({"block_hash":parent.hash,"l1":{"hash":origin}}),
            )?;
        }
        self.retry_selected = last_derived.map(|d| (parent.number, d));
        self.m2 = state.clone();
        self.selected_head = Some(parent.hash.clone());
        Ok(state)
    }
    fn persist_checkpoint(&mut self, state: &State, record: &Value) -> Result<()> {
        if self
            .checkpoint
            .as_ref()
            .and_then(|v| v["height"].as_u64())
            .is_some_and(|h| h > state.height())
        {
            return Ok(());
        }

        let Some(path) = &self.journal else {
            return Ok(());
        };
        if file_stamp(path)? != self.checkpoint_stamp {
            return Err(boole_devnet_archive::error::integrity(
                state.height(),
                "checkpoint",
                "M4 integrity: checkpoint changed before publication",
            ));
        }
        let value = json!({"version":1,"height":state.height(),"m2_root":boole_derivation::attributes::hex_string(&state.root),"l2_hash":record["block_hash"],"l1_hash":record["l1"]["hash"],"config_digest":self.config_digest()?});
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos();
        let attempt = CHECKPOINT_NONCE.fetch_add(1, Ordering::Relaxed);
        let temporary =
            path.with_extension(format!("tmp-{}-{nonce:x}-{attempt:x}", std::process::id()));
        let mut created = false;
        let mut renamed = false;
        let mut observed = None;
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            created = true;
            serde_json::to_writer(&mut file, &value)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            boole_devnet_archive::fault::hit("B6", state.height(), "driver")?;
            std::fs::rename(&temporary, path)?;
            renamed = true;
            boole_devnet_archive::fault::hit("B7", state.height(), "driver")?;
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                File::open(parent)?.sync_all()?;
            }
            let actual = stamp(&file.metadata()?)?;
            anyhow::ensure!(
                file_stamp(path)?.as_ref() == Some(&actual),
                "checkpoint changed during publication"
            );
            observed = Some(actual);
            Ok(())
        })();
        if result.is_err() && created {
            let _ = std::fs::remove_file(&temporary);
        }
        if let Err(error) = result {
            if renamed {
                return Err(boole_devnet_archive::error::integrity(
                    state.height(),
                    "checkpoint-publication",
                    format!("post-rename failure; restart required: {error:#}"),
                ));
            }
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::StorageFull)
            {
                return Err(boole_devnet_archive::error::availability(
                    state.height(),
                    "disk",
                    "checkpoint space unavailable; previous checkpoint preserved",
                ));
            }
            return Err(error.context("persist M2 checkpoint"));
        }
        self.checkpoint = Some(value);
        self.checkpoint_stamp = observed;
        Ok(())
    }
    pub fn set_recover_engine_tail(&mut self, enabled: bool) {
        self.recover_engine_tail = enabled;
    }
    /// Re-verify local inputs against the selected Engine chain before serving.
    pub fn resume(&mut self) -> Result<()> {
        if let Some(stall) = &self.stalled {
            return Err(stall.clone().into());
        }
        let mut height = 0;
        let result = (|| -> Result<()> {
            let parent = self.engine.head()?;
            height = parent.number;
            self.state_for_parent(&parent)?;
            if let Some(target) = self.checkpoint.as_ref().and_then(|v| v["height"].as_u64()) {
                for h in parent.number.saturating_add(1)..=target {
                    if !matches!(self.step(h)?, StepOutcome::Derived(_)) {
                        return Err(boole_devnet_archive::error::availability(
                            h,
                            "engine-tail",
                            "own journal tail unavailable; checkpoint preserved",
                        ));
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = &result {
            let kind = failure_kind(error);
            if self.stalled.is_none()
                && !matches!(
                    kind,
                    StallKind::Availability { .. } | StallKind::VerdictPending { .. }
                )
            {
                self.stalled = Some(Stall {
                    height,
                    kind,
                    reason: format!("{error:#}"),
                });
            }
        }
        result
    }
    pub fn set_evidence(&mut self, path: &Path) -> Result<()> {
        self.set_evidence_repair(path, false)?;
        Ok(())
    }
    pub fn set_evidence_repair(&mut self, path: &Path, repair: bool) -> Result<()> {
        self.evidence = Some(boole_devnet_archive::log::BoundLog::open(path, repair)?);
        Ok(())
    }
    pub fn last_failure(&self) -> Option<&Stall> {
        self.stalled.as_ref()
    }
    pub fn step(&mut self, height: u64) -> std::result::Result<StepOutcome, Stall> {
        if let Some(stall) = &self.stalled {
            return Err(stall.clone());
        }
        let mut record = json!({"height":height,"engine_calls":[],"slot_table_digest":boole_derivation::attributes::hex_string(&self.config.slot_table_digest()),"m2_config_digest":boole_derivation::attributes::hex_string(&self.config.digest())});
        let result = self.try_step(height, &mut record, None);
        self.complete_step(height, record, result, Derived::canonical_bytes)
    }

    /// Import an externally built execution payload only after the local fold
    /// and its mandatory M2ROOT commitment agree, before Engine adoption.
    pub fn import_payload(
        &mut self,
        height: u64,
        payload: Value,
    ) -> std::result::Result<StepOutcome, Stall> {
        if let Some(stall) = &self.stalled {
            return Err(stall.clone());
        }
        let mut record = json!({"height":height,"engine_calls":[],"import":true,"slot_table_digest":boole_derivation::attributes::hex_string(&self.config.slot_table_digest()),"m2_config_digest":boole_derivation::attributes::hex_string(&self.config.digest())});
        let result = self.try_step(height, &mut record, Some(payload));
        self.complete_step(height, record, result, Derived::canonical_bytes)
    }

    fn complete_step(
        &mut self,
        height: u64,
        mut record: Value,
        result: Result<StepOutcome>,
        canonical: impl FnOnce(&Derived) -> serde_json::Result<Vec<u8>>,
    ) -> std::result::Result<StepOutcome, Stall> {
        record["verification"] = json!(if self.follows(height) {
            "checkpoint-trusted"
        } else {
            "computed"
        });
        record["inputs"] = json!(self.l1.inputs_label(height));
        if matches!(&result, Ok(StepOutcome::Derived(_))) {
            let alerts = self.l1.archive_tripwire(
                height,
                record["block_hash"].as_str().unwrap_or(""),
                record["m2_root"].as_str().unwrap_or(""),
            );
            for alert in &alerts {
                eprintln!("{alert}");
            }
            if !alerts.is_empty() {
                record["archive_alerts"] = json!(alerts);
            }
            let stats = self.l1.archive_stats();
            if !stats.is_null() {
                record["archive_sources"] = stats;
            }
        }
        if let Some(supervisor) = &mut self.supervisor {
            record["gate_events"] = json!(std::mem::take(&mut supervisor.evidence));
            record["gate_alerts"] = json!(std::mem::take(&mut supervisor.alerts));
        }
        let result = result.and_then(|outcome| {
            match &outcome {
                StepOutcome::WaitingForFinality => {
                    record["outcome"] = json!("WAITING_FOR_FINALITY")
                }
                StepOutcome::Derived(derived) => {
                    record["outcome"] = json!(if record["recovered"] == true {
                        "RECOVERED"
                    } else {
                        "DERIVED"
                    });
                    record["derived"] = json!(derived);
                    let bytes = canonical(derived).context("canonical attribute encoding")?;
                    use sha2::{Digest, Sha256};
                    record["attribute_digest"] = json!(boole_derivation::attributes::hex_string(
                        &Sha256::digest(bytes)
                    ));
                }
            }
            Ok(outcome)
        });
        let replaying = self.tail_replay.is_some();
        if let Err(error) = &result {
            record["outcome"] = json!("STALL");
            record["error"] = json!(format!("{error:#}"));
            record["m2_root"] = json!(boole_derivation::attributes::hex_string(&self.m2.root));
        }
        let persisted = (|| -> Result<()> {
            if let Some(pending) = &mut self.tail_replay {
                if result.is_ok()
                    && matches!(record["outcome"].as_str(), Some("DERIVED" | "RECOVERED"))
                {
                    if let Some(file) = &mut self.evidence {
                        if let Some(saved) = file.record_at(height) {
                            if saved["block_hash"] != record["block_hash"]
                                || saved["l1_hash"] != record["l1"]["hash"]
                                || saved["m2_root"] != record["m2_root"]
                                || saved["attribute_digest"] != record["attribute_digest"]
                            {
                                return Err(boole_devnet_archive::error::integrity(
                                    height,
                                    "engine-tail",
                                    "re-derived tail disagrees with durable evidence",
                                ));
                            }
                        } else {
                            record["outcome"] = json!("RECOVERED");
                            pending.push(record.clone());
                        }
                    }
                    if self
                        .checkpoint
                        .as_ref()
                        .is_some_and(|v| v["height"] == height)
                    {
                        if let Some(file) = &mut self.evidence {
                            for row in pending.iter() {
                                if file
                                    .record_at(row["height"].as_u64().expect("replay height"))
                                    .is_none()
                                {
                                    file.append(row)?;
                                }
                            }
                        }
                        pending.clear();
                    }
                }
                return Ok(());
            }
            if let Some(file) = &mut self.evidence {
                if record["outcome"] != "RECOVERED" || file.record_at(height).is_none() {
                    file.append(&record)?;
                }
            }
            if record["outcome"] == "DERIVED" {
                boole_devnet_archive::fault::hit("B5", height, "driver")?;
            }
            Ok(())
        })();
        let result = match persisted {
            Ok(()) => result,
            Err(error) => Err(error.context("persist driver evidence")),
        };
        let checkpoint_attempt = result.is_ok();
        let pending = self.pending.take();
        let result = result.and_then(|outcome| {
            if let Some(state) = pending {
                let hash = record["block_hash"]
                    .as_str()
                    .context("selected block hash missing")?
                    .to_owned();
                self.persist_checkpoint(&state, &record)?;
                self.selected_head = Some(hash);
                self.m2 = state;
            }
            if replaying
                && self.m2.height() == height
                && self
                    .checkpoint
                    .as_ref()
                    .is_some_and(|v| v["height"] == height)
            {
                self.tail_replay = None;
                self.l1.set_journal_replay(false);
            }
            Ok(outcome)
        });
        if checkpoint_attempt && !replaying {
            if let Err(error) = &result {
                record["outcome"] = json!("STALL");
                record["error"] = json!(format!("{error:#}"));
                record["m2_root"] = json!(boole_derivation::attributes::hex_string(&self.m2.root));
                if let Some(file) = &mut self.evidence {
                    let appended = (|| -> Result<()> {
                        file.append(&record)?;
                        Ok(())
                    })();
                    if let Err(write_error) = appended {
                        let stall = Stall {
                            height,
                            kind: failure_kind(error),
                            reason: format!("{error:#}; append STALL evidence: {write_error:#}"),
                        };
                        self.stalled = Some(stall.clone());
                        return Err(stall);
                    }
                }
            }
        }
        match result {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                let stall = Stall {
                    height,
                    kind: failure_kind(&error),
                    reason: format!("{error:#}"),
                };
                if !matches!(
                    stall.kind,
                    StallKind::VerdictPending { .. } | StallKind::Availability { .. }
                ) {
                    self.stalled = Some(stall.clone());
                }
                Err(stall)
            }
        }
    }

    fn call(&mut self, record: &mut Value, method: &str, params: Value) -> Result<Value> {
        let result = self.engine.call(method, params.clone());
        let response = match &result {
            Ok(value) => json!({"result":value}),
            Err(error) => json!({"error":format!("{error:#}")}),
        };
        if let Some(calls) = record["engine_calls"].as_array_mut() {
            calls.push(json!({"method":method,"params":params,"response":response}));
        }
        result.with_context(|| format!("Engine {method}"))
    }

    fn try_step(
        &mut self,
        height: u64,
        record: &mut Value,
        supplied: Option<Value>,
    ) -> Result<StepOutcome> {
        let parent = self.engine.head().context("L2 parent")?;
        self.prepare_tail_replay(&parent)?;
        if let Some(path) = &self.journal {
            if file_stamp(path)? != self.checkpoint_stamp {
                return Err(boole_devnet_archive::error::integrity(
                    height,
                    "checkpoint",
                    "M4 integrity: checkpoint changed before derivation",
                ));
            }
        }
        let finalized = self.l1.finalized().context("L1 finalized")?;
        record["finalized"] = json!(finalized);
        if height > finalized {
            return Ok(StepOutcome::WaitingForFinality);
        }
        if parent.number == height && self.l1.input_journal().is_some() {
            self.state_for_parent(&parent)?;
            if let Some((h, d)) = &self.retry_selected {
                if *h == height {
                    record["recovered"] = json!(true);
                    record["block_hash"] = json!(parent.hash);
                    record["l1"] = json!({"hash":boole_derivation::system::l1_origin(parent.l1_info.as_deref().context("recovered origin")?,height)?});
                    record["m2_root"] =
                        json!(boole_derivation::attributes::hex_string(&self.m2.root));
                    return Ok(StepOutcome::Derived(Box::new(d.clone())));
                }
            }
        }
        if parent.number.checked_add(1) != Some(height) {
            bail!("restart height must be node head + 1");
        }
        let l1 = self.l1.block(height).context("L1 block")?;
        if l1.number != height {
            bail!("L1 returned wrong height");
        }
        let logs = self.l1.inbox_logs(&l1.hash).context("L1 inbox logs")?;
        let reread = self.l1.block(height).context("L1 block reread")?;
        if serde_json::to_value(&l1)? != serde_json::to_value(&reread)? {
            bail!("L1 block changed between reads");
        }
        self.check_m2_transport(&logs)?;
        let previous = self.state_for_parent(&parent)?;
        if self.follows(height) {
            if let Some(payload) = &supplied {
                let txs: Vec<String> = serde_json::from_value(payload["transactions"].clone())?;
                self.provide_follow_block(&l1, &txs)
                    .map_err(|_| VerdictMismatch { height })?;
            }
        }
        let (derived, next) = self
            .fold(&previous, &l1, &logs, &parent)
            .context("pure derivation input")?;
        if self
            .supervisor
            .as_ref()
            .is_some_and(|s| s.evidence.iter().any(|v| v["cache_hit"] == false))
        {
            boole_devnet_archive::fault::hit("B8", height, "driver")?;
        }
        record["m2_root"] = json!(boole_derivation::attributes::hex_string(&next.root));
        record["l1"] = json!(l1);
        record["parent"] = json!(parent);
        record["attributes"] = json!(derived.attributes);
        record["decisions"] = json!(derived.decisions);
        let payload = if let Some(payload) = supplied {
            payload
        } else {
            let state = json!({"headBlockHash":parent.hash,"safeBlockHash":parent.hash,"finalizedBlockHash":parent.hash});
            let built = self.call(
                record,
                "engine_forkchoiceUpdatedV3",
                json!([state, derived.attributes]),
            )?;
            valid(&built["payloadStatus"])?;
            let payload_id = built["payloadId"]
                .as_str()
                .filter(|s| !s.is_empty())
                .context("VALID build missing payloadId")?;
            let envelope = self.call(record, "engine_getPayloadV5", json!([payload_id]))?;
            boole_devnet_archive::fault::hit("B2", height, "driver")?;
            envelope["executionPayload"].clone()
        };
        let payload = &payload;
        if self.config.m3.is_some() {
            check_payload_transactions(payload, &derived)
                .map_err(|_| VerdictMismatch { height })?;
        }
        boole_derivation::system::check_m2_root(
            payload["transactions"][1].as_str(),
            height,
            &l1.hash,
            next.root,
        )?;
        check_payload_transactions(payload, &derived)?;
        if payload["blockNumber"] != format!("0x{height:x}")
            || payload["parentHash"] != parent.hash
            || payload["timestamp"] != derived.attributes.timestamp
            || payload["gasLimit"] != derived.attributes.gas_limit
        {
            bail!("payload header disagrees with derivation");
        }
        // Fence L1 again before importing a payload built by the client.
        let final_read = self.l1.block(height).context("L1 pre-import reread")?;
        if serde_json::to_value(&l1)? != serde_json::to_value(&final_read)? {
            bail!("L1 block changed during build");
        }
        if let Some(saved) = &self.checkpoint {
            if saved["height"] == height
                && (saved["m2_root"] != boole_derivation::attributes::hex_string(&next.root)
                    || saved["l1_hash"] != l1.hash
                    || saved["l2_hash"] != payload["blockHash"])
            {
                return Err(boole_devnet_archive::error::integrity(
                    height,
                    "engine-tail",
                    "engine-tail recovery disagrees with saved checkpoint",
                ));
            }
        }
        let imported = self.call(
            record,
            "engine_newPayloadV4",
            json!([payload, [], derived.attributes.parent_beacon_block_root, []]),
        )?;
        valid(&imported)?;
        boole_devnet_archive::fault::hit("B3", height, "driver")?;
        let hash = payload["blockHash"]
            .as_str()
            .context("payload missing blockHash")?;
        let selected = self.call(
            record,
            "engine_forkchoiceUpdatedV3",
            json!([{"headBlockHash":hash,"safeBlockHash":hash,"finalizedBlockHash":hash},null]),
        )?;
        valid(&selected["payloadStatus"])?;
        let head = self.engine.head().context("L2 post-selection head")?;
        if head.number != height || head.hash != hash {
            bail!("L2 head does not match selected payload");
        }
        boole_derivation::system::check_m2_root(
            head.m2_root_deposit.as_deref(),
            height,
            &l1.hash,
            next.root,
        )?;
        if self.config.m3.is_some() {
            let txs = self.engine.transactions(height)?;
            check_payload_transactions(&json!({"transactions":txs}), &derived)
                .map_err(|_| VerdictMismatch { height })?;
        }
        boole_devnet_archive::fault::hit("B4", height, "driver")?;
        record["block_hash"] = json!(hash);
        self.pending = Some(next);
        Ok(StepOutcome::Derived(Box::new(derived)))
    }
}

fn canonical_verdicts(
    c: &Config,
    l1: &boole_derivation::L1Block,
    txs: &[String],
) -> Result<Vec<boole_derivation::m3::IndexedVerdict>> {
    let sink = c.m3.as_ref().context("M3 profile")?.verdict_sink;
    let mut deposits = vec![];
    for tx in txs.iter().skip(2).take_while(|tx| tx.starts_with("0x7e")) {
        let bytes = boole_derivation::attributes::hex_bytes(tx, "canonical deposit")?;
        let fields = boole_derivation::rlp::decode(bytes.get(1..).context("deposit type")?)
            .map_err(anyhow::Error::msg)?
            .children
            .context("deposit fields")?;
        anyhow::ensure!(fields.len() == 8, "deposit fields");
        if fields[2].bytes == sink || fields[7].bytes.starts_with(b"m3vd") {
            deposits.push(tx.clone());
        } else {
            break;
        }
    }
    Ok(boole_derivation::m3::decode_verdict_deposits(
        c, l1, &deposits,
    )?)
}

fn valid(status: &Value) -> Result<()> {
    if status["status"] != "VALID" {
        bail!("non-VALID Engine payload status: {status}");
    }
    Ok(())
}

pub fn check_payload_transactions(payload: &Value, derived: &Derived) -> Result<()> {
    let txs = payload["transactions"]
        .as_array()
        .context("payload transactions missing")?;
    let expected = &derived.attributes.transactions;
    let system_count = expected
        .iter()
        .take_while(|tx| tx.starts_with("0x7e"))
        .count();
    if txs.len() < system_count
        || txs[..system_count]
            .iter()
            .zip(&expected[..system_count])
            .any(|(a, b)| a.as_str() != Some(b.as_str()))
    {
        bail!("payload system prefix missing, changed or reordered");
    }
    let mut remaining = expected.iter();
    for tx in txs {
        let raw = tx.as_str().context("non-string payload transaction")?;
        if !remaining.any(|expected| expected == raw) {
            bail!("payload transactions are not an order-preserving subsequence");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use boole_derivation::derive;
    struct Unused;
    impl L1Source for Unused {
        fn finalized(&mut self) -> Result<u64> {
            panic!("sticky stall must not read L1")
        }
        fn block(&mut self, _n: u64) -> Result<boole_derivation::L1Block> {
            panic!("unused")
        }
        fn inbox_logs(&mut self, _hash: &str) -> Result<Vec<boole_derivation::InboxLog>> {
            panic!("unused")
        }
    }
    impl Engine for Unused {
        fn head(&mut self) -> Result<boole_derivation::ParentHeader> {
            panic!("sticky stall must not read head")
        }
        fn call(&mut self, _method: &str, _params: Value) -> Result<Value> {
            panic!("sticky stall must not call Engine")
        }
    }
    #[test]
    fn canonical_encoding_failure_keeps_stall_state() {
        let rows: Vec<Value> = serde_json::from_str(include_str!(
            "../../../fixtures/devnet-m1/s1b-recorded-main.json"
        ))
        .unwrap();
        let row = &rows[0];
        let d = derive(
            &serde_json::from_value(row["l1"].clone()).unwrap(),
            &serde_json::from_value::<Vec<_>>(row["logs"].clone()).unwrap(),
            &serde_json::from_value(row["parent"].clone()).unwrap(),
        )
        .unwrap();
        let mut driver = Driver::new(Unused, Unused);
        let error = driver
            .complete_step(
                1,
                json!({"height":1,"engine_calls":[]}),
                Ok(StepOutcome::Derived(Box::new(d))),
                |_| serde_json::from_str::<Value>("invalid").map(|_| vec![]),
            )
            .unwrap_err();
        assert_eq!(driver.last_failure().map(|x| x.height), Some(1));
        assert_eq!(driver.step(1).unwrap_err().reason, error.reason);
    }
}

pub mod archive;
