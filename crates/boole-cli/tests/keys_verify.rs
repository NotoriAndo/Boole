//! S13a — `boole keys verify --pk <hex32> --signature <hex64> --payload <path|inline>`.
//!
//! Stateless: never touches BOOLE_KEYS_DIR. Default stdout is the bare
//! `valid` or `invalid` word; both exit 0 because the verification ran
//! successfully. Wire-malformed inputs (bad pk/sig hex) → exit 2 with
//! `bad_pk` / `bad_signature` typed envelope on stderr.

use std::process::Command;

use boole_core::SigningKeyV2;
use serde_json::Value;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
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

fn sign_with_random_key(payload: &Value) -> (String, String) {
    let key = SigningKeyV2::from_random().expect("ed25519 key");
    let envelope = key.sign(payload).expect("sign");
    (envelope.pk, envelope.signature)
}

#[test]
fn verify_valid_signature_prints_valid_exit_zero() {
    let payload = serde_json::json!({"k": "v"});
    let (pk, sig) = sign_with_random_key(&payload);
    let out = cli()
        .args([
            "keys",
            "verify",
            "--pk",
            &pk,
            "--signature",
            &sig,
            "--payload",
            &payload.to_string(),
        ])
        .output()
        .expect("run keys verify");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "valid");
}

#[test]
fn verify_tampered_payload_prints_invalid_exit_zero() {
    let original = serde_json::json!({"k": "v", "n": 1});
    let (pk, sig) = sign_with_random_key(&original);
    let tampered = serde_json::json!({"k": "v", "n": 2});

    let out = cli()
        .args([
            "keys",
            "verify",
            "--pk",
            &pk,
            "--signature",
            &sig,
            "--payload",
            &tampered.to_string(),
        ])
        .output()
        .expect("run keys verify");
    assert!(
        out.status.success(),
        "verify itself succeeded; the result is `invalid` not an error: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "invalid");
}

#[test]
fn verify_wrong_pk_prints_invalid_exit_zero() {
    let payload = serde_json::json!({"k": "v"});
    let (_, sig) = sign_with_random_key(&payload);
    let other_key = SigningKeyV2::from_random().expect("other key");

    let out = cli()
        .args([
            "keys",
            "verify",
            "--pk",
            &other_key.pk_hex(),
            "--signature",
            &sig,
            "--payload",
            &payload.to_string(),
        ])
        .output()
        .expect("run keys verify");
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "invalid");
}

#[test]
fn verify_malformed_pk_emits_bad_pk_typed_error() {
    let payload = serde_json::json!({"k": "v"});
    let out = cli()
        .args([
            "keys",
            "verify",
            "--pk",
            "this-is-not-hex",
            "--signature",
            &"0".repeat(128),
            "--payload",
            &payload.to_string(),
        ])
        .output()
        .expect("run keys verify");
    assert!(!out.status.success(), "malformed pk must reject");
    assert_eq!(out.status.code(), Some(2), "bad usage exits 2");
    assert!(
        out.stdout.is_empty(),
        "typed error must not pollute stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let envelope = parse_json(&out.stderr);
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["reason"], "bad_pk");
}

#[test]
fn verify_malformed_signature_emits_bad_signature_typed_error() {
    let payload = serde_json::json!({"k": "v"});
    for signature in ["way-too-short".to_string(), "A".repeat(128)] {
        let out = cli()
            .args([
                "keys",
                "verify",
                "--pk",
                &"0".repeat(64),
                "--signature",
                &signature,
                "--payload",
                &payload.to_string(),
            ])
            .output()
            .expect("run keys verify");
        assert!(!out.status.success());
        assert_eq!(out.status.code(), Some(2));
        let envelope = parse_json(&out.stderr);
        assert_eq!(envelope["ok"], false);
        assert_eq!(envelope["reason"], "bad_signature");
    }
}

// P2.5 — `keys verify --json` migrates from the ad-hoc
// `{"ok":true,"valid":bool}` shape to the unified envelope
// `{"ok":true,"version":"v1","command":"keys.verify","result":{"valid":bool}}`.
// Failures under `--json` also flip to the unified shape on stderr with
// kebab-case `reason` (`bad-pk`/`bad-signature`). The default-mode
// (PlainText) behavior is preserved by the four tests above.

#[test]
fn verify_valid_signature_emits_unified_envelope_when_json_set() {
    let payload = serde_json::json!({"k": "v"});
    let (pk, sig) = sign_with_random_key(&payload);
    let out = cli()
        .args([
            "keys",
            "verify",
            "--json",
            "--pk",
            &pk,
            "--signature",
            &sig,
            "--payload",
            &payload.to_string(),
        ])
        .output()
        .expect("run keys verify --json");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let env = parse_json(&out.stdout);
    assert_eq!(env["ok"], true);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "keys.verify");
    assert_eq!(env["result"]["valid"], true);
    assert!(
        env.get("error").is_none(),
        "success envelope must not carry an error field"
    );
}

#[test]
fn verify_tampered_payload_emits_unified_envelope_when_json_set() {
    let original = serde_json::json!({"k": "v", "n": 1});
    let (pk, sig) = sign_with_random_key(&original);
    let tampered = serde_json::json!({"k": "v", "n": 2});
    let out = cli()
        .args([
            "keys",
            "verify",
            "--json",
            "--pk",
            &pk,
            "--signature",
            &sig,
            "--payload",
            &tampered.to_string(),
        ])
        .output()
        .expect("run keys verify --json");
    assert!(
        out.status.success(),
        "verify ran; result is `valid:false` not an error envelope"
    );
    let env = parse_json(&out.stdout);
    assert_eq!(env["ok"], true);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "keys.verify");
    assert_eq!(env["result"]["valid"], false);
}

#[test]
fn verify_malformed_pk_under_json_emits_unified_error_envelope() {
    let payload = serde_json::json!({"k": "v"});
    let out = cli()
        .args([
            "keys",
            "verify",
            "--json",
            "--pk",
            "this-is-not-hex",
            "--signature",
            &"0".repeat(128),
            "--payload",
            &payload.to_string(),
        ])
        .output()
        .expect("run keys verify --json");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    assert!(
        out.stdout.is_empty(),
        "error envelope must not leak to stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let env = parse_json(&out.stderr);
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "keys.verify");
    assert_eq!(env["error"]["reason"], "bad-pk");
    assert!(
        env.get("result").is_none(),
        "failure envelope must not carry a result field"
    );
}

#[test]
fn verify_malformed_signature_under_json_emits_unified_error_envelope() {
    let payload = serde_json::json!({"k": "v"});
    let out = cli()
        .args([
            "keys",
            "verify",
            "--json",
            "--pk",
            &"0".repeat(64),
            "--signature",
            "way-too-short",
            "--payload",
            &payload.to_string(),
        ])
        .output()
        .expect("run keys verify --json");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let env = parse_json(&out.stderr);
    assert_eq!(env["ok"], false);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "keys.verify");
    assert_eq!(env["error"]["reason"], "bad-signature");
}
