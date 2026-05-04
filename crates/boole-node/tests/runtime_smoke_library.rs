use boole_core::CalibrationReport;
use boole_node::runtime::RuntimeConfig;
use boole_node::runtime_smoke::{
    run_runtime_smoke, run_runtime_smoke_scenario, RuntimeSmokeInput, RuntimeSmokeScenario,
};
use serde::Deserialize;
use serde_json::{Map, Value};

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
    ip: String,
    valid_bytes_hex: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Operation {
    name: String,
    #[serde(default)]
    body_patch: Map<String, Value>,
}

#[test]
fn runtime_smoke_runs_as_reusable_library_api() {
    let repo_root = env!("CARGO_MANIFEST_DIR").trim_end_matches("/crates/boole-node");
    let fixture_path = format!("{repo_root}/fixtures/protocol/admission/v1.json");
    let dir = std::env::temp_dir().join(format!(
        "boole-node-runtime-smoke-library-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blockstore.ndjson");

    let output = run_runtime_smoke(RuntimeSmokeInput {
        fixture_path: fixture_path.into(),
        block_path: block_path.clone(),
    })
    .expect("runtime smoke library call succeeds");

    assert!(output.ok);
    assert!(output.accepted);
    assert_eq!(output.height, 0);
    assert_eq!(output.replay_height, 1);
    assert_eq!(output.replay_latest_c, output.c);
    assert_eq!(output.runtime_head, output.c);
    assert_eq!(output.dropped_stale_shares, 1);
    assert!(block_path.exists());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runtime_smoke_scenario_runs_without_fixture_adapter() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    let mut cfg = fixture.cfg;
    cfg.T_share = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    cfg.T_block = "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();
    cfg.MinShareScoreMultiplier = 1.0;
    cfg.K_max = 4;

    let valid_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .expect("valid op");
    let constants = Constants {
        c: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        ..fixture.constants
    };
    let body = body_for(&constants, &valid_op.body_patch);
    let dir = std::env::temp_dir().join(format!(
        "boole-node-runtime-smoke-scenario-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blockstore.ndjson");

    let output = run_runtime_smoke_scenario(RuntimeSmokeScenario {
        config: RuntimeConfig::from_calibration_report(cfg, 60_000).expect("runtime config boots"),
        genesis_c: constants.c.clone(),
        body,
        ip: constants.ip,
        canon_tag: 0,
        block_path: block_path.clone(),
        ts: 1_800_000_000_123,
    })
    .expect("scenario smoke succeeds");

    assert!(output.ok);
    assert!(output.accepted);
    assert_eq!(output.height, 0);
    assert_eq!(output.replay_height, 1);
    assert_eq!(output.replay_latest_c, output.c);
    assert_eq!(output.runtime_head, output.c);
    assert_eq!(output.dropped_stale_shares, 1);
    assert!(block_path.exists());

    let _ = std::fs::remove_dir_all(&dir);
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
