use boole_node::{FileReputationLedger, PersistedReputationEvent};

const AGENT_PK: &str = "9999999999999999999999999999999999999999999999999999999999999999";

#[test]
fn reputation_ledger_recovers_agent_stats_from_ndjson_events() {
    let dir = std::env::temp_dir().join(format!("boole-reputation-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("reputation.ndjson");

    FileReputationLedger::append(
        &path,
        &PersistedReputationEvent {
            schema: "boole.reputation.event.v1".to_string(),
            agent_pk: AGENT_PK.to_string(),
            accepted_submits: 1,
            verified_reward_amount: "1".to_string(),
            source: "settlement-report".to_string(),
        },
    )
    .expect("append first reputation event");
    FileReputationLedger::append(
        &path,
        &PersistedReputationEvent {
            schema: "boole.reputation.event.v1".to_string(),
            agent_pk: AGENT_PK.to_string(),
            accepted_submits: 2,
            verified_reward_amount: "3".to_string(),
            source: "settlement-report".to_string(),
        },
    )
    .expect("append second reputation event");

    let ledger = FileReputationLedger::recover(&path).expect("recover reputation ledger");
    let stats = ledger.stats_for(AGENT_PK);
    assert_eq!(stats.agent_pk, AGENT_PK);
    assert_eq!(stats.accepted_submits, 3);
    assert_eq!(stats.verified_reward_amount, "4");
    assert_eq!(stats.event_count, 2);
    assert_eq!(ledger.size(), 2);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reputation_ledger_rejects_bad_agent_pk() {
    let dir =
        std::env::temp_dir().join(format!("boole-reputation-store-bad-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("reputation.ndjson");
    std::fs::write(
        &path,
        r#"{"schema":"boole.reputation.event.v1","agentPk":"not-hex","acceptedSubmits":1,"verifiedRewardAmount":"1","source":"settlement-report"}
"#,
    )
    .expect("write bad row");

    let err = FileReputationLedger::recover(&path).expect_err("bad pk must fail");
    assert!(
        err.to_string().contains("agentPk"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
