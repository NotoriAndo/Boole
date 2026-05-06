use std::process::Command;

use serde::Deserialize;
use serde_json::{json, Value};

#[test]
fn proof_to_block_benchmark_script_reports_smoke_metrics() {
    let repo_root = env!("CARGO_MANIFEST_DIR").trim_end_matches("/crates/boole-node");
    let script_path = format!("{repo_root}/scripts/proof-to-block-benchmark.sh");

    let dir = std::env::temp_dir().join(format!(
        "boole-node-proof-to-block-benchmark-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");

    let output = Command::new("bash")
        .arg(&script_path)
        .env("BOOLE_NODE_BIN", env!("CARGO_BIN_EXE_boole-node"))
        .env("BLOCK_STORE_DIR", dir.to_str().expect("utf8 temp path"))
        .output()
        .expect("run proof-to-block benchmark script");
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let parsed: Value = serde_json::from_slice(&output.stdout).expect("json output");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["benchmark"], "proof-to-block");
    assert_eq!(parsed["version"], 0);
    assert_eq!(parsed["summary"]["casesPassed"], 7);
    assert_eq!(parsed["summary"]["blocksProduced"], 17);
    assert_eq!(parsed["summary"]["replayFailures"], 0);
    assert_eq!(parsed["safety"]["invalidAccepted"], 0);
    assert_eq!(parsed["safety"]["chainDivergence"], 0);

    let _ = std::fs::remove_dir_all(&dir);
}

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
    blocks: Vec<RuntimeSmokeBlockOutput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSmokeBlockOutput {
    height: u64,
    prev_c: String,
    c: String,
    difficulty_epoch: u64,
    t_block: String,
    t_share: String,
    difficulty_weight: String,
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
    assert_eq!(parsed.blocks.len(), 1);
    assert_eq!(parsed.blocks[0].height, 0);
    assert_eq!(parsed.blocks[0].prev_c, parsed.prev_c);
    assert_eq!(parsed.blocks[0].c, parsed.c);
    assert_eq!(parsed.blocks[0].difficulty_epoch, 0);
    assert_eq!(
        parsed.blocks[0].t_block,
        "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe"
    );
    assert_eq!(
        parsed.blocks[0].t_share,
        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
    );
    assert_eq!(parsed.blocks[0].difficulty_weight, "1");

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
    assert_eq!(parsed.blocks.len(), 1);
    assert_eq!(parsed.blocks[0].height, 0);
    assert_eq!(parsed.blocks[0].prev_c, parsed.prev_c);
    assert_eq!(parsed.blocks[0].c, parsed.c);
    assert_eq!(parsed.blocks[0].difficulty_epoch, 0);
    assert_eq!(parsed.blocks[0].t_block, scenario["cfg"]["T_block"]);
    assert_eq!(parsed.blocks[0].t_share, scenario["cfg"]["T_share"]);
    assert_eq!(parsed.blocks[0].difficulty_weight, "1");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn node_runtime_smoke_accepts_multistep_scenario_json_input() {
    let repo_root = env!("CARGO_MANIFEST_DIR").trim_end_matches("/crates/boole-node");
    let scenario_path = format!("{repo_root}/fixtures/protocol/runtime-smoke/v1.json");
    let genesis_c = "0000000000000000000000000000000000000000000000000000000000000000";

    let dir = std::env::temp_dir().join(format!(
        "boole-node-runtime-smoke-multistep-cli-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blockstore.ndjson");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .args([
            "runtime-smoke",
            "--scenario",
            &scenario_path,
            "--block-store",
            block_path.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run boole-node runtime-smoke multistep scenario");
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let parsed: RuntimeSmokeOutput = serde_json::from_slice(&output.stdout).expect("json output");
    assert!(parsed.ok);
    assert!(parsed.accepted);
    assert_eq!(parsed.store_size, 2);
    assert_eq!(parsed.replay_height, 2);
    assert_eq!(parsed.blocks.len(), 2);
    assert_eq!(parsed.blocks[0].height, 0);
    assert_eq!(parsed.blocks[0].prev_c, genesis_c);
    assert_eq!(parsed.blocks[1].height, 1);
    assert_eq!(parsed.blocks[1].prev_c, parsed.blocks[0].c);
    assert_eq!(parsed.height, 1);
    assert_eq!(parsed.c, parsed.blocks[1].c);
    assert_eq!(parsed.runtime_head, parsed.c);
    assert_eq!(parsed.replay_latest_c, parsed.c);
    assert!(parsed.latest_matches_runtime);
    assert!(parsed.replay_matches_runtime);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn node_runtime_smoke_applies_epoch_retarget_policy() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../fixtures/protocol/admission/v1.json"))
            .expect("fixture parses");
    let constants = fixture.get("constants").expect("constants");
    let mut cfg = fixture.get("cfg").expect("cfg").clone();
    cfg["T_share"] = json!("0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
    cfg["T_block"] = json!("0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe");
    cfg["MinShareScoreMultiplier"] = json!(1.0);
    cfg["K_max"] = json!(4);
    cfg["perIpRateLimitPer60s"] = json!(1);

    let genesis_c = "0000000000000000000000000000000000000000000000000000000000000000";
    let body = json!({
        "c": genesis_c,
        "pk": constants["pk"],
        "n": constants["n"],
        "j": constants["j"],
        "nonceS": constants["nonceS"],
        "bytes": constants["validBytesHex"]
    });
    let scenario = json!({
        "cfg": cfg,
        "difficultyRetarget": {
            "targetBlockMs": 61000,
            "retargetEveryBlocks": 2,
            "maxAdjustmentFactor": 4
        },
        "genesisC": genesis_c,
        "steps": [
            {"body": body, "ip": "203.0.113.55", "canonTag": 0, "ts": 1800000000123u64},
            {"body": body, "cFromRuntimeHead": true, "ip": "198.51.100.88", "canonTag": 0, "ts": 1800000061123u64},
            {"body": body, "cFromRuntimeHead": true, "ip": "192.0.2.44", "canonTag": 0, "ts": 1800000122123u64}
        ]
    });

    let dir = std::env::temp_dir().join(format!(
        "boole-node-runtime-smoke-retarget-cli-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let scenario_path = dir.join("runtime-smoke-retarget-scenario.json");
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
        .expect("run boole-node runtime-smoke retarget scenario");
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let parsed: RuntimeSmokeOutput = serde_json::from_slice(&output.stdout).expect("json output");
    assert_eq!(parsed.store_size, 3);
    assert_eq!(parsed.replay_height, 3);
    assert_eq!(parsed.blocks.len(), 3);
    assert_eq!(parsed.blocks[0].difficulty_epoch, 0);
    assert_eq!(parsed.blocks[1].difficulty_epoch, 0);
    assert_eq!(parsed.blocks[2].difficulty_epoch, 1);
    assert_eq!(parsed.blocks[0].t_block, parsed.blocks[1].t_block);
    assert_eq!(parsed.blocks[1].t_block, parsed.blocks[2].t_block);
    assert!(parsed.replay_matches_runtime);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runtime_smoke_all_script_runs_multiple_checked_cases() {
    let repo_root = env!("CARGO_MANIFEST_DIR").trim_end_matches("/crates/boole-node");
    let script_path = format!("{repo_root}/scripts/runtime-smoke-all.sh");

    let dir = std::env::temp_dir().join(format!(
        "boole-node-runtime-smoke-all-script-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");

    let output = Command::new("bash")
        .arg(&script_path)
        .env("BOOLE_NODE_BIN", env!("CARGO_BIN_EXE_boole-node"))
        .env("BLOCK_STORE_DIR", dir.to_str().expect("utf8 temp path"))
        .output()
        .expect("run runtime smoke all script");
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let parsed: Value = serde_json::from_slice(&output.stdout).expect("json output");
    assert_eq!(parsed["ok"], true);
    let cases = parsed["cases"].as_array().expect("cases array");
    assert_eq!(cases.len(), 6);
    assert_eq!(cases[0]["name"], "runtime-smoke-multistep");
    assert_eq!(cases[0]["mode"], "scenario");
    assert_eq!(cases[0]["storeSize"], 2);
    assert_eq!(cases[0]["replayHeight"], 2);
    assert_eq!(cases[0]["latestMatchesRuntime"], true);
    assert_eq!(cases[0]["replayMatchesRuntime"], true);
    assert_eq!(cases[1]["name"], "admission-fixture-compat");
    assert_eq!(cases[1]["mode"], "fixture");
    assert_eq!(cases[1]["storeSize"], 1);
    assert_eq!(cases[1]["replayHeight"], 1);
    assert_eq!(cases[1]["latestMatchesRuntime"], true);
    assert_eq!(cases[1]["replayMatchesRuntime"], true);
    assert_eq!(cases[4]["name"], "runtime-smoke-retarget-v0");
    assert_eq!(cases[4]["storeSize"], 3);
    assert_eq!(cases[4]["replayHeight"], 3);
    assert_eq!(cases[4]["blocks"][0]["difficultyEpoch"], 0);
    assert_eq!(cases[4]["blocks"][1]["difficultyEpoch"], 0);
    assert_eq!(cases[4]["blocks"][2]["difficultyEpoch"], 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runtime_smoke_all_script_uses_tracked_case_manifest() {
    let repo_root = env!("CARGO_MANIFEST_DIR").trim_end_matches("/crates/boole-node");
    let script_path = format!("{repo_root}/scripts/runtime-smoke-all.sh");
    let manifest_path = format!("{repo_root}/fixtures/protocol/runtime-smoke/cases.v1.json");

    let dir = std::env::temp_dir().join(format!(
        "boole-node-runtime-smoke-manifest-script-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");

    let output = Command::new("bash")
        .arg(&script_path)
        .env("BOOLE_NODE_BIN", env!("CARGO_BIN_EXE_boole-node"))
        .env("BLOCK_STORE_DIR", dir.to_str().expect("utf8 temp path"))
        .env("RUNTIME_SMOKE_CASES", &manifest_path)
        .output()
        .expect("run runtime smoke all script with manifest");
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let parsed: Value = serde_json::from_slice(&output.stdout).expect("json output");
    assert_eq!(parsed["ok"], true);
    assert_eq!(
        parsed["manifest"],
        "fixtures/protocol/runtime-smoke/cases.v1.json"
    );
    let cases = parsed["cases"].as_array().expect("cases array");
    assert_eq!(cases.len(), 6);
    assert!(cases.iter().all(|case| case["input"].as_str().is_some()));
    assert!(cases
        .iter()
        .all(|case| case["expectedStoreSize"].as_u64().is_some()));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runtime_smoke_script_runs_tracked_scenario_and_validates_output() {
    let repo_root = env!("CARGO_MANIFEST_DIR").trim_end_matches("/crates/boole-node");
    let script_path = format!("{repo_root}/scripts/runtime-smoke.sh");

    let dir = std::env::temp_dir().join(format!(
        "boole-node-runtime-smoke-script-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("script-blockstore.ndjson");

    let output = Command::new("bash")
        .arg(&script_path)
        .env("BOOLE_NODE_BIN", env!("CARGO_BIN_EXE_boole-node"))
        .env("BLOCK_STORE", block_path.to_str().expect("utf8 temp path"))
        .output()
        .expect("run runtime smoke script");
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let parsed: RuntimeSmokeOutput = serde_json::from_slice(&output.stdout).expect("json output");
    assert!(parsed.ok);
    assert!(parsed.accepted);
    assert_eq!(parsed.store_size, 2);
    assert_eq!(parsed.replay_height, 2);
    assert_eq!(parsed.blocks.len(), 2);
    assert_eq!(parsed.block_store_path, block_path.to_string_lossy());
    assert_eq!(parsed.runtime_head, parsed.c);
    assert_eq!(parsed.replay_latest_c, parsed.c);
    assert!(parsed.latest_matches_runtime);
    assert!(parsed.replay_matches_runtime);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runtime_smoke_rejects_expected_prev_c_mismatch_before_admission() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/runtime-smoke/v1.json"
    ))
    .expect("fixture parses");
    let mut scenario = fixture;
    scenario["steps"][0]["expectedPrevC"] =
        json!("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");

    let dir = std::env::temp_dir().join(format!(
        "boole-node-runtime-smoke-prev-c-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let scenario_path = dir.join("expected-prev-c-mismatch.json");
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
            block_path.to_str().expect("utf8 block path"),
        ])
        .output()
        .expect("run boole-node runtime-smoke expectedPrevC mismatch");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("expectedPrevC") && stderr.contains("does not match runtime head"),
        "stderr={stderr}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn proof_to_block_benchmark_includes_restart_nblock_and_multiminer_cases() {
    let repo_root = env!("CARGO_MANIFEST_DIR").trim_end_matches("/crates/boole-node");
    let script_path = format!("{repo_root}/scripts/proof-to-block-benchmark.sh");

    let dir = std::env::temp_dir().join(format!(
        "boole-node-proof-to-block-expanded-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");

    let output = Command::new("bash")
        .arg(&script_path)
        .env("BOOLE_NODE_BIN", env!("CARGO_BIN_EXE_boole-node"))
        .env("BLOCK_STORE_DIR", dir.to_str().expect("utf8 temp path"))
        .output()
        .expect("run expanded proof-to-block benchmark script");
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let parsed: Value = serde_json::from_slice(&output.stdout).expect("json output");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["summary"]["casesPassed"], 7);
    assert_eq!(parsed["summary"]["blocksProduced"], 17);
    assert_eq!(parsed["summary"]["replayFailures"], 0);
    assert_eq!(parsed["safety"]["chainDivergence"], 0);
    let case_names = parsed["cases"]
        .as_array()
        .expect("cases array")
        .iter()
        .map(|case| case["name"].as_str().expect("case name"))
        .collect::<Vec<_>>();
    assert!(case_names.contains(&"runtime-smoke-restart-replay"));
    assert!(case_names.contains(&"runtime-smoke-three-block"));
    assert!(case_names.contains(&"runtime-smoke-retarget-v0"));
    assert!(case_names.contains(&"runtime-smoke-multiminer"));
    assert!(case_names.contains(&"lean-submit-proof-to-block"));

    let lean_case = parsed["cases"]
        .as_array()
        .expect("cases array")
        .iter()
        .find(|case| case["name"] == "lean-submit-proof-to-block")
        .expect("Lean-backed submit-lean benchmark case");
    assert_eq!(lean_case["mode"], "submit-lean");
    assert_eq!(lean_case["ok"], true);
    assert_eq!(lean_case["accepted"], true);
    assert_eq!(lean_case["shareAccepted"], true);
    assert_eq!(lean_case["blockProduced"], true);
    assert_eq!(lean_case["replayMatchesRuntime"], true);
    assert_eq!(lean_case["invalidAccepted"], 0);

    let multiminer = parsed["cases"]
        .as_array()
        .expect("cases array")
        .iter()
        .find(|case| case["name"] == "runtime-smoke-multiminer")
        .expect("multiminer case");
    let proposer_pks = multiminer["blocks"]
        .as_array()
        .expect("blocks array")
        .iter()
        .map(|block| block["proposerPk"].as_str().expect("proposerPk"))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(proposer_pks.len(), 3);

    let _ = std::fs::remove_dir_all(&dir);
}
