use serde::{Deserialize, Serialize};

use num_bigint::BigUint;

use crate::block_builder::{CanonicalOrderKey, PromotedBountyCredit, PromotedBountyShare};
use crate::{difficulty_weight, parse_biguint_hex, Hex32};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedShareEvidence {
    pub pk: String,
    pub n: String,
    pub j: String,
    pub c: String,
    pub canon_hash: String,
    pub proof_package: String,
    /// N0.4b (Path 2) — the family seed this share's canonical proof derives
    /// from. Lets `deep_verify_block` re-generate the Lean source and
    /// recompute the canon offline. NOT part of `block_hash` (consensus
    /// unchanged); empty on shares submitted without a seed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub seed_hex: String,
}

impl SelectedShareEvidence {
    /// N3-pre.2 — see `block_builder::compare_canonical`; this is the
    /// same `(pk, n, j)` key `CandidateShare::canonical_order_key` hands
    /// to that comparator, taken from the evidence a replayer actually
    /// has instead of the builder's live candidate.
    pub fn canonical_order_key(&self) -> CanonicalOrderKey<'_> {
        CanonicalOrderKey {
            pk: &self.pk,
            n: &self.n,
            j: &self.j,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedBlock {
    pub height: u64,
    pub prev_c: String,
    pub c: String,
    pub proposer_pk: String,
    pub selected_share_hashes: Vec<String>,
    pub selected_share_pks: Vec<String>,
    /// Reward recipients for selected base-lane shares. Empty on legacy
    /// blocks means reward each corresponding `selectedSharePks` owner.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_share_reward_pks: Vec<String>,
    /// Reward recipient for the proposer bonus. Empty on legacy blocks means
    /// reward `proposerPk`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proposer_reward_pk: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_share_evidence: Vec<SelectedShareEvidence>,
    pub min_share_score: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub min_share_score_multiplier_nanos: u64,
    pub kmax_applied: u64,
    pub difficulty_epoch: u64,
    pub t_block: String,
    pub t_share: String,
    pub difficulty_weight: String,
    pub dropped_below_min_score: u64,
    pub dropped_kernel_reject: u64,
    pub truncated_by_kmax: u64,
    pub ts: u64,
    /// S23b — bounty credits accrued by this block. Empty for base-only
    /// blocks (the default), so old persisted blocks deserialize with
    /// `Vec::new()`. `validate_shape` enforces hex-32 prover keys and
    /// non-negative `u128` amounts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub promoted_bounty_credits: Vec<PromotedBountyCredit>,
    /// P1.3b — every bounty share promoted into this block (including
    /// zero-credit shares), recorded so the bounty-event ledger's
    /// `share_promoted` rows are re-derivable from the block store after a
    /// crash mid-commit. NOT part of `block_hash` (consensus is unchanged);
    /// this is node-local audit/recovery data. `validate_shape` enforces
    /// hex-32 `proof_hash`/`prover`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub promoted_bounty_shares: Vec<PromotedBountyShare>,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

impl PersistedBlock {
    pub fn validate_shape(&self) -> anyhow::Result<()> {
        Hex32::from_hex(&self.prev_c)?;
        Hex32::from_hex(&self.c)?;
        Hex32::from_hex(&self.proposer_pk)?;
        for h in &self.selected_share_hashes {
            Hex32::from_hex(h)?;
        }
        for pk in &self.selected_share_pks {
            Hex32::from_hex(pk)?;
        }
        for pk in &self.selected_share_reward_pks {
            Hex32::from_hex(pk)?;
        }
        if !self.proposer_reward_pk.is_empty() {
            Hex32::from_hex(&self.proposer_reward_pk)?;
        }
        if self.selected_share_hashes.len() != self.selected_share_pks.len() {
            anyhow::bail!(
                "selectedSharePks length ({}) must equal selectedShareHashes length ({})",
                self.selected_share_pks.len(),
                self.selected_share_hashes.len()
            );
        }
        if !self.selected_share_reward_pks.is_empty()
            && self.selected_share_reward_pks.len() != self.selected_share_hashes.len()
        {
            anyhow::bail!(
                "selectedShareRewardPks length ({}) must equal selectedShareHashes length ({})",
                self.selected_share_reward_pks.len(),
                self.selected_share_hashes.len()
            );
        }
        if self.kmax_applied as usize != self.selected_share_hashes.len() {
            anyhow::bail!("kmaxApplied must equal selectedShareHashes length");
        }
        let _: BigUint = self.min_share_score.parse()?;
        let t_block = parse_biguint_hex(&self.t_block)?;
        let _ = parse_biguint_hex(&self.t_share)?;
        let expected_weight = difficulty_weight(&t_block)?.to_string();
        if self.difficulty_weight != expected_weight {
            anyhow::bail!(
                "difficultyWeight mismatch: got {}, expected {}",
                self.difficulty_weight,
                expected_weight
            );
        }
        for credit in &self.promoted_bounty_credits {
            Hex32::from_hex(&credit.prover)?;
            if credit.amount.parse::<u128>().is_err() {
                anyhow::bail!(
                    "promotedBountyCredits[{}].amount must be u128 decimal, got {}",
                    credit.bounty_id,
                    credit.amount
                );
            }
        }
        for share in &self.promoted_bounty_shares {
            Hex32::from_hex(&share.proof_hash)?;
            Hex32::from_hex(&share.prover)?;
        }
        Ok(())
    }
}
