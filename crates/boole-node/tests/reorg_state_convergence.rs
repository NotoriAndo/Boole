//! N4.3 — reorg re-derives state deterministically. When the runtime, sitting
//! on chain A, is handed a heavier competing chain B that shares A's genesis
//! prefix, it must rewind to the common ancestor and replay B so that the
//! resulting balances are byte-identical to a fresh replay of B — and the
//! rewrite must survive a restart (block store + reward ledger rewritten
//! atomically). This is the runtime-level reorg primitive; wiring it into the
//! p2p ingress/sync path is a later slice.

use boole_core::{replay_blocks, AdmissionDecision, CalibrationReport, PersistedBlock};
use boole_node::{FileBlockStore, ReorgOutcome, RuntimeAdmissionState, RuntimeConfig};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

const GENESIS_C: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    constants: Constants,
    cfg: CalibrationReport,
    operations: Vec<Operation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Constants {
    c: String,
    pk: String,
    n: String,
    j: String,
    nonce_s: String,
    valid_bytes_hex: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Operation {
    name: String,
    #[serde(default)]
    body_patch: Map<String, Value>,
}

fn load_fixture() -> Fixture {
    serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
        .expect("fixture parses")
}

fn config_from(fixture: &Fixture) -> RuntimeConfig {
    // Same knobs as runtime_policy_boot's multi-block tests: a trivially-met
    // T_share/T_block and multiplier 1 so committed shares clear the block gate.
    let mut cfg = fixture.cfg.clone();
    cfg.T_share = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    cfg.T_block = "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();
    cfg.MinShareScoreMultiplier = serde_json::Number::from(1);
    cfg.K_max = 4;
    RuntimeConfig::from_calibration_report(cfg, 60_000).expect("runtime config boots from report")
}

fn body_for(constants: &Constants, patch: &Map<String, Value>) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("c".to_string(), Value::String(constants.c.clone()));
    body.insert("pk".to_string(), Value::String(constants.pk.clone()));
    body.insert("n".to_string(), Value::String(constants.n.clone()));
    body.insert("j".to_string(), Value::String(constants.j.clone()));
    body.insert(
        "nonceS".to_string(),
        Value::String(constants.nonce_s.clone()),
    );
    body.insert(
        "bytes".to_string(),
        Value::String(constants.valid_bytes_hex.clone()),
    );
    for (key, value) in patch {
        if value.is_null() {
            body.remove(key);
        } else {
            body.insert(key.clone(), value.clone());
        }
    }
    body
}

