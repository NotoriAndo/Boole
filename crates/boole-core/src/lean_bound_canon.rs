//! N0.4a — the shared LeanBound canonical-package encoder.
//!
//! This is the single source of truth for the bytes a Lean-bound mined
//! share commits: the miner's `LeanBoundCanonicalizer` produces them, and
//! the node's `deep_verify_block` recomputes them from the persisted block
//! evidence (ADR-0007 (d)). It lives in `boole-core` because the node must
//! not depend on `boole-miner`; core is the consensus-primitive home.
//!
//! The package is the POFP-v2 opaque-digest shape (ADR-0001): two
//! domain-separated 256-bit digest slots. Each digest binds only data that
//! is reproducible offline from a persisted block — `verifier_hash`,
//! `checker_artifact_hash`, and the rendered canonical `lean_source` — and
//! deliberately NOT runtime values (lean/lake version, stdout), so a node
//! can recompute the identical bytes without re-running the exact toolchain
//! build that first produced them.

use sha2::{Digest, Sha256};

const POFP_MAGIC: &[u8; 4] = b"POFP";
const FORMAT_VERSION_V2: u32 = 2;
/// POFP-v2 opaque-digest expression tag (ADR-0001).
const EXPR_TAG_OPAQUE_DIGEST: u8 = 0x19;

fn stable_digest(
    domain: &[u8],
    verifier_hash: &str,
    checker_artifact_hash: &str,
    lean_source: &str,
) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(verifier_hash.as_bytes());
    hasher.update(checker_artifact_hash.as_bytes());
    hasher.update(b"\0lean-source\0");
    hasher.update(lean_source.as_bytes());
    hasher.finalize().into()
}

/// The `verifier_hash` identity a Lean-bound share binds, derived solely
/// from the family profile. Shared so the miner (live canonicalizer) and
/// the node (`deep_verify_block`) agree on the exact string without the
/// node having to know the miner's naming convention out-of-band — both
/// call this one function (N0.4c).
pub fn lean_bound_verifier_hash(profile: &str) -> String {
    format!("boole-miner-verifier:{profile}")
}

/// Build the canonical POFP-v2 package binding a rendered Lean proof to its
/// checker evidence. Output is deterministic and exactly 86 bytes.
pub fn lean_bound_canon_package(
    verifier_hash: &str,
    checker_artifact_hash: &str,
    lean_source: &str,
) -> Vec<u8> {
    let mut package = Vec::with_capacity(86);
    package.extend_from_slice(POFP_MAGIC);
    package.extend_from_slice(&FORMAT_VERSION_V2.to_le_bytes());
    package.extend_from_slice(&0u32.to_le_bytes()); // universeArity
    package.extend_from_slice(&0u32.to_le_bytes()); // theoremName: zero segments
    package.push(EXPR_TAG_OPAQUE_DIGEST);
    package.extend_from_slice(&stable_digest(
        b"pofp-v2:type",
        verifier_hash,
        checker_artifact_hash,
        lean_source,
    ));
    package.push(EXPR_TAG_OPAQUE_DIGEST);
    package.extend_from_slice(&stable_digest(
        b"pofp-v2:value",
        verifier_hash,
        checker_artifact_hash,
        lean_source,
    ));
    package.extend_from_slice(&0u32.to_le_bytes()); // declCount
    package
}
