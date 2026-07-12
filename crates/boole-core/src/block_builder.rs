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
    /// Announced bounty reward for this share, decimal `u128` string
    /// (JSON cannot carry full `u128` precision natively). Committed into
    /// the block-hash preimage (v3, ADR-0015 (a)); the credited amount is
    /// NOT this value but `min(reward, budget_left)` — replay re-derives
    /// it via `derive_bounty_settlement`, the same function the producer
    /// uses, so a block cannot declare a credit its family caps forbid.
    pub reward: String,
}

/// Credit row derived from a committed promoted bounty share. `amount`
/// is already capped against the per-family
/// `caps.max_reward_credit_per_block` budget by
/// `derive_bounty_settlement`. `amount == 0` rows are dropped before
/// persistence (they would land as no-op events on disk and complicate
/// replay diffs). Since preimage v3 (ADR-0015 (a)) credit rows are NOT
/// part of the block schema — they exist only as a derived view (reward
/// ledger events, replay balances).
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
/// + the node-owned bounty event ledger at commit time.
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
    /// N0.4b (Path 2) — family seed for offline re-derivation of the
    /// canonical Lean source; carried into `SelectedShareEvidence` at commit.
    #[serde(default)]
    pub seed_hex: String,
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
    /// N3-pre.6 — how many selected shares satisfied T_block this cycle.
    /// `1` is the ordinary case; `> 1` means two or more shares co-qualified
    /// as proposer and `proposer_index` was chosen deterministically (the
    /// lowest `compare_canonical` order among them) instead of refusing to
    /// build. Diagnostics-only: it does not gate whether a block is built.
    pub tied_proposer_count: usize,
    pub dropped_below_min_score: usize,
    pub dropped_kernel_reject: usize,
    pub truncated_by_kmax: usize,
    pub kernel_checked_tags: Vec<u8>,
    pub kernel_accepted: Vec<bool>,
    /// Bounty-lane shares that survived their kernel-tag check. Empty
    /// unless a caller passed promoted shares; never folded into the
    /// base-lane drop counters above (Hard-Guard). Since preimage v3
    /// (ADR-0015 (a)) these committed rows are the ONLY bounty content a
    /// block carries — credit rows are derived from them via
    /// `derive_bounty_settlement`, never attached.
    pub promoted_bounty_shares: Vec<PromotedBountyShare>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildSelectionResult {
    Ok(BuiltBlockSelection),
    NoProposer {
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
    credited_canon_hashes: &BTreeSet<String>,
    promoted_bounty_shares: &[PromotedBountyShare],
) -> anyhow::Result<BuildSelectionResult> {
    let t_block = normalize_hex256(&cfg.t_block)?;
    let mut dropped_below_min_score = 0usize;
    let mut score_survivors = Vec::new();

    for share in shares {
        if share.c != chain_head {
            continue;
        }
        // N4-pre.1 (ADR-0012 (b)) — proposer-side mirror of the replay
        // dedup rule: a share whose proof is already credited on this
        // chain can never be part of a valid block, so an honest node
        // drops it before selection instead of building a block replay
        // would reject. Shares without a canon_hash (legacy pool entries)
        // stay outside the rule, matching the replay-side exception.
        if !share.canon_hash.is_empty() && credited_canon_hashes.contains(&share.canon_hash) {
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
    // N4-pre.1 — within-block half of the dedup rule: two pool shares
    // carrying the same proof bytes (same canon_hash under different
    // (pk, n, j)) must not both be selected. Keep the first occurrence in
    // preselection order (deterministic: score desc, then canonical).
    let mut selected_canon_hashes: BTreeSet<String> = BTreeSet::new();
    score_survivors.retain(|share| {
        share.canon_hash.is_empty() || selected_canon_hashes.insert(share.canon_hash.clone())
    });
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

    survivors.sort_by(|a, b| compare_canonical(a.canonical_order_key(), b.canonical_order_key()));

    let share_hashes = survivors.iter().map(|share| share.share_hash.as_str());
    let (winner_index, tied_proposer_count) = select_qualifying_proposer(share_hashes, &t_block)?;

    let Some(proposer_index) = winner_index else {
        return Ok(BuildSelectionResult::NoProposer {
            dropped_kernel_reject,
            kernel_checked_tags,
            kernel_accepted,
        });
    };

    Ok(BuildSelectionResult::Ok(BuiltBlockSelection {
        selected: survivors,
        proposer_index,
        tied_proposer_count,
        dropped_below_min_score,
        dropped_kernel_reject,
        truncated_by_kmax,
        kernel_checked_tags,
        kernel_accepted,
        promoted_bounty_shares: promoted_bounty_shares.to_vec(),
    }))
}

fn compare_preselection(a: &CandidateShare, b: &CandidateShare) -> std::cmp::Ordering {
    let a_score = parse_score_decimal(&a.score).unwrap_or_else(|_| BigUint::zero());
    let b_score = parse_score_decimal(&b.score).unwrap_or_else(|_| BigUint::zero());
    b_score
        .cmp(&a_score)
        .then_with(|| compare_canonical(a.canonical_order_key(), b.canonical_order_key()))
}

fn parse_score_decimal(value: &str) -> anyhow::Result<BigUint> {
    value
        .parse::<BigUint>()
        .map_err(|err| anyhow::anyhow!("invalid decimal score: {err}"))
}

/// N3-pre.2 — the `(pk, n, j)` triple `compare_canonical` sorts by.
/// Borrowed rather than owned so both `CandidateShare` (block
/// construction) and `SelectedShareEvidence` (replay-side
/// re-verification, see `replay_evidence::verify_canonical_selection`)
/// can hand their fields to the same comparator without an intermediate
/// allocation or a shared supertype.
#[derive(Debug, Clone, Copy)]
pub struct CanonicalOrderKey<'a> {
    pub pk: &'a str,
    pub n: &'a str,
    pub j: &'a str,
}

impl CandidateShare {
    pub fn canonical_order_key(&self) -> CanonicalOrderKey<'_> {
        CanonicalOrderKey {
            pk: &self.pk,
            n: &self.n,
            j: &self.j,
        }
    }
}

/// N3-pre.2 — canonical `(pk, n, j)` share ordering. `build_block_selection`
/// sorts its post-kernel-gate survivors with this exact comparator before
/// picking the T_block proposer; `replay_evidence::verify_canonical_selection`
/// independently re-derives that same order from a persisted block's
/// `selectedShareEvidence` and rejects any block that isn't sorted this
/// way. Build and verify must keep calling this one function — never two
/// comparators that could silently drift apart. N3-pre.6's proposer
/// tie-break reuses this function too.
pub fn compare_canonical(a: CanonicalOrderKey, b: CanonicalOrderKey) -> std::cmp::Ordering {
    a.pk.cmp(b.pk)
        .then_with(|| a.n.cmp(b.n))
        .then_with(|| a.j.cmp(b.j))
}

/// N3-pre.6 (external review A-g1, critical) — the single, shared
/// deterministic proposer tie-break. `share_hashes` must already be in
/// `compare_canonical` order (both call sites guarantee this: the builder
/// sorts `survivors` with `compare_canonical` right before calling this,
/// and `replay_evidence::verify_canonical_selection` only calls this after
/// its own canonical-order check has passed). Among the shares satisfying
/// `share_hash < t_block`, the first one in that canonical order wins —
/// i.e. the lowest `compare_canonical` order among co-qualifiers.
///
/// Returns `(winner_index, qualifying_count)`: `winner_index` is `None`
/// when nothing qualifies (`NoProposer`); `qualifying_count > 1` means
/// two or more shares co-qualified and the tie was broken deterministically
/// rather than refusing to build/verify.
///
/// `build_block_selection` and `replay_evidence::verify_canonical_selection`
/// both call this exact function so a builder's tie-break winner and a
/// replayer's re-derived winner can never be defined two different ways.
pub fn select_qualifying_proposer<'a>(
    share_hashes: impl Iterator<Item = &'a str>,
    t_block: &str,
) -> anyhow::Result<(Option<usize>, usize)> {
    let normalized_t_block = normalize_hex256(t_block)?;
    let mut winner_index = None;
    let mut qualifying_count = 0usize;
    for (idx, share_hash) in share_hashes.enumerate() {
        if normalize_hex256(share_hash)? < normalized_t_block {
            qualifying_count += 1;
            if winner_index.is_none() {
                winner_index = Some(idx);
            }
        }
    }
    Ok((winner_index, qualifying_count))
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
