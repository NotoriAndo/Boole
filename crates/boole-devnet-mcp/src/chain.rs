//! Canonical M3 inputs and fuel lookahead. Cached RPC bytes never replace a fold.
use crate::client::{fixed, CanonicalBlock, L2Source};
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::hex_bytes,
    m2::{Config, Input, State, Step},
    m3::{self, ForkTable, IndexedVerdict},
    L1Block,
};
use boole_devnet_driver::{engine::retry_rpc, l1::L1Source};
use std::collections::BTreeMap;

pub(crate) struct Entry {
    pub block: CanonicalBlock,
    pub verdicts: Vec<IndexedVerdict>,
    pub l1: Option<L1Block>,
    pub header: Option<boole_derivation::ParentHeader>,
}
pub(crate) struct Chain {
    pub forks: ForkTable,
    pub blocks: BTreeMap<u64, Entry>,
    pub records: BTreeMap<u64, (IndexedVerdict, u64)>,
}
impl Chain {
    pub fn new(c: &Config, forks: Option<ForkTable>) -> Result<Self> {
        let forks = forks.map(Ok).unwrap_or_else(|| ForkTable::genesis(c))?;
        forks
            .validate(c)
            .map_err(|n| anyhow::anyhow!("S{n}: client fork table"))?;
        Ok(Self {
            forks,
            blocks: BTreeMap::new(),
            records: BTreeMap::new(),
        })
    }
    /// Read only finalized L1 heights. Later canonical verdicts provide fuel to
    /// cross an earlier avail+K; application still happens solely at its due.
    pub fn refresh(
        &mut self,
        c: &Config,
        l1: &mut impl L1Source,
        node: &mut dyn L2Source,
        finalized: u64,
        pins: Option<&boole_devnet_archive::pins::ReadOnlyPins>,
    ) -> Result<bool> {
        if self.blocks.is_empty() {
            let Some(genesis) = read_block(node, pins, 0)? else {
                return Ok(false);
            };
            ensure!(genesis.number == 0, "L2 genesis height");
            self.blocks.insert(
                0,
                Entry {
                    block: genesis,
                    verdicts: vec![],
                    l1: None,
                    header: read_header(node, pins, 0)?,
                },
            );
        }
        let tip = *self.blocks.last_key_value().context("L2 genesis")?.0;
        for h in tip.checked_add(1).context("L2 height overflow")?..=finalized {
            let Some(block) = read_block(node, pins, h)? else {
                break;
            };
            ensure!(
                block.number == h && block.parent_hash == self.blocks[&(h - 1)].block.hash,
                "canonical L2 parent link"
            );
            let source = retry_rpc(|| l1.block(h))?;
            ensure!(source.number == h, "L1 lookahead height");
            ensure!(
                hex_bytes(
                    block.transactions.first().context("missing L1-info")?,
                    "L1-info"
                )? == hex_bytes(&boole_derivation::system::l1_info(&source)?, "L1-info")?,
                "canonical L1-info differs from finalized L1"
            );
            boole_derivation::system::read_m2_root(
                block.transactions.get(1).context("missing M2ROOT")?,
                h,
                &source.hash,
            )?;
            let verdicts = decode_prefix(c, &source, &block.transactions)?;
            for record in &verdicts {
                ensure!(
                    !self.records.contains_key(&record.queue_index),
                    "duplicate canonical queue index"
                );
            }
            for record in &verdicts {
                self.records.insert(record.queue_index, (record.clone(), h));
            }
            self.blocks.insert(
                h,
                Entry {
                    block,
                    verdicts,
                    l1: Some(source),
                    header: read_header(node, pins, h)?,
                },
            );
        }
        let (&height, entry) = self.blocks.last_key_value().context("canonical tip")?;
        let Some(actual) = retry_rpc(|| node.block(height))? else {
            return Ok(false);
        };
        ensure!(
            actual.hash == entry.block.hash && actual.transactions == entry.block.transactions,
            "canonical L2 anchor changed"
        );
        if let Some(previous) = &entry.l1 {
            ensure!(
                fixed::<32>(&retry_rpc(|| l1.block(height))?.hash)? == fixed::<32>(&previous.hash)?,
                "lookahead finalized L1 anchor changed"
            );
        }
        Ok(true)
    }
    pub fn inputs(&self, s: &State, c: &Config, h: u64) -> Result<Vec<IndexedVerdict>> {
        let m = s.m3.as_ref().context("M3 state")?;
        let jobs = boole_gate_supervisor::supervisor::jobs_at_height(s, h, &self.forks)?;
        let mut out = vec![];
        for (job, _) in jobs.iter().skip(m.next_applied) {
            let Some((record, source_height)) = self.records.get(&job.queue_index) else {
                break;
            };
            let epoch = job.epoch_override.map(Ok).unwrap_or_else(|| {
                self.forks
                    .job_epoch(job.avail, c.m3.as_ref().expect("M3").k)
            })?;
            if record.record.gate_epoch != epoch {
                ensure!(*source_height > h, "canonical verdict has the wrong epoch");
                break;
            }
            out.push(record.clone());
        }
        Ok(out)
    }
    pub fn step(
        &self,
        s: &State,
        c: &Config,
        l1: &L1Block,
        logs: &[Input],
    ) -> Result<Option<Step>> {
        let Some(entry) = self.blocks.get(&l1.number) else {
            return Ok(None);
        };
        let records = self.inputs(s, c, l1.number)?;
        let step = match s.step_with_verdicts(c, l1, logs, &records, &self.forks) {
            Ok(step) => step,
            Err(boole_derivation::m2::FoldError::M3(m3::Error::Pending { .. })) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        ensure!(
            step.verdicts == entry.verdicts,
            "canonical verdict due height mismatch"
        );
        let mut expected = vec![
            boole_derivation::system::l1_info(l1)?,
            boole_derivation::system::m2_root_deposit(l1, step.state.root)?,
        ];
        expected.extend(m3::verdict_deposits(c, l1, &step.verdicts)?);
        for payment in &step.payments {
            let at = expected.len();
            m3::check_reward_deposit(
                c,
                l1,
                payment,
                entry.block.transactions.get(at).context("missing REWARD")?,
            )?;
            expected.push(m3::reward_deposit(c, l1, payment)?);
        }
        m3::check_system_prefix(&entry.block.transactions, &expected)?;
        Ok(Some(step))
    }
}

fn integer(bytes: &[u8]) -> Result<u128> {
    ensure!(
        bytes.len() <= 16 && bytes.first() != Some(&0),
        "noncanonical deposit integer"
    );
    Ok(bytes.iter().fold(0u128, |n, b| (n << 8) | u128::from(*b)))
}
fn decode_prefix(c: &Config, l1: &L1Block, txs: &[String]) -> Result<Vec<IndexedVerdict>> {
    let p = c.m3.as_ref().context("M3 profile")?;
    let mut verdicts = vec![];
    let mut reward = false;
    let mut users = false;
    let mut rewards = 0u64;
    for tx in txs.iter().skip(2) {
        let raw = hex_bytes(tx, "canonical transaction")?;
        if raw.first() != Some(&0x7e) {
            users = true;
            continue;
        }
        ensure!(!users, "system deposit after user transaction");
        let fields = boole_derivation::rlp::decode(&raw[1..])
            .map_err(anyhow::Error::msg)?
            .children
            .context("deposit list")?;
        ensure!(
            fields.len() == 8 && fields.iter().all(|f| f.children.is_none()),
            "deposit fields"
        );
        if fields[2].bytes == p.verdict_sink || fields[7].bytes.starts_with(b"m3vd") {
            ensure!(!reward, "VERDICT after REWARD");
            verdicts.push(tx.clone());
        } else {
            reward = true;
            rewards = rewards.checked_add(1).context("reward count")?;
            ensure!(rewards <= p.q_reward, "REWARD count exceeds cap");
            ensure!(
                fields[0].bytes.len() == 32
                    && fields[1].bytes
                        == hex_bytes(boole_derivation::config::DEPOSITOR, "depositor")?
                    && fields[2].bytes.len() == 20
                    && fields[6].bytes.is_empty()
                    && fields[7].bytes.is_empty(),
                "unknown or malformed system deposit"
            );
            let mint = integer(fields[3].bytes)?;
            ensure!(
                mint > 0
                    && mint == integer(fields[4].bytes)?
                    && integer(fields[5].bytes)? == u128::from(p.reward_gas),
                "malformed REWARD envelope"
            );
        }
    }
    Ok(m3::decode_verdict_deposits(c, l1, &verdicts)?)
}

fn read_block(
    node: &mut dyn L2Source,
    pins: Option<&boole_devnet_archive::pins::ReadOnlyPins>,
    h: u64,
) -> Result<Option<CanonicalBlock>> {
    if let Some(view) = pins.filter(|p| p.has_selected(h)) {
        let b = view.binding(h)?;
        return Ok(Some(CanonicalBlock {
            number: h,
            hash: fixed(&b.header.hash)?,
            parent_hash: b
                .header
                .parent_hash
                .as_deref()
                .map(fixed)
                .transpose()?
                .unwrap_or([0; 32]),
            transactions: b.transactions.clone(),
        }));
    }
    retry_rpc(|| node.block(h))
}
fn read_header(
    node: &mut dyn L2Source,
    pins: Option<&boole_devnet_archive::pins::ReadOnlyPins>,
    h: u64,
) -> Result<Option<boole_derivation::ParentHeader>> {
    let Some(view) = pins else { return Ok(None) };
    if view.has_selected(h) {
        return Ok(Some(view.binding(h)?.header.clone()));
    }
    retry_rpc(|| node.header(h))?.map(Some).ok_or_else(|| {
        boole_devnet_archive::error::availability(
            h,
            "pin-header",
            "S20: canonical L2 header is unavailable",
        )
    })
}
