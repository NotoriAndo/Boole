use std::io::Write as _;
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
    blocks: Vec<serde_json::Value>,
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
    // Typed envelope shape adopted in S3: anyhow-bearing top-level errors
    // surface as `reason: "internal_error"` (the kebab vocabulary the
    // server speaks at the HTTP boundary). The legacy `error: "runtime"`
    // shape was retired so CLI and node speak the same dialect.
    assert_eq!(parsed["reason"], "internal_error");
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliAuditReceiptsOutput {
    ok: bool,
    blocks_checked: u64,
    receipts_checked: u64,
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

#[test]
fn cli_audit_receipts_json_accepts_ledger_matching_blocks() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");
    let dir = std::env::temp_dir().join(format!("boole-cli-audit-receipts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let blocks_path = dir.join("blocks.ndjson");
    let receipts_path = dir.join("submit-receipts.ndjson");

    let mut blocks_file = std::fs::File::create(&blocks_path).expect("create blocks");
    for block in &fixture.blocks {
        writeln!(
            blocks_file,
            "{}",
            serde_json::to_string(block).expect("block json")
        )
        .expect("write block");
    }
    std::fs::write(
        &receipts_path,
        concat!(
            r#"{"schema":"boole.submit.receipt.v1","accepted":true,"route":"/submit","sessionPk":"9999999999999999999999999999999999999999999999999999999999999999","submittedBy":"9999999999999999999999999999999999999999999999999999999999999999","nonce":"n-audit-1","requestHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","blockHeight":0,"blockC":"4de4d7cc23ab12195fae90e2778deb07c8f7ebf16b3440f326680a2e3ae7750d","shareHash":"0101010101010101010101010101010101010101010101010101010101010101","proposerPk":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","rewardRecipient":"1111111111111111111111111111111111111111111111111111111111111111","rewardAmount":"1"}"#,
            "\n"
        ),
    )
    .expect("write receipts");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "chain",
            "audit-receipts",
            "--blocks",
            blocks_path.to_str().expect("utf8 blocks path"),
            "--receipts",
            receipts_path.to_str().expect("utf8 receipts path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: CliAuditReceiptsOutput =
        serde_json::from_slice(&output.stdout).expect("json output");
    assert!(parsed.ok);
    assert_eq!(parsed.blocks_checked, fixture.blocks.len() as u64);
    assert_eq!(parsed.receipts_checked, 1);

    let _ = std::fs::remove_dir_all(&dir);
}
