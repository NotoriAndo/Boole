//! S22b — activation-gated promotion of side-pool shares into the
//! `build_block_selection` `promoted_bounty_shares` argument — and, since
//! preimage v3 (ADR-0015 (a)), the consensus-shared settlement derivation.
//!
//! Two distinct responsibilities live here:
//!
//! 1. `select_promoted_bounty_selection` — PROPOSER-ONLY pool selection.
//!    Walks the registry, applies the proposer gates (activation, status
//!    eligibility, signature against operator pks, caps), and picks which
//!    side-pool shares enter the block. Only the block producer runs this.
//!
//! 2. `derive_bounty_settlement` — CONSENSUS-SHARED credit derivation.
//!    A pure function of (committed promoted-share rows, manifest
//!    registry, height) that both the producer and every replaying node
//!    run to derive the credit amounts. There is exactly one settlement
//!    policy: replay never trusts declared amounts (the block schema no
//!    longer carries any), it re-derives them from the committed rows.
//!
//! Gate ordering in selection matters: signature verification is the most
//! expensive step (ed25519 verify per (manifest, operator_pk) pair), so
//! cheaper gates short-circuit first.

use crate::block_builder::{PromotedBountyCredit, PromotedBountySelection, PromotedBountyShare};
use crate::bounty_side_pool::BountySidePool;
use crate::family_manifest::{verify_family_manifest_signature, FamilyManifest};
use crate::family_manifest_registry::FamilyManifestRegistry;

/// ADR-0015 (c-1) — the settlement-eligible family status set. Identical
/// to the announce-admission set (one policy, two consumers). `draft` and
/// `deprecated` parse but are not settlement-eligible; unknown values are
/// rejected by the manifest parser before a registry can hold them.
pub const SETTLEMENT_ELIGIBLE_STATUSES: &[&str] =
    &["bounty-only", "experimental", "capped-official", "official"];

fn settlement_eligible(manifest: &FamilyManifest, height: u64) -> bool {
    manifest.activation_height <= height
        && SETTLEMENT_ELIGIBLE_STATUSES.contains(&manifest.status.as_str())
}

/// ADR-0015 (a) — derive the credit rows for a block's committed
/// `promoted_bounty_shares` from the manifest registry. This is the ONE
/// settlement policy: the producer derives its ledger events with it and
/// replay re-derives balances with it, so a block cannot route bounty
/// credit its family manifest does not authorize.
///
/// Structural violations are typed rejects (the block is invalid):
/// - a committed row naming a family the registry does not hold,
/// - a family that is not settlement-eligible at `height`
///   (activation gate or status outside `SETTLEMENT_ELIGIBLE_STATUSES`),
/// - more rows for a family than `caps.max_shares_per_block`
///   (absent `caps` allows zero rows),
/// - a `reward` that does not parse as decimal `u128`.
///
/// Amounts are never a reject: each row credits
/// `min(reward, budget_left)` where `budget_left` starts at
/// `caps.max_reward_credit_per_block` and decrements per credited row
/// (block order). Families with `reward_policy.mode ==
/// "no_protocol_reward"` derive NO credit rows regardless of caps — the
/// committed share rows remain as provenance only. Zero-amount credit
/// rows are dropped.
pub fn derive_bounty_settlement(
    committed: &[PromotedBountyShare],
    registry: &FamilyManifestRegistry,
    height: u64,
) -> anyhow::Result<Vec<PromotedBountyCredit>> {
    let mut credits = Vec::new();
    let mut rows_per_family: std::collections::BTreeMap<&str, u64> =
        std::collections::BTreeMap::new();
    let mut budget_left_per_family: std::collections::BTreeMap<&str, u128> =
        std::collections::BTreeMap::new();
    for share in committed {
        let Some(manifest) = registry.get(&share.family_id) else {
            anyhow::bail!(
                "bounty settlement: committed promoted share names family {:?} \
                 which the family registry does not hold",
                share.family_id
            );
        };
        if !settlement_eligible(manifest, height) {
            anyhow::bail!(
                "bounty settlement: family {:?} is not settlement-eligible at height {} \
                 (activationHeight {}, status {:?})",
                share.family_id,
                height,
                manifest.activation_height,
                manifest.status
            );
        }
        let max_rows = manifest.caps.as_ref().map_or(0, |c| c.max_shares_per_block);
        let seen = rows_per_family
            .entry(manifest.family_id.as_str())
            .or_insert(0);
        *seen += 1;
        if *seen > max_rows {
            anyhow::bail!(
                "bounty settlement: family {:?} commits {} promoted shares, \
                 exceeding caps.max_shares_per_block {}",
                share.family_id,
                *seen,
                max_rows
            );
        }
        let reward: u128 = share.reward.parse().map_err(|_| {
            anyhow::anyhow!(
                "bounty settlement: promoted share reward must be a decimal u128, got {:?}",
                share.reward
            )
        })?;
        if manifest.reward_policy.mode == "no_protocol_reward" {
            continue;
        }
        let budget_left = budget_left_per_family
            .entry(manifest.family_id.as_str())
            .or_insert_with(|| {
                manifest
                    .caps
                    .as_ref()
                    .and_then(|c| c.max_reward_credit_per_block.parse::<u128>().ok())
                    .unwrap_or(0)
            });
        let credit = reward.min(*budget_left);
        if credit > 0 {
            credits.push(PromotedBountyCredit {
                family_id: share.family_id.clone(),
                bounty_id: share.bounty_id.clone(),
                prover: share.prover.clone(),
                amount: credit.to_string(),
            });
            *budget_left -= credit;
        }
    }
    Ok(credits)
}

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

/// S23a — proposer-only activation/caps gate. For each family whose
/// manifest passes (settlement-eligible at `runtime_height` ∧ signature
/// present ∧ signature verifies against `operator_pks` ∧ caps ≥ 1), take
/// up to `caps.max_shares_per_block` shares (FIFO) and stamp each with
/// its announced `reward`.
///
/// Credit rows are then derived by `derive_bounty_settlement` — the same
/// consensus-shared function replay runs over the committed rows — so
/// producer ledger events and replay balances cannot diverge by
/// construction. The signature/operator gates here are admission
/// convenience (ADR-0015 (c)): they bound what this producer SELECTS,
/// while settlement eligibility is what consensus enforces on what any
/// producer COMMITS.
pub fn select_promoted_bounty_selection(
    side_pool: &BountySidePool,
    registry: &FamilyManifestRegistry,
    runtime_height: u64,
    operator_pks: &[String],
) -> PromotedBountySelection {
    let mut shares = Vec::new();
    for manifest in registry.iter() {
        if !settlement_eligible(manifest, runtime_height) {
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
                reward: share.reward.to_string(),
            });
        }
    }
    let credits = derive_bounty_settlement(&shares, registry, runtime_height)
        .expect("selection picked only registry-held eligible families within caps");
    PromotedBountySelection { shares, credits }
}

fn signature_matches_any_operator(
    manifest: &crate::family_manifest::FamilyManifest,
    operator_pks: &[String],
) -> bool {
    operator_pks
        .iter()
        .any(|pk| matches!(verify_family_manifest_signature(pk, manifest), Ok(true)))
}
