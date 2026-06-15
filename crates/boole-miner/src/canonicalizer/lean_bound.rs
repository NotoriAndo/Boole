//! N0.2 — `LeanBoundCanonicalizer`: builds the canonical bytes from the
//! family's rendered canonical proof plus injected checker-artifact
//! evidence, as a POFP-v2-shaped package the core validator accepts.
//!
//! ADR-0007 decision (a): the miner renders; the node verifies. This impl
//! never executes Lean — the checker evidence (verifier hash + checker
//! artifact hash) is injected at construction, and Lean execution remains
//! node-side. The package layout mirrors the node's
//! `canonical_pofp_package_from_lean_result_and_source`
//! (`boole-node/src/proof_bridge.rs`): two domain-separated 256-bit opaque
//! digest slots per ADR-0001.
//!
//! Digest inputs are exactly ADR-0007's minimum persisted evidence set —
//! `verifier_hash`, `checker_artifact_hash`, and the rendered canonical
//! proof source — so a node can recompute the identical canon hash from a
//! persisted block alone. N0.4 pins that recomputation; any change to this
//! recipe must change both sides in lockstep.

use boole_core::lean_bound_canon_package;

use super::{CanonError, Canonicalizer, Target};
use crate::family_v1_lenbound;

pub struct LeanBoundCanonicalizer {
    verifier_hash: String,
    checker_artifact_hash: String,
}

impl LeanBoundCanonicalizer {
    /// `verifier_hash` and `checker_artifact_hash` are the checker evidence
    /// the node's Lean runner reports for the canonical checker package;
    /// the miner treats them as opaque identity inputs.
    pub fn new(verifier_hash: impl Into<String>, checker_artifact_hash: impl Into<String>) -> Self {
        Self {
            verifier_hash: verifier_hash.into(),
            checker_artifact_hash: checker_artifact_hash.into(),
        }
    }
}

impl Canonicalizer for LeanBoundCanonicalizer {
    /// The canon derives from the FAMILY-CANONICAL proof rendered from the
    /// target's instance, not from the model's raw answer: the model answer
    /// is what the verifier judges; the canonical render is what the block
    /// binds, so the canon hash is deterministic per instance and node-side
    /// re-derivation needs no model artifacts. The package bytes come from
    /// the shared `boole_core::lean_bound_canon_package` encoder so the node
    /// (deep_verify_block) can recompute the identical bytes (N0.4a).
    fn canonicalize(&self, _proof_source: &str, target: &Target) -> Result<Vec<u8>, CanonError> {
        let instance = family_v1_lenbound::generate_from_hex(&target.seed_hex).map_err(|err| {
            CanonError::InvalidProofSource(format!(
                "target seed is not valid hex for v1-lenbound instance generation: {err}"
            ))
        })?;
        let lean_source = family_v1_lenbound::render_canonical_proof(&instance);
        Ok(lean_bound_canon_package(
            &self.verifier_hash,
            &self.checker_artifact_hash,
            &lean_source,
        ))
    }
}
