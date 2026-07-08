//! N4-pre.1 (ADR-0012) — consensus-level proof dedup: a selected share's
//! canon_hash must not appear in any earlier block of the chain, nor twice
//! within one block. One proof yields at most one credit, ever — enforced
//! by replay from block data alone (invariant 5 "합의 강제"), not by any
//! node-local ledger file. The N2.3 `/submit` dedup ledger is demoted to
//! an admission early-reject cache by this rule.
//!
//! Key = the canon_hash `verify_selected_share_evidence` re-derives
//! (SHA-256 over the canonical package bytes) — cross-pk by construction:
//! the same proof bytes under a different (pk, n, j) is still the same
//! canon_hash. Legacy evidence-less blocks (reachable only via the
//! explicit `LegacyEvidenceOptIn`) carry no canon_hash and stay outside
//! the rule (ADR-0012 (c)).

use boole_core::{
    block_hash, build_block_selection, replay_blocks, share_hash, BlockBuilderConfig,
    BuildSelectionResult, CandidateShare, Hex32, PersistedBlock, SelectedShareEvidence,
};
use num_bigint::BigUint;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const T_BLOCK_MAX: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const T_SHARE_MAX: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

const PK_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const PK_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const N_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const N_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const J_A: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const J_B: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

/// A shape-valid POFP v2 package whose opaque digests are filled with
/// `fill` — two different fills yield two different canonical byte
/// strings, hence two different canon_hashes.
fn pofp_v2_package_hex(fill: u8) -> String {
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

fn canon_hash_of(package_hex: &str) -> String {
    let bytes = hex::decode(package_hex).expect("valid package hex");
    hex::encode(Sha256::digest(&bytes))
}

/// Evidence row + share hash for `(pk, n, j)` bound to `prev_c`, carrying
/// the given proof package.
fn share_at(
    prev_c: &str,
    pk: &str,
    n: &str,
    j: &str,
    package_hex: &str,
) -> (SelectedShareEvidence, String) {
    let canon_hash = canon_hash_of(package_hex);
    let hash = share_hash(
        &Hex32::from_hex(prev_c).expect("prev c hex32"),
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
            c: prev_c.to_string(),
            canon_hash,
            proof_package: package_hex.to_string(),
            seed_hex: String::new(),
        },
        hash,
    )
}

fn block_at(
    height: u64,
    prev_c: &str,
    ts: u64,
    shares: Vec<(SelectedShareEvidence, String)>,
) -> PersistedBlock {
    let evidence: Vec<SelectedShareEvidence> = shares.iter().map(|(e, _)| e.clone()).collect();
    let pks: Vec<String> = evidence.iter().map(|e| e.pk.clone()).collect();
    let hashes: Vec<String> = shares.iter().map(|(_, h)| h.clone()).collect();
    let proposer_pk = pks[0].clone();
    let kmax_applied = hashes.len() as u64;
    let mut block = PersistedBlock {
        height,
        prev_c: prev_c.to_string(),
        c: String::new(),
        proposer_pk,
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
        ts,
        promoted_bounty_credits: vec![],
        promoted_bounty_shares: vec![],
    };
    block.c = block_hash(&block).to_hex();
    block
}

#[test]
fn replay_rejects_chain_with_duplicate_canon_hash_across_blocks() {
    // The SAME proof bytes credited in block 0 come back in block 1 under
    // a different (pk, n, j) — the cross-pk re-credit N2.3 only blocked
    // with a local ledger file. Replay must now reject the chain itself.
    let package = pofp_v2_package_hex(0x11);
    let block0 = block_at(
        0,
        GENESIS,
        1_700_000_000_000,
        vec![share_at(GENESIS, PK_A, N_A, J_A, &package)],
    );
    let block1 = block_at(
        1,
        &block0.c.clone(),
        1_700_000_060_000,
        vec![share_at(&block0.c, PK_B, N_B, J_B, &package)],
    );

    let err = replay_blocks(&[block0, block1])
        .expect_err("replay must reject a chain that credits one proof twice");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("canon_hash") && msg.contains("credit"),
        "error should name the duplicate canon_hash credit: {err}"
    );
}

