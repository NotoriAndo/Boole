//! N1.2 (G3) — the runtime's block-builder-config helpers must all honor
//! difficulty retarget: at a retarget boundary `produce_block_for_current_c`
//! must stamp the retargeted `t_block`, not the static calibrated one. Before
//! the fix both helpers used `BlockBuilderConfig::from_policy` (static); this
//! pins that they route through `block_builder_config_for_height`.

use std::collections::BTreeSet;

use boole_core::{
    expected_retarget_difficulty_for_height, AdmissionDecision, CalibrationReport,
    DifficultyRetargetPolicy, PersistedBlock,
};
use boole_node::{RuntimeAdmissionState, RuntimeConfig};
use serde::Deserialize;
use serde_json::{Map, Value};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    constants: Constants,
    cfg: CalibrationReport,
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

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
        .expect("fixture parses")
}

const RETARGET_EVERY: u64 = 2;
const TARGET_BLOCK_MS: u64 = 61_000;

fn retarget_config() -> RuntimeConfig {
    let mut cfg = fixture().cfg;
    // Permissive thresholds so each single fixture share is a block proposer.
    cfg.T_share = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    cfg.T_block = "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();
    cfg.MinShareScoreMultiplier = serde_json::Number::from(1);
    cfg.K_max = 4;
    RuntimeConfig::from_calibration_report(cfg, 60_000)
        .expect("runtime config")
        .with_difficulty_retarget(DifficultyRetargetPolicy {
            target_block_ms: TARGET_BLOCK_MS,
            retarget_every_blocks: RETARGET_EVERY,
            max_adjustment_factor: 4,
        })
        .expect("retarget policy valid")
}

fn body_for(c: &Constants, head_c: &str) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("c".to_string(), Value::String(head_c.to_string()));
    body.insert("pk".to_string(), Value::String(c.pk.clone()));
    body.insert("n".to_string(), Value::String(c.n.clone()));
    body.insert("j".to_string(), Value::String(c.j.clone()));
    body.insert("nonceS".to_string(), Value::String(c.nonce_s.clone()));
    body.insert(
        "bytes".to_string(),
        Value::String(c.valid_bytes_hex.clone()),
    );
    body
}

/// N4-pre.1 — consensus proof dedup (ADR-0012): each credited block needs a
/// DISTINCT canon_hash. Vary the POFP v1 package's second-expr u32 payload
/// (hex window [44:52]); `nth == 1` reproduces the base fixture proof.
fn distinct_bytes(base_hex: &str, nth: u32) -> String {
    let payload: String = nth
        .to_le_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{}{}{}", &base_hex[..44], payload, &base_hex[52..])
}

fn static_t_block_hex(config: &RuntimeConfig) -> String {
    format!("0x{:064x}", config.policy.thresholds.t_block)
}

#[test]
fn produce_block_for_current_c_uses_retargeted_t_block() {
    let f = fixture();
    let config = retarget_config();
    let static_hex = static_t_block_hex(&config);

    let dir = std::env::temp_dir().join(format!("boole-n12-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let block_path = dir.join("blocks.ndjson");
    let tags = BTreeSet::from([0]);

    let mut runtime = RuntimeAdmissionState::new(config);
    // Commit two blocks (heights 0,1) with a FAST span (well under the 61s
    // target) so the retarget at height 2 raises difficulty (t_block drops).
    let mut head_c = f.constants.c.clone();
    let mut committed: Vec<PersistedBlock> = Vec::new();
    let mut ts = 1_800_000_000_000u64;
    // Distinct IP per block dodges the per-IP rate limit. The span is only
    // MILDLY fast (~0.9 × the 61s target, below) so the retarget nudges
    // t_block down ~10% — enough to differ from the static near-max value,
    // but not so far that the fixture share (hash ~0xcd06…) stops qualifying
    // as proposer (a 4× drop to 0x3f… would).
    for i in 0..RETARGET_EVERY {
        runtime.set_current_c(head_c.clone());
        let mut body = body_for(&f.constants, &head_c);
        // Each committed block must carry a distinct proof (nth = i + 2, so
        // h0/h1 differ from each other and from the base). The retarget-
        // boundary h2 below keeps the base proof: its known-good hash still
        // qualifies as proposer under the ~10%-lower retargeted t_block.
        body.insert(
            "bytes".to_string(),
            Value::String(distinct_bytes(&f.constants.valid_bytes_hex, i as u32 + 2)),
        );
        runtime.observe_ticket_from_body(&body).expect("observe");
        let ip = format!("198.51.100.{}", i + 1);
        assert!(matches!(
            runtime.admit_body_with_canon_tag(ts as i64, &ip, &body, 0),
            AdmissionDecision::Accepted { .. }
        ));
        let c = runtime
            .commit_next_block_for_current_c(&block_path, ts, &tags)
            .expect("commit block");
        committed.push(c.block.clone());
        head_c = c.block.c.clone();
        ts += 54_900; // ~0.9 × 61s target: mildly fast → ~10% harder
    }

    // Expected retargeted difficulty for the next height (== cached_blocks).
    let expected = expected_retarget_difficulty_for_height(
        &committed,
        &static_hex,
        &DifficultyRetargetPolicy {
            target_block_ms: TARGET_BLOCK_MS,
            retarget_every_blocks: RETARGET_EVERY,
            max_adjustment_factor: 4,
        },
    )
    .expect("expected retarget");
    assert_ne!(
        expected.t_block, static_hex,
        "test setup must actually move difficulty off the static value"
    );

    // Admit a share for the next height and produce (not commit) the block.
    runtime.set_current_c(head_c.clone());
    let body = body_for(&f.constants, &head_c);
    runtime.observe_ticket_from_body(&body).expect("observe h2");
    assert!(matches!(
        runtime.admit_body_with_canon_tag(ts as i64, "198.51.100.200", &body, 0),
        AdmissionDecision::Accepted { .. }
    ));
    let produced = runtime
        .produce_block_for_current_c(RETARGET_EVERY, ts, &tags)
        .expect("produce block at retarget boundary");

    assert_eq!(
        produced.t_block, expected.t_block,
        "produce_block_for_current_c must stamp the retargeted t_block, not static"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
