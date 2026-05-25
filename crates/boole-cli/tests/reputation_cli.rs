use std::process::Command;

use boole_node::{FileReputationLedger, PersistedReputationEvent};
use serde::Deserialize;

const AGENT_PK: &str = "9999999999999999999999999999999999999999999999999999999999999999";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReputationInspectOutput {
    ok: bool,
    source: String,
    ledger_events: u64,
    stats: ReputationStatsOutput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReputationStatsOutput {
    agent_pk: String,
    accepted_submits: u64,
    verified_reward_amount: String,
    event_count: u64,
}

#[test]
fn cli_reputation_inspect_reads_recovered_ledger_without_mutation() {
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-reputation-inspect-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let ledger_path = dir.join("reputation.ndjson");

    FileReputationLedger::append(
        &ledger_path,
        &PersistedReputationEvent {
            schema: "boole.reputation.event.v1".to_string(),
            agent_pk: AGENT_PK.to_string(),
            accepted_submits: 1,
            verified_reward_amount: "2".to_string(),
            source: "settlement-report".to_string(),
        },
    )
    .expect("append reputation event");

    let before = std::fs::read_to_string(&ledger_path).expect("read before");
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "reputation",
            "inspect",
            "--ledger",
            ledger_path.to_str().expect("utf8 ledger path"),
            "--agent-pk",
            AGENT_PK,
            "--json",
        ])
        .output()
        .expect("run boole-cli");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&ledger_path).expect("read after"),
        before,
        "inspect must not mutate reputation ledger"
    );

    // P2.5 — `--json` success path is the unified envelope; the
    // inspect payload lives under `result` so the top-level
    // `version`/`command` describe the CLI schema rather than the
    // reputation ledger shape.
    let env: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json envelope on stdout");
    assert_eq!(env["ok"], true);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "reputation.inspect");
    let parsed: ReputationInspectOutput =
        serde_json::from_value(env["result"].clone()).expect("typed inspect result");
    assert!(parsed.ok);
    assert_eq!(parsed.source, "reputation-ledger");
    assert_eq!(parsed.ledger_events, 1);
    assert_eq!(parsed.stats.agent_pk, AGENT_PK);
    assert_eq!(parsed.stats.accepted_submits, 1);
    assert_eq!(parsed.stats.verified_reward_amount, "2");
    assert_eq!(parsed.stats.event_count, 1);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_reputation_inspect_rejects_malformed_agent_pk_with_unified_envelope() {
    // P2.5 — `--json` failure path routes the malformed-agent-pk error
    // through the unified envelope on stderr with a kebab-case reason
    // token.
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-reputation-inspect-malformed-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let ledger_path = dir.join("reputation.ndjson");
    std::fs::write(&ledger_path, b"").expect("write empty ledger");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "reputation",
            "inspect",
            "--ledger",
            ledger_path.to_str().expect("utf8 ledger path"),
            "--agent-pk",
            "not-hex-32",
            "--json",
        ])
        .output()
        .expect("run boole-cli");
    assert_eq!(
        output.status.code(),
        Some(2),
        "malformed agent-pk must exit 2; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let env: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr json envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "reputation.inspect");
    assert_eq!(env["error"]["reason"], "malformed-agent-pk");
    assert_eq!(env["error"]["agentPk"], "not-hex-32");

    let _ = std::fs::remove_dir_all(&dir);
}
