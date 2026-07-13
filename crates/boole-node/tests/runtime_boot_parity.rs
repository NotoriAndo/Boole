//! SC.5 (GAP-08, critical) — boot/live replay parity. A named network's
//! boot-from-store must judge a chain with the SAME genesis-aware strict
//! contract live ingest uses: a chain live ingest rejects (evidence-less,
//! k_max overflow, empty seedHex under seed binding, foreign anchor) must
//! be rejected at boot too, instead of the legacy evidence-less opt-in
//! quietly accepting it from disk.

use std::path::PathBuf;

use boole_core::{
    block_hash, share_hash, GenesisInitialState, GenesisParams, GenesisSpec, Hex32, PersistedBlock,
    SelectedShareEvidence, CONSENSUS_RULE_VERSION,
};
use boole_node::{FileBlockStore, RuntimeAdmissionState, RuntimeConfig};
use boole_testkit::rand_suffix;
use serde_json::Value;
use sha2::{Digest, Sha256};

const ZEROS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const T_MAX: &str = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const PK_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const PK_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const N_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const J_A: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const J_B: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn spec(anchor: &str, k_max: u64, seed_required: bool) -> GenesisSpec {
    GenesisSpec {
        network_id: "boole-parity-test".to_string(),
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

fn block_at(prev_c: &str, shares: Vec<(SelectedShareEvidence, String)>) -> PersistedBlock {
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
        min_share_score: "1".to_string(),
        min_share_score_multiplier_nanos: 1_000_000_000,
        kmax_applied,
        difficulty_epoch: 0,
        t_block: T_MAX.to_string(),
        t_share: T_MAX.to_string(),
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

fn evidence_stripped(mut block: PersistedBlock) -> PersistedBlock {
    block.selected_share_evidence = vec![];
    block.c = block_hash(&block).to_hex();
    block
}

fn runtime_config() -> RuntimeConfig {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/runtime-smoke/v1.json"
    ))
    .expect("fixture parses");
    let report = serde_json::from_value(fixture["cfg"].clone()).expect("calibration report");
    RuntimeConfig::from_calibration_report(report, 60_000).expect("runtime config")
}

fn store_with(blocks: &[PersistedBlock]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-sc5-parity-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("blocks.ndjson");
    for block in blocks {
        FileBlockStore::append(&path, block).expect("append block");
    }
    path
}

/// SC.5 — the parity contract: for every corpus chain, boot-from-store
/// and live ingest must return the SAME verdict. Each corpus entry here
/// is one live ingest REJECTS; boot accepting any of them is the GAP-08
/// split-brain (one chain, two verdicts, path-dependent).
#[test]
fn boot_rejects_chain_rejected_by_live_ingest() {
    let corpus: Vec<(&str, GenesisSpec, Vec<PersistedBlock>)> = vec![
        (
            "evidence-less block",
            spec(ZEROS, 4, false),
            vec![evidence_stripped(block_at(
                ZEROS,
                vec![share_at(ZEROS, PK_A, J_A, 0x11)],
            ))],
        ),
        (
            "k_max overflow",
            spec(ZEROS, 1, false),
            vec![block_at(
                ZEROS,
                vec![
                    share_at(ZEROS, PK_A, J_A, 0x11),
                    share_at(ZEROS, PK_B, J_B, 0x33),
                ],
            )],
        ),
        (
            "empty seedHex under required seed binding",
            spec(ZEROS, 4, true),
            vec![block_at(ZEROS, vec![share_at(ZEROS, PK_A, J_A, 0x11)])],
        ),
        (
            "foreign anchor",
            spec(&"99".repeat(32), 4, false),
            vec![block_at(ZEROS, vec![share_at(ZEROS, PK_A, J_A, 0x11)])],
        ),
    ];

    for (name, genesis, chain) in corpus {
        // Live-path verdict: ingest and reorg both judge a candidate
        // chain via the strict genesis-aware replay (see
        // `reorg_to_heavier_chain` step 1 / `local_node::
        // ingest_candidate_chain`) — this call IS the live contract.
        assert!(
            boole_core::replay_blocks_with_genesis(&chain, &genesis).is_err(),
            "corpus sanity: the live strict contract must reject the {name} chain"
        );

        // Boot verdict — must match.
        let store = store_with(&chain);
        let boot = RuntimeAdmissionState::boot_from_store_with_genesis(
            runtime_config(),
            &store,
            None,
            None,
            boole_core::FamilyManifestRegistry::new(),
            &genesis,
        );
        assert!(
            boot.is_err(),
            "PARITY VIOLATION ({name}): live ingest rejects this chain but \
             boot-from-store accepted it"
        );
        let _ = std::fs::remove_dir_all(store.parent().expect("store dir"));
    }
}

/// SC.5 (2nd review item 9, CONFIRMED) — the reorg candidate path must
/// apply the same ts future-drift guard direct ingest applies: replay's
/// median-time-past check is RELATIVE, so a divergent heavier fork whose
/// suffix sits entirely in the future sails through replay and would
/// poison the retarget inputs once adopted. Direct extend-by-one ingest
/// already rejects such a tip; sync→reorg bypassed the guard entirely.
#[test]
fn reorg_rejects_candidate_suffix_beyond_future_drift() {
    let genesis = spec(ZEROS, 4, false);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_millis() as u64;

    // A chain valid in every respect except its tip ts sits far beyond
    // the future-drift allowance.
    let mut future_tip = block_at(ZEROS, vec![share_at(ZEROS, PK_A, J_A, 0x11)]);
    future_tip.ts = now_ms + 3 * 60 * 60 * 1_000; // three hours ahead (allowance is 2h)
    future_tip.c = block_hash(&future_tip).to_hex();
    let future_chain = vec![future_tip];
    assert!(
        boole_core::replay_blocks_with_genesis(&future_chain, &genesis).is_ok(),
        "corpus sanity: replay alone accepts the all-future suffix (relative MTP) — \
         only the drift guard can stop it"
    );

    let dir = std::env::temp_dir().join(format!(
        "boole-sc5-drift-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let store = dir.join("blocks.ndjson");

    let mut runtime = RuntimeAdmissionState::new(runtime_config());
    let outcome = runtime.reorg_to_heavier_chain(&store, &future_chain, &genesis);
    assert!(
        outcome.is_err(),
        "reorg must reject a candidate whose tip exceeds the future-drift allowance"
    );

    // Control: the same chain with a present-time tip is adopted.
    let mut present_tip = block_at(ZEROS, vec![share_at(ZEROS, PK_A, J_A, 0x11)]);
    present_tip.ts = now_ms;
    present_tip.c = block_hash(&present_tip).to_hex();
    runtime
        .reorg_to_heavier_chain(&store, &[present_tip], &genesis)
        .expect("a present-time candidate must still reorg");
    let _ = std::fs::remove_dir_all(&dir);
}

/// SC.5 (SC.7 위임) — a genesis-booted runtime must refuse to COMMIT a
/// self-produced block its own strict replay would reject. Here the
/// node's calibration t_share (MAX, from the smoke scenario) diverges
/// from the genesis commitment (EASED), so every block it produces
/// carries a t_share strict replay rejects (SC.7 binding) — before this
/// slice the commit path checked only linkage+shape and happily wrote an
/// unreplayable chain to disk.
#[test]
fn self_produced_block_survives_strict_replay() {
    let dir = std::env::temp_dir().join(format!(
        "boole-sc5-selfreplay-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let store = dir.join("blocks.ndjson");

    // Genesis commits an EASED t_share; the calibration policy (smoke
    // scenario) uses MAX — a config/genesis skew an operator can create.
    let mut genesis = spec(ZEROS, 4, false);
    genesis.params.t_share =
        "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();
    genesis.params.t_block =
        "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();

    let mut runtime = RuntimeAdmissionState::boot_from_store_with_genesis(
        runtime_config(),
        &store,
        None,
        None,
        boole_core::FamilyManifestRegistry::new(),
        &genesis,
    )
    .expect("empty store boots");
    runtime.set_current_c(ZEROS.to_string());

    // Admit one smoke share so the builder has a candidate.
    let raw = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/protocol/runtime-smoke/v1.json"),
    )
    .expect("scenario");
    let scenario: Value = serde_json::from_str(&raw).expect("scenario json");
    let body = scenario["steps"][0]["body"]
        .as_object()
        .expect("step body")
        .clone();
    runtime
        .observe_ticket_from_body(&body)
        .expect("ticket observes");
    let decision = runtime.admit_body(1_700_000_000, "127.0.0.1", &body);
    assert!(
        matches!(decision, boole_core::AdmissionDecision::Accepted { .. }),
        "smoke share admits: {decision:?}"
    );

    let mut tags = std::collections::BTreeSet::new();
    tags.insert(0u8);
    let outcome = runtime.commit_block_for_current_c(&store, 0, 1_700_000_000_000, &tags);
    assert!(
        outcome.is_err(),
        "commit must refuse a self-produced block strict replay rejects \
         (calibration t_share diverges from the genesis commitment)"
    );
    assert_eq!(
        std::fs::metadata(&store).map(|m| m.len()).unwrap_or(0),
        0,
        "the unreplayable block must not reach disk"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
