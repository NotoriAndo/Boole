//! S22b — activation-gated promotion of side-pool shares into the
//! `build_block_selection` `promoted_bounty_shares` argument.
//!
//! `select_promoted_bounty_shares` walks the registry, applies all four
//! gates, and emits a `Vec<PromotedBountyShare>` ready to hand to
//! `build_block_selection`. The block builder treats the slice as fully
//! vetted — every gate change lands here, not in the builder.
//!
//! Gate ordering matters: signature verification is the most expensive
//! step (ed25519 verify per (manifest, operator_pk) pair), so cheaper
//! gates short-circuit first.

use crate::block_builder::{PromotedBountyCredit, PromotedBountySelection, PromotedBountyShare};
use crate::bounty_side_pool::BountySidePool;
use crate::family_manifest::verify_family_manifest_signature;
use crate::family_manifest_registry::FamilyManifestRegistry;

/// S22b back-compat wrapper. Returns only the promoted shares slice —
/// callers that need credit rows (S23+) should use
/// `select_promoted_bounty_selection`.
pub fn select_promoted_bounty_shares(
    side_pool: &BountySidePool,
    registry: &FamilyManifestRegistry,
    runtime_height: u64,
    operator_pks: &[String],
) -> Vec<PromotedBountyShare> {
    select_promoted_bounty_selection(side_pool, registry, runtime_height, operator_pks).shares
}

/// S23a — full activation/caps gate. For each family whose manifest
/// passes (activation_height ≤ runtime_height ∧ signature present ∧
/// signature verifies against `operator_pks` ∧ caps ≥ 1):
///   1. Take up to `caps.max_shares_per_block` shares (FIFO).
///   2. For each, emit a credit row `min(share.reward, budget_left)`
///      and decrement `budget_left` (which starts at
///      `caps.max_reward_credit_per_block`).
///   3. Drop credit rows whose `amount == 0` — they represent
///      promoted-but-unpaid shares (e.g. budget exhausted, or family
///      with `max_reward_credit_per_block == 0`). The share itself
///      still counts in `shares`.
pub fn select_promoted_bounty_selection(
    side_pool: &BountySidePool,
    registry: &FamilyManifestRegistry,
    runtime_height: u64,
    operator_pks: &[String],
) -> PromotedBountySelection {
    let mut shares = Vec::new();
    let mut credits = Vec::new();
    for manifest in registry.iter() {
        if manifest.activation_height > runtime_height {
            continue;
        }
        if manifest.signature.is_none() {
            continue;
        }
        let Some(caps) = manifest.caps.as_ref() else {
            continue;
        };
        if caps.max_shares_per_block == 0 {
            continue;
        }
        if !signature_matches_any_operator(manifest, operator_pks) {
            continue;
        }
        let take = caps.max_shares_per_block as usize;
        let mut budget_left: u128 = caps
            .max_reward_credit_per_block
            .parse::<u128>()
            .unwrap_or(0);
        for share in side_pool
            .shares_for_family(&manifest.family_id)
            .iter()
            .take(take)
        {
            shares.push(PromotedBountyShare {
                family_id: share.family_id.clone(),
                bounty_id: share.bounty_id.clone(),
                proof_hash: share.proof_hash.clone(),
                prover: share.prover.clone(),
            });
            let credit = share.reward.min(budget_left);
            if credit > 0 {
                credits.push(PromotedBountyCredit {
                    family_id: share.family_id.clone(),
                    bounty_id: share.bounty_id.clone(),
                    prover: share.prover.clone(),
                    amount: credit.to_string(),
                });
                budget_left -= credit;
            }
        }
    }
    PromotedBountySelection { shares, credits }
}

fn signature_matches_any_operator(
    manifest: &crate::family_manifest::FamilyManifest,
    operator_pks: &[String],
) -> bool {
    operator_pks.iter().any(|pk| {
        matches!(
            verify_family_manifest_signature(pk, manifest),
            Ok(true)
        )
    })
}
