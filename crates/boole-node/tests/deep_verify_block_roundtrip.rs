//! N0.4c — `deep_verify_block` round-trip: a live-mined Lean-bound share
//! persisted on a block re-derives its canonical Lean source from the
//! stored `seedHex` and recomputes the canon, which must match the stored
//! `proofPackage`. The canon recompute is a pure file-hash check (no Lean
//! toolchain); the optional Lean re-elaboration runs only when lake/lean
//! are available, so the assertions here hold on any host.

use std::path::PathBuf;

use boole_core::{
    block_hash, family_v1_lenbound, lean_bound_canon_package, lean_bound_verifier_hash, share_hash,
    Hex32, PersistedBlock, SelectedShareEvidence,
};
use boole_node::{deep_verify_block, FileBlockStore};
use sha2::{Digest, Sha256};

fn canonical_checker_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../lean/checker")
        .canonicalize()
        .expect("canonical checker dir")
}

const PROFILE: &str = "v1-lenbound";
// Arbitrary 32-byte seed; generate_from_hex accepts any 32-byte hex.
const SEED_HEX: &str = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
const PREV_C: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const PK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const N: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const J: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// Build a block carrying one Lean-bound share whose `proofPackage` is the
/// canon the live miner would have ground for `SEED_HEX` against the
/// canonical checker. `tamper` flips the stored package so the negative
/// case can assert a divergence.
fn block_with_lean_bound_share(tamper: bool) -> PersistedBlock {
    let checker_hash = boole_lean_runner::checker_artifact_hash(&canonical_checker_dir())
        .expect("checker artifact hash");
    let verifier_hash = lean_bound_verifier_hash(PROFILE);
    let instance = family_v1_lenbound::generate_from_hex(SEED_HEX).expect("instance from seed");
    let lean_source = family_v1_lenbound::render_canonical_proof(&instance);
    let canon = lean_bound_canon_package(&verifier_hash, &checker_hash, &lean_source);
    let mut proof_package = hex::encode(&canon);
    if tamper {
        // Flip the value-slot digest so the recompute will not match.
        let mut bytes = canon.clone();
        bytes[50] ^= 0xff;
        proof_package = hex::encode(bytes);
    }
    let canon_hash = hex::encode(Sha256::digest(&canon));
    let sh = share_hash(
        &Hex32::from_hex(PREV_C).unwrap(),
        &Hex32::from_hex(PK).unwrap(),
        &Hex32::from_hex(N).unwrap(),
        &Hex32::from_hex(J).unwrap(),
        &Hex32::from_hex(&canon_hash).unwrap(),
    )
    .to_hex();
    let mut block = PersistedBlock {
        height: 0,
        prev_c: PREV_C.to_string(),
        c: String::new(),
        proposer_pk: PK.to_string(),
        selected_share_hashes: vec![sh],
        selected_share_pks: vec![PK.to_string()],
        selected_share_reward_pks: vec![],
        proposer_reward_pk: String::new(),
        selected_share_evidence: vec![SelectedShareEvidence {
            pk: PK.to_string(),
            n: N.to_string(),
            j: J.to_string(),
            c: PREV_C.to_string(),
            canon_hash,
            proof_package,
            seed_hex: SEED_HEX.to_string(),
            signed_work: None,
        }],
        min_share_score: "1".to_string(),
        min_share_score_multiplier_nanos: 1_000_000_000,
        kmax_applied: 1,
        difficulty_epoch: 0,
        t_block: "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        t_share: "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
        difficulty_weight: "1".to_string(),
        dropped_below_min_score: 0,
        dropped_kernel_reject: 0,
        truncated_by_kmax: 0,
        ts: 1_700_000_000_000,
        promoted_bounty_shares: vec![],
    };
    block.c = block_hash(&block).to_hex();
    block
}

fn temp_block_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-deep-verify-block-test-{}-{}",
        std::process::id(),
        tag
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("blocks.ndjson")
}

#[test]
fn live_mined_lean_bound_share_canon_reverifies_against_re_derived_source() {
    let path = temp_block_path("ok");
    FileBlockStore::append(&path, &block_with_lean_bound_share(false)).expect("append block");

    let report = deep_verify_block(&path, Some(canonical_checker_dir().as_path()), PROFILE)
        .expect("deep verify runs");

    assert_eq!(report.blocks_scanned, 1);
    assert_eq!(report.lean_bound_shares, 1, "one seedHex-carrying share");
    assert_eq!(
        report.canon_reverified, 1,
        "canon recomputed from the re-derived source must match the stored package"
    );
    assert_eq!(report.shares_skipped, 0);
    assert!(
        report.divergences.is_empty(),
        "no divergence expected: {:?}",
        report.divergences
    );

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn tampered_proof_package_is_reported_as_divergence() {
    let path = temp_block_path("tamper");
    FileBlockStore::append(&path, &block_with_lean_bound_share(true)).expect("append block");

    let report = deep_verify_block(&path, Some(canonical_checker_dir().as_path()), PROFILE)
        .expect("deep verify runs");

    assert_eq!(
        report.canon_reverified, 0,
        "tampered canon must not reverify"
    );
    assert_eq!(report.divergences.len(), 1, "one divergence expected");
    assert_eq!(report.divergences[0].field, "proofPackage");

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn shares_without_checker_dir_are_skipped() {
    let path = temp_block_path("skip");
    FileBlockStore::append(&path, &block_with_lean_bound_share(false)).expect("append block");

    let report = deep_verify_block(&path, None, PROFILE).expect("deep verify runs");

    assert_eq!(report.lean_bound_shares, 1);
    assert_eq!(
        report.shares_skipped, 1,
        "without a checker dir the canon cannot be recomputed"
    );
    assert_eq!(report.canon_reverified, 0);
    assert!(report.divergences.is_empty());

    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
