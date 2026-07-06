//! N3-pre.2 (L1 fitness review #2) — replay/verify must independently
//! re-derive the block-selection canonical share order, T_block
//! satisfaction and unique-proposer identity from block contents alone.
//! Before this slice, `replay_blocks` only checked each selected share's
//! own evidence identity (N3-pre.1) — it never checked that the shares
//! were persisted in `compare_canonical` order, nor that exactly one of
//! them satisfies T_block. A block could reverse the canonical order, or
//! attach evidence for two shares that both satisfy T_block (an
//! ambiguous proposer), and replay would still accept it.
//!
//! Pool-global optimality ("was this really the pool's top-k") stays
//! unverifiable from a single block's contents alone and is an explicit
//! non-goal of this slice.

use boole_core::{
    block_hash, replay_blocks, share_hash, Hex32, PersistedBlock, SelectedShareEvidence,
};
use sha2::{Digest, Sha256};

const PREV_C: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const T_BLOCK_MAX: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const T_SHARE_MAX: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

const PK_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const PK_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const N_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const N_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const J_A: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const J_B: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

/// Builds a valid, evidence-backed selected share for `(pk, n, j)` and
/// returns its evidence row plus its `shareHash`. PK_A/N_A/J_A sorts
/// before PK_B/N_B/J_B under `compare_canonical` (pk is the primary key
/// and `PK_A < PK_B`).
fn share(pk: &str, n: &str, j: &str, package_fill: u8) -> (SelectedShareEvidence, String) {
    let proof_package = valid_pofp_v2_package_hex(package_fill);
    let proof_package_bytes = hex::decode(&proof_package).expect("valid proof package hex");
    let canon_hash = hex::encode(Sha256::digest(&proof_package_bytes));
    let hash = share_hash(
        &Hex32::from_hex(PREV_C).expect("prev c hex32"),
        &Hex32::from_hex(pk).expect("pk hex32"),
        &Hex32::from_hex(n).expect("n hex32"),
        &Hex32::from_hex(j).expect("j hex32"),
        &Hex32::from_hex(&canon_hash).expect("canon hash hex32"),
    )
    .to_hex();

    (
        SelectedShareEvidence {
            pk: pk.to_string(),
            n: n.to_string(),
            j: j.to_string(),
            c: PREV_C.to_string(),
            canon_hash,
            proof_package,
            seed_hex: String::new(),
        },
        hash,
    )
}

fn block_with_shares(
    evidence: Vec<SelectedShareEvidence>,
    pks: Vec<String>,
    hashes: Vec<String>,
    proposer_pk: &str,
) -> PersistedBlock {
    let hex_hashes = hashes
        .iter()
        .map(|h| Hex32::from_hex(h).expect("share hash hex32"))
        .collect::<Vec<_>>();
    let c = block_hash(&Hex32::from_hex(PREV_C).expect("prev c hex32"), &hex_hashes).to_hex();
    let kmax_applied = hashes.len() as u64;

    PersistedBlock {
        height: 0,
        prev_c: PREV_C.to_string(),
        c,
        proposer_pk: proposer_pk.to_string(),
        selected_share_hashes: hashes,
        selected_share_pks: pks,
        selected_share_reward_pks: vec![],
        proposer_reward_pk: String::new(),
        selected_share_evidence: evidence,
        min_share_score: "1".to_string(),
        min_share_score_multiplier_nanos: 1_000_000_000,
        kmax_applied,
        difficulty_epoch: 0,
        t_block: T_BLOCK_MAX.to_string(),
        t_share: T_SHARE_MAX.to_string(),
        difficulty_weight: "1".to_string(),
        dropped_below_min_score: 0,
        dropped_kernel_reject: 0,
        truncated_by_kmax: 0,
        ts: 1_700_000_000_000,
        promoted_bounty_credits: vec![],
        promoted_bounty_shares: vec![],
    }
}

#[test]
fn replay_rejects_block_with_non_canonical_share_ordering() {
    let (evidence_a, hash_a) = share(PK_A, N_A, J_A, 0x11);
    let (evidence_b, hash_b) = share(PK_B, N_B, J_B, 0x33);

    // PK_B sorts after PK_A under compare_canonical, so persisting B
    // before A reverses the required canonical order.
    let block = block_with_shares(
        vec![evidence_b, evidence_a],
        vec![PK_B.to_string(), PK_A.to_string()],
        vec![hash_b, hash_a],
        PK_B,
    );

    let err = replay_blocks(&[block])
        .expect_err("replay must reject a block whose share evidence is out of canonical order");
    assert!(
        err.to_string().to_lowercase().contains("canonical order"),
        "error should name the canonical ordering violation: {err}"
    );
}

// N3-pre.6 (external review A-g1, critical) — before this slice,
// `build_block_selection` refused to build a block at all once two
// shares co-qualified as proposer (`AmbiguousProposer`), so a block with
// two T_block-satisfying selected shares could only reach replay via a
// malicious/malformed input and replay correctly rejected it. Now the
// builder resolves such ties deterministically instead of halting, so a
// legitimately-committed block can carry two co-qualifying shares.
// Replay must accept that same block on reboot/recovery — rejecting it
// would mean a node can commit a block it can never replay again.
#[test]
fn replay_accepts_block_with_co_qualifying_shares_via_tie_break() {
    let (evidence_a, hash_a) = share(PK_A, N_A, J_A, 0x11);
    let (evidence_b, hash_b) = share(PK_B, N_B, J_B, 0x33);

    // Canonical order is correct here (A before B); t_block = max means
    // both selected shares satisfy T_block — the same co-qualifying
    // scenario `build_block_selection` now tie-breaks instead of halting.
    let block = block_with_shares(
        vec![evidence_a, evidence_b],
        vec![PK_A.to_string(), PK_B.to_string()],
        vec![hash_a, hash_b],
        PK_A,
    );

    replay_blocks(&[block]).expect(
        "replay must accept a block where more than one selected share satisfies T_block, \
         mirroring build_block_selection's deterministic tie-break",
    );
}

/// N4-pre.1 — parameterized fill so two shares in one block carry two
/// DISTINCT proofs (the consensus dedup rule forbids one proof credited
/// twice; these tests are about ordering/tie-breaks, not dedup).
fn valid_pofp_v2_package_hex(fill: u8) -> String {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"POFP");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0x19);
    bytes.extend_from_slice(&[fill; 32]);
    bytes.push(0x19);
    bytes.extend_from_slice(&[0x22; 32]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    hex::encode(bytes)
}
