//! Seed↔prev-block binding at replay (pre-mortem audit 2026-07-05) — a
//! block's persisted `seedHex` is the protocol's claim that the chain posed
//! this share's problem. Replay must therefore re-derive it: a non-empty
//! `seedHex` has to equal `target_seed(c, pk, n, j_index)` for some
//! `j_index < TARGET_SEED_J_INDEX_BOUND`, where `c` is the block's
//! `prev_c`. Before this slice `verify_selected_share_evidence` never read
//! `seed_hex`, so a chain whose shares solved miner-chosen problems still
//! replayed. Empty `seedHex` stays accepted (pre-N0.4b legacy posture;
//! mandatory seeds are N3.3 scope).

use boole_core::{
    block_hash, replay_blocks, share_hash, target_seed, Hex32, PersistedBlock,
    SelectedShareEvidence, TARGET_SEED_J_INDEX_BOUND,
};
use sha2::{Digest, Sha256};

const PREV_C: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const T_MAX: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

const PK: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const N: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const J: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn derived_seed(j_index: u32) -> String {
    target_seed(
        &Hex32::from_hex(PREV_C).expect("prev c hex32"),
        &Hex32::from_hex(PK).expect("pk hex32"),
        &Hex32::from_hex(N).expect("n hex32"),
        j_index,
    )
    .to_hex()
}

/// Builds a valid, evidence-backed selected share for `(PK, N, J)` carrying
/// `seed_hex`, and returns its evidence row plus its `shareHash`.
fn share_with_seed(seed_hex: &str) -> (SelectedShareEvidence, String) {
    let proof_package = valid_pofp_v2_package_hex();
    let proof_package_bytes = hex::decode(&proof_package).expect("valid proof package hex");
    let canon_hash = hex::encode(Sha256::digest(&proof_package_bytes));
    let hash = share_hash(
        &Hex32::from_hex(PREV_C).expect("prev c hex32"),
        &Hex32::from_hex(PK).expect("pk hex32"),
        &Hex32::from_hex(N).expect("n hex32"),
        &Hex32::from_hex(J).expect("j hex32"),
        &Hex32::from_hex(&canon_hash).expect("canon hash hex32"),
    )
    .to_hex();

    (
        SelectedShareEvidence {
            pk: PK.to_string(),
            n: N.to_string(),
            j: J.to_string(),
            c: PREV_C.to_string(),
            canon_hash,
            proof_package,
            seed_hex: seed_hex.to_string(),
        },
        hash,
    )
}

fn block_with_share(evidence: SelectedShareEvidence, hash: String) -> PersistedBlock {
    let mut block = PersistedBlock {
        height: 0,
        prev_c: PREV_C.to_string(),
        c: String::new(),
        proposer_pk: PK.to_string(),
        selected_share_hashes: vec![hash],
        selected_share_pks: vec![PK.to_string()],
        selected_share_reward_pks: vec![],
        proposer_reward_pk: String::new(),
        selected_share_evidence: vec![evidence],
        min_share_score: "1".to_string(),
        min_share_score_multiplier_nanos: 1_000_000_000,
        kmax_applied: 1,
        difficulty_epoch: 0,
        t_block: T_MAX.to_string(),
        t_share: T_MAX.to_string(),
        difficulty_weight: "1".to_string(),
        dropped_below_min_score: 0,
        dropped_kernel_reject: 0,
        truncated_by_kmax: 0,
        ts: 1_700_000_000_000,
        promoted_bounty_credits: vec![],
        promoted_bounty_shares: vec![],
    };
    block.c = block_hash(&block).to_hex();
    block
}

#[test]
fn replay_accepts_evidence_with_empty_seed_hex() {
    let (evidence, hash) = share_with_seed("");
    replay_blocks(&[block_with_share(evidence, hash)])
        .expect("empty seedHex is the legacy posture and must stay replayable");
}

#[test]
fn replay_accepts_evidence_with_chain_derived_seed_hex() {
    let (evidence, hash) = share_with_seed(&derived_seed(0));
    replay_blocks(&[block_with_share(evidence, hash)])
        .expect("seedHex derived as target_seed(prev_c, pk, n, 0) must replay");
}

#[test]
fn replay_rejects_evidence_with_unbound_seed_hex() {
    let (evidence, hash) = share_with_seed(&"66".repeat(32));
    let err = replay_blocks(&[block_with_share(evidence, hash)]).expect_err(
        "an arbitrary persisted seedHex must fail replay — the miner chose its own problem",
    );
    assert!(
        err.to_string().to_lowercase().contains("seed"),
        "error should name the seed binding violation: {err}"
    );
}

#[test]
fn replay_rejects_seed_derived_beyond_j_index_bound() {
    let (evidence, hash) = share_with_seed(&derived_seed(TARGET_SEED_J_INDEX_BOUND));
    let err = replay_blocks(&[block_with_share(evidence, hash)])
        .expect_err("a seed derived at j_index == bound was never posed and must fail replay");
    assert!(
        err.to_string().to_lowercase().contains("seed"),
        "error should name the seed binding violation: {err}"
    );
}

fn valid_pofp_v2_package_hex() -> String {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"POFP");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0x19);
    bytes.extend_from_slice(&[0x11; 32]);
    bytes.push(0x19);
    bytes.extend_from_slice(&[0x22; 32]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    hex::encode(bytes)
}
