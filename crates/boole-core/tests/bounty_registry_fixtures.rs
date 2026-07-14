use boole_core::{BountyRegistry, CreateBountyInput, SubmitProofInput, UpdateStatusInput};
use serde::Deserialize;
use serde_json::{json, Map, Value};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    operations: Vec<OperationCase>,
    event_log: Vec<Value>,
    recovery_expected: RecoveryExpected,
}

#[derive(Debug, Deserialize)]
struct OperationCase {
    name: String,
    ok: bool,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecoveryExpected {
    list: Vec<Value>,
    list_open: Vec<Value>,
    size: usize,
    has_rejected_proof_alpha: bool,
    has_accepted_proof_alpha: bool,
}

#[test]
fn bounty_registry_matches_typescript_golden_fixture() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/protocol/bounty-registry/v1.json"
    ))
    .expect("fixture parses");

    let mut registry = BountyRegistry::new();

    for op in &fixture.operations {
        let got = apply_named_operation(&mut registry, &op.name);
        match (op.ok, got) {
            (true, Ok(value)) => assert_eq!(
                value,
                op.result.clone().unwrap_or(Value::Null),
                "{}",
                op.name
            ),
            (false, Err(message)) => assert_eq!(
                message,
                op.error.clone().expect("expected error"),
                "{}",
                op.name
            ),
            (true, Err(message)) => panic!("{} unexpectedly failed: {}", op.name, message),
            (false, Ok(value)) => panic!("{} unexpectedly succeeded: {}", op.name, value),
        }
    }

    let mut recovered = BountyRegistry::new();
    for event in &fixture.event_log {
        recovered
            .apply_event_fixture(event.clone())
            .expect("event replays");
    }

    assert_eq!(recovered.size(), fixture.recovery_expected.size);
    assert_eq!(
        serde_json::to_value(recovered.list()).unwrap(),
        Value::Array(fixture.recovery_expected.list)
    );
    assert_eq!(
        serde_json::to_value(recovered.list_open()).unwrap(),
        Value::Array(fixture.recovery_expected.list_open)
    );
    assert_eq!(
        recovered.has_proof("alpha-1", &hex1()),
        Some(fixture.recovery_expected.has_rejected_proof_alpha)
    );
    assert_eq!(
        recovered.has_proof("alpha-1", &hex2()),
        Some(fixture.recovery_expected.has_accepted_proof_alpha)
    );
}

fn apply_named_operation(registry: &mut BountyRegistry, name: &str) -> Result<Value, String> {
    match name {
        "create_alpha" => registry.create(create_alpha()).map(to_json),
        "duplicate_create_alpha" => registry.create(create_alpha()).map(to_json),
        "bad_id_whitespace" => registry
            .create(CreateBountyInput {
                id: "bad id".to_string(),
                ..create_alpha()
            })
            .map(to_json),
        "bad_problem_hash_uppercase" => registry
            .create(CreateBountyInput {
                id: "bad-hash".to_string(),
                problem_hash: hex_a().to_uppercase(),
                ..create_alpha()
            })
            .map(to_json),
        "bad_reward_zero" => registry
            .create(CreateBountyInput {
                id: "zero-reward".to_string(),
                reward: 0,
                ..create_alpha()
            })
            .map(to_json),
        "create_beta" => registry.create(create_beta()).map(to_json),
        "create_gamma" => registry.create(create_gamma()).map(to_json),
        "list_open_initial" => Ok(to_json(registry.list_open())),
        "reject_proof_alpha" => registry
            .submit_proof(SubmitProofInput {
                bounty_id: "alpha-1".to_string(),
                proof_hash: hex1(),
                prover: hex2(),
                accepted: false,
                ts: 1800000001000,
            })
            .map(to_json),
        "has_rejected_proof_alpha" => Ok(to_json(registry.has_proof("alpha-1", &hex1()))),
        "duplicate_rejected_proof_alpha" => registry
            .submit_proof(SubmitProofInput {
                bounty_id: "alpha-1".to_string(),
                proof_hash: hex1(),
                prover: hex2(),
                accepted: true,
                ts: 1800000001100,
            })
            .map(to_json),
        "accept_proof_alpha" => registry
            .submit_proof(SubmitProofInput {
                bounty_id: "alpha-1".to_string(),
                proof_hash: hex2(),
                prover: hex1(),
                accepted: true,
                ts: 1800000001200,
            })
            .map(to_json),
        "duplicate_accepted_proof_alpha_terminal" => registry
            .submit_proof(SubmitProofInput {
                bounty_id: "alpha-1".to_string(),
                proof_hash: hex2(),
                prover: hex1(),
                accepted: true,
                ts: 1800000001300,
            })
            .map(to_json),
        "new_proof_terminal_alpha" => registry
            .submit_proof(SubmitProofInput {
                bounty_id: "alpha-1".to_string(),
                proof_hash: hex3(),
                prover: hex1(),
                accepted: false,
                ts: 1800000001400,
            })
            .map(to_json),
        "withdraw_beta" => registry
            .update_status(UpdateStatusInput {
                id: "beta-1".to_string(),
                status: "withdrawn".to_string(),
                ts: 1800000001500,
            })
            .map(to_json),
        "terminal_transition_beta" => registry
            .update_status(UpdateStatusInput {
                id: "beta-1".to_string(),
                status: "open".to_string(),
                ts: 1800000001600,
            })
            .map(to_json),
        "list_open_final" => Ok(to_json(registry.list_open())),
        "size" => Ok(to_json(registry.size())),
        "get_alpha" => Ok(to_json(registry.get("alpha-1"))),
        "get_missing" => Ok(to_json(registry.get("missing"))),
        other => panic!("unknown op {other}"),
    }
}

