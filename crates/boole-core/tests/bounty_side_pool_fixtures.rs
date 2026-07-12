//! S21 — `BountySidePool` is a per-family side-pool that holds bounty
//! proofs whose `FamilyManifest`s have not yet been promoted (i.e.
//! `activation_height` not yet reached). It is **structurally separate**
//! from `SharePool` so the Hard Guard cannot leak: the base block
//! selection consumes only `SharePool` candidates, never the side-pool.
//!
//! These tests pin the data-plane invariants. The actual gating into
//! block selection is a S22 concern; the replay-divergence sweep is S23.

use boole_core::{BountyShare, BountySidePool, PromotedBountyShare};

fn share(family_id: &str, proof_hash: &str) -> BountyShare {
    BountyShare {
        bounty_id: format!("bnty-{proof_hash}"),
        proof_hash: proof_hash.to_string(),
        prover: "1111111111111111111111111111111111111111111111111111111111111111".to_string(),
        family_id: family_id.to_string(),
        ts: 1_700_000_000_000,
        reward: 0,
    }
}

fn promoted_of(s: &BountyShare) -> PromotedBountyShare {
    PromotedBountyShare {
        family_id: s.family_id.clone(),
        bounty_id: s.bounty_id.clone(),
        proof_hash: s.proof_hash.clone(),
        prover: s.prover.clone(),
        reward: s.reward.to_string(),
    }
}

#[test]
fn side_pool_starts_empty() {
    let pool = BountySidePool::new();
    assert_eq!(pool.total_share_count(), 0);
    assert_eq!(pool.family_count(), 0);
    assert!(pool.shares_for_family("anything").is_empty());
}

#[test]
fn insert_groups_by_family() {
    let mut pool = BountySidePool::new();
    pool.insert(share("alpha", "aa"));
    pool.insert(share("alpha", "bb"));
    pool.insert(share("beta", "cc"));
    assert_eq!(pool.total_share_count(), 3);
    assert_eq!(pool.family_count(), 2);
    assert_eq!(pool.shares_for_family("alpha").len(), 2);
    assert_eq!(pool.shares_for_family("beta").len(), 1);
}

#[test]
fn shares_for_unknown_family_is_empty_slice() {
    let mut pool = BountySidePool::new();
    pool.insert(share("alpha", "aa"));
    assert!(pool.shares_for_family("nonexistent").is_empty());
    assert_eq!(pool.total_share_count(), 1);
}

// P1.5a — `remove_promoted` is the post-commit hook that lets a node
// drop shares already credited into a committed block. Without it the
// side-pool grows unbounded and the same share is re-promoted into
// every subsequent block, double-crediting the prover.

#[test]
fn remove_promoted_drops_matching_share_only() {
    let mut pool = BountySidePool::new();
    let keep = share("alpha", "aa");
    let drop = share("alpha", "bb");
    pool.insert(keep.clone());
    pool.insert(drop.clone());

    let removed = pool.remove_promoted(&[promoted_of(&drop)]);

    assert_eq!(removed, 1);
    assert_eq!(pool.total_share_count(), 1);
    let remaining = pool.shares_for_family("alpha");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].proof_hash, "aa");
}

#[test]
fn remove_promoted_is_no_op_for_unknown_share() {
    let mut pool = BountySidePool::new();
    let inserted = share("alpha", "aa");
    pool.insert(inserted.clone());

    let unknown = PromotedBountyShare {
        family_id: "alpha".to_string(),
        bounty_id: "bnty-zz".to_string(),
        proof_hash: "zz".to_string(),
        prover: inserted.prover.clone(),
        reward: "0".to_string(),
    };
    let removed = pool.remove_promoted(&[unknown]);

    assert_eq!(removed, 0);
    assert_eq!(pool.total_share_count(), 1);
}

#[test]
fn remove_promoted_clears_family_bucket_when_emptied() {
    let mut pool = BountySidePool::new();
    let only = share("alpha", "aa");
    pool.insert(only.clone());
    assert_eq!(pool.family_count(), 1);

    let removed = pool.remove_promoted(&[promoted_of(&only)]);

    assert_eq!(removed, 1);
    assert_eq!(pool.total_share_count(), 0);
    assert_eq!(
        pool.family_count(),
        0,
        "emptied family bucket must be removed so subsequent inserts can re-create it cleanly"
    );
}

#[test]
fn remove_promoted_handles_cross_family_batch() {
    let mut pool = BountySidePool::new();
    let alpha_share = share("alpha", "aa");
    let beta_share = share("beta", "bb");
    let beta_keep = share("beta", "cc");
    pool.insert(alpha_share.clone());
    pool.insert(beta_share.clone());
    pool.insert(beta_keep.clone());

    let removed = pool.remove_promoted(&[promoted_of(&alpha_share), promoted_of(&beta_share)]);

    assert_eq!(removed, 2);
    assert_eq!(pool.total_share_count(), 1);
    assert!(pool.shares_for_family("alpha").is_empty());
    let beta = pool.shares_for_family("beta");
    assert_eq!(beta.len(), 1);
    assert_eq!(beta[0].proof_hash, "cc");
}
