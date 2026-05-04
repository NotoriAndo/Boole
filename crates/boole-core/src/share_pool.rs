use crate::{calibration_policy, CalibrationPolicy, CalibrationReport};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolShare {
    pub label: String,
    pub pk: String,
    pub n: String,
    pub j: String,
    pub c: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharePoolRejectReason {
    Duplicate,
    PkCapExceeded,
    StaleC,
}

impl SharePoolRejectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::PkCapExceeded => "pk_cap_exceeded",
            Self::StaleC => "stale_c",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptResult {
    Ok,
    Err { reason: SharePoolRejectReason },
}

impl AcceptResult {
    pub fn ok(&self) -> bool {
        matches!(self, Self::Ok)
    }

    pub fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Ok => None,
            Self::Err { reason } => Some(reason.as_str()),
        }
    }

    pub fn reason_typed(&self) -> Option<SharePoolRejectReason> {
        match self {
            Self::Ok => None,
            Self::Err { reason } => Some(*reason),
        }
    }
}

#[derive(Debug)]
pub struct SharePool {
    current_c: Option<String>,
    share_cap_per_pk_block: usize,
    by_key: BTreeMap<String, PoolShare>,
    insertion_order: Vec<String>,
    per_pk_per_c: BTreeMap<String, usize>,
}

impl SharePool {
    pub fn new(share_cap_per_pk_block: usize) -> Self {
        Self {
            current_c: None,
            share_cap_per_pk_block,
            by_key: BTreeMap::new(),
            insertion_order: Vec::new(),
            per_pk_per_c: BTreeMap::new(),
        }
    }

    pub fn from_policy(policy: &CalibrationPolicy) -> Self {
        Self::new(policy.share_cap_per_pk_block)
    }

    pub fn from_calibration_report(report: &CalibrationReport) -> Result<Self, String> {
        Ok(Self::from_policy(&calibration_policy(report)?))
    }

    pub fn set_current_c(&mut self, c: impl Into<String>) {
        let c = c.into();
        if self.current_c.as_deref() != Some(&c) {
            self.current_c = Some(c);
        }
    }

    pub fn accept(&mut self, share: PoolShare) -> AcceptResult {
        if let Some(current_c) = &self.current_c {
            if &share.c != current_c {
                return AcceptResult::Err {
                    reason: SharePoolRejectReason::StaleC,
                };
            }
        }
        let key = share_key(&share);
        if self.by_key.contains_key(&key) {
            return AcceptResult::Err {
                reason: SharePoolRejectReason::Duplicate,
            };
        }
        let cap_key = per_pk_key(&share.pk, &share.c);
        let used = self.per_pk_per_c.get(&cap_key).copied().unwrap_or(0);
        if used >= self.share_cap_per_pk_block {
            return AcceptResult::Err {
                reason: SharePoolRejectReason::PkCapExceeded,
            };
        }
        self.by_key.insert(key.clone(), share);
        self.insertion_order.push(key);
        self.per_pk_per_c.insert(cap_key, used + 1);
        AcceptResult::Ok
    }

    pub fn size(&self) -> usize {
        self.by_key.len()
    }

    pub fn for_chain(&self, c: &str) -> Vec<&PoolShare> {
        self.insertion_order
            .iter()
            .filter_map(|key| self.by_key.get(key))
            .filter(|share| share.c == c)
            .collect()
    }

    pub fn prune_to_height(&mut self, c: impl Into<String>) -> usize {
        let c = c.into();
        let mut dropped = 0usize;
        let keys = self.insertion_order.clone();
        for key in keys {
            let should_drop = self
                .by_key
                .get(&key)
                .map(|share| share.c != c)
                .unwrap_or(false);
            if should_drop {
                self.by_key.remove(&key);
                dropped += 1;
            }
        }
        self.insertion_order
            .retain(|key| self.by_key.contains_key(key));
        let suffix = format!("|{}", c);
        self.per_pk_per_c.retain(|key, _| key.ends_with(&suffix));
        self.current_c = Some(c);
        dropped
    }
}

fn share_key(s: &PoolShare) -> String {
    format!("{}|{}|{}", s.pk, s.n, s.j)
}

fn per_pk_key(pk: &str, c: &str) -> String {
    format!("{}|{}", pk, c)
}
