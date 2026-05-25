//! S13a — `boole keys sign --id <id> --payload <path|inline>`.
//!
//! Loads a stored v2 key, signs `payload` with ed25519, prints the bare
//! hex64 signature on stdout (or the full `boole.signed.v1` envelope under
//! `--json`). v1 keys are refused with `legacy_v1_key`. Errors exit 3 for
//! refused-operation (not_found / legacy / wrong_id) and 2 for bad usage.

use std::path::Path;
use std::process::Command;

use boole_core::verify_signature;
use serde_json::Value;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
}

fn fresh_tmp(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-keys-sign-{}-{}-{}",
        label,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn parse_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or_else(|err| {
        panic!(
            "expected JSON: {} (raw={})",
            err,
            String::from_utf8_lossy(bytes)
        )
    })
}

fn make_v2_key(dir: &Path, id: &str) -> Value {
    std::fs::create_dir_all(dir).expect("mkdir");
    let out = cli()
        .env("BOOLE_KEYS_DIR", dir)
        .args(["keys", "new", "--id", id, "--dev"])
        .output()
        .expect("run keys new");
    assert!(
        out.status.success(),
        "keys new failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    parse_json(&out.stdout)["key"].clone()
}

#[test]
fn sign_inline_payload_emits_hex64_signature_that_verifies_against_pk() {
    let dir = fresh_tmp("inline");
    let key = make_v2_key(&dir, "alice");
    let pk_hex = key["pk"].as_str().expect("pk").to_string();
    let payload_str = r#"{"hello":"world"}"#;

    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "sign", "--id", "alice", "--payload", payload_str])
        .output()
        .expect("run keys sign");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let sig_hex = stdout.trim();
    assert_eq!(sig_hex.len(), 128, "ed25519 sig hex64: {sig_hex}");
    assert!(
        sig_hex
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        "lowercase hex only: {sig_hex}"
    );

    let payload: Value = serde_json::from_str(payload_str).expect("payload json");
    let valid = verify_signature(&pk_hex, sig_hex, &payload).expect("verify must succeed");
    assert!(valid, "round-trip sign+verify must accept");
}

#[test]
fn sign_against_v1_key_returns_legacy_v1_key_typed_error() {
    let dir = fresh_tmp("legacy-sign");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let v1 = serde_json::json!({
        "schema": "boole.keys.v1",
        "id": "old-bob",
        "pk": "00".repeat(32),
        "createdAt": "2025-01-01T00:00:00Z",
    });
    std::fs::write(dir.join("old-bob.json"), v1.to_string()).expect("write v1");

    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "sign", "--id", "old-bob", "--payload", "{}"])
        .output()
        .expect("run keys sign");
    assert!(!out.status.success(), "v1 keys cannot sign");
    assert_eq!(out.status.code(), Some(3), "refused operation exits 3");
    assert!(
        out.stdout.is_empty(),
        "typed error must not pollute stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let envelope = parse_json(&out.stderr);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["reason"], "legacy_v1_key");
    assert_eq!(envelope["id"], "old-bob");
    assert_eq!(envelope["schema"], "boole.keys.v1");
}

#[test]
fn sign_with_unknown_id_returns_key_not_found() {
    let dir = fresh_tmp("missing-sign");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "sign", "--id", "ghost", "--payload", "{}"])
        .output()
        .expect("run keys sign");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(3));
    let envelope = parse_json(&out.stderr);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["reason"], "key_not_found");
    assert_eq!(envelope["id"], "ghost");
}

#[test]
fn sign_payload_from_file_path_matches_inline_signature() {
    let dir = fresh_tmp("file-payload");
    let key = make_v2_key(&dir, "alice");
    let pk_hex = key["pk"].as_str().expect("pk").to_string();
    let payload = serde_json::json!({"a": 1, "b": [true, false]});
    let payload_path = dir.join("payload.json");
    std::fs::write(&payload_path, payload.to_string()).expect("write payload");

    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args([
            "keys",
            "sign",
            "--id",
            "alice",
            "--payload",
            payload_path.to_str().expect("utf8"),
        ])
        .output()
        .expect("run keys sign");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let sig_hex = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(sig_hex.len(), 128);
    let valid = verify_signature(&pk_hex, &sig_hex, &payload).expect("verify");
    assert!(valid, "file-loaded payload must verify");
}

