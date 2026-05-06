use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliReplayOutput {
    ok: bool,
    latest_c: String,
    height: u64,
    balances: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct Fixture {
    expected: Expected,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expected {
    latest_c: String,
    height: u64,
    balances: std::collections::BTreeMap<String, String>,
}

#[test]
fn cli_runtime_error_json_goes_to_stderr_and_leaves_stdout_empty() {
    let missing_fixture = std::env::temp_dir().join(format!(
        "boole-cli-missing-fixture-{}.json",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&missing_fixture);

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "chain",
            "replay",
            "--fixture",
            missing_fixture.to_str().expect("utf8 path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert!(!output.status.success(), "missing fixture should fail");
    assert!(
        output.stdout.is_empty(),
        "runtime error JSON must not pollute stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&output.stderr).expect("stderr json");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["error"], "runtime");
}

#[test]
fn cli_replay_json_matches_replay_fixture() {
    let fixture_path = format!(
        "{}/fixtures/protocol/replay/v1.json",
        env!("CARGO_MANIFEST_DIR").trim_end_matches("/crates/boole-cli")
    );
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["chain", "replay", "--fixture", &fixture_path, "--json"])
        .output()
        .expect("run boole-cli");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: CliReplayOutput = serde_json::from_slice(&output.stdout).expect("json output");
    assert!(parsed.ok);
    assert_eq!(parsed.latest_c, fixture.expected.latest_c);
    assert_eq!(parsed.height, fixture.expected.height);
    assert_eq!(parsed.balances, fixture.expected.balances);
}
