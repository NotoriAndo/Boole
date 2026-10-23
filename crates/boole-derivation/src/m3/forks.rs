use super::Error;
use crate::m2::{Config, Hash};

/// R-4 consensus kind bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum ForkKind {
    Genesis = 0,
    KernelFix = 1,
    Quarantine = 2,
    Policy = 3,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum Disposition {
    Recheck = 1,
    Void = 2,
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForkItem {
    pub commit_hash: Hash,
    pub disposition: Disposition,
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForkRow {
    pub epoch: u32,
    pub height: u64,
    pub gate_rule_digest: Hash,
    pub kind: ForkKind,
    pub list: Vec<ForkItem>,
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForkTable {
    pub rows: Vec<ForkRow>,
}
impl ForkRow {
    /// Hash only the canonical, strictly increasing list allowed for this kind.
    pub fn list_digest(&self) -> Result<Hash, Error> {
        use sha2::{Digest, Sha256};
        if (self.kind == ForkKind::Quarantine) == self.list.is_empty()
            || self
                .list
                .windows(2)
                .any(|w| w[0].commit_hash >= w[1].commit_hash)
        {
            return Err(Error::Format("noncanonical fork list"));
        }
        let count = u32::try_from(self.list.len()).map_err(|_| Error::Overflow)?;
        let mut digest = Sha256::new();
        digest.update(b"BOOLE-M3/FORK-LIST/v1");
        digest.update(count.to_be_bytes());
        for item in &self.list {
            digest.update(item.commit_hash);
            digest.update([item.disposition as u8]);
        }
        Ok(digest.finalize().into())
    }
    /// R-4's 69-byte body; E_h prefixes it with the 0x08 record tag.
    pub fn activation_record(&self) -> Result<[u8; 69], Error> {
        let mut record = [0; 69];
        record[..4].copy_from_slice(&self.epoch.to_be_bytes());
        record[4..36].copy_from_slice(&self.gate_rule_digest);
        record[36] = self.kind as u8;
        record[37..].copy_from_slice(&self.list_digest()?);
        Ok(record)
    }
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
                || row.list_digest().is_err()
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
    pub fn validate_history(&self, state: &crate::m2::State) -> Result<(), Error> {
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
    /// K2's state-derived set in original first-verdict queue order.
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
