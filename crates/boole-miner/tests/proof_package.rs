use boole_miner::proof_package::{expr_tag, level_tag, lit_tag};
use boole_miner::{
    bppk_canon_hash, encode_placeholder_bppk, walk_bppk, BppkDecodeError, Canonicalizer,
    StructuralCanonicalizer, Target, FORMAT_VERSION, MAGIC,
};
use sha2::{Digest, Sha256};

fn target_a() -> Target {
    Target {
        seed_hex: "11".repeat(32),
        d: 3,
        profile: "v01".to_string(),
        n: 1,
        render: "[stub] target A".to_string(),
    }
}

fn target_b() -> Target {
    Target {
        seed_hex: "22".repeat(32),
        d: 3,
        profile: "v031".to_string(),
        n: 4,
        render: "[stub] target B".to_string(),
    }
}

#[test]
fn test_canonicalizer_emits_valid_bppk_walk_succeeds() {
    let cz = StructuralCanonicalizer;
    let bytes = cz
        .canonicalize("by trivial", &target_a())
        .expect("canonicalize");
    let w = walk_bppk(&bytes).expect("walk_bppk");
    assert_eq!(w.decl_count, 0);
    assert_eq!(w.universe_arity, 0);
    assert_eq!(w.size, bytes.len());
}

#[test]
fn test_canonicalizer_starts_with_magic_and_version() {
    let bytes = StructuralCanonicalizer
        .canonicalize("p", &target_a())
        .expect("canonicalize");
    assert_eq!(&bytes[..4], &MAGIC);
    let ver_le = &bytes[4..8];
    assert_eq!(u32::from_le_bytes(ver_le.try_into().unwrap()), FORMAT_VERSION);
}

#[test]
fn test_canonicalizer_distinct_proofs_yield_distinct_canon_hash() {
    let cz = StructuralCanonicalizer;
    let ha = bppk_canon_hash(&cz.canonicalize("by exact rfl", &target_a()).unwrap());
    let hb = bppk_canon_hash(&cz.canonicalize("by trivial", &target_a()).unwrap());
    assert_ne!(ha, hb);
}

#[test]
fn test_canonicalizer_distinct_targets_yield_distinct_canon_hash() {
    let cz = StructuralCanonicalizer;
    let ha = bppk_canon_hash(&cz.canonicalize("by trivial", &target_a()).unwrap());
    let hb = bppk_canon_hash(&cz.canonicalize("by trivial", &target_b()).unwrap());
    assert_ne!(ha, hb);
}

#[test]
fn test_canonicalizer_canon_hash_equals_sha256_of_bytes() {
    let bytes = StructuralCanonicalizer
        .canonicalize("by rfl", &target_a())
        .unwrap();
    let expected = Sha256::digest(&bytes);
    let got = bppk_canon_hash(&bytes);
    assert_eq!(got.as_bytes(), expected.as_slice());
}

#[test]
fn test_canonicalizer_is_deterministic_for_same_inputs() {
    let cz = StructuralCanonicalizer;
    let a = cz.canonicalize("the same proof", &target_a()).unwrap();
    let b = cz.canonicalize("the same proof", &target_a()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn test_canonicalizer_carries_proof_source_in_str_val_lit() {
    let proof = "exact List.nodup_dedup _";
    let bytes = encode_placeholder_bppk(proof, &target_a());
    let proof_bytes = proof.as_bytes();
    let found = bytes
        .windows(proof_bytes.len())
        .any(|w| w == proof_bytes);
    assert!(found, "proof source must appear verbatim inside the strVal lit");
}

#[test]
fn test_walk_bppk_rejects_bad_magic() {
    let bad = vec![0u8; 20];
    assert!(matches!(walk_bppk(&bad), Err(BppkDecodeError::BadMagic)));
}

#[test]
fn test_walk_bppk_rejects_unsupported_version() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&153u32.to_le_bytes()); // version=153
    bytes.extend_from_slice(&0u32.to_le_bytes()); // universeArity
    bytes.extend_from_slice(&0u32.to_le_bytes()); // theoremName parts=0
    bytes.push(expr_tag::SORT);
    bytes.push(level_tag::ZERO);
    bytes.push(expr_tag::LIT);
    bytes.push(lit_tag::NAT_VAL);
    bytes.extend_from_slice(&0u32.to_le_bytes()); // natVal payload
    bytes.extend_from_slice(&0u32.to_le_bytes()); // declCount=0
    let err = walk_bppk(&bytes).unwrap_err();
    assert!(matches!(err, BppkDecodeError::UnsupportedVersion(153)));
}

#[test]
fn test_walk_bppk_rejects_truncated_input() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&[0x01, 0x00, 0x00]); // missing 1 byte of version
    assert!(matches!(walk_bppk(&bytes), Err(BppkDecodeError::UnexpectedEof)));
}

#[test]
fn test_walk_bppk_rejects_deeply_nested_succ_with_recursion_limit() {
    const DEPTH: usize = 5000;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC);
    bytes.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes()); // universeArity
    bytes.extend_from_slice(&0u32.to_le_bytes()); // theoremName parts=0
    bytes.push(expr_tag::SORT); // theoremType: sort <level>
    bytes.extend(std::iter::repeat_n(level_tag::SUCC, DEPTH));
    bytes.push(level_tag::ZERO);
    bytes.push(expr_tag::LIT);
    bytes.push(lit_tag::NAT_VAL);
    bytes.extend_from_slice(&0u32.to_le_bytes()); // natVal payload
    bytes.extend_from_slice(&0u32.to_le_bytes()); // declCount=0
    let err = walk_bppk(&bytes).unwrap_err();
    assert!(matches!(
        err,
        BppkDecodeError::RecursionLimit { where_tag: "CanonLevel", .. }
    ));
}

#[test]
fn test_walk_bppk_rejects_trailing_bytes() {
    let mut bytes = encode_placeholder_bppk("by trivial", &target_a());
    bytes.push(0xff);
    let err = walk_bppk(&bytes).unwrap_err();
    assert!(matches!(err, BppkDecodeError::TrailingBytes { .. }));
}

#[test]
fn test_bppk_builder_round_trips_through_walker() {
    use boole_miner::BppkBuilder;
    let mut b = BppkBuilder::new();
    b.push_bytes(&MAGIC);
    b.push_u32_le(FORMAT_VERSION);
    b.push_u32_le(0); // universeArity
    b.push_name(&["a", "b"]);
    b.push(expr_tag::SORT).push(level_tag::ZERO);
    b.push(expr_tag::LIT)
        .push(lit_tag::STR_VAL)
        .push_string("hi");
    b.push_u32_le(0); // declCount
    let bytes = b.build();
    let w = walk_bppk(&bytes).expect("walk_bppk");
    assert_eq!(w.size, bytes.len());
    assert_eq!(w.decl_count, 0);
    assert_eq!(w.universe_arity, 0);
}
