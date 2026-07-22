//! BF.1 — useful-work task contract and pure state machine (B1 + C1 + C2).
//!
//! Pins the two-identity split: `TaskSpecIdentity` (the problem, fixed
//! before the epoch seed, `TaskKind` tagged enum) vs `SubmissionIdentity`
//! (the miner's result). Everything here is pure data + pure transitions:
//! no files, no HTTP, no verifier processes (BF.1 non-goals).

use boole_core::useful_work::{
    transition, SubmissionIdentity, TaskEvent, TaskSpecIdentity, TaskState, TaskTransitionError,
    UsefulWorkError,
};
use boole_core::Hex32;
use serde_json::{json, Value};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/protocol/useful-work/v0.json"
);

fn digest(byte: u8) -> String {
    hex::encode([byte; 32])
}

fn build_new_task() -> Value {
    json!({
        "specId": "groth16-paper-3.2",
        "variantId": "bn254",
        "componentId": "verifier-equation",
        "propertyId": "acceptance-soundness",
        "specVersion": 1,
        "taskKind": { "kind": "buildNew" }
    })
}

fn audit_existing_task() -> Value {
    json!({
        "specId": "poseidon-circomlib",
        "variantId": "circom-bn254",
        "componentId": "full-round",
        "propertyId": "constraint-completeness",
        "specVersion": 2,
        "taskKind": {
            "kind": "auditExisting",
            "inputArtifactDigest": digest(0x11),
            "targetReleaseDigest": digest(0x22)
        }
    })
}

#[test]
fn task_spec_missing_required_field_is_rejected() {
    let mut task = build_new_task();
    task.as_object_mut().unwrap().remove("propertyId");
    let err = TaskSpecIdentity::from_json_value(&task).unwrap_err();
    assert_eq!(err.label(), "malformed-json");
}

#[test]
fn task_spec_unknown_field_is_rejected() {
    let mut task = build_new_task();
    task.as_object_mut()
        .unwrap()
        .insert("bonusHint".into(), json!("cheat"));
    let err = TaskSpecIdentity::from_json_value(&task).unwrap_err();
    assert_eq!(err.label(), "malformed-json");
}

#[test]
fn build_new_with_artifact_digest_is_rejected() {
    let mut task = build_new_task();
    task["taskKind"]
        .as_object_mut()
        .unwrap()
        .insert("inputArtifactDigest".into(), json!(digest(0x33)));
    let err = TaskSpecIdentity::from_json_value(&task).unwrap_err();
    assert_eq!(err.label(), "build-new-carries-digest");
}

#[test]
fn audit_existing_missing_one_digest_is_rejected() {
    let mut task = audit_existing_task();
    task["taskKind"]
        .as_object_mut()
        .unwrap()
        .remove("targetReleaseDigest");
    let err = TaskSpecIdentity::from_json_value(&task).unwrap_err();
    assert_eq!(err.label(), "audit-existing-missing-digest");
}

#[test]
fn unknown_task_kind_is_rejected() {
    let mut task = build_new_task();
    task["taskKind"]["kind"] = json!("governanceOverride");
    let err = TaskSpecIdentity::from_json_value(&task).unwrap_err();
    assert_eq!(err.label(), "unknown-task-kind");
}

#[test]
fn empty_identity_field_is_rejected() {
    let mut task = build_new_task();
    task["specId"] = json!("");
    let err = TaskSpecIdentity::from_json_value(&task).unwrap_err();
    assert_eq!(err.label(), "empty-field");
}

#[test]
fn invalid_digest_shape_is_rejected() {
    let mut task = audit_existing_task();
    task["taskKind"]["inputArtifactDigest"] = json!("NOT-HEX");
    let err = TaskSpecIdentity::from_json_value(&task).unwrap_err();
    assert_eq!(err.label(), "invalid-digest");
}

#[test]
fn equivalent_reencodings_yield_same_task_id() {
    // Same identity, different JSON key order: dedup must key on the
    // derived task_id, so re-encoding games cannot mint a "new" task.
    let a = TaskSpecIdentity::from_json_value(&audit_existing_task()).expect("valid task");
    let reordered: Value = serde_json::from_str(
        &r#"{
            "taskKind": {
                "targetReleaseDigest": "TRD",
                "inputArtifactDigest": "IAD",
                "kind": "auditExisting"
            },
            "specVersion": 2,
            "propertyId": "constraint-completeness",
            "componentId": "full-round",
            "variantId": "circom-bn254",
            "specId": "poseidon-circomlib"
        }"#
        .replace("IAD", &digest(0x11))
        .replace("TRD", &digest(0x22)),
    )
    .expect("json parses");
    let b = TaskSpecIdentity::from_json_value(&reordered).expect("valid task");
    assert_eq!(a, b);
    assert_eq!(a.task_id(), b.task_id());
}

