//! S21 — `BountySidePool` is a per-family side-pool that holds bounty
//! proofs whose `FamilyManifest`s have not yet been promoted (i.e.
//! `activation_height` not yet reached). It is **structurally separate**
//! from `SharePool` so the Hard Guard cannot leak: the base block
//! selection consumes only `SharePool` candidates, never the side-pool.
//!
//! These tests pin the data-plane invariants. The actual gating into
//! block selection is a S22 concern; the replay-divergence sweep is S23.

use boole_core::{BountyShare, BountySidePool};

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