fn to_json<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap()
}

fn create_alpha() -> CreateBountyInput {
    CreateBountyInput {
        id: "alpha-1".to_string(),
        domain: "lean.protocol-invariant".to_string(),
        problem_hash: hex_a(),
        verifier_kind: "lean".to_string(),
        verifier_metadata: metadata(&[("verifierHash", json!(hex_b())), ("profile", json!("v1"))]),
        reward: 7,
        deadline: 1900000000000,
        ts: 1800000000000,
    }
}

fn create_beta() -> CreateBountyInput {
    CreateBountyInput {
        id: "beta-1".to_string(),
        domain: "code.spec-template".to_string(),
        problem_hash: hex_c(),
        verifier_kind: "wasm".to_string(),
        verifier_metadata: metadata(&[
            ("verifierHash", json!(hex_d())),
            ("template", json!("parser-roundtrip.v01")),
        ]),
        reward: 11,
        deadline: 1900000005000,
        ts: 1800000000500,
    }
}

fn create_gamma() -> CreateBountyInput {
    CreateBountyInput {
        id: "gamma-1".to_string(),
        domain: "lean.protocol-invariant".to_string(),
        problem_hash: hex_e(),
        verifier_kind: "lean".to_string(),
        verifier_metadata: metadata(&[("verifierHash", json!(hex_f())), ("profile", json!("v2"))]),
        reward: 13,
        deadline: 1900000001000,
        ts: 1800000000600,
    }
}

fn metadata(values: &[(&str, Value)]) -> Map<String, Value> {
    values
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}
fn hex_a() -> String {
    "aa".repeat(32)
}
fn hex_b() -> String {
    "bb".repeat(32)
}
fn hex_c() -> String {
    "cc".repeat(32)
}
fn hex_d() -> String {
    "dd".repeat(32)
}
fn hex_e() -> String {
    "ee".repeat(32)
}
fn hex_f() -> String {
    "ff".repeat(32)
}
fn hex1() -> String {
    "11".repeat(32)
}
fn hex2() -> String {
    "22".repeat(32)
}
fn hex3() -> String {
    "33".repeat(32)
}

// P1.5d — `apply_event_fixture` must enforce the same status-transition
// guard as the live `update_status` path. Without unification, a
// fixture or audit-log replay could swing a bounty from a terminal
// status (`solved`, `expired`, `withdrawn`) back to `open`, drifting
// the registry from what the live HTTP route would have allowed.

#[test]
fn apply_event_fixture_rejects_terminal_to_open_transition() {
    let mut registry = BountyRegistry::new();
    let _ = registry.create(create_alpha()).expect("seed alpha");

    let solve = json!({
        "kind": "status",
        "id": "alpha-1",
        "status": "solved",
        "ts": 1_800_000_001_000u64,
    });
    registry
        .apply_event_fixture(solve)
        .expect("solved is a legal first transition from open");

    let revert = json!({
        "kind": "status",
        "id": "alpha-1",
        "status": "open",
        "ts": 1_800_000_002_000u64,
    });
    let err = registry
        .apply_event_fixture(revert)
        .expect_err("solved->open must be rejected");
    assert!(
        err.contains("terminal status solved"),
        "error must name the terminal status: {err}"
    );
}

#[test]
fn apply_event_fixture_rejects_unknown_status_value() {
    let mut registry = BountyRegistry::new();
    let _ = registry.create(create_alpha()).expect("seed alpha");

    let bad = json!({
        "kind": "status",
        "id": "alpha-1",
        "status": "bogus",
        "ts": 1_800_000_001_000u64,
    });
    let err = registry
        .apply_event_fixture(bad)
        .expect_err("unknown status must be rejected");
    assert!(
        err.contains("invalid status: bogus"),
        "error must name the invalid status: {err}"
    );
}
