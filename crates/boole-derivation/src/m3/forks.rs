use super::Error;
use crate::m2::{Config, Hash};

/// Logical kinds only. No unassigned event08 byte encoding is implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForkKind {
    Genesis,
    KernelFix,
    Quarantine,
    Policy,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Recheck,
    Void,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkItem {
    pub commit_hash: Hash,
    pub disposition: Disposition,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkRow {
    pub epoch: u32,
    pub height: u64,
    pub gate_rule_digest: Hash,
    pub kind: ForkKind,
    pub list: Vec<ForkItem>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkTable {
    pub rows: Vec<ForkRow>,
}
impl ForkTable {
    pub fn genesis(c: &Config) -> Result<Self, Error> {
        let p = c.m3.as_ref().ok_or(Error::ProfileRequired)?;
        Ok(Self {
            rows: vec![ForkRow {
                epoch: 0,
                height: 0,
                gate_rule_digest: p.genesis_gate_rule_digest,
                kind: ForkKind::Genesis,
                list: vec![],
            }],
        })
    }
    pub fn validate(&self, c: &Config) -> Result<(), u8> {
        let p = c.m3.as_ref().ok_or(14u8)?;
        let Some(first) = self.rows.first() else {
            return Err(14);
        };
        if first.epoch != 0
            || first.height != 0
            || first.kind != ForkKind::Genesis
            || first.gate_rule_digest != p.genesis_gate_rule_digest
        {
            return Err(14);
        }
        for (index, row) in self.rows.iter().enumerate() {
            if u32::try_from(index).ok() != Some(row.epoch)
                || index > 0
                    && (row.height <= self.rows[index - 1].height || row.kind == ForkKind::Genesis)
                || row.kind == ForkKind::Quarantine && row.list.is_empty()
            {
                return Err(14);
            }
        }
        Ok(())
    }
    pub fn epoch_at(&self, height: u64) -> Option<&ForkRow> {
        self.rows.iter().rev().find(|row| row.height <= height)
    }
    pub fn job_epoch(&self, avail: u64, k: u64) -> Result<u32, Error> {
        self.epoch_at(avail.checked_add(k).ok_or(Error::Overflow)?)
            .map(|row| row.epoch)
            .ok_or(Error::Format("missing epoch"))
    }
    pub(crate) fn validate_history(&self, state: &crate::m2::State) -> Result<(), Error> {
        let m = state.m3.as_ref().ok_or(Error::ProfileRequired)?;
        if state.height() == 0 && m.fork_history.is_empty() {
            return Ok(());
        }
        let past: Vec<_> = self
            .rows
            .iter()
            .take_while(|r| r.height <= state.height())
            .cloned()
            .collect();
        if past != m.fork_history {
            return Err(Error::PastForkChanged);
        }
        Ok(())
    }
    /// K2's logical set, independent of the unresolved activation record bytes.
    /// The fold validates the table separately before using an activation plan.
    pub fn recheck_set(
        &self,
        state: &crate::m2::State,
        height: u64,
    ) -> Result<Vec<crate::m2::Key>, Error> {
        use crate::m2::Status;
        let Some(row) = self
            .rows
            .iter()
            .find(|row| row.height == height && row.kind == ForkKind::KernelFix)
        else {
            return Ok(vec![]);
        };
        let m = state.m3.as_ref().ok_or(Error::ProfileRequired)?;
        let mut candidates = Vec::new();
        for (key, candidate) in &state.candidates {
            let status = candidate.status();
            if !matches!(status, Status::Accepted | Status::Passed | Status::Rejected) {
                continue;
            }
            let result = m
                .results
                .get(key)
                .ok_or(Error::Format("recheck candidate has no result"))?;
            if result.gate_epoch >= row.epoch
                || status == Status::Rejected && result.reason != super::KERNEL_FAULT
            {
                continue;
            }
            let index = candidate
                .live()
                .and_then(|l| l.open.as_ref())
                .and_then(|o| o.queue_index)
                .ok_or(Error::Format(
                    "recheck candidate has no original queue index",
                ))?;
            candidates.push((index, *key));
        }
        candidates.sort();
        Ok(candidates.into_iter().map(|(_, key)| key).collect())
    }
}
