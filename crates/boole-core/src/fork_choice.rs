//! N4.1 — fork-choice weight. A chain's total proof-of-work is the sum of its
//! blocks' `difficulty_weight` (each ≈ 2^256 / t_block, so a harder block
//! contributes more). Competing chains are ordered by this total, not by
//! length: a shorter chain of harder blocks can outweigh a longer chain of
//! easy ones. This is the weight primitive N4.2's canonical-head selection is
//! built on; selection and reorg live in later slices.

use num_bigint::BigUint;
use num_traits::Zero;

use crate::{block_hash, Hex32, PersistedBlock};

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

/// The consensus hash of a chain's head (tip) block, recomputed from its
/// canonical inputs (`prev_c` + `selected_share_hashes`) rather than trusting
/// the stored `c` — the same derivation `replay` verifies each block against.
fn head_block_hash(head: &PersistedBlock) -> anyhow::Result<Hex32> {
    let prev = Hex32::from_hex(&head.prev_c)?;
    let share_hashes = head
        .selected_share_hashes
        .iter()
        .map(|h| Hex32::from_hex(h))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(block_hash(&prev, &share_hashes))
}

/// Choose the canonical head among competing candidate chains: the head of the
/// chain with the greatest cumulative work, breaking an exact tie by the lowest
/// head block hash so every honest node converges on the same tip. Returns the
/// chosen head's block hash.
///
/// Errors if there are no candidates or if any candidate chain is empty (a
/// chain with no head has no hash to choose). Each candidate is a chain in
/// height order; its head is the last block.
pub fn choose_canonical_head(candidates: &[Vec<PersistedBlock>]) -> anyhow::Result<Hex32> {
    let mut best: Option<(BigUint, Hex32)> = None;
    for chain in candidates {
        let head = chain
            .last()
            .ok_or_else(|| anyhow::anyhow!("a candidate chain has no blocks"))?;
        let weight = cumulative_difficulty_weight(chain)?;
        let hash = head_block_hash(head)?;
        let wins = match &best {
            None => true,
            Some((best_weight, best_hash)) => {
                weight > *best_weight || (weight == *best_weight && hash < *best_hash)
            }
        };
        if wins {
            best = Some((weight, hash));
        }
    }
    best.map(|(_, hash)| hash)
        .ok_or_else(|| anyhow::anyhow!("no candidate chains to choose from"))
}
