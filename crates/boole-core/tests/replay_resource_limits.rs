//! SC.10-ii-d (ADR-0016 (c-2), audit C-09) — admission and replay must
//! enforce the SAME committed proof-resource limits.
//!
//! Live admission rejects an oversized or over-declared base-lane proof
//! package via `validate_proof_package_with_policy` (the closed-local base
//! lane commits `L = 16384`, `D_max = 1024` — see
//! `fixtures/protocol/config/v1.json`). Replay/re-verify, however, only
//! checked the package *shape* (`validate_proof_package_shape`) and never
//! applied those byte/decl ceilings, so a peer chain or bootstrap snapshot
//! could smuggle a package past re-verification that admission would have
//! refused (disk/CPU/memory exhaustion, admission-parity break). This RED
//! pins that the replay path enforces the same committed base-lane
//! resource limits, drawn from the rule constants
//! `BASE_LANE_MAX_PROOF_BYTES` / `BASE_LANE_MAX_DECLS`.

use boole_core::{
    block_hash, replay_blocks, share_hash, Hex32, PersistedBlock, SelectedShareEvidence,
};
use sha2::{Digest, Sha256};

const PREV_C: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const T_BLOCK_MAX: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const T_SHARE_MAX: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

const PK_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const N_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const J_A: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

// The committed base-lane ceilings (rules.rs `BASE_LANE_MAX_PROOF_BYTES` /
// `BASE_LANE_MAX_DECLS`), mirrored here as the values admission already
// enforces via the base-lane calibration `L` / `D_max`.
const BASE_LANE_MAX_PROOF_BYTES: usize = 16_384;
const BASE_LANE_MAX_DECLS: usize = 1_024;

/// Builds a shape-valid POFP v2 canonical package with `decl_count` trivial
/// declarations and a top-level name padded by `name_pad_len` bytes. Both
/// knobs let a test grow the package past exactly one committed ceiling
/// while staying under the other and remaining structurally decodable
/// (`walk_package` accepts it), so the ONLY thing that can reject it is the
/// resource-limit check under test.
fn pofp_v2_package(decl_count: u32, name_pad_len: usize) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"POFP");
    b.extend_from_slice(&2u32.to_le_bytes()); // format version 2
    b.extend_from_slice(&0u32.to_le_bytes()); // universe arity
                                              // package-level name: pad via one length-prefixed part when requested.
    if name_pad_len == 0 {
        b.extend_from_slice(&0u32.to_le_bytes()); // 0 name parts
    } else {
        b.extend_from_slice(&1u32.to_le_bytes()); // 1 name part
        b.extend_from_slice(&(name_pad_len as u32).to_le_bytes());
        b.extend(std::iter::repeat_n(0u8, name_pad_len));
    }
    // two opaque-digest exprs (v2 statement/proof digests)
    b.push(0x19);
    b.extend_from_slice(&[0x11; 32]);
    b.push(0x19);
    b.extend_from_slice(&[0x22; 32]);
    // declarations
    b.extend_from_slice(&decl_count.to_le_bytes());
    for _ in 0..decl_count {
        b.extend_from_slice(&0u32.to_le_bytes()); // decl name: 0 parts
        b.push(0x10); // expr: BVar-style, u32 payload
        b.extend_from_slice(&0u32.to_le_bytes());
        b.push(0x10);
        b.extend_from_slice(&0u32.to_le_bytes());
    }
    b
}

/// One evidence-backed base-lane selected share carrying `package`.
fn share_carrying(package: &[u8]) -> (SelectedShareEvidence, String) {
    let canon_hash = hex::encode(Sha256::digest(package));
    let hash = share_hash(
        &Hex32::from_hex(PREV_C).expect("prev c hex32"),
        &Hex32::from_hex(PK_A).expect("pk hex32"),
        &Hex32::from_hex(N_A).expect("n hex32"),
        &Hex32::from_hex(J_A).expect("j hex32"),
        &Hex32::from_hex(&canon_hash).expect("canon hash hex32"),
    )
    .to_hex();
    (
        SelectedShareEvidence {
            pk: PK_A.to_string(),
            n: N_A.to_string(),
            j: J_A.to_string(),
            c: PREV_C.to_string(),
            canon_hash,
            proof_package: hex::encode(package),
            seed_hex: String::new(),
            signed_work: None,
        },
        hash,
    )
}

fn single_share_block(evidence: SelectedShareEvidence, hash: String) -> PersistedBlock {
    let mut block = PersistedBlock {
        height: 0,
        prev_c: PREV_C.to_string(),
        c: String::new(),
        proposer_pk: PK_A.to_string(),
        selected_share_hashes: vec![hash],
        selected_share_pks: vec![PK_A.to_string()],
        selected_share_reward_pks: vec![],
        proposer_reward_pk: String::new(),
        selected_share_evidence: vec![evidence],
        min_share_score: "1".to_string(),
        min_share_score_multiplier_nanos: 1_000_000_000,
        kmax_applied: 1,
        difficulty_epoch: 0,
        t_block: T_BLOCK_MAX.to_string(),
        t_share: T_SHARE_MAX.to_string(),
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

/// Sanity: a small, well-formed package under both ceilings replays cleanly,
/// so the rejections below isolate the resource-limit check, not some
/// unrelated block-validity failure.
#[test]
fn replay_accepts_share_within_committed_resource_limits() {
    let package = pofp_v2_package(1, 0);
    assert!(package.len() <= BASE_LANE_MAX_PROOF_BYTES);
    let (evidence, hash) = share_carrying(&package);
    let block = single_share_block(evidence, hash);
    replay_blocks(std::slice::from_ref(&block))
        .expect("a package under the committed base-lane limits must replay");
}

#[test]
fn admission_and_replay_share_proof_resource_limits() {
    // Part A — byte ceiling: a shape-valid package one byte over
    // BASE_LANE_MAX_PROOF_BYTES, well under the decl ceiling.
    let oversized = pofp_v2_package(0, BASE_LANE_MAX_PROOF_BYTES + 1 - 90);
    assert_eq!(
        oversized.len(),
        BASE_LANE_MAX_PROOF_BYTES + 1,
        "test package must sit exactly one byte over the committed byte ceiling"
    );
    let (evidence, hash) = share_carrying(&oversized);
    let block = single_share_block(evidence, hash);
    let err = replay_blocks(std::slice::from_ref(&block)).expect_err(
        "replay must reject a selected share whose proof package exceeds the committed \
         base-lane byte ceiling, matching admission",
    );
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("large") || msg.contains("byte") || msg.contains("limit"),
        "byte-ceiling rejection should name the resource limit: {err}"
    );

    // Part B — declaration ceiling: a shape-valid package with one decl over
    // BASE_LANE_MAX_DECLS, well under the byte ceiling.
    let over_decls = pofp_v2_package((BASE_LANE_MAX_DECLS + 1) as u32, 0);
    assert!(
        over_decls.len() <= BASE_LANE_MAX_PROOF_BYTES,
        "decl-ceiling package must stay under the byte ceiling to isolate the decl check"
    );
    let (evidence, hash) = share_carrying(&over_decls);
    let block = single_share_block(evidence, hash);
    let err = replay_blocks(std::slice::from_ref(&block)).expect_err(
        "replay must reject a selected share whose proof package exceeds the committed \
         base-lane declaration ceiling, matching admission",
    );
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("decl") || msg.contains("limit"),
        "decl-ceiling rejection should name the resource limit: {err}"
    );
}