#[test]
fn replay_rejects_block_with_duplicate_canon_hash_within_block() {
    // Two shares in ONE block carrying the same proof bytes under two
    // different (pk, n, j) identities — same double-credit, one block.
    let package = pofp_v2_package_hex(0x33);
    let block = block_at(
        0,
        GENESIS,
        1_700_000_000_000,
        vec![
            share_at(GENESIS, PK_A, N_A, J_A, &package),
            share_at(GENESIS, PK_B, N_B, J_B, &package),
        ],
    );

    let err = replay_blocks(&[block])
        .expect_err("replay must reject a block that credits one proof twice");
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("canon_hash") && msg.contains("credit"),
        "error should name the duplicate canon_hash credit: {err}"
    );
}

#[test]
fn replay_accepts_chain_of_distinct_proofs() {
    // Over-rejection guard: two blocks crediting two DIFFERENT proofs must
    // stay valid — the rule keys on the proof bytes, not on pk or height.
    let block0 = block_at(
        0,
        GENESIS,
        1_700_000_000_000,
        vec![share_at(
            GENESIS,
            PK_A,
            N_A,
            J_A,
            &pofp_v2_package_hex(0x55),
        )],
    );
    let block1 = block_at(
        1,
        &block0.c.clone(),
        1_700_000_060_000,
        vec![share_at(
            &block0.c,
            PK_B,
            N_B,
            J_B,
            &pofp_v2_package_hex(0x66),
        )],
    );
    replay_blocks(&[block0, block1]).expect("distinct proofs across blocks must replay clean");
}

#[test]
fn block_builder_excludes_already_credited_canon_hash() {
    // Proposer-side mirror of the replay rule (ADR-0012 (b)): an honest
    // node must never BUILD a block that replay would reject. A pool share
    // whose canon_hash is already credited on the chain is dropped before
    // selection; a fresh proof in the same pool still proposes.
    let credited_package = pofp_v2_package_hex(0x77);
    let fresh_package = pofp_v2_package_hex(0x88);
    let (credited_evidence, credited_hash) = share_at(GENESIS, PK_A, N_A, J_A, &credited_package);
    let (fresh_evidence, fresh_hash) = share_at(GENESIS, PK_B, N_B, J_B, &fresh_package);

    let candidate = |evidence: &SelectedShareEvidence, hash: &str| CandidateShare {
        label: String::new(),
        pk: evidence.pk.clone(),
        reward_pk: evidence.pk.clone(),
        n: evidence.n.clone(),
        j: evidence.j.clone(),
        c: GENESIS.to_string(),
        share_hash: hash.to_string(),
        score: "1".to_string(),
        canon_tag: 0,
        canon_hash: evidence.canon_hash.clone(),
        proof_package: evidence.proof_package.clone(),
        seed_hex: String::new(),
    };
    let shares = vec![
        candidate(&credited_evidence, &credited_hash),
        candidate(&fresh_evidence, &fresh_hash),
    ];
    let cfg = BlockBuilderConfig {
        t_block: T_BLOCK_MAX.to_string(),
        t_share: T_SHARE_MAX.to_string(),
        min_share_score: BigUint::from(1u8),
        min_share_score_multiplier_nanos: 1_000_000_000,
        k_max: 4,
        difficulty_epoch: 0,
        difficulty_weight: "1".to_string(),
    };
    let accepted_tags = BTreeSet::from([0u8]);
    let credited: BTreeSet<String> = BTreeSet::from([credited_evidence.canon_hash.clone()]);

    let selection =
        build_block_selection(GENESIS, &shares, &cfg, &accepted_tags, &credited, &[], &[])
            .expect("selection must not error");
    match selection {
        BuildSelectionResult::Ok(built) => {
            assert_eq!(
                built.selected.len(),
                1,
                "only the fresh proof may be selected"
            );
            assert_eq!(
                built.selected[0].canon_hash, fresh_evidence.canon_hash,
                "the already-credited share must be excluded from selection"
            );
        }
        other => panic!("expected a block from the fresh share, got {other:?}"),
    }
}
