use std::path::Path;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
}

/// work.v2 — reward recipient pk the signed payload authorizes. These tests
/// have no submit flow, so any valid 64-char lowercase hex works.
const REWARD_RECIPIENT: &str = "abababababababababababababababababababababababababababababababab";

fn fresh_tmp(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-signer-{label}-{}-{}",
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

fn payload() -> serde_json::Value {
    serde_json::json!({"artifactHash":"abc"})
}

fn payload_text() -> &'static str {
    "{\"artifactHash\":\"abc\"}"
}

fn payload_hash() -> String {
    boole_core::canonical_payload_hash_hex(&payload())
}

fn setup_session(keys: &Path, sessions: &Path, id: &str) -> String {
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
    let envelope = parse_json(&out.stdout);
    envelope["session"]["sessionPk"]
        .as_str()
        .expect("sessionPk")
        .to_string()
}

#[test]
fn signer_sign_work_allowed_by_policy_emits_bound_signed_v1_without_secret() {
    let keys = fresh_tmp("keys-allow");
    let sessions = fresh_tmp("sessions-allow");
    let nonces = fresh_tmp("nonces-allow");
    let session_pk = setup_session(&keys, &sessions, "claude-allow");
    let request_hash = payload_hash();

    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .env("BOOLE_SESSIONS_DIR", &sessions)
        .env("BOOLE_SIGNER_NONCE_DIR", &nonces)
        .args([
            "signer",
            "sign-work",
            "--network",
            "testnet",
            "--session-id",
            "claude-allow",
            "--route",
            "/submit",
            "--family",
            "boole.protocol-invariant.v01",
            "--verifier",
            "lean-runner-v01",
            "--fee",
            "1",
            "--request-hash",
            &request_hash,
            "--nonce",
            "n1",
            "--reward-recipient",
            REWARD_RECIPIENT,
            "--payload",
            payload_text(),
            "--json",
        ])
        .output()
        .expect("signer sign-work");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout_text = String::from_utf8(out.stdout.clone()).expect("utf8");
    assert!(stdout_text.contains("boole.signed.v1"), "{stdout_text}");
    // P2.5 — `--json` flips the signer surface to the unified envelope.
    // The signed envelope nests under `result.envelope` for the same
    // reason `keys.sign` does: the unified `version: "v1"` describes
    // the CLI schema while `envelope.schema: "boole.signed.v1"`
    // describes the signed payload.
    let env = parse_json(&out.stdout);
    assert_eq!(env["ok"], true);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "signer.sign-work");
    let signed = &env["result"]["envelope"];
    assert_eq!(signed["pk"], session_pk);
    let signed_payload = &signed["payload"];
    assert_eq!(signed_payload["schema"], "boole.signer.work.v2");
    assert_eq!(signed_payload["route"], "/submit");
    assert_eq!(signed_payload["familyId"], "boole.protocol-invariant.v01");
    assert_eq!(signed_payload["verifierId"], "lean-runner-v01");
    assert_eq!(signed_payload["fee"], "1");
    assert_eq!(signed_payload["requestHash"], request_hash);
    assert_eq!(signed_payload["nonce"], "n1");
    assert_eq!(signed_payload["rewardRecipient"], REWARD_RECIPIENT);
    assert_eq!(signed_payload["workPayload"], payload());
    assert!(
        !stdout_text.contains("\"sk\""),
        "stdout must not contain `sk`; got {stdout_text}"
    );
    assert!(
        !stdout_text.contains("\"sessionSk\""),
        "stdout must not contain `sessionSk`; got {stdout_text}"
    );
}

#[test]
fn signer_sign_work_rejects_request_hash_mismatch() {
    let keys = fresh_tmp("keys-mismatch");
    let sessions = fresh_tmp("sessions-mismatch");
    let nonces = fresh_tmp("nonces-mismatch");
    let _ = setup_session(&keys, &sessions, "claude-mismatch");

    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .env("BOOLE_SESSIONS_DIR", &sessions)
        .env("BOOLE_SIGNER_NONCE_DIR", &nonces)
        .args([
            "signer",
            "sign-work",
            "--network",
            "testnet",
            "--session-id",
            "claude-mismatch",
            "--route",
            "/submit",
            "--family",
            "boole.protocol-invariant.v01",
            "--verifier",
            "lean-runner-v01",
            "--fee",
            "1",
            "--request-hash",
            "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
            "--nonce",
            "n1",
            "--reward-recipient",
            REWARD_RECIPIENT,
            "--payload",
            payload_text(),
            "--json",
        ])
        .output()
        .expect("signer sign-work");
    assert_eq!(out.status.code(), Some(3));
    let env = parse_json(&out.stderr);
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "signer.sign-work");
    assert_eq!(env["error"]["reason"], "request-hash-mismatch");
}

