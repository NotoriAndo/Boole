use std::collections::BTreeSet;

use crate::{min_share_score, CalibrationPolicy};
use num_traits::ToPrimitive;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateShare {
    pub label: String,
    pub pk: String,
    pub n: String,
    pub j: String,
    pub c: String,
    pub share_hash: String,
    pub score: String,
    pub canon_tag: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockBuilderConfig {
    pub t_block: String,
    pub min_share_score: u128,
    pub k_max: usize,
}

impl BlockBuilderConfig {
    pub fn from_policy(policy: &CalibrationPolicy) -> anyhow::Result<Self> {
        let multiplier_nanos = (policy.min_share_score_multiplier * 1_000_000_000.0).round() as u64;
        let min_share_score = min_share_score(&policy.thresholds.t_share, multiplier_nanos)?
            .to_u128()
            .ok_or_else(|| anyhow::anyhow!("min share score exceeds u128"))?;
        Ok(Self {
            t_block: format!("0x{:064x}", policy.thresholds.t_block),
            min_share_score,
            k_max: policy.k_max,
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
) -> anyhow::Result<BuildSelectionResult> {
    let t_block = normalize_hex256(&cfg.t_block)?;
    let mut dropped_below_min_score = 0usize;
    let mut score_survivors = Vec::new();

    for share in shares {
        if share.c != chain_head {
            continue;
        }
        let score: u128 = share.score.parse()?;
        if score < cfg.min_share_score {
            dropped_below_min_score += 1;
            continue;
        }
        score_survivors.push(share.clone());
    }

    score_survivors.sort_by(|a, b| compare_preselection(a, b));
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
    }))
}

fn compare_preselection(a: &CandidateShare, b: &CandidateShare) -> std::cmp::Ordering {
    let a_score: u128 = a.score.parse().unwrap_or(0);
    let b_score: u128 = b.score.parse().unwrap_or(0);
    b_score.cmp(&a_score).then_with(|| compare_canonical(a, b))
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
    if !without_prefix
        .bytes()
        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F'))
    {
        anyhow::bail!("hex256 contains non-hex characters");
    }
    Ok(format!("{:0>64}", without_prefix.to_ascii_lowercase()))
}
