use crate::{difficulty_weight, parse_biguint_hex, PersistedBlock};
use num_bigint::BigUint;
use num_traits::{One, Zero};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DifficultyEvidence {
    pub mode: String,
    pub retarget: String,
    pub difficulty_epoch: u64,
    pub t_block: String,
    pub t_share: String,
    pub difficulty_weight: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DifficultyRetargetPolicy {
    pub target_block_ms: u64,
    pub retarget_every_blocks: u64,
    pub max_adjustment_factor: u64,
}

impl DifficultyRetargetPolicy {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.target_block_ms == 0 {
            anyhow::bail!("targetBlockMs must be > 0");
        }
        if self.retarget_every_blocks < 2 {
            anyhow::bail!("retargetEveryBlocks must be >= 2");
        }
        if self.max_adjustment_factor < 1 {
            anyhow::bail!("maxAdjustmentFactor must be >= 1");
        }
        Ok(())
    }
}

pub fn retarget_t_block(
    current_t_block: &BigUint,
    actual_span_ms: u64,
    expected_span_ms: u64,
    policy: &DifficultyRetargetPolicy,
) -> anyhow::Result<BigUint> {
    policy.validate()?;
    if current_t_block.is_zero() {
        anyhow::bail!("current T_block must be > 0");
    }
    if expected_span_ms == 0 {
        anyhow::bail!("expected span must be > 0");
    }

    let factor = policy.max_adjustment_factor;
    let min_actual = (expected_span_ms / factor).max(1);
    let max_actual = expected_span_ms.saturating_mul(factor);
    let clamped_actual = actual_span_ms.clamp(min_actual, max_actual);

    let numerator = current_t_block * BigUint::from(clamped_actual);
    let mut next = numerator / BigUint::from(expected_span_ms);
    if next.is_zero() {
        next = BigUint::one();
    }
    let max_target = (BigUint::one() << 256usize) - BigUint::one();
    if next > max_target {
        next = max_target;
    }
    Ok(next)
}

pub fn expected_retarget_difficulty_for_height(
    existing_blocks: &[PersistedBlock],
    initial_t_block: &str,
    policy: &DifficultyRetargetPolicy,
) -> anyhow::Result<DifficultyEvidence> {
    policy.validate()?;
    let height = existing_blocks.len() as u64;
    if height == 0 {
        return difficulty_evidence(initial_t_block, 0, false);
    }
    if height < policy.retarget_every_blocks || !height.is_multiple_of(policy.retarget_every_blocks)
    {
        let previous = existing_blocks
            .last()
            .ok_or_else(|| anyhow::anyhow!("missing previous block for difficulty"))?;
        return difficulty_evidence(&previous.t_block, previous.difficulty_epoch, true);
    }

    let window_len = policy.retarget_every_blocks as usize;
    let start = existing_blocks.len().saturating_sub(window_len);
    let window = &existing_blocks[start..];
    let first = window
        .first()
        .ok_or_else(|| anyhow::anyhow!("empty retarget window"))?;
    let last = window
        .last()
        .ok_or_else(|| anyhow::anyhow!("empty retarget window"))?;
    let current = parse_biguint_hex(&last.t_block)?;
    let actual_span_ms = last.ts.saturating_sub(first.ts).max(1);
    let expected_span_ms = policy
        .target_block_ms
        .saturating_mul(policy.retarget_every_blocks.saturating_sub(1))
        .max(1);
    let next = retarget_t_block(&current, actual_span_ms, expected_span_ms, policy)?;
    difficulty_evidence(
        &format!("0x{next:064x}"),
        height / policy.retarget_every_blocks,
        true,
    )
}

pub fn validate_retargeted_difficulty(
    blocks: &[PersistedBlock],
    initial_t_block: &str,
    policy: &DifficultyRetargetPolicy,
) -> anyhow::Result<()> {
    let mut previous = Vec::with_capacity(blocks.len());
    for block in blocks {
        let expected = expected_retarget_difficulty_for_height(&previous, initial_t_block, policy)?;
        if block.difficulty_epoch != expected.difficulty_epoch {
            anyhow::bail!(
                "difficultyEpoch mismatch at height {}: got {}, expected {}",
                block.height,
                block.difficulty_epoch,
                expected.difficulty_epoch
            );
        }
        if block.t_block != expected.t_block {
            anyhow::bail!(
                "tBlock mismatch at height {}: got {}, expected {}",
                block.height,
                block.t_block,
                expected.t_block
            );
        }
        if block.difficulty_weight != expected.difficulty_weight {
            anyhow::bail!(
                "difficultyWeight mismatch at height {}: got {}, expected {}",
                block.height,
                block.difficulty_weight,
                expected.difficulty_weight
            );
        }
        previous.push(block.clone());
    }
    Ok(())
}

fn difficulty_evidence(
    t_block: &str,
    difficulty_epoch: u64,
    retarget_enabled: bool,
) -> anyhow::Result<DifficultyEvidence> {
    let target = parse_biguint_hex(t_block)?;
    Ok(DifficultyEvidence {
        mode: if retarget_enabled {
            "epoch-retarget-v0".to_string()
        } else {
            "static-calibrated".to_string()
        },
        retarget: if retarget_enabled {
            "enabled".to_string()
        } else {
            "not-enabled".to_string()
        },
        difficulty_epoch,
        t_block: format!("0x{target:064x}"),
        t_share: "".to_string(),
        difficulty_weight: difficulty_weight(&target)?.to_string(),
    })
}
