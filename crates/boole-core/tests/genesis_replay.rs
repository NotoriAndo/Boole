//! N5.1b (ADR-0014 (c)/(d)) — genesis-aware replay: the `GenesisSpec` is
//! the consensus source replay validates against, not per-node scenario
//! config. Pinned here: the chain anchor (block 0 must link to the spec's
//! `genesis_c` — previously a hardcoded all-zeros assumption), the `k_max`
//! cap (previously only self-consistency, never an enforced limit), static
//! difficulty (`retarget: None` ⇒ every block at the spec's initial
//! `t_block`), and `seed_binding_required` (empty `seedHex` rejected from
//! height 0 — the ADR-0014 (d) shared-testnet posture).

use boole_core::{
    block_hash, replay_blocks, replay_blocks_with_genesis, share_hash, GenesisInitialState,
    GenesisParams, GenesisSpec, Hex32, PersistedBlock, SelectedShareEvidence,
    CONSENSUS_RULE_VERSION,
};
use sha2::{Digest, Sha256};

const ZEROS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const ANCHOR: &str = "9999999999999999999999999999999999999999999999999999999999999999";
const T_MAX: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const T_EASED: &str = "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe";

const PK_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const PK_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const N_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const J_A: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const J_B: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn spec(anchor: &str, k_max: u64, seed_required: bool) -> GenesisSpec {
    GenesisSpec {
        network_id: "boole-dev".to_string(),
        params: GenesisParams {
            consensus_rule_version: CONSENSUS_RULE_VERSION,
            t_block: T_MAX.to_string(),
            t_share: T_MAX.to_string(),
            k_max,
            retarget: None,
            seed_binding_required: seed_required,
            checker_artifact_hash: None,
            family_manifest_root: None,
        },
        initial_state: GenesisInitialState {
            genesis_c: anchor.to_string(),
        },
    }
}

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

fn share_at(prev_c: &str, pk: &str, j: &str, fill: u8) -> (SelectedShareEvidence, String) {
    let package_hex = pofp_v2_package_hex(fill);
    let canon_hash = {
        let bytes = hex::decode(&package_hex).expect("valid package hex");
        hex::encode(Sha256::digest(&bytes))
    };
    let hash = share_hash(
        &Hex32::from_hex(prev_c).expect("prev c hex32"),
        &Hex32::from_hex(pk).expect("pk hex32"),
        &Hex32::from_hex(N_A).expect("n hex32"),
        &Hex32::from_hex(j).expect("j hex32"),
        &Hex32::from_hex(&canon_hash).expect("canon hash hex32"),
    )
    .to_hex();
    (
        SelectedShareEvidence {
            pk: pk.to_string(),
            n: N_A.to_string(),
            j: j.to_string(),
            c: prev_c.to_string(),
            canon_hash,
            proof_package: package_hex,
            seed_hex: String::new(),
            signed_work: None,
        },
        hash,
    )
}

fn block_at(
    prev_c: &str,
    t_block: &str,
    shares: Vec<(SelectedShareEvidence, String)>,
) -> PersistedBlock {
    block_at_with_t_share(prev_c, t_block, T_MAX, shares)
}

