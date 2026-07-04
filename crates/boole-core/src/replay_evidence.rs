use sha2::{Digest, Sha256};

use crate::block::PersistedBlock;
use crate::block_builder::compare_canonical;
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

/// N3-pre.2 — replay/verify independently re-derives, from a block's
/// contents alone, everything `build_block_selection` guarantees except
/// pool-global optimality: the `compare_canonical` ordering of the
/// selected shares, T_block satisfaction, and a unique proposer (exactly
/// one selected share's hash satisfies T_block — mirroring
/// `build_block_selection`'s own `NoProposer`/`AmbiguousProposer` split).
/// Pool-global optimality ("was this really the pool's top-k") is NOT
/// checkable from a single block — the replayer never had the candidate
/// pool the proposer chose from — and stays an explicit non-goal, along
/// with cross-checking the block's declared `proposerPk` identity/reward
/// field against the qualifying share (that field is reward routing, not
/// part of the selection shape this check re-derives).
///
/// A no-op for a block with empty `selectedShareEvidence`: the canonical
/// order is defined over the evidence's `(pk, n, j)` triples, which only
/// exist once evidence is present, so — like `verify_selected_share_evidence`
/// itself — this check is only meaningful for evidence-bearing blocks.
/// Call this only after `verify_selected_share_evidence` has passed, so
/// `selected_share_pks`/`selected_share_hashes`/`selected_share_evidence`
/// are already known to line up index-for-index.
pub(crate) fn verify_canonical_selection(block: &PersistedBlock) -> anyhow::Result<()> {
    if block.selected_share_evidence.is_empty() {
        return Ok(());
    }

    for (idx, pair) in block.selected_share_evidence.windows(2).enumerate() {
        let (a, b) = (&pair[0], &pair[1]);
        if compare_canonical(a.canonical_order_key(), b.canonical_order_key())
            == std::cmp::Ordering::Greater
        {
            anyhow::bail!(
                "selected share evidence is out of canonical order between index {} \
                 (pk={}) and index {} (pk={}): compare_canonical requires ascending order",
                idx,
                a.pk,
                idx + 1,
                b.pk
            );
        }
    }

    let t_block = parse_biguint_hex(&block.t_block)?;
    let mut proposer_count = 0usize;
    for hash in &block.selected_share_hashes {
        if parse_biguint_hex(hash)? < t_block {
            proposer_count += 1;
        }
    }

    if proposer_count == 0 {
        anyhow::bail!(
            "no selected share satisfies T_block; replay found no proposer among {} shares",
            block.selected_share_hashes.len()
        );
    }
    if proposer_count > 1 {
        anyhow::bail!(
            "ambiguous proposer: {} selected shares satisfy T_block, expected exactly one",
            proposer_count
        );
    }

    Ok(())
}
