use sha2::{Digest, Sha256};

use crate::proof_package::{expr_tag, level_tag, lit_tag, BppkBuilder, FORMAT_VERSION, MAGIC};

use super::{Canonicalizer, Target};

const NAME_DOMAIN: &[u8] = b"bppk-name";

fn target_fingerprint(t: &Target) -> String {
    let mut hasher = Sha256::new();
    hasher.update(NAME_DOMAIN);
    hasher.update(t.seed_hex.as_bytes());
    hasher.update(t.profile.as_bytes());
    hasher.update(format!("D={}", t.d).as_bytes());
    hasher.update(format!("N={}", t.n).as_bytes());
    let digest = hasher.finalize();
    hex::encode(&digest[..8])
}

/// Encode a placeholder POFP whose intensional payload is `proof_source` and
/// whose theoremName segment carries a target-derived fingerprint. Distinct
/// (target, proof) pairs always produce distinct bytes.
pub fn encode_placeholder_bppk(proof_source: &str, target: &Target) -> Vec<u8> {
    let fp = target_fingerprint(target);
    let mut b = BppkBuilder::new();
    b.push_bytes(&MAGIC);
    b.push_u32_le(FORMAT_VERSION);
    b.push_u32_le(0); // universeArity
    b.push_name(&["boole", "instance", &fp]);
    // theoremType = forallE (sort 0) (sort 0)
    b.push(expr_tag::FORALL_E);
    b.push(expr_tag::SORT).push(level_tag::ZERO);
    b.push(expr_tag::SORT).push(level_tag::ZERO);
    // proofExpr = lit (strVal proofSource)
    b.push(expr_tag::LIT).push(lit_tag::STR_VAL);
    b.push_string(proof_source);
    b.push_u32_le(0); // declCount
    b.build()
}

pub struct StructuralCanonicalizer;

impl Canonicalizer for StructuralCanonicalizer {
    fn canonicalize(&self, proof_source: &str, target: &Target) -> anyhow::Result<Vec<u8>> {
        Ok(encode_placeholder_bppk(proof_source, target))
    }
}
