use std::collections::BTreeSet;

use crate::{difficulty_weight, min_share_score, parse_biguint_hex, CalibrationPolicy};
use num_bigint::BigUint;
use num_traits::Zero;
use serde::{Deserialize, Serialize};

/// Bounty proof that has cleared its `FamilyManifest`'s activation +
/// signature gates and is admitted as additional block content alongside
/// the base PoF lane. Carries the routing fields the audit log + reward
/// ledger need (`family_id`, `bounty_id`, `proof_hash`, `prover`).
///
/// The block builder treats this slice as fully vetted: activation
/// gating, signature verification, and per-family caps (`max_shares_per_block`,
/// `max_score_multiplier_bps`) are the *caller's* responsibility — see
/// `select_promoted_bounty_shares`. `build_block_selection` does not
/// apply base-lane kernel-acceptance to promoted shares either; bounty
/// proofs run through their family's `BountyProofVerifier`, which is a
/// different namespace from the base canonicalizer's `canon_tag`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotedBountyShare {
    pub family_id: String,
    pub bounty_id: String,
    pub proof_hash: String,
    pub prover: String,
}

/// Credit row attached to a promoted bounty share. `amount` is already
/// capped against the per-family `caps.max_reward_credit_per_block`
/// budget at selection time. `amount == 0` rows are dropped before
/// persistence (they would land as no-op events on disk and complicate
/// replay diffs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotedBountyCredit {
    pub family_id: String,
    pub bounty_id: String,
    pub prover: String,
    /// Decimal `u128` string — JSON cannot carry full `u128` precision
    /// natively, so we pin it as text the same way `FamilyCaps` does.
    pub amount: String,
}

