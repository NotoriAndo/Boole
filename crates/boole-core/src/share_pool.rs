use crate::{calibration_policy, CalibrationPolicy, CalibrationReport};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_SEMANTIC_REJECT_TOMBSTONES: usize = 4096;

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
    StaleC,
    PkCapExceeded,
    GlobalCapExceeded,
}

impl SharePoolRejectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::StaleC => "stale_c",
            Self::PkCapExceeded => "pk_cap_exceeded",
            Self::GlobalCapExceeded => "global_cap_exceeded",
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
    global_share_cap: usize,
    by_key: BTreeMap<String, PoolShare>,
    insertion_order: Vec<String>,
    per_pk_per_c: BTreeMap<String, usize>,
    /// Capacity-free duplicate memory for shares removed while their semantic
    /// checker runs (and for semantic rejects). It is deliberately separate
    /// from `by_key`: invalid-but-structural shares must not fill the eligible
    /// global pool. Tombstones are bounded and scoped to `current_c`.
    semantic_reject_tombstones: BTreeSet<String>,
    semantic_reject_order: VecDeque<String>,
    semantic_reject_tombstone_cap: usize,
}

impl SharePool {
    pub fn new(share_cap_per_pk_block: usize) -> Self {
        Self::new_with_global_cap(share_cap_per_pk_block, usize::MAX)
    }

    pub fn new_with_global_cap(share_cap_per_pk_block: usize, global_share_cap: usize) -> Self {
        Self {
            current_c: None,
            share_cap_per_pk_block,
            global_share_cap,
            by_key: BTreeMap::new(),
            insertion_order: Vec::new(),
            per_pk_per_c: BTreeMap::new(),
            semantic_reject_tombstones: BTreeSet::new(),
            semantic_reject_order: VecDeque::new(),
            semantic_reject_tombstone_cap: global_share_cap
                .clamp(1, MAX_SEMANTIC_REJECT_TOMBSTONES),
        }
    }

    pub fn from_policy(policy: &CalibrationPolicy) -> Self {
        Self::new_with_global_cap(policy.share_cap_per_pk_block, policy.global_share_cap)
    }

    pub fn from_calibration_report(report: &CalibrationReport) -> Result<Self, String> {
        Ok(Self::from_policy(&calibration_policy(report)?))
    }

    pub fn set_current_c(&mut self, c: impl Into<String>) {
        let c = c.into();
        if self.current_c.as_deref() != Some(&c) {
            self.current_c = Some(c);
            self.semantic_reject_tombstones.clear();
            self.semantic_reject_order.clear();
        }
    }

    pub fn current_c(&self) -> Option<&str> {
        self.current_c.as_deref()
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
        if self.by_key.contains_key(&key) || self.semantic_reject_tombstones.contains(&key) {
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
        if self.by_key.len() >= self.global_share_cap {
            return AcceptResult::Err {
                reason: SharePoolRejectReason::GlobalCapExceeded,
            };
        }
        self.by_key.insert(key.clone(), share);
        self.insertion_order.push(key);
        self.per_pk_per_c.insert(cap_key, used + 1);
        AcceptResult::Ok
    }

    /// Move one active, structurally admitted share into a capacity-free
    /// semantic reservation. The exact `(pk,n,j)` remains a duplicate for the
    /// current head, but no longer consumes global or per-pk eligible capacity.
    pub fn reserve_for_semantic_check(&mut self, share: &PoolShare) -> bool {
        let key = share_key(share);
        let Some(removed) = self.remove_active(&key) else {
            return false;
        };
        self.insert_semantic_tombstone(key);
        debug_assert_eq!(removed.pk, share.pk);
        debug_assert_eq!(removed.c, share.c);
        true
    }

    /// Promote one successfully verified reservation back into the eligible
    /// pool. Failure leaves the tombstone in place and therefore fails closed.
    pub fn restore_after_semantic_check(&mut self, share: PoolShare) -> bool {
        let key = share_key(&share);
        if !self.semantic_reject_tombstones.contains(&key)
            || self.current_c.as_deref().is_some_and(|c| c != share.c)
            || self.by_key.contains_key(&key)
        {
            return false;
        }
        let cap_key = per_pk_key(&share.pk, &share.c);
        let used = self.per_pk_per_c.get(&cap_key).copied().unwrap_or(0);
        if used >= self.share_cap_per_pk_block || self.by_key.len() >= self.global_share_cap {
            return false;
        }

        self.semantic_reject_tombstones.remove(&key);
        self.semantic_reject_order.retain(|entry| entry != &key);
        self.by_key.insert(key.clone(), share);
        self.insertion_order.push(key);
        self.per_pk_per_c.insert(cap_key, used + 1);
        true
    }

    /// Release an in-flight semantic reservation after the verifier could not
    /// reach a verdict. The share stays ineligible, but its capacity-free
    /// duplicate tombstone is removed so the exact request can be retried.
    /// Deterministic rejects deliberately do not call this method.
    pub fn release_semantic_reservation(&mut self, share: &PoolShare) -> bool {
        let key = share_key(share);
        let removed = self.semantic_reject_tombstones.remove(&key);
        if removed {
            self.semantic_reject_order.retain(|entry| entry != &key);
        }
        removed
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
        self.semantic_reject_tombstones.clear();
        self.semantic_reject_order.clear();
        dropped
    }

    fn remove_active(&mut self, key: &str) -> Option<PoolShare> {
        let share = self.by_key.remove(key)?;
        self.insertion_order.retain(|entry| entry != key);
        let cap_key = per_pk_key(&share.pk, &share.c);
        if let Some(used) = self.per_pk_per_c.get_mut(&cap_key) {
            *used = used.saturating_sub(1);
            if *used == 0 {
                self.per_pk_per_c.remove(&cap_key);
            }
        }
        Some(share)
    }

    fn insert_semantic_tombstone(&mut self, key: String) {
        if self.semantic_reject_tombstones.insert(key.clone()) {
            self.semantic_reject_order.push_back(key);
        }
        while self.semantic_reject_order.len() > self.semantic_reject_tombstone_cap {
            if let Some(oldest) = self.semantic_reject_order.pop_front() {
                self.semantic_reject_tombstones.remove(&oldest);
            }
        }
    }
}

fn share_key(s: &PoolShare) -> String {
    format!("{}|{}|{}", s.pk, s.n, s.j)
}

fn per_pk_key(pk: &str, c: &str) -> String {
    format!("{}|{}", pk, c)
}
