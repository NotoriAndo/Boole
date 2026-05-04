use std::process::Command;

use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSmokeOutput {
    ok: bool,
    accepted: bool,
    height: u64,
    prev_c: String,
    c: String,
    replay_height: u64,
    replay_latest_c: String,
    runtime_head: String,
    dropped_stale_shares: usize,
    store_size: usize,
    latest_matches_runtime: bool,
    replay_matches_runtime: bool,
    block_store_path: String,
}

#[test]
fn node_runtime_smoke_commits_replayable_block_from_fixture() {
    let repo_root = env!("CARGO_MANIFEST_DIR").trim_end_matches("/crates/boole-node");
    let fixture_path = format!("{repo_root}/fixtures/protocol/admission/v1.json");
    let dir = std::env::temp_dir().join(format!(
        "boole-node-runtime-smoke-cli-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blockstore.ndjson");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .args([
            "runtime-smoke",
            "--fixture",
            &fixture_path,
            "--block-store",
            block_path.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run boole-node runtime-smoke");
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let parsed: RuntimeSmokeOutput = serde_json::from_slice(&output.stdout).expect("json output");
    assert!(parsed.ok);
    assert!(parsed.accepted);
    assert_eq!(parsed.height, 0);
    assert_eq!(
        parsed.prev_c,
        "0000000000000000000000000000000000000000000000000000000000000000"
    );
    assert_eq!(parsed.replay_height, 1);
    assert_eq!(parsed.replay_latest_c, parsed.c);
    assert_eq!(parsed.runtime_head, parsed.c);
    assert_eq!(parsed.dropped_stale_shares, 1);
    assert_eq!(parsed.store_size, 1);
    assert!(parsed.latest_matches_runtime);
    assert!(parsed.replay_matches_runtime);
    assert_eq!(parsed.block_store_path, block_path.to_string_lossy());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn node_runtime_smoke_accepts_scenario_json_input() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    let constants = fixture.get("constants").expect("constants");
    let mut cfg = fixture.get("cfg").expect("cfg").clone();
    cfg["T_share"] = json!("0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    cfg["T_block"] = json!("0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe");
    cfg["MinShareScoreMultiplier"] = json!(1.0);
    cfg["K_max"] = json!(4);

    let genesis_c = "0000000000000000000000000000000000000000000000000000000000000000";
    let scenario = json!({
        "cfg": cfg,
        "genesisC": genesis_c,
        "body": {
            "c": genesis_c,
            "pk": constants["pk"],
            "n": constants["n"],
            "j": constants["j"],
            "nonceS": constants["nonceS"],
            "bytes": constants["validBytesHex"]
        },
        "ip": constants["ip"],
        "canonTag": 0,
        "ts": 1800000000123u64
    });

    let dir = std::env::temp_dir().join(format!(
        "boole-node-runtime-smoke-scenario-cli-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let scenario_path = dir.join("runtime-smoke-scenario.json");
    let block_path = dir.join("blockstore.ndjson");
    std::fs::write(
        &scenario_path,
        serde_json::to_vec(&scenario).expect("scenario json"),
    )
    .expect("write scenario");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .args([
            "runtime-smoke",
            "--scenario",
            scenario_path.to_str().expect("utf8 scenario path"),
            "--block-store",
            block_path.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run boole-node runtime-smoke scenario");
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let parsed: RuntimeSmokeOutput = serde_json::from_slice(&output.stdout).expect("json output");
    assert!(parsed.ok);
    assert!(parsed.accepted);
    assert_eq!(parsed.height, 0);
    assert_eq!(parsed.prev_c, genesis_c);
    assert_eq!(parsed.replay_height, 1);
    assert_eq!(parsed.replay_latest_c, parsed.c);
    assert_eq!(parsed.runtime_head, parsed.c);
    assert_eq!(parsed.dropped_stale_shares, 1);
    assert_eq!(parsed.store_size, 1);
    assert!(parsed.latest_matches_runtime);
    assert!(parsed.replay_matches_runtime);
    assert_eq!(parsed.block_store_path, block_path.to_string_lossy());

    let _ = std::fs::remove_dir_all(&dir);
}
