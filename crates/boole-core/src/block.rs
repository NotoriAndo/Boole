use serde::{Deserialize, Serialize};

use num_bigint::BigUint;

use crate::block_builder::{CanonicalOrderKey, PromotedBountyShare};
use crate::{difficulty_weight, parse_biguint_hex, Hex32};

/// Evidence v2 (§SC reset window, ADR-0015 (b)) — the persisted form of the
/// submitter's signed work envelope (`boole.signed.v1` envelope over a
/// `boole.signer.work.v2` payload). A serializable mirror of
/// `signed_envelope::SignedEnvelope` (which is not serde-derived); SC.1's
/// enforcement reconstructs the digest and verifies `signature` against
/// `pk` from exactly these fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareWorkAuthorization {
    pub schema: String,
    pub payload: serde_json::Value,
    pub pk: String,
    pub signature: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_id: Option<String>,
}

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
    /// Evidence v2 (§SC reset window, ADR-0015 (b)) — the submitter's full
    /// `boole.signer.work.v2` signed envelope, carried so replay can verify
    /// that the block's reward routing was authorized by the share winner
    /// (the v2 payload covers `rewardRecipient`). Optional in the schema:
    /// SC.1 lands the enforcement that makes it required on named networks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_work: Option<ShareWorkAuthorization>,
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
    /// Preimage v3 (ADR-0015 (a)) — every bounty share promoted into this
    /// block, each carrying its announced `reward`. COMMITTED into
    /// `block_hash` since v3: these rows are the settlement inputs from
    /// which replay derives credit amounts via `derive_bounty_settlement`
    /// (the declared credit rows that v2 committed left the schema).
    /// `validate_shape` enforces hex-32 `proof_hash`/`prover` and a decimal
    /// `u128` `reward`.
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
        for share in &self.promoted_bounty_shares {
            Hex32::from_hex(&share.proof_hash)?;
            Hex32::from_hex(&share.prover)?;
            if share.reward.parse::<u128>().is_err() {
                anyhow::bail!(
                    "promotedBountyShares[{}].reward must be u128 decimal, got {}",
                    share.bounty_id,
                    share.reward
                );
            }
        }
        Ok(())
    }
}
