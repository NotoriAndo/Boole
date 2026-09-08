#![cfg(feature = "development-task-admission")]

use boole_native_shadow_protocol::{
    fresh_answer_canary::{
        development_task::{self, DevelopmentTaskGrant, DevelopmentTaskSpec},
        CanaryBindings, CanaryTrustRoot,
    },
    sha256_hex,
};
use ed25519_dalek::{Signer, SigningKey};

fn spec(seed: &str) -> DevelopmentTaskSpec {
    DevelopmentTaskSpec {
        type_name: "DevelopmentPoint".into(),
        field_types: vec!["i32".into(), "bool".into()],
        task_seed: seed.repeat(32),
        a0: 17,
        mul: 3,
        coeffs: vec![2, -5],
    }
}

#[test]
fn signed_scope_expansion_bad_specs_and_material_substitution_are_rejected() {
    let key = SigningKey::from_bytes(&[51; 32]);
    let root = CanaryTrustRoot::new(key.verifying_key().to_bytes()).unwrap();
    let original = serde_json::to_value(
        DevelopmentTaskGrant::one_task("11".repeat(32), "22".repeat(32), 30, spec("33")).unwrap(),
    )
    .unwrap();
    for (section, field, replacement) in [
        ("", "activationAllowed", serde_json::json!(true)),
        ("", "nonIssuable", serde_json::json!(false)),
        ("", "maxCheckerExecutions", serde_json::json!(2)),
        ("", "extra", serde_json::json!("unrecognized")),
        (
            "spec",
            "typeName",
            serde_json::json!("Point; fn injected() {}"),
        ),
        (
            "spec",
            "fieldTypes",
            serde_json::json!(["std::fs::File", "bool"]),
        ),
        ("spec", "coeffs", serde_json::json!([1])),
        ("spec", "a0", serde_json::json!(1_000_001)),
        ("spec", "taskSeed", serde_json::json!("aa".repeat(32))),
        ("bindings", "taskDigest", serde_json::json!("aa".repeat(32))),
        (
            "bindings",
            "checkerArtifactHash",
            serde_json::json!("aa".repeat(32)),
        ),
        (
            "bindings",
            "executionPolicyDigest",
            serde_json::json!("aa".repeat(32)),
        ),
        (
            "bindings",
            "registryDigest",
            serde_json::json!("aa".repeat(32)),
        ),
    ] {
        let mut value = original.clone();
        if section.is_empty() {
            value[field] = replacement;
        } else {
            value[section][field] = replacement;
        }
        let bytes = serde_json::to_vec(&value).unwrap();
        let signature = key.sign(&[development_task::SIGNING_DOMAIN, bytes.as_slice()].concat());
        assert!(
            development_task::verify_grant(&bytes, &signature.to_bytes(), &root).is_err(),
            "{section}/{field}"
        );
    }
    let bytes = serde_json::to_vec(&original).unwrap();
    let signature = key.sign(&[development_task::SIGNING_DOMAIN, bytes.as_slice()].concat());
    let other =
        CanaryTrustRoot::new(SigningKey::from_bytes(&[52; 32]).verifying_key().to_bytes()).unwrap();
    assert!(development_task::verify_grant(&bytes, &signature.to_bytes(), &other).is_err());
    assert!(development_task::verify_grant(&bytes, &key.sign(&bytes).to_bytes(), &root).is_err());
}

