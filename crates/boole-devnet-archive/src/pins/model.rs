use anyhow::{ensure, Context, Result};
use boole_derivation::{
    m2::{env_id, Config, Hash, Key, State, Status},
    m3::{ForkKind, ForkTable, RecordKind, KERNEL_FAULT},
    ParentHeader,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

/// Closed, node-local reasons. Reasons are accumulated, never unpinned.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Accepted,
    Stale,
    KernelFaultReject,
    Quarantine,
    Checking,
    Passed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidatePin {
    pub key: Key,
    pub reasons: BTreeSet<Reason>,
    pub commit_height: u64,
    pub open_height: u64,
    pub completion_height: u64,
    pub reveal_height: u64,
    pub gates: BTreeSet<(u32, Hash)>,
    pub components: BTreeSet<Hash>,
}
impl CandidatePin {
    pub fn intervals(&self) -> IntervalUnion {
        let mut out = IntervalUnion::default();
        out.insert(self.commit_height, self.commit_height);
        out.insert(self.open_height, self.completion_height);
        out.insert(self.reveal_height, self.reveal_height);
        out
    }
    pub fn validate(&self, through: u64) -> Result<()> {
        ensure!(
            !self.reasons.is_empty()
                && self.commit_height > 0
                && self.open_height >= self.commit_height
                && self.completion_height >= self.open_height
                && self.completion_height <= through
                && self.reveal_height > 0
                && self.reveal_height <= through,
            "pin candidate heights/reasons"
        );
        ensure!(
            !self.gates.is_empty() && !self.components.is_empty(),
            "pin gate/environment requirements"
        );
        Ok(())
    }
    pub fn covers(&self, required: &Self) -> bool {
        self.key == required.key
            && self.commit_height == required.commit_height
            && self.open_height == required.open_height
            && self.completion_height == required.completion_height
            && self.reveal_height == required.reveal_height
            && self.reasons.is_superset(&required.reasons)
            && self.gates.is_superset(&required.gates)
            && self.components.is_superset(&required.components)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IntervalUnion(pub BTreeMap<u64, u64>);
impl IntervalUnion {
    pub fn insert(&mut self, mut lo: u64, mut hi: u64) {
        if lo > hi {
            return;
        }
        if let Some((&a, &b)) = self.0.range(..=lo).next_back() {
            if b.saturating_add(1) >= lo {
                lo = a;
                hi = hi.max(b);
                self.0.remove(&a);
            }
        }
        let merged: Vec<_> = self
            .0
            .range(lo..)
            .take_while(|(a, _)| **a <= hi.saturating_add(1))
            .map(|(a, b)| (*a, *b))
            .collect();
        // Extend iteratively: each adjacent interval can extend the merge frontier.
        for (a, b) in merged {
            self.0.remove(&a);
            hi = hi.max(b);
        }
        loop {
            let next = self.0.range(lo..).next().map(|(a, b)| (*a, *b));
            match next {
                Some((a, b)) if a <= hi.saturating_add(1) => {
                    self.0.remove(&a);
                    hi = hi.max(b);
                }
                _ => break,
            }
        }
        self.0.insert(lo, hi);
    }
    pub fn contains(&self, h: u64) -> bool {
        self.0
            .range(..=h)
            .next_back()
            .is_some_and(|(_, end)| *end >= h)
    }
    pub fn extend(&mut self, other: &Self) {
        for (&lo, &hi) in &other.0 {
            self.insert(lo, hi);
        }
    }
}

pub fn requirements(
    s: &State,
    c: &Config,
    forks: &ForkTable,
    live: bool,
) -> Result<Vec<CandidatePin>> {
    let Some(m) = &s.m3 else { return Ok(vec![]) };
    let mut out = vec![];
    for (key, candidate) in &s.candidates {
        let mut reasons = BTreeSet::new();
        match candidate.status() {
            Status::Accepted => {
                reasons.insert(Reason::Accepted);
            }
            Status::Stale => {
                reasons.insert(Reason::Stale);
            }
            Status::Rejected if m.results.get(key).is_some_and(|r| r.reason == KERNEL_FAULT) => {
                reasons.insert(Reason::KernelFaultReject);
            }
            Status::Checking if live => {
                reasons.insert(Reason::Checking);
            }
            Status::Passed if live => {
                reasons.insert(Reason::Passed);
            }
            _ => {}
        }
        if m.queue.iter().any(|job| {
            job.key == *key
                && job.kind == RecordKind::Verdict
                && forks.rows.iter().any(|row| {
                    job.epoch_override == Some(row.epoch)
                        && row.kind == ForkKind::Quarantine
                        && row.list.iter().any(|item| item.commit_hash == key.1)
                })
        }) {
            reasons.insert(Reason::Quarantine);
        }
        if reasons.is_empty() {
            continue;
        }
        let candidate = candidate.live().context("K4 retained candidate metadata")?;
        let open = candidate.open.as_ref().context("K4 retained OPEN")?;
        let p = s.problems.get(&key.0).context("K4 problem")?;
        let pkgs = c
            .env_allowed
            .iter()
            .find(|pkgs| env_id(c.schema_id, c.base_root, pkgs) == open.env_id)
            .context("K4 environment outside Config")?;
        let mut components: BTreeSet<_> = pkgs.iter().copied().collect();
        components.insert(c.base_root);
        let mut gates = BTreeSet::new();
        // Retain the release active when transport completed as well as the
        // table-selected due/recheck release. A future activation may fall
        // exactly on avail+K; retaining the earlier release is conservative.
        let completed = open.h_r.context("K4 completion")?;
        let original = forks.epoch_at(completed).context("K4 completion epoch")?;
        gates.insert((original.epoch, original.gate_rule_digest));

        for job in m.queue.iter().filter(|j| j.key == *key) {
            let original = forks.job_epoch(job.avail, c.m3.as_ref().expect("profile").k)?;
            if job.kind == RecordKind::Verdict {
                let row = forks
                    .rows
                    .get(original as usize)
                    .context("original K4 epoch")?;
                gates.insert((original, row.gate_rule_digest));
            }
            let epoch = job.epoch_override.unwrap_or(original);
            let row = forks.rows.get(epoch as usize).context("K4 gate epoch")?;
            gates.insert((epoch, row.gate_rule_digest));
        }
        if let Some(r) = m.results.get(key) {
            let row = forks
                .rows
                .get(r.gate_epoch as usize)
                .context("K4 result gate")?;
            gates.insert((r.gate_epoch, row.gate_rule_digest));
        }
        let pin = CandidatePin {
            key: *key,
            reasons,
            commit_height: candidate.h_c,
            open_height: open.h_o,
            completion_height: open.h_r.context("K4 completion")?,
            reveal_height: p.reveal_pos.0,
            gates,
            components,
        };
        pin.validate(s.height())?;
        out.push(pin);
    }
    Ok(out)
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ArtifactRole {
    StubDescriptor {
        epoch: u32,
        gate: Hash,
    },
    StubBinary {
        epoch: u32,
        gate: Hash,
        profile: String,
    },
    Environment {
        component: Hash,
    },
}
#[derive(Clone, Debug)]
pub struct ArtifactSource {
    pub role: ArtifactRole,
    pub digest: Hash,
    pub path: PathBuf,
    pub executable: bool,
}
/// Real gates require the complete transitive release inventory from M3d-c.
/// A stub registry or environment table cannot synthesize this contract.
#[derive(Clone, Debug)]
pub struct RealGateInventoryPending {
    pub epoch: u32,
    pub gate: Hash,
}
impl RealGateInventoryPending {
    pub fn require_inventory(&self) -> Result<()> {
        Err(crate::error::availability(
            0,
            "real-gate-pin-inventory",
            "S20: complete transitive real-gate inventory is not released",
        ))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderBinding {
    pub header: ParentHeader,
    pub transactions: Vec<String>,
    pub root: Hash,
}
impl HeaderBinding {
    pub fn verify(
        &self,
        network: &crate::journal::NetworkIdentity,
        l1: &boole_derivation::L1Block,
    ) -> Result<()> {
        ensure!(self.header.number == l1.number, "pin L2/L1 height");
        if self.header.number == 0 {
            ensure!(
                self.header.hash == network.l2_genesis_hash,
                "pin L2 genesis"
            );
            return Ok(());
        }
        ensure!(self.header.parent_hash.is_some(), "pin L2 parent");
        ensure!(
            self.header.l1_info.as_ref() == self.transactions.first()
                && self.header.m2_root_deposit.as_ref() == self.transactions.get(1),
            "pin system header binding"
        );
        let origin = boole_derivation::system::l1_origin(
            self.header.l1_info.as_deref().context("pin L1-info")?,
            self.header.number,
        )?;
        ensure!(origin.eq_ignore_ascii_case(&l1.hash), "pin L2 origin");
        boole_derivation::system::check_m2_root(
            self.header.m2_root_deposit.as_deref(),
            self.header.number,
            &l1.hash,
            self.root,
        )?;
        Ok(())
    }
}