// P2.5 — `keys sign --json` migrates from the ad-hoc
// `{"ok":true,"envelope":{..}}` shape to the unified envelope shape
// `{"ok":true,"version":"v1","command":"keys.sign","result":{"envelope":{..}}}`.
// The signed envelope is nested under `result.envelope` (not flattened
// into `result`) so the unified envelope's `version: "v1"` describes the
// CLI schema while the nested `envelope.schema: "boole.signed.v1"`
// describes the signed payload — two distinct schemas, never confused.
// Failures under `--json` also flip to the unified shape on stderr with
// kebab-case `reason` tokens.

#[test]
fn sign_json_flag_emits_unified_envelope_with_signed_envelope_under_result() {
    let dir = fresh_tmp("sign-json");
    let key = make_v2_key(&dir, "alice");
    let pk_hex = key["pk"].as_str().expect("pk").to_string();
    let payload = serde_json::json!({"action": "announce", "n": 7});

    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args([
            "keys",
            "sign",
            "--id",
            "alice",
            "--payload",
            &payload.to_string(),
            "--json",
        ])
        .output()
        .expect("run keys sign --json");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let env = parse_json(&out.stdout);
    assert_eq!(env["ok"], true);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "keys.sign");
    let signed = &env["result"]["envelope"];
    assert_eq!(signed["schema"], "boole.signed.v1");
    assert_eq!(signed["payload"], payload);
    assert_eq!(signed["pk"], pk_hex);
    let sig = signed["signature"].as_str().expect("sig");
    assert_eq!(sig.len(), 128);
    let valid = verify_signature(&pk_hex, sig, &payload).expect("verify");
    assert!(valid);
    assert!(
        env.get("error").is_none(),
        "success envelope must not carry an error field"
    );
}

#[test]
fn sign_against_v1_key_under_json_emits_unified_error_envelope() {
    let dir = fresh_tmp("legacy-sign-json");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let v1 = serde_json::json!({
        "schema": "boole.keys.v1",
        "id": "old-bob",
        "pk": "00".repeat(32),
        "createdAt": "2025-01-01T00:00:00Z",
    });
    std::fs::write(dir.join("old-bob.json"), v1.to_string()).expect("write v1");

    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args([
            "keys",
            "sign",
            "--id",
            "old-bob",
            "--payload",
            "{}",
            "--json",
        ])
        .output()
        .expect("run keys sign --json");
    assert!(!out.status.success(), "v1 keys cannot sign");
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty());
    let env = parse_json(&out.stderr);
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "keys.sign");
    assert_eq!(env["error"]["reason"], "legacy-v1-key");
    assert_eq!(env["error"]["id"], "old-bob");
    assert!(
        env.get("result").is_none(),
        "failure envelope must not carry a result field"
    );
}

#[test]
fn sign_with_unknown_id_under_json_emits_unified_error_envelope() {
    let dir = fresh_tmp("missing-sign-json");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args(["keys", "sign", "--id", "ghost", "--payload", "{}", "--json"])
        .output()
        .expect("run keys sign --json");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(3));
    let env = parse_json(&out.stderr);
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "keys.sign");
    assert_eq!(env["error"]["reason"], "key-not-found");
    assert_eq!(env["error"]["id"], "ghost");
}

#[test]
fn sign_with_bad_id_under_json_emits_unified_error_envelope() {
    let dir = fresh_tmp("bad-id-sign-json");
    std::fs::create_dir_all(&dir).expect("mkdir");
    // `/` is rejected by validate_key_id (path-traversal guard).
    let out = cli()
        .env("BOOLE_KEYS_DIR", &dir)
        .args([
            "keys",
            "sign",
            "--id",
            "../escape",
            "--payload",
            "{}",
            "--json",
        ])
        .output()
        .expect("run keys sign --json");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    let env = parse_json(&out.stderr);
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "keys.sign");
    assert_eq!(env["error"]["reason"], "bad-request");
    assert_eq!(env["error"]["field"], "id");
}
