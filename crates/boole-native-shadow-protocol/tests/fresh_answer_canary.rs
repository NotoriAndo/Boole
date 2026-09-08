#![cfg(feature = "fresh-answer-canary")]

use boole_native_shadow_protocol::fresh_answer_canary::{
    verify_grant, CanaryBindings, CanaryGrant, CanaryTrustRoot, SIGNING_DOMAIN,
};
use ed25519_dalek::{Signer, SigningKey};

#[test]
fn only_the_separate_operator_key_can_authorize_the_installed_task() {
    let key = SigningKey::from_bytes(&[47; 32]);
    let bindings = CanaryBindings::installed().unwrap();
    let grant = CanaryGrant::one_task("11".repeat(32), "22".repeat(32), 10, bindings.clone());
    let bytes = serde_json::to_vec(&grant).unwrap();
    let signature = key.sign(&[SIGNING_DOMAIN, bytes.as_slice()].concat());
    let root = CanaryTrustRoot::new(key.verifying_key().to_bytes()).unwrap();
    assert!(verify_grant(&bytes, &signature.to_bytes(), &root, &bindings).is_ok());
    let other =
        CanaryTrustRoot::new(SigningKey::from_bytes(&[48; 32]).verifying_key().to_bytes()).unwrap();
    assert!(verify_grant(&bytes, &signature.to_bytes(), &other, &bindings).is_err());
}

fn authorized() -> boole_native_shadow_protocol::fresh_answer_canary::VerifiedCanaryGrant {
    let key = SigningKey::from_bytes(&[47; 32]);
    let bindings = CanaryBindings::installed().unwrap();
    let grant = CanaryGrant::one_task("11".repeat(32), "22".repeat(32), 10, bindings.clone());
    let bytes = serde_json::to_vec(&grant).unwrap();
    let signature = key.sign(&[SIGNING_DOMAIN, bytes.as_slice()].concat());
    verify_grant(
        &bytes,
        &signature.to_bytes(),
        &CanaryTrustRoot::new(key.verifying_key().to_bytes()).unwrap(),
        &bindings,
    )
    .unwrap()
}

#[test]
fn fresh_answer_is_bound_to_task_and_extracted_source_not_a_fixture_digest() {
    let grant = authorized();
    let raw = b"Here is a fresh candidate.\n```rust\nfn main() {}\n```";
    let request = grant
        .prepare_request(&"33".repeat(32), raw, b"fn main() {}")
        .unwrap();
    assert_eq!(
        request.candidate_digest_hex(),
        boole_native_shadow_protocol::sha256_hex(raw)
    );
    assert!(grant.validate_request(&request).is_ok());
    assert!(grant
        .prepare_request(&"33".repeat(32), raw, b"fn substituted() {}")
        .is_err());
}