#[test]
fn task_id_is_domain_separated_and_kind_sensitive() {
    let build = TaskSpecIdentity::from_json_value(&build_new_task()).expect("valid task");
    let audit = TaskSpecIdentity::from_json_value(&audit_existing_task()).expect("valid task");
    assert_ne!(build.task_id(), audit.task_id());

    // Same fields, different audit input digest => different task identity
    // (an audit of different bytes is a different problem).
    let mut other = audit_existing_task();
    other["taskKind"]["inputArtifactDigest"] = json!(digest(0x44));
    let other = TaskSpecIdentity::from_json_value(&other).expect("valid task");
    assert_ne!(audit.task_id(), other.task_id());
}

#[test]
fn state_machine_accepts_the_single_valid_path() {
    let mut state = TaskState::Registered;
    for event in [
        TaskEvent::Assign,
        TaskEvent::Commit,
        TaskEvent::Verify,
        TaskEvent::Reward,
    ] {
        state = transition(state, event).expect("valid transition");
    }
    assert_eq!(state, TaskState::Rewarded);
}

#[test]
fn state_machine_rejects_skip_duplicate_and_expired_transitions() {
    // Registered -> Rewarded skip (the "open -> rewarded" attack).
    let err = transition(TaskState::Registered, TaskEvent::Reward).unwrap_err();
    assert_eq!(
        err,
        TaskTransitionError::InvalidTransition {
            from: TaskState::Registered,
            event: TaskEvent::Reward,
        }
    );
    // Duplicate reveal: verifying an already-verified task.
    assert!(transition(TaskState::Verified, TaskEvent::Verify).is_err());
    // Duplicate commit.
    assert!(transition(TaskState::Committed, TaskEvent::Commit).is_err());
    // Commit after expiry.
    assert!(transition(TaskState::Expired, TaskEvent::Commit).is_err());
    // Double reward.
    assert!(transition(TaskState::Rewarded, TaskEvent::Reward).is_err());
    // Expiry only before verification.
    assert!(transition(TaskState::Registered, TaskEvent::Expire).is_ok());
    assert!(transition(TaskState::Assigned, TaskEvent::Expire).is_ok());
    assert!(transition(TaskState::Committed, TaskEvent::Expire).is_ok());
    assert!(transition(TaskState::Verified, TaskEvent::Expire).is_err());
    assert!(transition(TaskState::Rewarded, TaskEvent::Expire).is_err());
}

fn submission(reward_pk: u8, artifact: u8) -> SubmissionIdentity {
    let task = TaskSpecIdentity::from_json_value(&audit_existing_task()).expect("valid task");
    SubmissionIdentity {
        task_id: task.task_id(),
        source_root: Hex32::from_hex(&digest(0x51)).unwrap(),
        implementation_digest: Hex32::from_hex(&digest(0x52)).unwrap(),
        release_digest: Hex32::from_hex(&digest(0x53)).unwrap(),
        artifact_root: Hex32::from_hex(&digest(artifact)).unwrap(),
        reward_pk: Hex32::from_hex(&digest(reward_pk)).unwrap(),
    }
}

#[test]
fn submission_id_binds_reward_pk_and_artifact_root() {
    let base = submission(0x61, 0x71).submission_id();
    assert_ne!(
        base,
        submission(0x62, 0x71).submission_id(),
        "reward_pk swap must change the submission identity"
    );
    assert_ne!(
        base,
        submission(0x61, 0x72).submission_id(),
        "artifact_root swap must change the submission identity"
    );
    let mut other = submission(0x61, 0x71);
    other.implementation_digest = Hex32::from_hex(&digest(0x54)).unwrap();
    assert_ne!(base, other.submission_id());
    let mut other = submission(0x61, 0x71);
    other.release_digest = Hex32::from_hex(&digest(0x55)).unwrap();
    assert_ne!(base, other.submission_id());
}

