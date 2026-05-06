use serde::{Deserialize, Serialize};

use num_bigint::BigUint;

use crate::{difficulty_weight, parse_biguint_hex, Hex32};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedBlock {
    pub height: u64,
    pub prev_c: String,
    pub c: String,
    pub proposer_pk: String,
    pub selected_share_hashes: Vec<String>,
    pub selected_share_pks: Vec<String>,
    pub min_share_score: String,
    pub kmax_applied: u64,
    pub difficulty_epoch: u64,
    pub t_block: String,
    pub t_share: String,
    pub difficulty_weight: String,
    pub dropped_below_min_score: u64,
    pub dropped_kernel_reject: u64,
    pub truncated_by_kmax: u64,
    pub ts: u64,
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
        if self.selected_share_hashes.len() != self.selected_share_pks.len() {
            anyhow::bail!(
                "selectedSharePks length ({}) must equal selectedShareHashes length ({})",
                self.selected_share_pks.len(),
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
        Ok(())
    }
}