/// Bundled output of the activation/caps gate. Shares feed
/// `build_block_selection`'s bounty slot; credits feed `RewardLedger`
/// + `FileBountyEventLedger` at commit time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PromotedBountySelection {
    pub shares: Vec<PromotedBountyShare>,
    pub credits: Vec<PromotedBountyCredit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateShare {
    pub label: String,
    pub pk: String,
    /// Reward sink for this share. Empty means legacy behavior: credit `pk`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reward_pk: String,
    pub n: String,
    pub j: String,
    pub c: String,
    pub share_hash: String,
    pub score: String,
    pub canon_tag: u8,
    #[serde(default)]
    pub canon_hash: String,
    #[serde(default)]
    pub proof_package: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockBuilderConfig {
    pub t_block: String,
    pub t_share: String,
    pub min_share_score: BigUint,
    pub min_share_score_multiplier_nanos: u64,
    pub k_max: usize,
    pub difficulty_epoch: u64,
    pub difficulty_weight: String,
}

impl BlockBuilderConfig {
    pub fn from_policy(policy: &CalibrationPolicy) -> anyhow::Result<Self> {
        Self::from_policy_with_t_block(policy, format!("0x{:064x}", policy.thresholds.t_block), 0)
    }

    pub fn from_policy_with_t_block(
        policy: &CalibrationPolicy,
        t_block: String,
        difficulty_epoch: u64,
    ) -> anyhow::Result<Self> {
        let min_share_score = min_share_score(
            &policy.thresholds.t_share,
            policy.min_share_score_multiplier_nanos,
        )?;
        let t_block_value = parse_biguint_hex(&t_block)?;
        Ok(Self {
            t_block,
            t_share: format!("0x{:064x}", policy.thresholds.t_share),
            min_share_score,
            min_share_score_multiplier_nanos: policy.min_share_score_multiplier_nanos,
            k_max: policy.k_max,
            difficulty_epoch,
            difficulty_weight: difficulty_weight(&t_block_value)?.to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltBlockSelection {
    pub selected: Vec<CandidateShare>,
    pub proposer_index: usize,
    pub dropped_below_min_score: usize,
    pub dropped_kernel_reject: usize,
    pub truncated_by_kmax: usize,
    pub kernel_checked_tags: Vec<u8>,
    pub kernel_accepted: Vec<bool>,
    /// Bounty-lane shares that survived their kernel-tag check. Empty
    /// unless a caller passed promoted shares; never folded into the
    /// base-lane drop counters above (Hard-Guard).
    pub promoted_bounty_shares: Vec<PromotedBountyShare>,
    /// S23b — bounty credit rows attached to this block. Already capped
    /// against per-family `max_reward_credit_per_block` by the selection
    /// gate. Empty unless promoted credits were supplied.
    pub promoted_bounty_credits: Vec<PromotedBountyCredit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildSelectionResult {
    Ok(BuiltBlockSelection),
    NoProposer {
        dropped_kernel_reject: usize,
        kernel_checked_tags: Vec<u8>,
        kernel_accepted: Vec<bool>,
    },
    AmbiguousProposer {
        count: usize,
        dropped_kernel_reject: usize,
        kernel_checked_tags: Vec<u8>,
        kernel_accepted: Vec<bool>,
    },
}

pub fn build_block_selection(
    chain_head: &str,
    shares: &[CandidateShare],
    cfg: &BlockBuilderConfig,
    accepted_canon_tags: &BTreeSet<u8>,
    promoted_bounty_shares: &[PromotedBountyShare],
    promoted_bounty_credits: &[PromotedBountyCredit],
) -> anyhow::Result<BuildSelectionResult> {
    let t_block = normalize_hex256(&cfg.t_block)?;
    let mut dropped_below_min_score = 0usize;
    let mut score_survivors = Vec::new();

    for share in shares {
        if share.c != chain_head {
            continue;
        }
        let score = parse_score_decimal(&share.score)?;
        if score < cfg.min_share_score {
            dropped_below_min_score += 1;
            continue;
        }
        score_survivors.push(share.clone());
    }

    score_survivors.sort_by(compare_preselection);
    let truncated_by_kmax = score_survivors.len().saturating_sub(cfg.k_max);
    let preselected = score_survivors
        .into_iter()
        .take(cfg.k_max)
        .collect::<Vec<_>>();

    let mut survivors = Vec::new();
    let mut dropped_kernel_reject = 0usize;
    let mut kernel_checked_tags = Vec::new();
    let mut kernel_accepted = Vec::new();
    for share in preselected {
        let accepted = accepted_canon_tags.contains(&share.canon_tag);
        kernel_checked_tags.push(share.canon_tag);
        kernel_accepted.push(accepted);
        if accepted {
            survivors.push(share);
        } else {
            dropped_kernel_reject += 1;
        }
    }

    survivors.sort_by(compare_canonical);

    let mut proposer_index = 0usize;
    let mut proposer_count = 0usize;
    for (idx, share) in survivors.iter().enumerate() {
        let share_hash = normalize_hex256(&share.share_hash)?;
        if share_hash < t_block {
            proposer_index = idx;
            proposer_count += 1;
        }
    }

    if proposer_count == 0 {
        return Ok(BuildSelectionResult::NoProposer {
            dropped_kernel_reject,
            kernel_checked_tags,
            kernel_accepted,
        });
    }
    if proposer_count > 1 {
        return Ok(BuildSelectionResult::AmbiguousProposer {
            count: proposer_count,
            dropped_kernel_reject,
            kernel_checked_tags,
            kernel_accepted,
        });
    }

    Ok(BuildSelectionResult::Ok(BuiltBlockSelection {
        selected: survivors,
        proposer_index,
        dropped_below_min_score,
        dropped_kernel_reject,
        truncated_by_kmax,
        kernel_checked_tags,
        kernel_accepted,
        promoted_bounty_shares: promoted_bounty_shares.to_vec(),
        promoted_bounty_credits: promoted_bounty_credits.to_vec(),
    }))
}

fn compare_preselection(a: &CandidateShare, b: &CandidateShare) -> std::cmp::Ordering {
    let a_score = parse_score_decimal(&a.score).unwrap_or_else(|_| BigUint::zero());
    let b_score = parse_score_decimal(&b.score).unwrap_or_else(|_| BigUint::zero());
    b_score.cmp(&a_score).then_with(|| compare_canonical(a, b))
}

fn parse_score_decimal(value: &str) -> anyhow::Result<BigUint> {
    value
        .parse::<BigUint>()
        .map_err(|err| anyhow::anyhow!("invalid decimal score: {err}"))
}

fn compare_canonical(a: &CandidateShare, b: &CandidateShare) -> std::cmp::Ordering {
    a.pk.cmp(&b.pk)
        .then_with(|| a.n.cmp(&b.n))
        .then_with(|| a.j.cmp(&b.j))
}

fn normalize_hex256(value: &str) -> anyhow::Result<String> {
    let without_prefix = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    if without_prefix.len() > 64 {
        anyhow::bail!("hex256 value too long");
    }
    if !without_prefix.bytes().all(|b| b.is_ascii_hexdigit()) {
        anyhow::bail!("hex256 contains non-hex characters");
    }
    Ok(format!("{:0>64}", without_prefix.to_ascii_lowercase()))
}