/// Vary the POFP v1 package's second-expr u32 payload (hex window [44:52]) so
/// each block on a chain credits a DISTINCT canon_hash — the consensus proof
/// dedup rule (ADR-0012) refuses to credit the same proof twice.
fn distinct_proof_bytes(base: &str, nth: u32) -> String {
    let payload: String = nth
        .to_le_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{}{}{}", &base[..44], payload, &base[52..])
}

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("boole-reorg-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Build `count` linked, strict-replayable blocks from `GENESIS_C` by driving a
/// throwaway runtime through the real admit → commit cycle.
fn build_chain(
    config: &RuntimeConfig,
    fixture: &Fixture,
    count: u32,
    tag: &str,
) -> Vec<PersistedBlock> {
    let mut runtime = RuntimeAdmissionState::new(config.clone());
    runtime.set_current_c(GENESIS_C.to_string());
    let valid_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .expect("valid op");
    let base_body = body_for(&fixture.constants, &valid_op.body_patch);
    let base_bytes = base_body
        .get("bytes")
        .and_then(Value::as_str)
        .expect("proof bytes")
        .to_string();

    let block_path = scratch_dir(tag).join("blockstore.ndjson");
    let accepted_tags = BTreeSet::from([0u8]);
    let mut blocks = Vec::new();
    let mut prev_c = GENESIS_C.to_string();
    for h in 0..count {
        let admit_ts: i64 = 1_800_000_000_000 + (h as i64) * 61_000;
        let mut body = base_body.clone();
        body.insert("c".to_string(), Value::String(prev_c.clone()));
        body.insert(
            "bytes".to_string(),
            Value::String(distinct_proof_bytes(&base_bytes, h + 1)),
        );
        runtime
            .observe_ticket_from_body(&body)
            .expect("observe ticket");
        let ip = format!("198.51.100.{}", 40 + h);
        let decision = runtime.admit_body_with_canon_tag(admit_ts, &ip, &body, 0);
        assert!(
            matches!(decision, AdmissionDecision::Accepted { .. }),
            "admit block {h}: {decision:?}"
        );
        let committed = runtime
            .commit_next_block_for_current_c(&block_path, admit_ts as u64 + 123, &accepted_tags)
            .expect("commit block");
        assert_eq!(committed.block.height, h as u64);
        prev_c = committed.block.c.clone();
        blocks.push(committed.block);
    }
    blocks
}

/// Boot a node whose on-disk store holds exactly `blocks`, with a reward ledger
/// re-derived from them. Returns the node plus its store/ledger paths.
fn boot_node_on(
    config: &RuntimeConfig,
    blocks: &[PersistedBlock],
    tag: &str,
) -> (RuntimeAdmissionState, PathBuf, PathBuf) {
    let dir = scratch_dir(tag);
    let block_path = dir.join("blockstore.ndjson");
    let reward_path = dir.join("reward.ndjson");
    for block in blocks {
        FileBlockStore::append(&block_path, block).expect("seed block store");
    }
    let node = RuntimeAdmissionState::boot_from_store(
        config.clone(),
        &block_path,
        Some(reward_path.clone()),
    )
    .expect("boot node");
    (node, block_path, reward_path)
}

#[test]
fn reorg_to_heavier_chain_rederives_balances_byte_identical() {
    let fixture = load_fixture();
    let config = config_from(&fixture);

    // A two-block chain; chain A = its first block (the shared genesis prefix),
    // chain B = the full two blocks (strictly heavier cumulative work).
    let chain_b = build_chain(&config, &fixture, 2, "build-heavier");
    let chain_a = chain_b[..1].to_vec();

    let (mut node, block_path, reward_path) = boot_node_on(&config, &chain_a, "node-a");
    assert_eq!(node.cached_block_count(), 1);
    assert_eq!(node.current_c(), Some(chain_a[0].c.as_str()));

    // Pre-reorg balances match a fresh replay of A.
    let replay_a = replay_blocks(&chain_a).expect("replay A");
    for (pk, bal) in &replay_a.balances {
        assert_eq!(node.balance_for(pk), *bal, "pre-reorg balance for {pk}");
    }

    let outcome = node
        .reorg_to_heavier_chain(&block_path, &chain_b)
        .expect("reorg to heavier chain");
    assert_eq!(outcome, ReorgOutcome::Reorged { new_head_height: 1 });

    // In-memory state now mirrors chain B.
    assert_eq!(node.cached_block_count(), 2);
    assert_eq!(node.current_c(), Some(chain_b[1].c.as_str()));
    assert_eq!(node.cached_blocks(), chain_b.as_slice());

    // Balances are byte-identical to a fresh replay of B.
    let replay_b = replay_blocks(&chain_b).expect("replay B");
    for (pk, bal) in &replay_b.balances {
        assert_eq!(node.balance_for(pk), *bal, "post-reorg balance for {pk}");
    }

    // The on-disk store was rewritten to chain B.
    let recovered = FileBlockStore::recover(&block_path).expect("recover after reorg");
    assert_eq!(recovered.blocks(), chain_b.as_slice());

    // A fresh boot over the rewritten files reconstructs identical state — the
    // reorg's atomic rewrite survives a restart.
    let reboot =
        RuntimeAdmissionState::boot_from_store(config.clone(), &block_path, Some(reward_path))
            .expect("reboot after reorg");
    assert_eq!(reboot.current_c(), Some(chain_b[1].c.as_str()));
    for (pk, bal) in &replay_b.balances {
        assert_eq!(reboot.balance_for(pk), *bal, "reboot balance for {pk}");
    }
}

#[test]
fn lighter_chain_is_not_adopted() {
    let fixture = load_fixture();
    let config = config_from(&fixture);

    let chain_full = build_chain(&config, &fixture, 2, "build-lighter");
    let chain_light = chain_full[..1].to_vec();

    // Node sits on the heavier two-block chain; a one-block candidate loses.
    let (mut node, block_path, _reward_path) = boot_node_on(&config, &chain_full, "node-b");
    let head_before = node.current_c().map(str::to_string);

    let outcome = node
        .reorg_to_heavier_chain(&block_path, &chain_light)
        .expect("evaluate lighter candidate");
    assert_eq!(outcome, ReorgOutcome::KeptCurrent);

    // Nothing changed: head, cache, and on-disk store are untouched.
    assert_eq!(node.current_c().map(str::to_string), head_before);
    assert_eq!(node.cached_block_count(), 2);
    let recovered = FileBlockStore::recover(&block_path).expect("recover unchanged store");
    assert_eq!(recovered.blocks(), chain_full.as_slice());
}
