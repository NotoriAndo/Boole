//! Per-family side-pool for bounty proofs that have been verified
//! (`accepted = true`) but whose `FamilyManifest` is not yet promoted —
//! i.e. signed and with `activation_height` ≤ runtime height.
//!
//! The side-pool is intentionally **structurally separate** from
//! `SharePool`: it has no `accept` / `for_chain` / `prune_to_height`
//! methods, and `build_block_selection` does not consume from it.
//! That isolation is what makes the Hard Guard hold in S21 — bounty
//! traffic can flow through the verifier and audit log without ever
//! touching base-lane economics.
//!
//! Activation gating + caps + reward credit ride on top of this in
//! S22/S23.

use std::collections::HashMap;

use crate::block_builder::PromotedBountyShare;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BountyShare {
    pub bounty_id: String,
    pub proof_hash: String,
    pub prover: String,
    pub family_id: String,
    pub ts: u64,
    /// Reward stamped from the matching `Bounty.reward` at side-pool
    /// insert time. The promotion gate (`select_promoted_bounty_selection`)
    /// caps a share's credit at `min(reward, family_budget_left)`.
    pub reward: u128,
}

#[derive(Debug, Default)]
pub struct BountySidePool {
    by_family: HashMap<String, Vec<BountyShare>>,
}

impl BountySidePool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a verified bounty share to its family's bucket. We do not
    /// dedup here — `bounty_proof_json` already short-circuits on
    /// `BountyRegistry::has_proof`, so the side-pool only sees first-
    /// time accepted hashes.
    pub fn insert(&mut self, share: BountyShare) {
        self.by_family
            .entry(share.family_id.clone())
            .or_default()
            .push(share);
    }

    pub fn shares_for_family(&self, family_id: &str) -> &[BountyShare] {
        self.by_family
            .get(family_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn family_count(&self) -> usize {
        self.by_family.len()
    }

    pub fn total_share_count(&self) -> usize {
        self.by_family.values().map(Vec::len).sum()
    }

    /// P1.5a — drop shares that have been promoted into a committed
    /// block. Matching is by `(family_id, bounty_id, proof_hash)`, the
    /// same identity carried into `PromotedBountyShare`. Returns the
    /// number of shares actually removed; callers can use the count to
    /// log / metric the drop and to assert against the size of the
    /// promotion batch. Families whose bucket empties are dropped from
    /// the map so `family_count()` reflects the live set.
    pub fn remove_promoted(&mut self, promoted: &[PromotedBountyShare]) -> usize {
        let mut removed = 0usize;
        for entry in promoted {
            let Some(bucket) = self.by_family.get_mut(&entry.family_id) else {
                continue;
            };
            let before = bucket.len();
            bucket.retain(|share| {
                !(share.bounty_id == entry.bounty_id && share.proof_hash == entry.proof_hash)
            });
            removed += before - bucket.len();
            if bucket.is_empty() {
                self.by_family.remove(&entry.family_id);
            }
        }
        removed
    }
}
