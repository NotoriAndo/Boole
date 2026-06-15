//! N0.4a — the shared LeanBound canon encoder lives in boole-core so both
//! the miner (LeanBoundCanonicalizer) and the node (deep_verify_block) can
//! produce/recompute the EXACT same canon bytes from the same evidence
//! (ADR-0007 (d): node-side re-derivation from persisted block data).

use boole_core::lean_bound_canon_package;
use sha2::{Digest, Sha256};

fn expected_digest(
    domain: &[u8],
    verifier_hash: &str,
    checker_hash: &str,
    lean_source: &str,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(domain);
    h.update(verifier_hash.as_bytes());
    h.update(checker_hash.as_bytes());
    h.update(b"\0lean-source\0");
    h.update(lean_source.as_bytes());
    h.finalize().into()
}

#[test]
fn lean_bound_canon_package_has_pinned_pofp_v2_layout() {
    let vh = "verifier-hash-x";
    let ch = "checker-artifact-hash-y";
    let src = "by\n  intro xs\n  calc";
    let pkg = lean_bound_canon_package(vh, ch, src);

    // 86-byte POFP-v2 package: magic + version + universeArity + theoremName
    // (0 segments) + (0x19 + 32B digest) x2 + declCount.
    assert_eq!(pkg.len(), 86, "package must be exactly 86 bytes");
    assert_eq!(&pkg[..4], b"POFP", "POFP magic");
    assert_eq!(
        u32::from_le_bytes(pkg[4..8].try_into().unwrap()),
        2,
        "format v2"
    );
    assert_eq!(
        u32::from_le_bytes(pkg[8..12].try_into().unwrap()),
        0,
        "universeArity"
    );
    assert_eq!(
        u32::from_le_bytes(pkg[12..16].try_into().unwrap()),
        0,
        "theoremName segs"
    );
    assert_eq!(pkg[16], 0x19, "type slot opaque-digest tag");
    assert_eq!(&pkg[17..49], &expected_digest(b"pofp-v2:type", vh, ch, src));
    assert_eq!(pkg[49], 0x19, "value slot opaque-digest tag");
    assert_eq!(
        &pkg[50..82],
        &expected_digest(b"pofp-v2:value", vh, ch, src)
    );
    assert_eq!(
        u32::from_le_bytes(pkg[82..86].try_into().unwrap()),
        0,
        "declCount"
    );
}

#[test]
fn lean_bound_canon_package_is_deterministic_and_input_sensitive() {
    let a = lean_bound_canon_package("vh", "ch", "src");
    assert_eq!(
        a,
        lean_bound_canon_package("vh", "ch", "src"),
        "deterministic"
    );
    assert_ne!(
        a,
        lean_bound_canon_package("vh2", "ch", "src"),
        "verifier hash matters"
    );
    assert_ne!(
        a,
        lean_bound_canon_package("vh", "ch2", "src"),
        "checker hash matters"
    );
    assert_ne!(
        a,
        lean_bound_canon_package("vh", "ch", "src2"),
        "lean source matters"
    );
}
