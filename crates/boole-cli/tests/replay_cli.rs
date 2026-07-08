use std::io::Write as _;
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CliReplayResult {
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
fn cli_chain_replay_rejects_missing_fixture_with_typed_bad_request() {
    // P2.5 follow-up — missing --fixture path is a usage error, exit 2
    // with reason="fixture_unreadable" so automation can distinguish
    // operator typos from chain corruption (which exits 3).
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

    assert_eq!(
        output.status.code(),
        Some(2),
        "missing fixture must exit 2; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "runtime error JSON must not pollute stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    // P2.5 — `--json` error path is the unified envelope with kebab-case
    // `reason`. `chain.replay` shares the `fixture-unreadable` /
    // `fixture-invalid` / `replay-mismatch` vocabulary with the
    // PlainText snake_case dialect on the default path.
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let env: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr json envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "chain.replay");
    assert_eq!(env["error"]["reason"], "fixture-unreadable");
}

#[test]
fn cli_chain_replay_rejects_malformed_fixture_with_typed_bad_request() {
    // P2.5 follow-up — malformed JSON in --fixture is a usage error
    // (operator handed the wrong file), exit 2 with reason="fixture_invalid".
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-malformed-fixture-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let fixture_path = dir.join("fixture.json");
    std::fs::write(&fixture_path, b"{not valid json").expect("write malformed fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "chain",
            "replay",
            "--fixture",
            fixture_path.to_str().expect("utf8 path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert_eq!(
        output.status.code(),
        Some(2),
        "malformed fixture must exit 2; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let env: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr json envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "chain.replay");
    assert_eq!(env["error"]["reason"], "fixture-invalid");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_chain_replay_rejects_tampered_blocks_with_replay_mismatch() {
    // P2.5 follow-up — chain break inside the fixture is a corruption
    // (operation refused), exit 3 with reason="replay_mismatch", same
    // vocabulary as `state verify` so downstream tools share a dialect.
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");
    // Drop block 0 — replay starts at the genesis successor, breaking
    // the chain-continuity invariant.
    let only_second = fixture
        .blocks
        .get(1)
        .expect("fixture has at least two blocks")
        .clone();
    let tampered = serde_json::json!({
        "blocks": [only_second],
        "expected": {
            "latestC": fixture.expected.latest_c,
            "height": fixture.expected.height,
            "balances": fixture.expected.balances,
        }
    });

    let dir =
        std::env::temp_dir().join(format!("boole-cli-tampered-fixture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let fixture_path = dir.join("fixture.json");
    std::fs::write(
        &fixture_path,
        serde_json::to_vec_pretty(&tampered).expect("serialize tampered"),
    )
    .expect("write tampered fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "chain",
            "replay",
            "--fixture",
            fixture_path.to_str().expect("utf8 path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert_eq!(
        output.status.code(),
        Some(3),
        "tampered fixture must exit 3; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let env: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr json envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "chain.replay");
    assert_eq!(env["error"]["reason"], "replay-mismatch");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_chain_audit_receipts_rejects_missing_blocks_with_typed_bad_request() {
    // P2.5 follow-up — missing --blocks path is a usage error; exit 2
    // with reason="blocks_unreadable" mirrors the `state verify` dialect
    // so automation can distinguish operator typos from corruption.
    let missing = std::env::temp_dir().join(format!(
        "boole-cli-audit-missing-blocks-{}.ndjson",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&missing);
    let receipts_dummy = std::env::temp_dir().join(format!(
        "boole-cli-audit-missing-blocks-receipts-{}.ndjson",
        std::process::id()
    ));
    std::fs::write(&receipts_dummy, b"").expect("write empty receipts");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "chain",
            "audit-receipts",
            "--blocks",
            missing.to_str().expect("utf8 blocks path"),
            "--receipts",
            receipts_dummy.to_str().expect("utf8 receipts path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert_eq!(
        output.status.code(),
        Some(2),
        "missing blocks must exit 2; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // P2.5 — `--json` error path is the unified envelope with kebab-case
    // `reason`. `chain.audit-receipts` shares the `blocks-unreadable` /
    // `blocks-invalid` / `receipts-unreadable` / `receipts-invalid` /
    // `audit-mismatch` vocabulary with the PlainText snake_case dialect
    // on the default path.
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let env: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr json envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "chain.audit-receipts");
    assert_eq!(env["error"]["reason"], "blocks-unreadable");

    let _ = std::fs::remove_file(&receipts_dummy);
}

#[test]
fn cli_chain_audit_receipts_rejects_missing_receipts_with_typed_bad_request() {
    // P2.5 follow-up — missing --receipts path is a usage error; exit 2
    // with reason="receipts_unreadable". Same dialect as blocks_unreadable
    // so operators get an immediately-recognizable failure mode.
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-audit-missing-receipts-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let blocks_dummy = dir.join("blocks.ndjson");
    std::fs::write(&blocks_dummy, b"").expect("write empty blocks");
    let missing_receipts = dir.join("receipts-missing.ndjson");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "chain",
            "audit-receipts",
            "--blocks",
            blocks_dummy.to_str().expect("utf8 blocks path"),
            "--receipts",
            missing_receipts.to_str().expect("utf8 receipts path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert_eq!(
        output.status.code(),
        Some(2),
        "missing receipts must exit 2; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let env: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr json envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "chain.audit-receipts");
    assert_eq!(env["error"]["reason"], "receipts-unreadable");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_chain_audit_receipts_rejects_malformed_blocks_with_typed_bad_request() {
    // P2.5 follow-up — invalid JSON inside --blocks is also a usage
    // error (operator handed the wrong file); exit 2 with
    // reason="blocks_invalid" so it stays distinct from corruption (3).
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-audit-malformed-blocks-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let blocks_path = dir.join("blocks.ndjson");
    std::fs::write(&blocks_path, b"{not valid json}\n").expect("write malformed blocks");
    let receipts_path = dir.join("receipts.ndjson");
    std::fs::write(&receipts_path, b"").expect("write empty receipts");

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

    assert_eq!(
        output.status.code(),
        Some(2),
        "malformed blocks must exit 2; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let env: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr json envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "chain.audit-receipts");
    assert_eq!(env["error"]["reason"], "blocks-invalid");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_chain_audit_receipts_rejects_mismatched_audit_with_typed_audit_mismatch() {
    // P2.5 follow-up — a receipt that doesn't line up with the block log
    // is a verifier refusal (chain says one thing, receipt says another).
    // Exit 3 with reason="audit_mismatch" so it sits in the same family
    // as state verify's replay_mismatch (corruption / operation refused).
    let fixture: Fixture =
        serde_json::from_str(include_str!("../../../fixtures/protocol/replay/v1.json"))
            .expect("fixture parses");
    let dir = std::env::temp_dir().join(format!("boole-cli-audit-mismatch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let blocks_path = dir.join("blocks.ndjson");
    let receipts_path = dir.join("receipts.ndjson");
    // Same write_audit_fixture_with_receipt tamper used by settlement
    // tests: claim rewardAmount=2 while block credits amount=1.
    write_audit_fixture_with_receipt(
        &fixture,
        &blocks_path,
        &receipts_path,
        r#"{"schema":"boole.submit.receipt.v1","accepted":true,"route":"/submit","sessionPk":"9999999999999999999999999999999999999999999999999999999999999999","submittedBy":"9999999999999999999999999999999999999999999999999999999999999999","nonce":"n-audit-1","requestHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","blockHeight":0,"blockC":"3ac401a345c27e266387c9f084ef1aa94a2f4fb4764edca71f1ade48c1a6ddcc","shareHash":"0101010101010101010101010101010101010101010101010101010101010101","proposerPk":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","rewardRecipient":"1111111111111111111111111111111111111111111111111111111111111111","rewardAmount":"2"}"#,
    );

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

    assert_eq!(
        output.status.code(),
        Some(3),
        "mismatched audit must exit 3; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let env: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr json envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "chain.audit-receipts");
    assert_eq!(env["error"]["reason"], "audit-mismatch");

    let _ = std::fs::remove_dir_all(&dir);
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
    // P2.5 — `--json` success path is the unified envelope; replay
    // payload lives under `result` so the top-level `version`/`command`
    // describe the CLI schema rather than the chain state.
    let env: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json envelope on stdout");
    assert_eq!(env["ok"], true);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "chain.replay");
    let parsed: CliReplayResult =
        serde_json::from_value(env["result"].clone()).expect("typed replay result");
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
    // P2.5 — `--json` success path is the unified envelope; audit
    // payload lives under `result` so the top-level `version`/`command`
    // describe the CLI schema rather than the audit shape.
    let env: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json envelope on stdout");
    assert_eq!(env["ok"], true);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "chain.audit-receipts");
    let parsed: CliAuditReceiptsOutput =
        serde_json::from_value(env["result"].clone()).expect("typed audit result");
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
    // P2.5 — `--json` success path is the unified envelope; settlement
    // payload lives under `result` so the top-level `version`/`command`
    // describe the CLI schema rather than the settlement shape.
    let env: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json envelope on stdout");
    assert_eq!(env["ok"], true);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "chain.settlement-report");
    let raw = env["result"].clone();
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

    // P2.5 — settlement-report `--json` now wraps under the unified
    // envelope; the export bookkeeping fields live under `result`.
    let env: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("json envelope on stdout");
    assert_eq!(env["ok"], true);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "chain.settlement-report");
    let parsed = &env["result"];
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
        r#"{"schema":"boole.submit.receipt.v1","accepted":true,"route":"/submit","sessionPk":"9999999999999999999999999999999999999999999999999999999999999999","submittedBy":"9999999999999999999999999999999999999999999999999999999999999999","nonce":"n-audit-1","requestHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","blockHeight":0,"blockC":"3ac401a345c27e266387c9f084ef1aa94a2f4fb4764edca71f1ade48c1a6ddcc","shareHash":"0101010101010101010101010101010101010101010101010101010101010101","proposerPk":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","rewardRecipient":"1111111111111111111111111111111111111111111111111111111111111111","rewardAmount":"2"}"#,
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
        r#"{"schema":"boole.submit.receipt.v1","accepted":true,"route":"/submit","sessionPk":"9999999999999999999999999999999999999999999999999999999999999999","submittedBy":"9999999999999999999999999999999999999999999999999999999999999999","nonce":"n-audit-1","requestHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","blockHeight":0,"blockC":"3ac401a345c27e266387c9f084ef1aa94a2f4fb4764edca71f1ade48c1a6ddcc","shareHash":"0101010101010101010101010101010101010101010101010101010101010101","proposerPk":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","rewardRecipient":"1111111111111111111111111111111111111111111111111111111111111111","rewardAmount":"2"}"#,
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

    // P2.5 — tampered receipt is an audit refusal (exit 3, reason
    // kebab-case `audit-mismatch`) routed through the unified envelope
    // on `--json`, matching `chain audit-receipts` and `state verify`.
    // settlement-report still suppresses settlement stdout on audit
    // failure (the failure envelope goes to stderr, stdout stays empty).
    assert_eq!(
        output.status.code(),
        Some(3),
        "tampered receipt must exit 3; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "settlement stdout must be suppressed on audit failure: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let parsed: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr json envelope");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["version"], "v1");
    assert_eq!(parsed["command"], "chain.settlement-report");
    assert_eq!(parsed["error"]["reason"], "audit-mismatch");
    let detail = parsed["error"]["detail"].as_str().expect("detail string");
    assert!(
        detail.contains("rewardAmount mismatch"),
        "unexpected detail: {detail}"
    );
    assert!(parsed.get("settlement").is_none());
    assert!(parsed["error"].get("settlement").is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cli_chain_settlement_report_rejects_missing_blocks_with_typed_bad_request() {
    // P2.5 follow-up — settlement-report shares blocks/receipts inputs
    // with audit-receipts; align the missing-input envelope so operators
    // get the same operator-typo signal across both commands.
    let missing = std::env::temp_dir().join(format!(
        "boole-cli-settlement-missing-blocks-{}.ndjson",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&missing);
    let receipts_dummy = std::env::temp_dir().join(format!(
        "boole-cli-settlement-missing-blocks-receipts-{}.ndjson",
        std::process::id()
    ));
    std::fs::write(&receipts_dummy, b"").expect("write empty receipts");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "chain",
            "settlement-report",
            "--blocks",
            missing.to_str().expect("utf8 blocks path"),
            "--receipts",
            receipts_dummy.to_str().expect("utf8 receipts path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert_eq!(
        output.status.code(),
        Some(2),
        "missing blocks must exit 2; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let parsed: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr json envelope");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["version"], "v1");
    assert_eq!(parsed["command"], "chain.settlement-report");
    assert_eq!(parsed["error"]["reason"], "blocks-unreadable");

    let _ = std::fs::remove_file(&receipts_dummy);
}

#[test]
fn cli_chain_settlement_report_rejects_missing_receipts_with_typed_bad_request() {
    // P2.5 follow-up — same dialect as missing blocks, this time for
    // the receipts input. Exit 2 reason="receipts_unreadable" mirrors
    // chain audit-receipts.
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-settlement-missing-receipts-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let blocks_dummy = dir.join("blocks.ndjson");
    std::fs::write(&blocks_dummy, b"").expect("write empty blocks");
    let missing_receipts = dir.join("receipts-missing.ndjson");

    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "chain",
            "settlement-report",
            "--blocks",
            blocks_dummy.to_str().expect("utf8 blocks path"),
            "--receipts",
            missing_receipts.to_str().expect("utf8 receipts path"),
            "--json",
        ])
        .output()
        .expect("run boole-cli");

    assert_eq!(
        output.status.code(),
        Some(2),
        "missing receipts must exit 2; stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let parsed: serde_json::Value =
        serde_json::from_str(stderr_text.trim()).expect("stderr json envelope");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["version"], "v1");
    assert_eq!(parsed["command"], "chain.settlement-report");
    assert_eq!(parsed["error"]["reason"], "receipts-unreadable");

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
        r#"{"schema":"boole.submit.receipt.v1","accepted":true,"route":"/submit","sessionPk":"9999999999999999999999999999999999999999999999999999999999999999","submittedBy":"9999999999999999999999999999999999999999999999999999999999999999","nonce":"n-audit-1","requestHash":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","blockHeight":0,"blockC":"3ac401a345c27e266387c9f084ef1aa94a2f4fb4764edca71f1ade48c1a6ddcc","shareHash":"0101010101010101010101010101010101010101010101010101010101010101","proposerPk":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","rewardRecipient":"1111111111111111111111111111111111111111111111111111111111111111","rewardAmount":"1"}"#,
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