#[cfg(unix)]
#[test]
fn execution_reservation_survives_restart_and_never_reexecutes() {
    use boole_native_shadow_protocol::fresh_answer_canary::{CanaryBudget, CanaryBudgetRole};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let path = std::env::temp_dir().join(format!("boole-canary-budget-{}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let directory = std::fs::File::open(&path).unwrap();
    let metadata = directory.metadata().unwrap();
    let grant = authorized();
    let request = grant
        .prepare_request(
            &"33".repeat(32),
            b"```rust\nfn main() {}\n```",
            b"fn main() {}",
        )
        .unwrap();
    let mut budget = CanaryBudget::open(
        &directory,
        &grant,
        CanaryBudgetRole::Launcher,
        metadata.uid(),
        metadata.gid(),
    )
    .unwrap();
    budget.reserve_execution(&grant, &request).unwrap();
    drop(budget);
    let mut recovered = CanaryBudget::open(
        &directory,
        &grant,
        CanaryBudgetRole::Launcher,
        metadata.uid(),
        metadata.gid(),
    )
    .unwrap();
    assert!(recovered.reserve_execution(&grant, &request).is_err());
    let changed = grant
        .prepare_request(
            &"44".repeat(32),
            b"```rust\nfn other() {}\n```",
            b"fn other() {}",
        )
        .unwrap();
    assert!(recovered.reserve_execution(&grant, &changed).is_err());
    drop(recovered);
    std::fs::remove_file(path.join("launcher-budget-v1.jsonl")).unwrap();
    std::fs::remove_dir(path).unwrap();
}

#[test]
fn signed_scope_mutations_and_cross_domain_signatures_are_rejected() {
    let key = SigningKey::from_bytes(&[47; 32]);
    let root = CanaryTrustRoot::new(key.verifying_key().to_bytes()).unwrap();
    let bindings = CanaryBindings::installed().unwrap();
    let original = serde_json::to_value(CanaryGrant::one_task(
        "11".repeat(32),
        "22".repeat(32),
        10,
        bindings.clone(),
    ))
    .unwrap();
    for (field, changed) in [
        ("maxCheckerExecutions", serde_json::json!(2)),
        ("maxRedeliveries", serde_json::json!(2)),
        ("activationAllowed", serde_json::json!(true)),
        ("nonIssuable", serde_json::json!(false)),
        ("epoch", serde_json::json!(0)),
    ] {
        let mut value = original.clone();
        value[field] = changed;
        let bytes = serde_json::to_vec(&value).unwrap();
        let signature = key.sign(&[SIGNING_DOMAIN, bytes.as_slice()].concat());
        assert!(
            verify_grant(&bytes, &signature.to_bytes(), &root, &bindings).is_err(),
            "{field}"
        );
    }
    for field in [
        "templateId",
        "challengeSha256",
        "checkerDigest",
        "checkerPolicyDigest",
        "toolchainDigest",
    ] {
        let mut value = original.clone();
        value["bindings"][field] = serde_json::json!("aa".repeat(32));
        let bytes = serde_json::to_vec(&value).unwrap();
        let signature = key.sign(&[SIGNING_DOMAIN, bytes.as_slice()].concat());
        assert!(
            verify_grant(&bytes, &signature.to_bytes(), &root, &bindings).is_err(),
            "{field}"
        );
    }
    let bytes = serde_json::to_vec(&original).unwrap();
    assert!(verify_grant(&bytes, &key.sign(&bytes).to_bytes(), &root, &bindings).is_err());
}

#[cfg(unix)]
#[test]
fn identical_redelivery_needs_separate_operator_signature_and_is_durably_spent() {
    use boole_native_shadow_protocol::fresh_answer_canary::{
        verify_redelivery, CanaryBudget, CanaryBudgetRole, CanaryRedelivery,
        REDELIVERY_SIGNING_DOMAIN,
    };
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let path = std::env::temp_dir().join(format!("boole-canary-redelivery-{}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let directory = std::fs::File::open(&path).unwrap();
    let metadata = directory.metadata().unwrap();
    let grant = authorized();
    let request = grant
        .prepare_request(
            &"33".repeat(32),
            b"```rust\nfn main() {}\n```",
            b"fn main() {}",
        )
        .unwrap();
    let key = SigningKey::from_bytes(&[47; 32]);
    let root = CanaryTrustRoot::new(key.verifying_key().to_bytes()).unwrap();
    let recovery = CanaryRedelivery::for_candidate(
        &grant,
        request.candidate_digest_hex(),
        request.submission_digest_hex(),
    );
    let bytes = serde_json::to_vec(&recovery).unwrap();
    let signature = key.sign(&[REDELIVERY_SIGNING_DOMAIN, bytes.as_slice()].concat());
    let mut budget = CanaryBudget::open(
        &directory,
        &grant,
        CanaryBudgetRole::Node,
        metadata.uid(),
        metadata.gid(),
    )
    .unwrap();
    budget.reserve_execution(&grant, &request).unwrap();
    let permit = verify_redelivery(&bytes, &signature.to_bytes(), &root, &grant).unwrap();
    budget
        .consume_redelivery(
            &grant,
            permit,
            request.candidate_digest_hex(),
            request.submission_digest_hex(),
        )
        .unwrap();
    drop(budget);
    let mut budget = CanaryBudget::open(
        &directory,
        &grant,
        CanaryBudgetRole::Node,
        metadata.uid(),
        metadata.gid(),
    )
    .unwrap();
    let permit = verify_redelivery(&bytes, &signature.to_bytes(), &root, &grant).unwrap();
    assert!(budget
        .consume_redelivery(
            &grant,
            permit,
            request.candidate_digest_hex(),
            request.submission_digest_hex()
        )
        .is_err());
    assert!(verify_redelivery(
        &bytes,
        &key.sign(&[SIGNING_DOMAIN, bytes.as_slice()].concat())
            .to_bytes(),
        &root,
        &grant
    )
    .is_err());
    drop(budget);
    std::fs::remove_file(path.join("node-budget-v1.jsonl")).unwrap();
    std::fs::remove_dir(path).unwrap();
}

#[cfg(unix)]
#[test]
fn offline_operator_tool_requires_private_key_and_never_installs_or_overwrites() {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::temp_dir().join(format!("boole-canary-operator-{}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let key = path.join("test-key");
    std::fs::write(&key, [47; 32]).unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    let run = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_boole-canary-authority"))
            .env_clear()
            .args([
                "create-grant",
                key.to_str().unwrap(),
                &"11".repeat(32),
                &"22".repeat(32),
                "10",
                path.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    assert!(run().status.success());
    assert!(
        !run().status.success(),
        "operator tool must not overwrite an issued grant"
    );
    let root = CanaryTrustRoot::new(
        std::fs::read(path.join("operator-public-key.bin"))
            .unwrap()
            .try_into()
            .unwrap(),
    )
    .unwrap();
    verify_grant(
        &std::fs::read(path.join("grant.json")).unwrap(),
        &std::fs::read(path.join("grant.sig")).unwrap(),
        &root,
        &CanaryBindings::installed().unwrap(),
    )
    .unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        !run().status.success(),
        "operator tool must reject an exposed private key"
    );
    for name in [
        "test-key",
        "grant.json",
        "grant.sig",
        "operator-public-key.bin",
    ] {
        std::fs::remove_file(path.join(name)).unwrap();
    }
    std::fs::remove_dir(path).unwrap();
}

#[test]
fn launcher_rejects_changed_checker_binding_even_in_a_valid_wire_frame() {
    let grant = authorized();
    let request = grant
        .prepare_request(
            &"33".repeat(32),
            b"```rust\nfn main() {}\n```",
            b"fn main() {}",
        )
        .unwrap();
    let mut value = serde_json::to_value(request).unwrap();
    value["checkerArtifactHashHex"] = serde_json::json!("aa".repeat(32));
    let bytes = serde_json::to_vec(&value).unwrap();
    let frame = [&(bytes.len() as u32).to_be_bytes()[..], bytes.as_slice()].concat();
    let request =
        boole_native_shadow_protocol::decode_complete_execution_request_frame(&frame).unwrap();
    assert!(grant.validate_request(&request).is_err());
}

#[cfg(unix)]
#[test]
fn ambiguous_budget_tail_empty_file_and_concurrent_owner_fail_closed() {
    use boole_native_shadow_protocol::fresh_answer_canary::{CanaryBudget, CanaryBudgetRole};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let path = std::env::temp_dir().join(format!("boole-canary-corrupt-{}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let directory = std::fs::File::open(&path).unwrap();
    let metadata = directory.metadata().unwrap();
    let grant = authorized();
    let open = || {
        CanaryBudget::open(
            &directory,
            &grant,
            CanaryBudgetRole::Launcher,
            metadata.uid(),
            metadata.gid(),
        )
    };
    let budget = open().unwrap();
    assert!(
        open().is_err(),
        "a second live journal owner cannot reserve an execution"
    );
    drop(budget);
    let file = path.join("launcher-budget-v1.jsonl");
    let mut bytes = std::fs::read(&file).unwrap();
    bytes.extend_from_slice(b"{\"event\":\"exec");
    std::fs::write(&file, bytes).unwrap();
    assert!(
        open().is_err(),
        "ambiguous append must never be repaired into a new budget"
    );
    std::fs::write(&file, []).unwrap();
    assert!(
        open().is_err(),
        "empty existing file must not be reinitialized"
    );
    std::fs::remove_file(file).unwrap();
    std::fs::remove_dir(path).unwrap();
}

#[cfg(unix)]
#[test]
fn changing_operator_key_cannot_adopt_an_existing_private_run_journal() {
    use boole_native_shadow_protocol::fresh_answer_canary::{CanaryBudget, CanaryBudgetRole};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let path = std::env::temp_dir().join(format!("boole-canary-key-drift-{}", std::process::id()));
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let directory = std::fs::File::open(&path).unwrap();
    let metadata = directory.metadata().unwrap();
    let first = authorized();
    drop(
        CanaryBudget::open(
            &directory,
            &first,
            CanaryBudgetRole::Launcher,
            metadata.uid(),
            metadata.gid(),
        )
        .unwrap(),
    );
    let other_key = SigningKey::from_bytes(&[48; 32]);
    let bindings = CanaryBindings::installed().unwrap();
    let bytes = serde_json::to_vec(&CanaryGrant::one_task(
        "11".repeat(32),
        "22".repeat(32),
        10,
        bindings.clone(),
    ))
    .unwrap();
    let second = verify_grant(
        &bytes,
        &other_key
            .sign(&[SIGNING_DOMAIN, bytes.as_slice()].concat())
            .to_bytes(),
        &CanaryTrustRoot::new(other_key.verifying_key().to_bytes()).unwrap(),
        &bindings,
    )
    .unwrap();
    assert!(CanaryBudget::open(
        &directory,
        &second,
        CanaryBudgetRole::Launcher,
        metadata.uid(),
        metadata.gid()
    )
    .is_err());
    std::fs::remove_file(path.join("launcher-budget-v1.jsonl")).unwrap();
    std::fs::remove_dir(path).unwrap();
}
