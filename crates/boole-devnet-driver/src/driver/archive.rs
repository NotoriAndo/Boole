//! Archive only blocks already computed and verified by this ordinary driver.
//! A stalled sink retains its cursor; it never asks the driver to re-apply a fold.
use super::{canonical_verdicts, check_payload_transactions, AnchoredHistory, Driver};
use crate::{engine::Engine, l1::L1Source};
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    m2::*,
    m3::*,
};
use boole_devnet_archive::{
    archive::{ArchiveStore, Selection},
    error::{availability, integrity},
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
#[derive(Clone)]
pub struct CapturePaths {
    pub cache: PathBuf,
    pub artifacts: PathBuf,
    pub scratch: PathBuf,
}
pub struct ArchiveSink {
    pub store: Arc<Mutex<ArchiveStore>>,
    state: State,
    last_hash: String,
    paths: Option<CapturePaths>,
}
impl ArchiveSink {
    pub fn new<L: L1Source, E: Engine>(
        driver: &mut Driver<L, E>,
        store: Arc<Mutex<ArchiveStore>>,
        paths: Option<CapturePaths>,
    ) -> Result<Self> {
        ensure!(
            !driver.supervisor.as_ref().is_some_and(|s| !matches!(
                s.mode,
                boole_gate_supervisor::supervisor::Mode::Verify
            )),
            "archive node requires verify mode"
        );
        ensure!(
            driver.l1.input_journal().is_some(),
            "archive node requires its authenticated own journal"
        );
        let genesis = driver.engine.header(0)?;
        let network = boole_devnet_archive::journal::NetworkIdentity::new(
            &driver.config,
            driver.forks.as_ref(),
            genesis.hash.clone(),
            driver.l2_gas_limit,
        )?;
        ensure!(
            store
                .lock()
                .map_err(|_| anyhow::anyhow!("archive lock poisoned"))?
                .identity()
                .network
                == network,
            "S18: archive/driver identity mismatch"
        );
        let state = State::new(&driver.config, driver.l2_gas_limit)?;
        Ok(Self {
            store,
            state,
            last_hash: genesis.hash,
            paths,
        })
    }
    pub fn synchronize<L: L1Source, E: Engine>(&mut self, driver: &mut Driver<L, E>) -> Result<()> {
        let h = driver.m2.height();
        let result = self.synchronize_inner(driver);
        result.map_err(|e| {
            if e.is::<boole_devnet_archive::error::Error>() {
                e
            } else {
                availability(
                    h,
                    "archive-ingest",
                    format!("public archive sink paused: {e:#}"),
                )
            }
        })
    }
    fn synchronize_inner<L: L1Source, E: Engine>(
        &mut self,
        driver: &mut Driver<L, E>,
    ) -> Result<()> {
        let through = driver.m2.height();
        let head = driver.engine.head()?;
        ensure!(
            head.number == through
                && (driver
                    .selected_head
                    .as_ref()
                    .is_some_and(|hash| hash == &head.hash)
                    || (through == 0 && head.hash == self.last_hash)),
            "archive sink requires the driver's verified selected head"
        );
        let row0 = driver.l1.input_row(0)?;
        if self.state.height() == 0 {
            self.store
                .lock()
                .map_err(|_| anyhow::anyhow!("archive lock poisoned"))?
                .ingest(
                    &row0,
                    through,
                    &Selection {
                        l2_number: 0,
                        l2_hash: self.last_hash.clone(),
                        root: hex_string(&self.state.root),
                        system: vec![],
                        inputs: driver.l1.inputs_label(0).into(),
                        verdicts: "computed".into(),
                    },
                )?;
            self.capture_artifacts(driver)?;
        }
        for height in self.state.height() + 1..=through {
            let row = driver.l1.input_row(height)?;
            let origin = row.verify()?;
            let parent = driver.engine.header(height - 1)?;
            ensure!(
                parent.hash == self.last_hash,
                "archive Engine prefix changed"
            );
            let header = driver.engine.header(height)?;
            ensure!(
                header.parent_hash.as_ref() == Some(&parent.hash),
                "archive Engine parent link"
            );
            let txs = driver.engine.transactions(height)?;
            let verdicts = if driver.config.m3.is_some() {
                canonical_verdicts(&driver.config, &origin.block, &txs)?
            } else {
                vec![]
            };
            let (derived, next) = if let Some(forks) = &driver.forks {
                boole_derivation::attributes::derive_with_m3(
                    &self.state,
                    &driver.config,
                    &origin.block,
                    &origin.logs,
                    &parent,
                    &verdicts,
                    forks,
                )?
            } else {
                boole_derivation::attributes::derive_with_m2(
                    &self.state,
                    &driver.config,
                    &origin.block,
                    &origin.logs,
                    &parent,
                )?
            };
            check_payload_transactions(&serde_json::json!({"transactions":txs}), &derived)?;
            boole_derivation::system::check_m2_root(
                header.m2_root_deposit.as_deref(),
                height,
                &origin.block.hash,
                next.root,
            )?;
            self.capture_candidates(driver, &next, &verdicts, height)?;
            let system = derived
                .attributes
                .transactions
                .iter()
                .take_while(|t| t.starts_with("0x7e"))
                .cloned()
                .collect();
            self.store
                .lock()
                .map_err(|_| anyhow::anyhow!("archive lock poisoned"))?
                .ingest(
                    &row,
                    through,
                    &Selection {
                        l2_number: height,
                        l2_hash: header.hash.clone(),
                        root: hex_string(&next.root),
                        system,
                        inputs: driver.l1.inputs_label(height).into(),
                        verdicts: "computed".into(),
                    },
                )?;
            self.state = next;
            self.last_hash = header.hash;
        }
        ensure!(
            self.state.root == driver.m2.root && self.last_hash == head.hash,
            "archive fold differs from verified driver"
        );
        let reread = driver.engine.head()?;
        ensure!(
            reread.hash == head.hash && reread.number == head.number,
            "Engine changed during archive catch-up"
        );
        Ok(())
    }
    fn capture_artifacts<L: L1Source, E: Engine>(&self, driver: &Driver<L, E>) -> Result<()> {
        let Some(supervisor) = &driver.supervisor else {
            return Ok(());
        };
        let paths = self
            .paths
            .as_ref()
            .context("archive capture paths required for M3")?;
        let store = self
            .store
            .lock()
            .map_err(|_| anyhow::anyhow!("archive lock poisoned"))?;
        for worker in supervisor.workers.values() {
            worker.validate()?;
            let bytes =
                boole_devnet_archive::journal::read_bound(&worker.binary, RAW_CAP as usize)?.0;
            store.put_content("gate", worker.binary_sha256, &bytes)?;
        }
        let ids = std::iter::once(driver.config.base_root)
            .chain(driver.config.env_allowed.iter().flatten().copied())
            .collect::<std::collections::BTreeSet<_>>();
        for id in ids {
            let bytes = artifact(&paths.artifacts, id)?;
            store.put_content("env", id, &bytes)?;
        }
        Ok(())
    }
    fn capture_candidates<L: L1Source, E: Engine>(
        &self,
        driver: &mut Driver<L, E>,
        next: &State,
        records: &[IndexedVerdict],
        height: u64,
    ) -> Result<()> {
        if driver.config.m3.is_none() {
            return Ok(());
        }
        let paths = self.paths.as_ref().context("archive capture paths")?;
        let c = &driver.config;
        let codec = driver.supervisor.as_ref().and_then(|s| s.codec.as_ref());
        let mut history = AnchoredHistory {
            l1: &mut driver.l1,
            engine: &mut driver.engine,
            maximum: height,
        };
        let completed = next
            .candidates
            .iter()
            .filter(|(_, cand)| {
                cand.live()
                    .and_then(|l| l.open.as_ref())
                    .is_some_and(|o| o.h_r == Some(height))
            })
            .map(|(key, _)| *key)
            .collect::<Vec<_>>();
        for key in completed {
            let rebuilt = boole_gate_supervisor::rebuild::rebuild(
                next,
                c,
                key,
                &mut history,
                codec,
                &paths.scratch,
            )?;
            let open = next.candidates[&key].live().unwrap().open.as_ref().unwrap();
            let t = public_cancel_hash(next, key, &mut history)?;
            let store = self
                .store
                .lock()
                .map_err(|_| anyhow::anyhow!("archive lock poisoned"))?;
            store.put_transport(next, key, t, &rebuilt.body, &rebuilt.transport)?;
            if !rebuilt.input.raw.is_empty()
                && boole_gate_supervisor::codec::digest(&rebuilt.input.raw) == open.export_digest
            {
                store.put_export(next, key, &rebuilt.input.raw)?;
            }
            let carried = carried_package(next, key, &mut history)?;
            if !carried.is_empty() {
                store.put_content(
                    "pkg",
                    boole_gate_supervisor::codec::digest(&carried),
                    &carried,
                )?;
            }
        }
        let Some(forks) = &driver.forks else {
            return Ok(());
        };
        let jobs = boole_gate_supervisor::supervisor::jobs_at_height(&self.state, height, forks)?;
        for indexed in records {
            let (job, forced_void) = jobs
                .iter()
                .find(|(j, _)| j.queue_index == indexed.queue_index)
                .context("applied archive job")?;
            let epoch = match job.epoch_override {
                Some(epoch) => epoch,
                None => forks.job_epoch(job.avail, c.m3.as_ref().unwrap().k)?,
            };
            let row = &forks.rows[epoch as usize];
            let worker = driver
                .supervisor
                .as_ref()
                .context("archive supervisor")?
                .workers
                .get(&row.gate_rule_digest)
                .context("archive epoch binary")?;
            let mut rebuilt = boole_gate_supervisor::rebuild::rebuild(
                &self.state,
                c,
                job.key,
                &mut history,
                codec,
                &paths.scratch,
            )?;
            for id in rebuilt.components {
                rebuilt
                    .input
                    .artifacts
                    .push(artifact(&paths.artifacts, id)?);
            }
            if *forced_void {
                rebuilt.input.early = Some((3, boole_derivation::m3::QUARANTINE_VOID));
            }
            // Only the rebound typed result is read. No cache key, MAC or private file
            // is ever copied into the public archive.
            let cache = boole_gate_supervisor::cache::Cache::new(&paths.cache)?;
            let key = boole_gate_supervisor::cache::CacheKey {
                gate: row.gate_rule_digest,
                queue_index: job.queue_index,
                commit_hash: job.key.1,
                kind: job.kind as u8,
            };
            let result = match cache.load(&key, &rebuilt.input, epoch) {
                Some(result) => result,
                None => match worker.run(
                    &rebuilt.input,
                    &driver.supervisor.as_ref().unwrap().envelope,
                    &paths.scratch,
                ) {
                    boole_gate_supervisor::GateReply::Result { value } => value,
                    boole_gate_supervisor::GateReply::Defer { .. } => {
                        return Err(availability(
                            height,
                            "archive-vstmt",
                            "public preimage recomputation unavailable; no new verdict",
                        ))
                    }
                },
            };
            let checked = boole_gate_supervisor::supervisor::checked_record(
                &rebuilt.input,
                &worker.descriptor,
                job,
                epoch,
                &result,
            )?;
            if checked != indexed.record {
                return Err(integrity(height,"archive-vstmt","public preimage result differs from the already applied verdict; no state changed"));
            }
            let _ = cache.save(&key, &rebuilt.input, epoch, &result);
            let input = &rebuilt.input;
            let statement = VerdictStatement {
                chain_id: input.chain_id,
                problem_id: input.problem_id,
                commit_hash: input.commit_hash,
                solver: input.solver,
                env_id: input.env_id,
                export_digest: input.export_digest,
                statement_id: result.statement_id,
                gate_rule_digest: worker.descriptor.digest(),
                kernel_set_digest: worker.descriptor.kernel_set_digest,
                outcome: checked.outcome,
                reason: checked.reason,
                ext: result.ext,
            };
            let bytes = statement.preimage(input.lane)?;
            ensure!(
                boole_gate_supervisor::codec::digest(&bytes) == indexed.record.vstmt_digest,
                "archive vstmt digest"
            );
            self.store
                .lock()
                .map_err(|_| anyhow::anyhow!("archive lock poisoned"))?
                .put_content("vstmt", indexed.record.vstmt_digest, &bytes)?;
        }
        Ok(())
    }
}
fn artifact(root: &Path, id: Hash) -> Result<Vec<u8>> {
    let bytes =
        boole_devnet_archive::journal::read_bound(&root.join(hex_string(&id)), RAW_CAP as usize)?.0;
    ensure!(
        boole_gate_supervisor::codec::digest(&bytes) == id,
        "archive public environment artifact pin"
    );
    Ok(bytes)
}
fn carried_package(
    state: &State,
    key: Key,
    history: &mut dyn boole_gate_supervisor::rebuild::History,
) -> Result<Vec<u8>> {
    let p = state
        .problems
        .get(&key.0)
        .context("archive original problem")?;
    let (_, logs) = history.origin(p.reveal_pos.0)?;
    let log = logs
        .iter()
        .find(|l| l.transaction_index == p.reveal_pos.1 && l.log_index == p.reveal_pos.2)
        .context("archive original reveal")?;
    let data = hex_bytes(&log.data, "reveal")?;
    let (kind, b) = boole_derivation::inbox::item(&data).map_err(anyhow::Error::msg)?;
    ensure!(kind == 9 && b.first() == Some(&2), "archive reveal version");
    let req = usize::from(*b.get(11).context("archive required count")?);
    let at = 12 + 32 * req;
    let depth = usize::from(*b.get(at + 37).context("archive path depth")?);
    let end = at + 38 + 32 * depth;
    let raw = b.get(end..end + 4).context("archive carried length")?;
    let size = u32::from_be_bytes(raw.try_into()?) as usize;
    let carried = b.get(end + 4..).context("archive carried package")?;
    ensure!(size == carried.len(), "archive carried framing");
    Ok(carried.to_vec())
}

fn public_cancel_hash(
    state: &State,
    key: Key,
    history: &mut dyn boole_gate_supervisor::rebuild::History,
) -> Result<Hash> {
    let open = state.candidates[&key]
        .live()
        .and_then(|l| l.open.as_ref())
        .context("archive public OPEN")?;
    let (_, logs) = history.origin(open.open_pos.0)?;
    let log = logs
        .iter()
        .find(|l| l.transaction_index == open.open_pos.1 && l.log_index == open.open_pos.2)
        .context("archive OPEN position")?;
    let data = hex_bytes(&log.data, "public OPEN")?;
    let (kind, body) = boole_derivation::inbox::item(&data).map_err(anyhow::Error::msg)?;
    ensure!(kind == 3, "archive OPEN item kind");
    Ok(body
        .get(21..53)
        .context("public cancellation hash")?
        .try_into()?)
}