#[cfg(unix)]
#[test]
fn offline_task_admission_exports_the_exact_signed_problem_and_never_overwrites() {
    use std::os::unix::fs::PermissionsExt;
    let path =
        std::env::temp_dir().join(format!("boole-development-task-cli-{}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let key = SigningKey::from_bytes(&[51; 32]);
    std::fs::write(path.join("seed"), key.to_bytes()).unwrap();
    std::fs::set_permissions(path.join("seed"), std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(
        path.join("spec.json"),
        serde_json::to_vec(&spec("33")).unwrap(),
    )
    .unwrap();
    let invoke = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_boole-canary-authority"))
            .arg("create-task-grant")
            .arg(path.join("seed"))
            .arg(path.join("spec.json"))
            .arg("11".repeat(32))
            .arg("22".repeat(32))
            .arg("30")
            .arg(&path)
            .output()
            .unwrap()
    };
    let result = invoke();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!invoke().status.success());
    let grant = development_task::verify_grant(
        &std::fs::read(path.join("grant.json")).unwrap(),
        &std::fs::read(path.join("grant.sig")).unwrap(),
        &CanaryTrustRoot::new(key.verifying_key().to_bytes()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(path.join("task.json")).unwrap(),
        grant.task_bytes()
    );
    assert_eq!(
        std::fs::read(path.join("anchor.rs")).unwrap(),
        grant.anchor_bytes()
    );
    for name in [
        "seed",
        "spec.json",
        "grant.json",
        "grant.sig",
        "operator-public-key.bin",
        "task.json",
        "anchor.rs",
    ] {
        std::fs::remove_file(path.join(name)).unwrap();
    }
    std::fs::remove_dir(path).unwrap();
}

#[test]
fn new_problem_is_signed_and_bound_without_becoming_historical_canary_authority() {
    let key = SigningKey::from_bytes(&[51; 32]);
    let root = CanaryTrustRoot::new(key.verifying_key().to_bytes()).unwrap();
    let grant =
        DevelopmentTaskGrant::one_task("11".repeat(32), "22".repeat(32), 30, spec("33")).unwrap();
    let bytes = serde_json::to_vec(&grant).unwrap();
    let signature = key.sign(&[development_task::SIGNING_DOMAIN, bytes.as_slice()].concat());
    let verified = development_task::verify_grant(&bytes, &signature.to_bytes(), &root).unwrap();
    assert_ne!(
        verified.bindings().get("taskDigest"),
        CanaryBindings::installed().unwrap().get("taskDigest")
    );
    assert_eq!(
        verified.bindings().get("taskDigest"),
        sha256_hex(verified.task_bytes())
    );
    assert!(
        boole_native_shadow_protocol::fresh_answer_canary::verify_grant(
            &bytes,
            &signature.to_bytes(),
            &root,
            &CanaryBindings::installed().unwrap(),
        )
        .is_err()
    );
    let request = verified
        .prepare_request(
            &"44".repeat(32),
            b"```rust\nfn new_answer() {}\n```",
            b"fn new_answer() {}",
        )
        .unwrap();
    assert!(verified.validate_request(&request).is_ok());
    assert_eq!(request.task_digest_hex(), sha256_hex(verified.task_bytes()));
}

#[test]
fn changing_constants_with_the_same_seed_changes_public_problem_identity() {
    let first = serde_json::to_value(
        DevelopmentTaskGrant::one_task("11".repeat(32), "22".repeat(32), 30, spec("33")).unwrap(),
    )
    .unwrap();
    let mut changed = spec("33");
    changed.a0 += 1;
    let second = serde_json::to_value(
        DevelopmentTaskGrant::one_task("11".repeat(32), "22".repeat(32), 30, changed).unwrap(),
    )
    .unwrap();
    assert_ne!(
        first["bindings"]["challengeSha256"],
        second["bindings"]["challengeSha256"]
    );
}

#[cfg(unix)]
#[test]
fn execution_consumes_the_new_task_and_restart_cannot_reexecute_or_swap_problems() {
    use boole_native_shadow_protocol::fresh_answer_canary::{CanaryBudget, CanaryBudgetRole};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let key = SigningKey::from_bytes(&[51; 32]);
    let root = CanaryTrustRoot::new(key.verifying_key().to_bytes()).unwrap();
    let make = |seed| {
        let bytes = serde_json::to_vec(
            &DevelopmentTaskGrant::one_task("11".repeat(32), "22".repeat(32), 30, spec(seed))
                .unwrap(),
        )
        .unwrap();
        let signature = key.sign(&[development_task::SIGNING_DOMAIN, bytes.as_slice()].concat());
        development_task::verify_grant(&bytes, &signature.to_bytes(), &root).unwrap()
    };
    let grant = make("33");
    let different = make("55");
    let request = grant
        .prepare_request(
            &"44".repeat(32),
            b"```rust\nfn new_answer() {}\n```",
            b"fn new_answer() {}",
        )
        .unwrap();
    assert!(different.validate_request(&request).is_err());
    let path = std::env::temp_dir().join(format!("boole-development-task-{}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let directory = std::fs::File::open(&path).unwrap();
    let meta = directory.metadata().unwrap();
    let open = |grant| {
        CanaryBudget::open(
            &directory,
            grant,
            CanaryBudgetRole::Launcher,
            meta.uid(),
            meta.gid(),
        )
    };
    let mut budget = open(&grant).unwrap();
    let authorization = budget.reserve_execution(&grant, &request).unwrap();
    let actual_task = authorization.task_bytes().to_vec();
    drop(budget);
    assert!(open(&different).is_err());
    let mut recovered = open(&grant).unwrap();
    assert!(recovered.reserve_execution(&grant, &request).is_err());
    drop(recovered);
    std::fs::remove_file(path.join("launcher-budget-v1.jsonl")).unwrap();
    std::fs::remove_dir(path).unwrap();
    assert_eq!(actual_task, grant.task_bytes());
}
