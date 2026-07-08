use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::block::PersistedBlock;
use crate::difficulty::verify_block_ts_median_time_past;
use crate::replay_evidence::{
    verify_canonical_selection, verify_selected_share_evidence, EvidencePolicy,
};
use crate::{block_hash, Hex32};

/// N3-pre.1 — explicit opt-in to replay a pre-evidence legacy chain (a
/// block whose `selectedShareEvidence` is empty). `replay_blocks` and
/// `replay_blocks_with_retarget` reject such blocks by default; this
/// type is the ONLY way to relax that, and it is constructible only via
/// `for_legacy_replay_only` so every call site is greppable.
///
/// For test code and legacy/local replay callers ONLY — e.g. a node
/// reconciling its own historical block store at boot, or the offline
/// `boole state verify` / `boole chain replay` CLI tools. The future p2p
/// ingest replay path must call `replay_blocks` (which has no parameter
/// that could accept this); never thread this opt-in through any code
/// path that replays a block received from a peer.
#[derive(Debug, Clone, Copy)]
pub struct LegacyEvidenceOptIn(());

impl LegacyEvidenceOptIn {
    /// For test code and legacy/local replay callers ONLY. Read the type
    /// doc comment before wiring this into any new call site.
    pub fn for_legacy_replay_only() -> Self {
        LegacyEvidenceOptIn(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedCredit {
    pub pk: String,
    pub amount: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedRewardEvent {
    pub height: u64,
    pub c: String,
    pub credits: Vec<PersistedCredit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayResult {
    pub latest_c: String,
    pub height: u64,
    /// Per-pk balances. Folds in BOTH base-lane proposer/share credits
    /// (one unit per credit row) AND bounty-lane promoted credits
    /// (`PromotedBountyCredit.amount` for each row in the block).
    pub balances: BTreeMap<String, u128>,
    /// S23b — per-family bounty credit totals across all replayed
    /// blocks. Empty for chains with no promoted credits.
    /// `verify_ledger_matches_replay` cross-checks this against the
    /// node runtime store.
    pub bounty_credit_by_family: BTreeMap<String, u128>,
}

pub fn compute_block_credits(
    proposer_pk: &str,
    share_owners: &[String],
) -> anyhow::Result<Vec<PersistedCredit>> {
    compute_block_credits_for_reward_pks(proposer_pk, share_owners)
}

pub fn compute_block_credits_for_reward_pks(
    proposer_reward_pk: &str,
    share_reward_pks: &[String],
) -> anyhow::Result<Vec<PersistedCredit>> {
    if share_reward_pks.is_empty() {
        anyhow::bail!("computeBlockCredits: share owners list must not be empty");
    }
    Hex32::from_hex(proposer_reward_pk)?;
    let mut totals: BTreeMap<String, u128> = BTreeMap::new();
    for pk in share_reward_pks {
        Hex32::from_hex(pk)?;
        *totals.entry(pk.clone()).or_insert(0) += 1;
    }
    *totals.entry(proposer_reward_pk.to_string()).or_insert(0) += 1;
    Ok(totals
        .into_iter()
        .map(|(pk, amount)| PersistedCredit {
            pk,
            amount: amount.to_string(),
        })
        .collect())
}

pub fn compute_block_reward_credits(
    block: &PersistedBlock,
) -> anyhow::Result<Vec<PersistedCredit>> {
    let proposer_reward_pk = if block.proposer_reward_pk.is_empty() {
        block.proposer_pk.as_str()
    } else {
        block.proposer_reward_pk.as_str()
    };
    let share_reward_pks = if block.selected_share_reward_pks.is_empty() {
        &block.selected_share_pks
    } else {
        &block.selected_share_reward_pks
    };
    compute_block_credits_for_reward_pks(proposer_reward_pk, share_reward_pks)
}

/// N1.3 (G2) — retarget-aware replay. Folds
/// `validate_retargeted_difficulty` (every persisted block's `t_block` /
/// `difficulty_epoch` / `difficulty_weight` must match what the policy
/// computes for its height) ahead of the standard share-evidence + linkage
/// replay. Any path that claims `difficultyMode = epoch-retarget-v0` MUST
/// use this; `replay_blocks` stays the non-retarget-aware entry point.
pub fn replay_blocks_with_retarget(
    blocks: &[PersistedBlock],
    initial_t_block: &str,
    policy: &crate::DifficultyRetargetPolicy,
) -> anyhow::Result<ReplayResult> {
    crate::validate_retargeted_difficulty(blocks, initial_t_block, policy)?;
    replay_blocks(blocks)
}

/// N3-pre.1 — same as `replay_blocks_with_retarget`, but for a pre-evidence
/// legacy chain. Requires an explicit `LegacyEvidenceOptIn` (test code /
/// legacy-local replay callers only — see that type's doc comment).
pub fn replay_blocks_with_retarget_allow_legacy_evidence_less(
    blocks: &[PersistedBlock],
    initial_t_block: &str,
    policy: &crate::DifficultyRetargetPolicy,
    opt_in: LegacyEvidenceOptIn,
) -> anyhow::Result<ReplayResult> {
    crate::validate_retargeted_difficulty(blocks, initial_t_block, policy)?;
    replay_blocks_allow_legacy_evidence_less(blocks, opt_in)
}

/// N3-pre.1 — the replay entry point every current node/CLI boot path
/// (and, in future, p2p ingest) uses. Rejects a block whose
/// `selectedShareEvidence` is empty by default (see
/// `replay_evidence::verify_selected_share_evidence`); this is a
/// consensus-critical evidence requirement, not merely a shape check.
pub fn replay_blocks(blocks: &[PersistedBlock]) -> anyhow::Result<ReplayResult> {
    replay_blocks_with_evidence_policy(blocks, EvidencePolicy::Strict)
}

/// N3-pre.1 — replay a pre-evidence legacy chain (existing golden
/// fixtures, hand-built test chains). Requires an explicit
/// `LegacyEvidenceOptIn` — see that type's doc comment for the callers
/// this is (and is not) meant for.
pub fn replay_blocks_allow_legacy_evidence_less(
    blocks: &[PersistedBlock],
    _opt_in: LegacyEvidenceOptIn,
) -> anyhow::Result<ReplayResult> {
    replay_blocks_with_evidence_policy(blocks, EvidencePolicy::AllowLegacyEvidenceLess)
}

fn replay_blocks_with_evidence_policy(
    blocks: &[PersistedBlock],
    evidence_policy: EvidencePolicy,
) -> anyhow::Result<ReplayResult> {
    // N3-pre.3 (review #3) — deterministic ts trust gate, upfront and
    // unconditional (both the retarget-aware and plain replay entry points,
    // strict or legacy-evidence-less, funnel through here). Without this,
    // `actual_span_ms` in `expected_retarget_difficulty_for_height` would
    // trust a self-reported `ts` that could be rewound to steer difficulty
    // retargeting.
    verify_block_ts_median_time_past(blocks)?;

    let mut latest_c =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    let mut balances: BTreeMap<String, u128> = BTreeMap::new();
    let mut bounty_credit_by_family: BTreeMap<String, u128> = BTreeMap::new();
    // N4-pre.1 (ADR-0012 (d)) — chain-order index of every canon_hash
    // credited so far. Purely re-derived from block data; no new persisted
    // consensus state.
    let mut credited_canon_hashes: BTreeSet<String> = BTreeSet::new();

    for (expected_height, block) in blocks.iter().enumerate() {
        block.validate_shape()?;
        if block.height != expected_height as u64 {
            anyhow::bail!(
                "block height mismatch: got {}, expected {}",
                block.height,
                expected_height
            );
        }
        if block.prev_c != latest_c {
            anyhow::bail!(
                "prevC mismatch: block prevC={} but replay head={}",
                block.prev_c,
                latest_c
            );
        }
        let expected_c = block_hash(block).to_hex();
        if block.c != expected_c {
            anyhow::bail!("block c mismatch: got {}, expected {}", block.c, expected_c);
        }
        verify_selected_share_evidence(block, evidence_policy)?;
        // N3-pre.2 — same policy layer as the evidence check above: a
        // no-op unless selectedShareEvidence is present (see that
        // function's doc comment), so this never rejects a legacy
        // evidence-less block that the policy above already allowed.
        verify_canonical_selection(block)?;
        // N4-pre.1 (ADR-0012) — consensus-level proof dedup: a selected
        // share's canon_hash must not have been credited in any earlier
        // block, nor twice within this block. Runs AFTER
        // verify_selected_share_evidence, which already proved each
        // evidence.canon_hash equals the re-derived SHA-256 of the package
        // bytes — so the key checked here IS the re-derived one (decision
        // (c)), cross-pk by construction. Legacy evidence-less blocks
        // contribute no canon_hash and stay outside the rule (their only
        // entry path is the explicit LegacyEvidenceOptIn).
        for evidence in &block.selected_share_evidence {
            if !credited_canon_hashes.insert(evidence.canon_hash.clone()) {
                anyhow::bail!(
                    "block {} would credit an already-credited proof: canon_hash {} \
                     was already credited on this chain (consensus proof dedup, ADR-0012)",
                    block.height,
                    evidence.canon_hash
                );
            }
        }

        for credit in compute_block_reward_credits(block)? {
            let amount: u128 = credit.amount.parse()?;
            *balances.entry(credit.pk).or_insert(0) += amount;
        }
        for credit in &block.promoted_bounty_credits {
            let amount: u128 = credit.amount.parse()?;
            *balances.entry(credit.prover.clone()).or_insert(0) += amount;
            *bounty_credit_by_family
                .entry(credit.family_id.clone())
                .or_insert(0) += amount;
        }
        latest_c = block.c.clone();
    }

    Ok(ReplayResult {
        latest_c,
        height: blocks.len() as u64,
        balances,
        bounty_credit_by_family,
    })
}
