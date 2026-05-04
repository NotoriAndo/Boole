use boole_node::runtime_smoke::{run_runtime_smoke, RuntimeSmokeInput};

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