/// SC.7 — variant that lets a test author the block's self-declared
/// `t_share` (still arithmetically consistent with its `minShareScore`),
/// so the genesis-binding rejection is the only thing that can fire.
fn block_at_with_t_share(
    prev_c: &str,
    t_block: &str,
    t_share: &str,
    shares: Vec<(SelectedShareEvidence, String)>,
) -> PersistedBlock {
    let evidence: Vec<SelectedShareEvidence> = shares.iter().map(|(e, _)| e.clone()).collect();
    let pks: Vec<String> = evidence.iter().map(|e| e.pk.clone()).collect();
    let hashes: Vec<String> = shares.iter().map(|(_, h)| h.clone()).collect();
    let proposer_pk = pks[0].clone();
    let kmax_applied = hashes.len() as u64;
    let mut block = PersistedBlock {
        height: 0,
        prev_c: prev_c.to_string(),
        c: String::new(),
        proposer_pk,
        selected_share_hashes: hashes,
        selected_share_pks: pks,
        selected_share_reward_pks: vec![],
        proposer_reward_pk: String::new(),
        selected_share_evidence: evidence,
        min_share_score: boole_core::min_share_score(
            &boole_core::parse_biguint_hex(t_share).expect("t_share parses"),
            1_000_000_000,
        )
        .expect("min share score computes")
        .to_string(),
        min_share_score_multiplier_nanos: 1_000_000_000,
        kmax_applied,
        difficulty_epoch: 0,
        t_block: t_block.to_string(),
        t_share: t_share.to_string(),
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

#[test]
fn replay_with_genesis_anchors_chain_at_spec_genesis_c() {
    // A chain anchored at a NON-zero genesis: the legacy entry point
    // (hardcoded all-zeros anchor) must reject it, the genesis-aware one
    // must replay it against the spec's anchor.
    let chain = vec![block_at(
        ANCHOR,
        T_MAX,
        vec![share_at(ANCHOR, PK_A, J_A, 0x11)],
    )];

    replay_blocks(&chain).expect_err("zero-anchor replay must reject a foreign-anchor chain");

    let result = replay_blocks_with_genesis(&chain, &spec(ANCHOR, 4, false))
        .expect("the genesis-aware replay must anchor at the spec's genesis_c");
    assert_eq!(result.balances.get(PK_A).copied(), Some(2));
}

#[test]
fn replay_with_genesis_rejects_foreign_anchor() {
    let chain = vec![block_at(
        ZEROS,
        T_MAX,
        vec![share_at(ZEROS, PK_A, J_A, 0x11)],
    )];

    let err = replay_blocks_with_genesis(&chain, &spec(ANCHOR, 4, false))
        .expect_err("a chain not anchored at the spec's genesis_c must be rejected");
    assert!(
        err.to_string().contains("prevC"),
        "anchor rejection should surface as a prevC linkage error: {err}"
    );
}

#[test]
fn replay_with_genesis_enforces_k_max() {
    let chain = vec![block_at(
        ZEROS,
        T_MAX,
        vec![
            share_at(ZEROS, PK_A, J_A, 0x11),
            share_at(ZEROS, PK_B, J_B, 0x33),
        ],
    )];

    replay_blocks_with_genesis(&chain, &spec(ZEROS, 2, false))
        .expect("two shares within k_max=2 must replay");

    let err = replay_blocks_with_genesis(&chain, &spec(ZEROS, 1, false))
        .expect_err("a block exceeding the genesis k_max must be rejected");
    assert!(
        err.to_string().contains("k_max") || err.to_string().contains("kMax"),
        "expected a k_max cap error: {err}"
    );
}

#[test]
fn replay_with_genesis_static_difficulty_must_match_spec() {
    // retarget: None ⇒ static difficulty: every block must carry the
    // spec's initial t_block verbatim (as a target value).
    let chain = vec![block_at(
        ZEROS,
        T_EASED,
        vec![share_at(ZEROS, PK_A, J_A, 0x11)],
    )];

    let err = replay_blocks_with_genesis(&chain, &spec(ZEROS, 4, false))
        .expect_err("a static-difficulty chain must stay at the genesis t_block");
    assert!(
        err.to_string().contains("t_block") || err.to_string().contains("tBlock"),
        "expected a static-difficulty error: {err}"
    );
}

#[test]
fn replay_with_genesis_rejects_empty_seed_when_required() {
    // The builder above leaves seedHex empty (the pre-N5 legacy posture) —
    // fine while seed_binding_required=false, a consensus reject once the
    // genesis requires seeds (ADR-0014 (d): no grandfathering).
    let chain = vec![block_at(
        ZEROS,
        T_MAX,
        vec![share_at(ZEROS, PK_A, J_A, 0x11)],
    )];

    replay_blocks_with_genesis(&chain, &spec(ZEROS, 4, false))
        .expect("empty seedHex stays accepted while the genesis does not require seeds");

    let err = replay_blocks_with_genesis(&chain, &spec(ZEROS, 4, true))
        .expect_err("empty seedHex must be rejected when the genesis requires seed binding");
    assert!(
        err.to_string().contains("seed"),
        "expected a seed-binding error: {err}"
    );
}

// SC.7 — the share threshold must come from the genesis commitment, not
// the block's own claim ("the examinee writes the passing grade on the
// answer sheet"). A block whose self-declared `t_share` diverges from
// `GenesisSpec.params.t_share` is rejected even when its declared
// `minShareScore` is arithmetically consistent with that self-declared
// value.
#[test]
fn replay_rejects_block_whose_t_share_diverges_from_genesis() {
    let divergent = vec![block_at_with_t_share(
        ZEROS,
        T_MAX,
        T_EASED,
        vec![share_at(ZEROS, PK_A, J_A, 0x11)],
    )];

    let err = replay_blocks_with_genesis(&divergent, &spec(ZEROS, 4, false))
        .expect_err("a t_share diverging from the genesis commitment must reject");
    assert!(
        err.to_string().contains("t_share"),
        "error should name the t_share divergence: {err}"
    );

    // Control: the same block shape with the genesis-committed t_share replays.
    let aligned = vec![block_at_with_t_share(
        ZEROS,
        T_MAX,
        T_MAX,
        vec![share_at(ZEROS, PK_A, J_A, 0x11)],
    )];
    replay_blocks_with_genesis(&aligned, &spec(ZEROS, 4, false))
        .expect("genesis-aligned t_share must replay");
}

// SC.7 (2nd review item 2) — the share-threshold verdict is a pure
// function of (block, genesis/rule): `t_share`/`minShareScore` live
// OUTSIDE the block.v3 preimage, so two blocks can share a hash while
// declaring different thresholds — the genesis binding is what keeps one
// hash from carrying two verdicts. Pinned: hash-equal variants exist,
// and replay accepts exactly the genesis-committed one.
#[test]
fn same_block_hash_implies_same_share_threshold_verdict() {
    let aligned =
        block_at_with_t_share(ZEROS, T_MAX, T_MAX, vec![share_at(ZEROS, PK_A, J_A, 0x11)]);
    let variant = block_at_with_t_share(
        ZEROS,
        T_MAX,
        T_EASED,
        vec![share_at(ZEROS, PK_A, J_A, 0x11)],
    );
    assert_eq!(
        aligned.c, variant.c,
        "t_share is outside the preimage — the variants must share one hash"
    );

    replay_blocks_with_genesis(&[aligned], &spec(ZEROS, 4, false))
        .expect("the genesis-committed variant replays");
    replay_blocks_with_genesis(&[variant], &spec(ZEROS, 4, false)).expect_err(
        "the hash-equal variant with a divergent threshold must reject — one hash, one verdict",
    );
}