#[test]
fn submission_id_and_task_id_use_distinct_domains() {
    // A submission whose canonical bytes could collide with a task's must
    // still hash differently thanks to domain separation; sanity-pin that
    // the two derivations never agree on the fixture data.
    let task = TaskSpecIdentity::from_json_value(&audit_existing_task()).expect("valid task");
    let sub = submission(0x61, 0x71);
    assert_ne!(task.task_id(), sub.submission_id());
}

#[test]
fn golden_fixture_round_trip_is_stable() {
    let fixture: Value =
        serde_json::from_str(&std::fs::read_to_string(FIXTURE_PATH).expect("fixture readable"))
            .expect("fixture parses");
    let cases = fixture["validTasks"].as_array().expect("validTasks");
    assert!(!cases.is_empty());
    for case in cases {
        let task = TaskSpecIdentity::from_json_value(&case["task"]).expect("fixture task valid");
        assert_eq!(
            task.task_id().to_hex(),
            case["expectedTaskId"].as_str().expect("expectedTaskId"),
            "task_id must match the golden fixture"
        );
        // Round-trip: serialize -> parse -> identical identity and id.
        let reparsed =
            TaskSpecIdentity::from_json_value(&task.to_json_value()).expect("round-trip");
        assert_eq!(task, reparsed);
    }
    for case in fixture["rejectedTasks"].as_array().expect("rejectedTasks") {
        let err = TaskSpecIdentity::from_json_value(&case["task"]).unwrap_err();
        assert_eq!(
            err.label(),
            case["reason"].as_str().expect("reason"),
            "rejection reason must match the golden fixture"
        );
    }
    for case in fixture["validSubmissions"]
        .as_array()
        .expect("validSubmissions")
    {
        let sub = SubmissionIdentity::from_json_value(&case["submission"])
            .expect("fixture submission valid");
        assert_eq!(
            sub.submission_id().to_hex(),
            case["expectedSubmissionId"]
                .as_str()
                .expect("expectedSubmissionId"),
        );
        let reparsed =
            SubmissionIdentity::from_json_value(&sub.to_json_value()).expect("round-trip");
        assert_eq!(sub, reparsed);
    }
}

/// Regen helper mirroring `block_hash_fixtures.rs` conventions — rewrites
/// the golden fixture in place from the in-code cases.
#[test]
#[ignore = "regen helper: cargo test -p boole-core --test useful_work_contract regen_useful_work_golden_fixture -- --ignored"]
fn regen_useful_work_golden_fixture() {
    let build = TaskSpecIdentity::from_json_value(&build_new_task()).expect("valid task");
    let audit = TaskSpecIdentity::from_json_value(&audit_existing_task()).expect("valid task");
    let sub = submission(0x61, 0x71);

    let mut rejected_build = build_new_task();
    rejected_build["taskKind"]
        .as_object_mut()
        .unwrap()
        .insert("inputArtifactDigest".into(), json!(digest(0x33)));
    let mut rejected_audit = audit_existing_task();
    rejected_audit["taskKind"]
        .as_object_mut()
        .unwrap()
        .remove("targetReleaseDigest");
    let mut rejected_kind = build_new_task();
    rejected_kind["taskKind"]["kind"] = json!("governanceOverride");

    let fixture = json!({
        "domain": "boole.useful-work.v0",
        "validTasks": [
            { "task": build.to_json_value(), "expectedTaskId": build.task_id().to_hex() },
            { "task": audit.to_json_value(), "expectedTaskId": audit.task_id().to_hex() },
        ],
        "validSubmissions": [
            {
                "submission": sub.to_json_value(),
                "expectedSubmissionId": sub.submission_id().to_hex(),
            },
        ],
        "rejectedTasks": [
            { "task": rejected_build, "reason": "build-new-carries-digest" },
            { "task": rejected_audit, "reason": "audit-existing-missing-digest" },
            { "task": rejected_kind, "reason": "unknown-task-kind" },
        ],
    });
    let pretty = format!("{}\n", serde_json::to_string_pretty(&fixture).unwrap());
    std::fs::create_dir_all(std::path::Path::new(FIXTURE_PATH).parent().unwrap()).unwrap();
    std::fs::write(FIXTURE_PATH, pretty).expect("write fixture");
}

// Keep the typed error surface honest: labels are part of the contract.
#[test]
fn error_labels_are_stable() {
    assert_eq!(
        UsefulWorkError::BuildNewCarriesDigest.label(),
        "build-new-carries-digest"
    );
    assert_eq!(
        UsefulWorkError::AuditExistingMissingDigest.label(),
        "audit-existing-missing-digest"
    );
}
