use sha2::{Digest, Sha256};

use crate::block::PersistedBlock;
use crate::block_builder::{compare_canonical, select_qualifying_proposer};
use crate::rules::{BASE_LANE_MAX_DECLS, BASE_LANE_MAX_PROOF_BYTES};
use crate::{
    find_target_seed_j_index, min_share_score, parse_biguint_hex, share_hash,
    validate_proof_package_with_limits, Hex32, ValidationReason, ValidationResult,
    TARGET_SEED_J_INDEX_BOUND,
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
    // §SC W1.a — the multiplier's consensus source is the Tier-2 rule
    // constant, so the declared value must match it exactly; checking
    // only the minShareScore arithmetic below would let a proposer move
    // the floor by declaring a consistent (multiplier, minShareScore)
    // pair of its own choosing.
    if block.min_share_score_multiplier_nanos != crate::rules::MIN_SHARE_SCORE_MULTIPLIER_NANOS {
        anyhow::bail!(
            "selected share evidence minShareScoreMultiplierNanos must equal the \
             consensus rule constant {}, got {}",
            crate::rules::MIN_SHARE_SCORE_MULTIPLIER_NANOS,
            block.min_share_score_multiplier_nanos
        );
    }
    let t_share = parse_biguint_hex(&block.t_share)?;
    let expected_min_share_score =
        min_share_score(&t_share, block.min_share_score_multiplier_nanos)?;
    if block.min_share_score != expected_min_share_score.to_string() {
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
        // C-09 (ADR-0016 (c-2)) — replay enforces the SAME committed
        // base-lane proof-resource ceiling admission applies, not just the
        // package shape, so a peer chain or bootstrap snapshot cannot smuggle
        // an oversized/over-declared package past re-verification. The two
        // rejection categories stay distinct: a package that DECODES but
        // exceeds the committed byte/decl ceiling is the resource-limit
        // rejection this slice adds (the parity admission enforces), while a
        // package that fails to decode is the pre-existing shape rejection —
        // keeping its own message so that contract is unchanged.
        match validate_proof_package_with_limits(
            &package_bytes,
            BASE_LANE_MAX_PROOF_BYTES as usize,
            BASE_LANE_MAX_DECLS as usize,
        ) {
            ValidationResult::Ok { .. } => {}
            ValidationResult::Err {
                reason:
                    reason @ (ValidationReason::TooLarge { .. } | ValidationReason::TooManyDecls { .. }),
            } => {
                anyhow::bail!(
                    "selected share evidence proofPackage exceeds the committed base-lane \
                     resource limit at index {}: {:?}",
                    idx,
                    reason
                );
            }
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
        let expected_share_hash_digest = share_hash(&c, &pk, &n, &j, &canon_hash);
        let expected_share_hash = expected_share_hash_digest.to_hex();
        if expected_share_hash != block.selected_share_hashes[idx] {
            anyhow::bail!(
                "selected share evidence shareHash mismatch at index {}: got {}, expected {}",
                idx,
                expected_share_hash,
                block.selected_share_hashes[idx]
            );
        }

        // SC.7 (masterplan audit item 1, Critical) — the committed floor
        // is only meaningful if every selected share actually clears it:
        // re-derive the share's score from its (already re-derived) hash
        // — a pure function — and reject the block on any shortfall.
        // Until this check, replay verified the DECLARED minimum's
        // arithmetic but never ran the per-share predicate, so a block
        // could commit shares below its own floor.
        let score = crate::share_score(&expected_share_hash_digest);
        if score < expected_min_share_score {
            anyhow::bail!(
                "selected share at index {} scores {}, below the committed minimum share \
                 score {}",
                idx,
                score,
                expected_min_share_score
            );
        }

        // Seed↔prev-block binding — a non-empty persisted `seedHex` claims
        // the chain posed this share's problem, so replay re-derives it:
        // `target_seed(c, pk, n, j_index)` for some in-bound `j_index`
        // (`c == prev_c` is already enforced above). Empty `seedHex` stays
        // accepted (pre-N0.4b legacy posture; mandatory seeds are N3.3
        // scope). Admission enforces the same rule on the live path.
        if !evidence.seed_hex.is_empty()
            && find_target_seed_j_index(&c, &pk, &n, &evidence.seed_hex).is_none()
        {
            anyhow::bail!(
                "selected share evidence seedHex at index {} does not derive from \
                 target_seed(c, pk, n, j_index) for any j_index < {}: got {}",
                idx,
                TARGET_SEED_J_INDEX_BOUND,
                evidence.seed_hex
            );
        }
    }

    Ok(())
}

/// N3-pre.2 — replay/verify independently re-derives, from a block's
/// contents alone, everything `build_block_selection` guarantees except
/// pool-global optimality: the `compare_canonical` ordering of the
/// selected shares and T_block satisfaction (at least one selected
/// share's hash satisfies T_block — mirroring `build_block_selection`'s
/// own `NoProposer` case).
///
/// N3-pre.6 — when more than one selected share satisfies T_block, this
/// no longer rejects the block. It calls the exact same
/// `select_qualifying_proposer` tie-break `build_block_selection` uses,
/// so replay re-derives the identical deterministic winner (the lowest
/// `compare_canonical` order among co-qualifiers) instead of the two
/// sides ever defining "the winner" two different ways. Before this
/// slice, `build_block_selection` refused to build a block at all once
/// two shares co-qualified, so this path was unreachable; now that the
/// builder resolves the tie, replay must accept the same resolution on
/// reboot/recovery instead of rejecting a block the node itself already
/// committed.
///
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

    let share_hashes = block.selected_share_hashes.iter().map(|hash| hash.as_str());
    let (winner_index, _tied_proposer_count) =
        select_qualifying_proposer(share_hashes, &block.t_block)?;

    if winner_index.is_none() {
        anyhow::bail!(
            "no selected share satisfies T_block; replay found no proposer among {} shares",
            block.selected_share_hashes.len()
        );
    }

    Ok(())
}
