pub mod structural;

pub use structural::{encode_placeholder_bppk, StructuralCanonicalizer};

#[derive(Debug, Clone)]
pub struct Target {
    pub seed_hex: String,
    pub d: u32,
    pub profile: String,
    pub n: u32,
    pub render: String,
}

pub trait Canonicalizer {
    fn canonicalize(&self, proof_source: &str, target: &Target) -> anyhow::Result<Vec<u8>>;
}
