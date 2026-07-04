use sha2::{Digest, Sha256};

use crate::block::PersistedBlock;
use crate::{
    min_share_score, parse_biguint_hex, share_hash, validate_proof_package_shape, Hex32,
    ValidationResult,
};

/// N3-pre.1 — internal switch for how `verify_selected_share_evidence`
/// treats a block whose `selectedShareEvidence` is empty.
///
/// `Strict` is the only variant `replay_blocks` (and `replay_blocks_with_retarget`)
/// ever pass — that is the entry point the future p2p ingest replay path
/// will call. `AllowLegacyEvidenceLess` exists solely so a caller that
/// explicitly holds a `crate::replay::LegacyEvidenceOptIn` (test code, or
/// local/offline replay of a pre-evidence legacy chain) can replay blocks
/// that predate `selectedShareEvidence`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum EvidencePolicy {
    #[default]
    Strict,
    AllowLegacyEvidenceLess,
}

pub(crate) fn verify_selected_share_evidence(
    block: &PersistedBlock,
    policy: EvidencePolicy,
) -> anyhow::Result<()> {
    if block.selected_share_evidence.is_empty() {
        return match policy {
            EvidencePolicy::Strict => Err(anyhow::anyhow!(
                "selected share evidence is required and must not be empty; \
                 pass a LegacyEvidenceOptIn via replay_blocks_allow_legacy_evidence_less \
                 to replay a pre-evidence legacy chain"
            )),
            EvidencePolicy::AllowLegacyEvidenceLess => Ok(()),
        };
    }
    if block.selected_share_evidence.len() != block.selected_share_hashes.len() {
        anyhow::bail!(
            "selected share evidence count mismatch: got {}, expected {}",
            block.selected_share_evidence.len(),
            block.selected_share_hashes.len()
        );
    }
    if block.min_share_score_multiplier_nanos == 0 {
        anyhow::bail!("selected share evidence requires minShareScoreMultiplierNanos");
    }
    let t_share = parse_biguint_hex(&block.t_share)?;
    let expected_min_share_score =
        min_share_score(&t_share, block.min_share_score_multiplier_nanos)?.to_string();
    if block.min_share_score != expected_min_share_score {
        anyhow::bail!(
            "selected share evidence minShareScore mismatch: got {}, expected {}",
            block.min_share_score,
            expected_min_share_score
        );
    }

    for (idx, evidence) in block.selected_share_evidence.iter().enumerate() {
        if evidence.c != block.prev_c {
            anyhow::bail!(
                "selected share evidence c mismatch at index {}: got {}, expected {}",
                idx,
                evidence.c,
                block.prev_c
            );
        }
        if evidence.pk != block.selected_share_pks[idx] {
            anyhow::bail!(
                "selected share evidence pk mismatch at index {}: got {}, expected {}",
                idx,
                evidence.pk,
                block.selected_share_pks[idx]
            );
        }

        let package_bytes = hex::decode(&evidence.proof_package).map_err(|err| {
            anyhow::anyhow!(
                "selected share evidence proofPackage hex invalid at index {idx}: {err}"
            )
        })?;
        match validate_proof_package_shape(&package_bytes) {
            ValidationResult::Ok { .. } => {}
            ValidationResult::Err { reason } => {
                anyhow::bail!(
                    "selected share evidence proofPackage invalid at index {}: {:?}",
                    idx,
                    reason
                );
            }
        }

        let expected_canon_hash = hex::encode(Sha256::digest(&package_bytes));
        if evidence.canon_hash != expected_canon_hash {
            anyhow::bail!(
                "selected share evidence canonHash mismatch at index {}: got {}, expected {}",
                idx,
                evidence.canon_hash,
                expected_canon_hash
            );
        }

        let c = Hex32::from_hex(&evidence.c)?;
        let pk = Hex32::from_hex(&evidence.pk)?;
        let n = Hex32::from_hex(&evidence.n)?;
        let j = Hex32::from_hex(&evidence.j)?;
        let canon_hash = Hex32::from_hex(&evidence.canon_hash)?;
        let expected_share_hash = share_hash(&c, &pk, &n, &j, &canon_hash).to_hex();
        if expected_share_hash != block.selected_share_hashes[idx] {
            anyhow::bail!(
                "selected share evidence shareHash mismatch at index {}: got {}, expected {}",
                idx,
                expected_share_hash,
                block.selected_share_hashes[idx]
            );
        }
    }

    Ok(())
}
