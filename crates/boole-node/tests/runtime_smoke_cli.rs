use std::process::Command;

use serde::Deserialize;

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

    let _ = std::fs::remove_dir_all(&dir);
}
