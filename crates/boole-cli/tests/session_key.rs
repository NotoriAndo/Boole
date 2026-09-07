use std::path::Path;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_boole"))
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

fn setup_session(keys: &Path, sessions: &Path, id: &str) {
    let owner = cli()
        .env("BOOLE_KEYS_DIR", keys)
        .args(["keys", "new", "--id", "owner", "--dev"])
        .output()
        .expect("owner");
    assert!(
        owner.status.success(),
        "owner key creation failed: stderr={}",
        String::from_utf8_lossy(&owner.stderr)
    );
    let agent = cli()
        .env("BOOLE_KEYS_DIR", keys)
        .args(["keys", "new", "--id", "agent", "--dev"])
        .output()
        .expect("agent");
    assert!(
        agent.status.success(),
        "agent key creation failed: stderr={}",
        String::from_utf8_lossy(&agent.stderr)
    );
    let out = cli()
        .env("BOOLE_KEYS_DIR", keys)
        .env("BOOLE_SESSIONS_DIR", sessions)
        .args([
            "session-key",
            "create",
            "--local",
            "--id",
            id,
            "--owner-id",
            "owner",
            "--agent-id",
            "agent",
            "--allowed-route",
            "/submit",
            "--allowed-route",
            "/verify-answer",
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
        "session create failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
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
            "--allowed-route",
            "/submit",
            "--allowed-route",
            "/verify-answer",
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
    assert_eq!(envelope["session"]["allowedRoutes"][0], "/submit");
    assert_eq!(envelope["session"]["allowedRoutes"][1], "/verify-answer");
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

#[test]
fn session_key_inspect_redacts_secret_and_reports_revoked_state() {
    let keys = fresh_tmp("keys-inspect");
    let sessions = fresh_tmp("sessions-inspect");
    setup_session(&keys, &sessions, "claude-inspect");

    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .env("BOOLE_SESSIONS_DIR", &sessions)
        .args(["session-key", "inspect", "--id", "claude-inspect"])
        .output()
        .expect("session-key inspect");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let envelope = parse_json(&out.stdout);
    assert_eq!(envelope["ok"], true);
    assert_eq!(envelope["session"]["id"], "claude-inspect");
    assert_eq!(envelope["session"]["revoked"], false);

    let stdout_text = String::from_utf8(out.stdout.clone()).expect("utf8");
    assert!(
        !stdout_text.contains("\"sessionSk\""),
        "stdout must not contain the `sessionSk` field; got {stdout_text}"
    );
    assert!(
        !stdout_text.contains("\"sk\""),
        "stdout must not contain the `sk` field; got {stdout_text}"
    );
}

#[test]
fn session_key_revoke_local_sets_revoked_true() {
    let keys = fresh_tmp("keys-revoke");
    let sessions = fresh_tmp("sessions-revoke");
    setup_session(&keys, &sessions, "claude-revoke");

    let revoke = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .env("BOOLE_SESSIONS_DIR", &sessions)
        .args(["session-key", "revoke", "--local", "--id", "claude-revoke"])
        .output()
        .expect("session-key revoke");
    assert!(
        revoke.status.success(),
        "revoke failed: stderr={}",
        String::from_utf8_lossy(&revoke.stderr)
    );
    let revoke_envelope = parse_json(&revoke.stdout);
    assert_eq!(revoke_envelope["ok"], true);
    assert_eq!(revoke_envelope["session"]["revoked"], true);
    let revoke_text = String::from_utf8(revoke.stdout.clone()).expect("utf8");
    assert!(
        revoke_text.contains("remote revocation pending"),
        "revoke stdout should warn about pending remote revocation; got {revoke_text}"
    );

    let inspect = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .env("BOOLE_SESSIONS_DIR", &sessions)
        .args(["session-key", "inspect", "--id", "claude-revoke"])
        .output()
        .expect("session-key inspect post-revoke");
    assert!(
        inspect.status.success(),
        "inspect failed: stderr={}",
        String::from_utf8_lossy(&inspect.stderr)
    );
    let inspect_envelope = parse_json(&inspect.stdout);
    assert_eq!(inspect_envelope["session"]["revoked"], true);
}

#[test]
fn session_key_create_requires_explicit_allowed_route() {
    let keys = fresh_tmp("keys-missing-route");
    let sessions = fresh_tmp("sessions-missing-route");

    let owner = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .args(["keys", "new", "--id", "owner", "--dev"])
        .output()
        .expect("owner");
    assert!(owner.status.success());
    let agent = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .args(["keys", "new", "--id", "agent", "--dev"])
        .output()
        .expect("agent");
    assert!(agent.status.success());

    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .env("BOOLE_SESSIONS_DIR", &sessions)
        .args([
            "session-key",
            "create",
            "--local",
            "--id",
            "claude-no-route",
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
    assert_eq!(out.status.code(), Some(2));
    let envelope = parse_json(&out.stderr);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["reason"], "bad_request");
    assert_eq!(envelope["field"], "allowed-route");
}

fn create_session_command(keys: &Path, sessions: &Path, id: &str) -> Command {
    let mut command = cli();
    command
        .env("BOOLE_KEYS_DIR", keys)
        .env("BOOLE_SESSIONS_DIR", sessions)
        .args([
            "session-key",
            "create",
            "--local",
            "--id",
            id,
            "--owner-id",
            "owner",
            "--agent-id",
            "agent",
            "--allowed-route",
            "/submit",
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
        ]);
    command
}

fn write_public_key_fixtures(keys: &Path) {
    std::fs::create_dir_all(keys).expect("create fixture keys directory");
    for (name, pk) in [("owner", "11".repeat(32)), ("agent", "22".repeat(32))] {
        std::fs::write(
            keys.join(format!("{name}.json")),
            serde_json::to_vec(&serde_json::json!({"pk": pk})).expect("serialize public key"),
        )
        .expect("write public key fixture");
    }
}

#[cfg(unix)]
#[test]
fn session_create_does_not_replace_a_reserved_dangling_symlink() {
    use std::os::unix::fs::symlink;
    let dir = fresh_tmp("reserved-path");
    let keys = dir.join("keys");
    let sessions = dir.join("sessions");
    write_public_key_fixtures(&keys);
    std::fs::create_dir(&sessions).expect("create session fixture directory");
    let destination = sessions.join("reserved.json");
    let reserved_target = dir.join("not-yet-created");
    symlink(&reserved_target, &destination).expect("reserve final path");

    let output = create_session_command(&keys, &sessions, "reserved")
        .output()
        .expect("run session create");
    let still_symlink = std::fs::symlink_metadata(&destination)
        .expect("reserved path remains")
        .file_type()
        .is_symlink();
    std::fs::remove_dir_all(&dir).expect("remove isolated fixtures");
    assert_eq!(
        output.status.code(),
        Some(3),
        "an occupied pathname must not be replaced"
    );
    assert_eq!(
        parse_json(&output.stderr)["reason"],
        "session_already_exists"
    );
    assert!(
        still_symlink,
        "a session creator must never overwrite a preexisting directory entry"
    );
}

#[test]
fn session_create_collision_preserves_existing_bytes() {
    let dir = fresh_tmp("existing-path");
    let keys = dir.join("keys");
    let sessions = dir.join("sessions");
    write_public_key_fixtures(&keys);
    std::fs::create_dir(&sessions).expect("session fixture directory");
    let destination = sessions.join("existing.json");
    let original = b"existing fixture bytes; do not replace";
    std::fs::write(&destination, original).expect("reserve existing path");
    let output = create_session_command(&keys, &sessions, "existing")
        .output()
        .expect("session create");
    let after = std::fs::read(&destination).expect("read existing fixture");
    std::fs::remove_dir_all(&dir).expect("remove isolated fixture");
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        parse_json(&output.stderr)["reason"],
        "session_already_exists"
    );
    assert_eq!(after, original);
}

#[test]
fn concurrent_session_creators_publish_exactly_one_winner() {
    use std::sync::{Arc, Barrier};
    let dir = fresh_tmp("concurrent-create");
    let keys = dir.join("keys");
    let sessions = dir.join("sessions");
    write_public_key_fixtures(&keys);
    let barrier = Arc::new(Barrier::new(16));
    let outputs = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let mut command = create_session_command(&keys, &sessions, "same-session");
                scope.spawn(move || {
                    barrier.wait();
                    command.output().expect("run competing creator")
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("creator thread"))
            .collect::<Vec<_>>()
    });
    let disk = parse_json(&std::fs::read(sessions.join("same-session.json")).expect("read winner"));
    let entries = std::fs::read_dir(&sessions)
        .expect("list session files")
        .count();
    std::fs::remove_dir_all(&dir).expect("remove isolated fixtures");
    let winners: Vec<_> = outputs
        .iter()
        .filter(|output| output.status.success())
        .collect();
    assert_eq!(
        winners.len(),
        1,
        "exactly one process may publish a session"
    );
    assert_eq!(
        parse_json(&winners[0].stdout)["session"]["sessionPk"],
        disk["sessionPk"]
    );
    for output in outputs.iter().filter(|output| !output.status.success()) {
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(
            parse_json(&output.stderr)["reason"],
            "session_already_exists"
        );
    }
    assert_eq!(
        entries, 1,
        "failed creators must not leave temporary key files"
    );
}
