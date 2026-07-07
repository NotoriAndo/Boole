//! N4.1 — fork-choice weight. A chain's total proof-of-work is the sum of its
//! blocks' `difficulty_weight` (each ≈ 2^256 / t_block, so a harder block
//! contributes more). Competing chains are ordered by this total, not by
//! length: a shorter chain of harder blocks can outweigh a longer chain of
//! easy ones. This is the weight primitive N4.2's canonical-head selection is
//! built on; selection and reorg live in later slices.

use num_bigint::BigUint;
use num_traits::Zero;

use crate::PersistedBlock;

/// Sum the persisted per-block `difficulty_weight` across a chain.
///
/// Each block stores its weight as a DECIMAL string — the `BigUint` returned by
/// [`crate::difficulty_weight`] rendered via `to_string()` (see how
/// `block_builder`/`difficulty` persist it, and how `PersistedBlock::validate_shape`
/// re-derives the same decimal). So the weight is parsed as base-10, NOT as hex
/// like `t_block`/`t_share`. An empty chain has zero work.
pub fn cumulative_difficulty_weight(blocks: &[PersistedBlock]) -> anyhow::Result<BigUint> {
    let mut total = BigUint::zero();
    for block in blocks {
        let weight = block.difficulty_weight.parse::<BigUint>().map_err(|e| {
            anyhow::anyhow!(
                "block at height {} has an unparseable difficulty_weight {:?}: {e}",
                block.height,
                block.difficulty_weight
            )
        })?;
        total += weight;
    }
    Ok(total)
}
