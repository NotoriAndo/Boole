// P2.5 — unified CLI JSON envelope contract.
//
// Boole CLI commands that emit JSON must do so through a single envelope
// shape so downstream tools (operator scripts, IDE plugins, the boole-mcp
// proxy) can parse every command's output with a single schema instead of
// per-command bespoke shapes.
//
// Envelope shape (schema "v1"):
//
//   success: {"ok": true,  "version": "v1", "command": "<dotted-path>", "result": <any>}
//   failure: {"ok": false, "version": "v1", "command": "<dotted-path>", "error": {"reason": "<kebab>", ...}}
//
// The "version" key is the envelope schema version, NOT a domain-data
// field. Command-specific data lives strictly inside `result` (success) or
// `error` (failure) so a top-level reader never confuses envelope metadata
// with payload.
//
// These tests pin the contract on two surfaces:
//   (1) `boole_cli::cli_envelope::Envelope` shape — pure unit-level checks.
//   (2) `boole_cli::cli_envelope::INVENTORY` matrix — every leaf CLI
//       command appears with a declared output kind, so a future drift
//       (someone adds a subcommand and forgets to classify its JSON
//       behavior) trips a test instead of shipping silently.

use boole_cli::cli_envelope::{
    encode_err, encode_ok, OutputKind, COMMAND_INVENTORY, ENVELOPE_VERSION,
};
use serde_json::{json, Value};

#[test]
fn ok_envelope_has_expected_shape() {
    let body = encode_ok("version", json!({"name": "boole", "version": "0.1.0"}));
    let parsed: Value = serde_json::from_str(&body).expect("envelope parses as JSON");
    assert_eq!(parsed["ok"], json!(true), "ok must be literal true");
    assert_eq!(
        parsed["version"],
        json!(ENVELOPE_VERSION),
        "top-level version is the envelope schema version, not domain data"
    );
    assert_eq!(parsed["command"], json!("version"));
    assert_eq!(
        parsed["result"],
        json!({"name": "boole", "version": "0.1.0"})
    );
    assert!(
        parsed.get("error").is_none(),
        "success envelope must not carry an error field"
    );
}

#[test]
fn err_envelope_has_expected_shape() {
    let body = encode_err("keys.sign", "missing-arg", json!({"arg": "key_id"}));
    let parsed: Value = serde_json::from_str(&body).expect("envelope parses as JSON");
    assert_eq!(parsed["ok"], json!(false));
    assert_eq!(parsed["version"], json!(ENVELOPE_VERSION));
    assert_eq!(parsed["command"], json!("keys.sign"));
    let error = parsed.get("error").expect("error field present on failure");
    assert_eq!(error["reason"], json!("missing-arg"));
    assert_eq!(error["arg"], json!("key_id"));
    assert!(
        parsed.get("result").is_none(),
        "failure envelope must not carry a result field"
    );
}

#[test]
fn err_envelope_accepts_no_extra_fields() {
    let body = encode_err("chain.replay", "bad-fixture", Value::Null);
    let parsed: Value = serde_json::from_str(&body).expect("envelope parses as JSON");
    let error = parsed.get("error").expect("error field present on failure");
    assert_eq!(error["reason"], json!("bad-fixture"));
    let obj = error.as_object().expect("error is JSON object");
    assert_eq!(
        obj.len(),
        1,
        "with null extras the error object only carries `reason`"
    );
}

#[test]
fn envelope_schema_version_is_v1() {
    assert_eq!(ENVELOPE_VERSION, "v1");
}

#[test]
fn inventory_covers_known_command_paths() {
    // Drift guard: if a clap subcommand is added or removed, this matrix
    // (and consequently the inventory test) must be updated in the same
    // commit. The list below mirrors the explore inventory captured for
    // P2.5; it is the canonical "what is the CLI surface today" record.
    let expected_paths: &[&[&str]] = &[
        &["version"],
        &["chain", "replay"],
        &["chain", "audit-receipts"],
        &["chain", "settlement-report"],
        &["node", "start"],
        &["block", "latest"],
        &["block", "get"],
        &["account", "balance"],
        &["reputation", "inspect"],
        &["work", "list"],
        &["work", "get"],
        &["bounty", "list"],
        &["bounty", "get"],
        &["bounty", "submit"],
        &["bounty", "announce"],
        &["bounty", "status"],
        &["keys", "new"],
        &["keys", "list"],
        &["keys", "show"],
        &["keys", "sign"],
        &["keys", "verify"],
        &["keys", "export-secret"],
        &["session-key", "create"],
        &["session-key", "inspect"],
        &["session-key", "revoke"],
        &["signer", "sign-work"],
        &["state", "verify"],
        &["mine", "init"],
        &["mine", "address"],
        &["mine", "config", "get"],
        &["mine", "config", "set"],
        &["mine", "start"],
        &["mine", "bounty"],
        &["wallet", "init"],
        &["wallet", "address"],
        &["wallet", "sign"],
        &["wallet", "migrate"],
    ];

    let actual_paths: Vec<&[&str]> = COMMAND_INVENTORY.iter().map(|c| c.path).collect();
    assert_eq!(
        actual_paths.len(),
        expected_paths.len(),
        "inventory entry count drifted from the captured CLI surface"
    );
    for expected in expected_paths {
        assert!(
            actual_paths.contains(expected),
            "inventory missing command path {expected:?}"
        );
    }
}

#[test]
fn inventory_kinds_are_well_formed() {
    for entry in COMMAND_INVENTORY {
        assert!(
            !entry.path.is_empty(),
            "inventory entry has empty command path"
        );
        // If --json is not accepted, the json-mode output kind must match
        // the default-mode kind: there is no toggle to observe.
        if !entry.has_json_flag {
            assert_eq!(
                entry.output_with_json, entry.output_default,
                "command {:?} has no --json flag but declares divergent kinds",
                entry.path
            );
        }
        // EventStream and JsonAlways are properties of the command's only
        // possible output mode; rejecting them on the json side of a
        // toggleable command catches the mis-classification.
        if entry.has_json_flag {
            assert!(
                !matches!(entry.output_with_json, OutputKind::EventStream),
                "command {:?} cannot both have a --json flag and stream NDJSON events",
                entry.path
            );
        }
    }
}

#[test]
fn version_subcommand_emits_unified_envelope_when_json_set() {
    // Reference migration: `boole version --json` is the first command to
    // adopt the unified envelope. The shape contract from
    // `ok_envelope_has_expected_shape` is asserted here against the real
    // binary output so the helper is wired into the actual CLI path, not
    // just unit-callable.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["version", "--json"])
        .output()
        .expect("spawn boole-cli");
    assert!(
        out.status.success(),
        "version --json exit status non-zero: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("stdout is UTF-8");
    let parsed: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("version --json output not JSON: {e}: {stdout}"));
    assert_eq!(parsed["ok"], json!(true));
    assert_eq!(parsed["version"], json!(ENVELOPE_VERSION));
    assert_eq!(parsed["command"], json!("version"));
    let result = parsed.get("result").expect("envelope carries result field");
    assert_eq!(result["name"], json!("boole"));
    assert!(
        result.get("version").is_some(),
        "domain `version` lives inside result, not top-level"
    );
}
