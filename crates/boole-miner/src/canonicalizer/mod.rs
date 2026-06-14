use std::path::Path;

pub mod lean_bound;
pub mod structural;

pub use lean_bound::LeanBoundCanonicalizer;
pub use structural::{encode_placeholder_bppk, StructuralCanonicalizer};

/// N0.3 — the canonicalizer the live `mine start` path uses.
///
/// With a checker directory present, the live loop grinds Lean-bound
/// canon: `LeanBoundCanonicalizer` whose `checker_artifact_hash` is read
/// directly from the configured checker package (a pure file hash — no
/// Lean process is spawned) and whose `verifier_hash` is derived from the
/// profile. Without a checker directory (the dev-tools mock path, where no
/// real Lean verification runs), there is no honest checker identity to
/// bind, so the loop falls back to the structural placeholder — that path
/// makes no real claim (its verifier is mocked or rejecting).
///
/// Returns an error (so `mine start` fails loudly) when a checker dir is
/// configured but its artifact hash cannot be computed — a malformed
/// checker package must not silently downgrade to an empty-evidence canon.
pub fn live_canonicalizer(
    lean_dir: Option<&Path>,
    profile: &str,
) -> Result<Box<dyn Canonicalizer>, CanonError> {
    match lean_dir {
        Some(dir) => {
            let checker_artifact_hash =
                boole_lean_runner::checker_artifact_hash(dir).map_err(|err| {
                    CanonError::Encode(format!(
                        "failed to compute checker artifact hash for {}: {err}",
                        dir.display()
                    ))
                })?;
            let verifier_hash = format!("boole-miner-verifier:{profile}");
            Ok(Box::new(LeanBoundCanonicalizer::new(
                verifier_hash,
                checker_artifact_hash,
            )))
        }
        None => Ok(Box::new(StructuralCanonicalizer)),
    }
}

/// N0-pre.8 — typed canonicalization failure. Fixes the `Canonicalizer`
/// error contract before the N0.2 `LeanBoundCanonicalizer` lands, so a
/// canon failure is never folded into transport (`network_errors`)
/// accounting again.
#[derive(Debug, thiserror::Error)]
pub enum CanonError {
    /// The proof source was rejected before encoding (shape or contract
    /// violation — e.g. an instance the family renderer cannot represent).
    #[error("invalid proof source: {0}")]
    InvalidProofSource(String),
    /// Encoding the canonical package failed.
    #[error("encode failed: {0}")]
    Encode(String),
}

#[derive(Debug, Clone)]
pub struct Target {
    pub seed_hex: String,
    pub d: u32,
    pub profile: String,
    pub n: u32,
    pub render: String,
}

pub trait Canonicalizer {
    fn canonicalize(&self, proof_source: &str, target: &Target) -> Result<Vec<u8>, CanonError>;
}
