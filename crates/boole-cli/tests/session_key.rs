use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
}

fn fresh_tmp(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-session-key-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn parse_json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).expect("json")
}

#[test]
fn session_key_create_local_writes_policy_without_secret_in_stdout() {
    let keys = fresh_tmp("keys");
    let sessions = fresh_tmp("sessions");

    let owner = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .args(["keys", "new", "--id", "owner", "--dev"])
        .output()
        .expect("owner");
    assert!(
        owner.status.success(),
        "owner key creation failed: stderr={}",
        String::from_utf8_lossy(&owner.stderr)
    );
    let agent = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .args(["keys", "new", "--id", "agent", "--dev"])
        .output()
        .expect("agent");
    assert!(
        agent.status.success(),
        "agent key creation failed: stderr={}",
        String::from_utf8_lossy(&agent.stderr)
    );

    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .env("BOOLE_SESSIONS_DIR", &sessions)
        .args([
            "session-key",
            "create",
            "--local",
            "--id",
            "claude-local",
            "--owner-id",
            "owner",
            "--agent-id",
            "agent",
            "--allowed-family",
            "boole.protocol-invariant.v01",
            "--allowed-verifier",
            "lean-runner-v01",
            "--max-fee",
            "12",
            "--daily-fee-cap",
            "100",
            "--expiry-height",
            "1000",
        ])
        .output()
        .expect("session-key create");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let envelope = parse_json(&out.stdout);
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["session"]["id"], "claude-local");
    let stdout_text = String::from_utf8(out.stdout.clone()).expect("utf8");
    assert!(
        !stdout_text.contains("\"sk\""),
        "stdout must not contain the `sk` field; got {stdout_text}"
    );
    assert!(
        sessions.join("claude-local.json").is_file(),
        "session policy file should exist at {}",
        sessions.join("claude-local.json").display()
    );
}