#[test]
fn signer_sign_work_denies_route_not_in_session_envelope() {
    let keys = fresh_tmp("keys-route");
    let sessions = fresh_tmp("sessions-route");
    let nonces = fresh_tmp("nonces-route");
    let _ = setup_session(&keys, &sessions, "claude-route");
    let request_hash = payload_hash();

    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .env("BOOLE_SESSIONS_DIR", &sessions)
        .env("BOOLE_SIGNER_NONCE_DIR", &nonces)
        .args([
            "signer",
            "sign-work",
            "--network",
            "testnet",
            "--session-id",
            "claude-route",
            "--route",
            "/withdraw",
            "--family",
            "boole.protocol-invariant.v01",
            "--verifier",
            "lean-runner-v01",
            "--fee",
            "1",
            "--request-hash",
            &request_hash,
            "--nonce",
            "n1",
            "--reward-recipient",
            REWARD_RECIPIENT,
            "--payload",
            payload_text(),
            "--json",
        ])
        .output()
        .expect("signer sign-work");
    assert_eq!(out.status.code(), Some(3));
    let env = parse_json(&out.stderr);
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "signer.sign-work");
    assert_eq!(env["error"]["reason"], "policy-denied");
    assert!(env["error"]["detail"]
        .as_str()
        .unwrap_or("")
        .contains("route"));
}

#[test]
fn signer_sign_work_denies_over_fee_with_policy_denied() {
    let keys = fresh_tmp("keys-deny");
    let sessions = fresh_tmp("sessions-deny");
    let nonces = fresh_tmp("nonces-deny");
    let _ = setup_session(&keys, &sessions, "claude-deny");
    let request_hash = payload_hash();

    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .env("BOOLE_SESSIONS_DIR", &sessions)
        .env("BOOLE_SIGNER_NONCE_DIR", &nonces)
        .args([
            "signer",
            "sign-work",
            "--network",
            "testnet",
            "--session-id",
            "claude-deny",
            "--route",
            "/submit",
            "--family",
            "boole.protocol-invariant.v01",
            "--verifier",
            "lean-runner-v01",
            "--fee",
            "999",
            "--request-hash",
            &request_hash,
            "--nonce",
            "n1",
            "--reward-recipient",
            REWARD_RECIPIENT,
            "--payload",
            payload_text(),
            "--json",
        ])
        .output()
        .expect("signer sign-work");
    assert_eq!(
        out.status.code(),
        Some(3),
        "expected exit 3; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let env = parse_json(&out.stderr);
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "signer.sign-work");
    assert_eq!(env["error"]["reason"], "policy-denied");
}

#[test]
fn signer_sign_work_rejects_duplicate_nonce() {
    let keys = fresh_tmp("keys-nonce");
    let sessions = fresh_tmp("sessions-nonce");
    let nonces = fresh_tmp("nonces-nonce");
    let _ = setup_session(&keys, &sessions, "claude-nonce");
    let request_hash = payload_hash();

    let common_args = [
        "signer",
        "sign-work",
        "--network",
        "testnet",
        "--session-id",
        "claude-nonce",
        "--route",
        "/submit",
        "--family",
        "boole.protocol-invariant.v01",
        "--verifier",
        "lean-runner-v01",
        "--fee",
        "1",
        "--request-hash",
        &request_hash,
        "--nonce",
        "n-dup",
        "--reward-recipient",
        REWARD_RECIPIENT,
        "--payload",
        payload_text(),
        "--json",
    ];

    let first = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .env("BOOLE_SESSIONS_DIR", &sessions)
        .env("BOOLE_SIGNER_NONCE_DIR", &nonces)
        .args(common_args)
        .output()
        .expect("first sign");
    assert!(
        first.status.success(),
        "first sign failed: stderr={}",
        String::from_utf8_lossy(&first.stderr)
    );

    let second = cli()
        .env("BOOLE_KEYS_DIR", &keys)
        .env("BOOLE_SESSIONS_DIR", &sessions)
        .env("BOOLE_SIGNER_NONCE_DIR", &nonces)
        .args(common_args)
        .output()
        .expect("second sign");
    assert_eq!(
        second.status.code(),
        Some(3),
        "expected exit 3 on duplicate nonce; stderr={}",
        String::from_utf8_lossy(&second.stderr)
    );
    let env = parse_json(&second.stderr);
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "signer.sign-work");
    assert_eq!(env["error"]["reason"], "nonce-reuse");
}
