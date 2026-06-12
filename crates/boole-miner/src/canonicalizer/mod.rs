pub mod structural;

pub use structural::{encode_placeholder_bppk, StructuralCanonicalizer};

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
