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
    audit_mode: String,
    lineage_required: bool,
    blocks_checked: u64,
    receipts_checked: u64,
    evidence: CliAuditReceiptsEvidence,
    settlement: CliAuditReceiptsSettlement,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliSettlementReportOutput {
    ok: bool,
    source: String,
    audit_mode: String,
    lineage_required: bool,
    blocks_checked: u64,
    receipts_checked: u64,
    settlement: CliAuditReceiptsSettlement,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliAuditReceiptsEvidence {
    block_heights: Vec<u64>,
    reward_recipients: Vec<String>,
    request_hashes: Vec<String>,
    signed_work_checked: u64,
    checks: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliAuditReceiptsSettlement {
    reward_credits: Vec<CliPersistedCredit>,
    reputation_deltas: Vec<CliReputationDelta>,
    checks: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliPersistedCredit {
    pk: String,
    amount: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliReputationDelta {
    agent_pk: String,
    accepted_submits: u64,
    verified_reward_amount: String,
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
    write_audit_fixture(&fixture, &blocks_path, &receipts_path);

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
    assert_eq!(parsed.audit_mode, "shape-only");
    assert!(!parsed.lineage_required);
    assert_eq!(parsed.blocks_checked, fixture.blocks.len() as u64);
    assert_eq!(parsed.receipts_checked, 1);
    assert_eq!(parsed.evidence.block_heights, vec![0, 1]);
    assert_eq!(
        parsed.evidence.reward_recipients,
        vec!["1111111111111111111111111111111111111111111111111111111111111111"]
    );
    assert_eq!(
        parsed.evidence.request_hashes,
        vec!["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
    );
    assert_eq!(parsed.evidence.signed_work_checked, 0);
    assert_eq!(parsed.evidence.checks["blockChainContinuity"], true);
    assert_eq!(parsed.evidence.checks["rewardCreditBinding"], true);
    assert_eq!(parsed.evidence.checks["signedWorkLineage"], false);
    assert_eq!(parsed.settlement.reward_credits.len(), 1);
    assert_eq!(
        parsed.settlement.reward_credits[0].pk,
        "1111111111111111111111111111111111111111111111111111111111111111"
    );
    assert_eq!(parsed.settlement.reward_credits[0].amount, "1");
    assert_eq!(parsed.settlement.reputation_deltas.len(), 1);
    assert_eq!(
        parsed.settlement.reputation_deltas[0].agent_pk,
        "9999999999999999999999999999999999999999999999999999999999999999"
    );
    assert_eq!(parsed.settlement.reputation_deltas[0].accepted_submits, 1);
    assert_eq!(
        parsed.settlement.reputation_deltas[0].verified_reward_amount,
        "1"
    );
    assert_eq!(parsed.settlement.checks["rewardCreditsReplayBound"], true);
    assert_eq!(
        parsed.settlement.checks["reputationBoundToSubmittedBy"],
        true
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_settlement_report_json_exposes_settlement_only_surface() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-settlement-report-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let blocks_path = dir.join("blocks.ndjson");
    let receipts_path = dir.join("submit-receipts.ndjson");
    write_audit_fixture(&fixture, &blocks_path, &receipts_path);

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "chain",
            "settlement-report",
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
    let raw: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json output");
    assert_eq!(
        raw["claimBoundary"],
        "shape-only local audit; no ledger mutation"
    );
    assert_eq!(raw["lineageVerified"], false);
    assert_eq!(raw["rewardLedgerMutated"], false);
    assert_eq!(raw["reputationLedgerMutated"], false);
    assert!(raw.get("rewardCredited").is_none());
    assert!(raw.get("reputationCredited").is_none());

    let parsed: CliSettlementReportOutput = serde_json::from_value(raw).expect("typed json output");
    assert!(parsed.ok);
    assert_eq!(parsed.source, "audit-receipts-shape-only");
    assert_eq!(parsed.audit_mode, "shape-only");
    assert!(!parsed.lineage_required);
    assert_eq!(parsed.blocks_checked, fixture.blocks.len() as u64);
    assert_eq!(parsed.receipts_checked, 1);
    assert_eq!(parsed.settlement.reward_credits.len(), 1);
    assert_eq!(
        parsed.settlement.reward_credits[0].pk,
        "1111111111111111111111111111111111111111111111111111111111111111"
    );
    assert_eq!(parsed.settlement.reward_credits[0].amount, "1");
    assert_eq!(parsed.settlement.reputation_deltas.len(), 1);
    assert_eq!(
        parsed.settlement.reputation_deltas[0].agent_pk,
        "9999999999999999999999999999999999999999999999999999999999999999"
    );
    assert_eq!(parsed.settlement.reputation_deltas[0].accepted_submits, 1);
    assert_eq!(
        parsed.settlement.reputation_deltas[0].verified_reward_amount,
        "1"
    );
    assert_eq!(parsed.settlement.checks["rewardCreditsReplayBound"], true);
    assert_eq!(
        parsed.settlement.checks["reputationBoundToSubmittedBy"],
        true
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_settlement_report_exports_reputation_events_without_mutating_ledger() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-settlement-reputation-export-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let blocks_path = dir.join("blocks.ndjson");
    let receipts_path = dir.join("submit-receipts.ndjson");
    let events_path = dir.join("reputation-events.ndjson");
    write_audit_fixture(&fixture, &blocks_path, &receipts_path);

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "chain",
            "settlement-report",
            "--blocks",
            blocks_path.to_str().expect("utf8 blocks path"),
            "--receipts",
            receipts_path.to_str().expect("utf8 receipts path"),
            "--export-reputation-events",
            events_path.to_str().expect("utf8 events path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json output");
    assert_eq!(parsed["reputationEventsExported"], 1);
    assert_eq!(
        parsed["reputationEventsPath"],
        events_path.to_str().expect("utf8 events path")
    );

    let exported = std::fs::read_to_string(&events_path).expect("reputation events written");
    let lines: Vec<_> = exported.lines().collect();
    assert_eq!(lines.len(), 1, "expected one event row: {exported}");
    let event: serde_json::Value = serde_json::from_str(lines[0]).expect("event json");
    assert_eq!(event["schema"], "boole.reputation.event.v1");
    assert_eq!(
        event["agentPk"],
        "9999999999999999999999999999999999999999999999999999999999999999"
    );
    assert_eq!(event["acceptedSubmits"], 1);
    assert_eq!(event["verifiedRewardAmount"], "1");
    assert_eq!(event["source"], "settlement-report-shape-only");
    assert_eq!(event["lineageVerified"], false);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_settlement_report_does_not_export_reputation_events_on_audit_failure() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-settlement-reputation-export-fail-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let blocks_path = dir.join("blocks.ndjson");
    let receipts_path = dir.join("submit-receipts.ndjson");
    let events_path = dir.join("reputation-events.ndjson");
    write_audit_fixture_with_receipt(
        &fixture,
        &blocks_path,
        &receipts_path,
        r#"{"schema":"boole.submit.receipt.v1","accepted":true,"route":"/submit","sessionPk":"9999999999999999999999999999999999999999999999999999999999999999","submittedBy":"9999999999999999999999999999999999999999999999999999999999999999","nonce":"n-audit-1","requestHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","blockHeight":0,"blockC":"4de4d7cc23ab12195fae90e2778deb07c8f7ebf16b3440f326680a2e3ae7750d","shareHash":"0101010101010101010101010101010101010101010101010101010101010101","proposerPk":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","rewardRecipient":"1111111111111111111111111111111111111111111111111111111111111111","rewardAmount":"2"}"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "chain",
            "settlement-report",
            "--blocks",
            blocks_path.to_str().expect("utf8 blocks path"),
            "--receipts",
            receipts_path.to_str().expect("utf8 receipts path"),
            "--export-reputation-events",
            events_path.to_str().expect("utf8 events path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert!(!output.status.success(), "tampered receipt must fail");
    assert!(
        !events_path.exists(),
        "audit failure must not write reputation export rows"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_settlement_report_rejects_tampered_receipt_without_settlement_stdout() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-settlement-report-tamper-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let blocks_path = dir.join("blocks.ndjson");
    let receipts_path = dir.join("submit-receipts.ndjson");
    write_audit_fixture_with_receipt(
        &fixture,
        &blocks_path,
        &receipts_path,
        r#"{"schema":"boole.submit.receipt.v1","accepted":true,"route":"/submit","sessionPk":"9999999999999999999999999999999999999999999999999999999999999999","submittedBy":"9999999999999999999999999999999999999999999999999999999999999999","nonce":"n-audit-1","requestHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","blockHeight":0,"blockC":"4de4d7cc23ab12195fae90e2778deb07c8f7ebf16b3440f326680a2e3ae7750d","shareHash":"0101010101010101010101010101010101010101010101010101010101010101","proposerPk":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","rewardRecipient":"1111111111111111111111111111111111111111111111111111111111111111","rewardAmount":"2"}"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "chain",
            "settlement-report",
            "--blocks",
            blocks_path.to_str().expect("utf8 blocks path"),
            "--receipts",
            receipts_path.to_str().expect("utf8 receipts path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert!(!output.status.success(), "tampered receipt must fail");
    assert!(
        output.stdout.is_empty(),
        "settlement stdout must be suppressed on audit failure: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&output.stderr).expect("stderr json");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["reason"], "internal_error");
    let detail = parsed["detail"].as_str().expect("detail string");
    assert!(
        detail.contains("settlement suppressed"),
        "unexpected detail: {detail}"
    );
    assert!(
        detail.contains("rewardAmount mismatch"),
        "unexpected detail: {detail}"
    );
    assert!(parsed.get("settlement").is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

fn write_audit_fixture(
    fixture: &Fixture,
    blocks_path: &std::path::Path,
    receipts_path: &std::path::Path,
) {
    write_audit_fixture_with_receipt(
        fixture,
        blocks_path,
        receipts_path,
        r#"{"schema":"boole.submit.receipt.v1","accepted":true,"route":"/submit","sessionPk":"9999999999999999999999999999999999999999999999999999999999999999","submittedBy":"9999999999999999999999999999999999999999999999999999999999999999","nonce":"n-audit-1","requestHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","blockHeight":0,"blockC":"4de4d7cc23ab12195fae90e2778deb07c8f7ebf16b3440f326680a2e3ae7750d","shareHash":"0101010101010101010101010101010101010101010101010101010101010101","proposerPk":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","rewardRecipient":"1111111111111111111111111111111111111111111111111111111111111111","rewardAmount":"1"}"#,
    );
}

fn write_audit_fixture_with_receipt(
    fixture: &Fixture,
    blocks_path: &std::path::Path,
    receipts_path: &std::path::Path,
    receipt_line: &str,
) {
    let mut blocks_file = std::fs::File::create(blocks_path).expect("create blocks");
    for block in &fixture.blocks {
        writeln!(
            blocks_file,
            "{}",
            serde_json::to_string(block).expect("block json")
        )
        .expect("write block");
    }
    std::fs::write(receipts_path, format!("{receipt_line}\n")).expect("write receipts");
}
