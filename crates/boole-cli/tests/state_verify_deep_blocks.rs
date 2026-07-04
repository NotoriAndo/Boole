//! N3-pre.4 — wire `deep_verify_block` into `boole state verify --deep`.
//!
//! Until this slice, `--deep` only streamed the bounty audit ledger
//! (`deep_verify_bounty_events`); the block store's persisted Lean-bound
//! shares were never re-verified through the CLI (the only caller of
//! `deep_verify_block` was the `boole-node` integration test). An operator
//! running `--deep` against a tampered block store therefore got a clean
//! `ok=true` report even though a share's canon no longer matched its
//! re-derived Lean source.
//!
//! This test pins the fix: supplying `--blocks` alongside `--deep`
//! re-verifies every persisted block's Lean-bound shares (in addition to
//! the existing bounty ledger audit) and folds any divergence into the
//! same envelope / exit-3 contract the bounty path already uses.
//!
//! Block construction mirrors
//! `crates/boole-node/tests/deep_verify_block_roundtrip.rs`: a share whose
//! `proofPackage` is the real canon ground against the repo's canonical
//! Lean checker package (`lean/checker`), so these tests exercise the real
//! checker artifact hash and (when lake/lean are on PATH) the real Lean
//! re-elaboration path — no gating/`#[ignore]` needed since the canon
//! recompute is a pure file-hash check and the package carries no heavy
//! dependencies (matches the roundtrip test's convention).

use std::path::PathBuf;

use boole_core::{
    block_hash, family_v1_lenbound, lean_bound_canon_package, lean_bound_verifier_hash, share_hash,
    Hex32, PersistedBlock, SelectedShareEvidence,
};
use boole_node::FileBlockStore;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const PROFILE: &str = "v1-lenbound";
// Arbitrary 32-byte seed; generate_from_hex accepts any 32-byte hex.
const SEED_HEX: &str = "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
const PREV_C: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const PK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const N: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const J: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeepVerifyOutput {
    ok: bool,
    blocks_scanned: u64,
    lean_bound_shares: u64,
    canon_reverified: u64,
    shares_skipped: u64,
    divergences: Vec<serde_json::Value>,
}

fn canonical_checker_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../lean/checker")
        .canonicalize()
        .expect("canonical checker dir")
}

/// Build a block carrying one Lean-bound share whose `proofPackage` is the
/// canon the live miner would have ground for `SEED_HEX` against the
/// canonical checker. `tamper` flips the stored package so the negative
/// case can assert a divergence (mirrors
/// `deep_verify_block_roundtrip::block_with_lean_bound_share`).
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
    let c = block_hash(
        &Hex32::from_hex(PREV_C).unwrap(),
        &[Hex32::from_hex(&sh).unwrap()],
    )
    .to_hex();
    PersistedBlock {
        height: 0,
        prev_c: PREV_C.to_string(),
        c,
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
        promoted_bounty_credits: vec![],
        promoted_bounty_shares: vec![],
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-state-deep-blocks-{}-{}",
        std::process::id(),
        tag
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_empty_bounty_events(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("bounty-events.ndjson");
    std::fs::File::create(&path).expect("create empty bounty ledger");
    path
}

#[test]
fn state_verify_deep_reverifies_persisted_blocks_with_real_lean() {
    let dir = temp_dir("ok");
    let events_path = write_empty_bounty_events(&dir);
    let blocks_path = dir.join("blocks.ndjson");
    FileBlockStore::append(&blocks_path, &block_with_lean_bound_share(false))
        .expect("append block");
    let checker_dir = canonical_checker_dir();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "state",
            "verify",
            "--deep",
            "--bounty-events",
            events_path.to_str().expect("utf8 events path"),
            "--blocks",
            blocks_path.to_str().expect("utf8 blocks path"),
            "--lean-checker-dir",
            checker_dir.to_str().expect("utf8 checker dir"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert!(
        output.status.success(),
        "deep verify must succeed on an untampered block store; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: DeepVerifyOutput =
        serde_json::from_slice(&output.stdout).expect("json envelope on stdout");
    assert!(parsed.ok, "ok=true expected when the block store is clean");
    assert_eq!(
        parsed.blocks_scanned, 1,
        "--deep with --blocks must re-verify the persisted block, not just the bounty ledger"
    );
    assert_eq!(parsed.lean_bound_shares, 1);
    assert_eq!(
        parsed.canon_reverified, 1,
        "canon recomputed from the re-derived source must match the stored package"
    );
    assert_eq!(parsed.shares_skipped, 0);
    assert!(parsed.divergences.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn state_verify_deep_reports_divergence_on_tampered_canon() {
    let dir = temp_dir("tamper");
    let events_path = write_empty_bounty_events(&dir);
    let blocks_path = dir.join("blocks.ndjson");
    FileBlockStore::append(&blocks_path, &block_with_lean_bound_share(true)).expect("append block");
    let checker_dir = canonical_checker_dir();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "state",
            "verify",
            "--deep",
            "--bounty-events",
            events_path.to_str().expect("utf8 events path"),
            "--blocks",
            blocks_path.to_str().expect("utf8 blocks path"),
            "--lean-checker-dir",
            checker_dir.to_str().expect("utf8 checker dir"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert_eq!(
        output.status.code(),
        Some(3),
        "tampered canon on a persisted block must exit 3; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let envelope: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr envelope JSON");
    assert_eq!(envelope.get("ok"), Some(&serde_json::Value::Bool(false)));
    let divergences = envelope
        .get("divergences")
        .and_then(serde_json::Value::as_array)
        .expect("divergences array");
    assert_eq!(divergences.len(), 1, "exactly one divergence expected");
    assert_eq!(
        divergences[0]
            .get("field")
            .and_then(serde_json::Value::as_str),
        Some("proofPackage"),
        "tampered proofPackage must surface as the divergent field"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
