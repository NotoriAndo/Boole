//! BF.3 — bound useful-product packet and deterministic receipt (C2+B4).
//!
//! The packet manifest binds source/build/runtime/Lean/release into one
//! verdict-bearing `artifact_root` (identity fields excluded — C2
//! anti-self-reference), and the audit normalizes its result into a
//! small `VerificationReceipt` that is byte-identical when recomputed
//! from the same inputs. A wall-clock timeout is an availability
//! outcome, never a reject verdict (B4 / ADR-0016 alignment). No real
//! Circom/Rust/EVM runners here — common contract only (BF.3 non-goal).

use boole_core::useful_product::{
    audit_packet, ObservedVerification, UsefulProductError, UsefulProductManifest,
    VerificationOutcome,
};
use boole_core::useful_work::TaskSpecIdentity;
use boole_core::Hex32;
use serde_json::{json, Value};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/protocol/useful-product/v0.json"
);

fn digest(byte: u8) -> String {
    hex::encode([byte; 32])
}

fn hex32(byte: u8) -> Hex32 {
    Hex32::from_hex(&digest(byte)).unwrap()
}

fn manifest_json() -> Value {
    json!({
        "sourceTreeRoot": digest(0x01),
        "buildRecipeDigest": digest(0x02),
        "toolchainPin": "lean:v4.29.1+circom:2.1.9",
        "implementationDigest": digest(0x03),
        "releaseDigest": digest(0x04),
        "runtimeOkVectorRoot": digest(0x05),
        "runtimeRejectVectorRoot": digest(0x06),
        "statementHash": digest(0x07),
        "leanProofDigest": digest(0x08),
        "checkerHash": digest(0x09),
        "verifierHash": digest(0x0a),
        "canonicalizerHash": digest(0x0b),
        "rewardPk": digest(0x0c)
    })
}

fn manifest() -> UsefulProductManifest {
    UsefulProductManifest::from_json_value(&manifest_json()).expect("valid manifest")
}

fn task() -> TaskSpecIdentity {
    TaskSpecIdentity::from_json_value(&json!({
        "specId": "poseidon-circomlib",
        "variantId": "circom-bn254",
        "componentId": "full-round",
        "propertyId": "constraint-completeness",
        "specVersion": 1,
        "taskKind": { "kind": "buildNew" }
    }))
    .expect("valid task")
}

fn observed_matching() -> ObservedVerification {
    ObservedVerification {
        rebuilt_source_tree_root: hex32(0x01),
        rebuilt_release_digest: hex32(0x04),
        runtime_ok_vectors_passed: true,
        runtime_reject_vectors_passed: true,
        protocol_statement_hash: hex32(0x07),
        proof_has_sorry: false,
        proof_new_axioms: false,
        checker_hash: hex32(0x09),
        verifier_hash: hex32(0x0a),
        canonicalizer_hash: hex32(0x0b),
        wall_clock_timeout: false,
    }
}

#[test]
fn missing_source_and_missing_runtime_vectors_are_rejected() {
    for field in [
        "sourceTreeRoot",
        "runtimeOkVectorRoot",
        "runtimeRejectVectorRoot",
        "leanProofDigest",
    ] {
        let mut value = manifest_json();
        value.as_object_mut().unwrap().remove(field);
        let err = UsefulProductManifest::from_json_value(&value).unwrap_err();
        assert_eq!(err.label(), "malformed-json", "missing {field}");
    }
}

#[test]
fn self_referential_identity_fields_are_rejected() {
    // C2: submission_id / commitment must never appear inside the packet
    // whose bytes artifact_root hashes — that would be circular.
    for field in ["submissionId", "commitment"] {
        let mut value = manifest_json();
        value
            .as_object_mut()
            .unwrap()
            .insert(field.into(), json!(digest(0x77)));
        let err = UsefulProductManifest::from_json_value(&value).unwrap_err();
        assert_eq!(err.label(), "self-referential-field", "field {field}");
    }
    // Any other unknown field stays a plain malformed rejection.
    let mut value = manifest_json();
    value
        .as_object_mut()
        .unwrap()
        .insert("bonusNote".into(), json!("hi"));
    let err = UsefulProductManifest::from_json_value(&value).unwrap_err();
    assert_eq!(err.label(), "malformed-json");
}

#[test]
fn artifact_root_covers_every_verdict_bearing_field() {
    let base_root = manifest().artifact_root();
    let verdict_bearing = [
        ("sourceTreeRoot", json!(digest(0x71))),
        ("buildRecipeDigest", json!(digest(0x72))),
        ("toolchainPin", json!("lean:v4.30.0")),
        ("implementationDigest", json!(digest(0x73))),
        ("releaseDigest", json!(digest(0x74))),
        ("runtimeOkVectorRoot", json!(digest(0x75))),
        ("runtimeRejectVectorRoot", json!(digest(0x76))),
        ("statementHash", json!(digest(0x77))),
        ("leanProofDigest", json!(digest(0x78))),
        ("checkerHash", json!(digest(0x79))),
        ("verifierHash", json!(digest(0x7a))),
        ("canonicalizerHash", json!(digest(0x7b))),
    ];
    for (field, tampered_value) in verdict_bearing {
        let mut value = manifest_json();
        value[field] = tampered_value;
        let tampered = UsefulProductManifest::from_json_value(&value).expect("parses");
        assert_ne!(
            base_root,
            tampered.artifact_root(),
            "artifact_root must commit {field}"
        );
    }
}

#[test]
fn reward_pk_is_outside_artifact_root_but_inside_submission_id() {
    // C2 split: the product bytes are reward-neutral (copy detection keys
    // on artifact_root), while the submission identity binds the payee.
    let base = manifest();
    let mut value = manifest_json();
    value["rewardPk"] = json!(digest(0x7c));
    let repointed = UsefulProductManifest::from_json_value(&value).expect("parses");

    assert_eq!(
        base.artifact_root(),
        repointed.artifact_root(),
        "reward_pk swap must NOT change the product bytes identity"
    );
    let task_id = task().task_id();
    assert_ne!(
        base.submission_identity(&task_id).submission_id(),
        repointed.submission_identity(&task_id).submission_id(),
        "reward_pk swap MUST change the submission identity"
    );
}

#[test]
fn key_order_does_not_change_the_manifest_or_its_root() {
    let a = manifest();
    let mut reordered = serde_json::Map::new();
    let original = manifest_json();
    let mut keys: Vec<String> = original.as_object().unwrap().keys().cloned().collect();
    keys.reverse();
    for key in keys {
        reordered.insert(key.clone(), original[&key].clone());
    }
    let b = UsefulProductManifest::from_json_value(&Value::Object(reordered)).expect("parses");
    assert_eq!(a, b);
    assert_eq!(a.artifact_root(), b.artifact_root());
}

#[test]
fn swapping_bytes_between_fields_changes_the_root() {
    // Canonical bytes are length-prefixed in a fixed field order: moving
    // the same 32 bytes from one field to another is a different packet.
    let mut value = manifest_json();
    value["sourceTreeRoot"] = json!(digest(0x04));
    value["releaseDigest"] = json!(digest(0x01));
    let swapped = UsefulProductManifest::from_json_value(&value).expect("parses");
    assert_ne!(manifest().artifact_root(), swapped.artifact_root());
}

#[test]
fn matching_observation_yields_a_byte_identical_receipt() {
    let task_id = task().task_id();
    let VerificationOutcome::Verdict(receipt_a) =
        audit_packet(&manifest(), &observed_matching(), &task_id)
    else {
        panic!("expected a verdict");
    };
    let VerificationOutcome::Verdict(receipt_b) =
        audit_packet(&manifest(), &observed_matching(), &task_id)
    else {
        panic!("expected a verdict");
    };
    assert!(receipt_a.accepted());
    assert_eq!(
        receipt_a.canonical_bytes(),
        receipt_b.canonical_bytes(),
        "same inputs must re-derive a byte-identical receipt"
    );
    assert_eq!(receipt_a.receipt_digest(), receipt_b.receipt_digest());
    assert_eq!(
        receipt_a.submission_id,
        manifest().submission_identity(&task_id).submission_id()
    );
}

#[test]
fn every_observed_mismatch_is_a_typed_rejection() {
    let task_id = task().task_id();
    let cases: Vec<(&str, ObservedVerification)> = vec![
        ("rebuild-source-mismatch", {
            let mut o = observed_matching();
            o.rebuilt_source_tree_root = hex32(0x61);
            o
        }),
        ("rebuild-release-mismatch", {
            let mut o = observed_matching();
            o.rebuilt_release_digest = hex32(0x62);
            o
        }),
        ("runtime-ok-vectors-failed", {
            let mut o = observed_matching();
            o.runtime_ok_vectors_passed = false;
            o
        }),
        ("runtime-reject-vectors-failed", {
            let mut o = observed_matching();
            o.runtime_reject_vectors_passed = false;
            o
        }),
        // The Lean statement describes a different function than the
        // protocol-owned exact theorem for this task.
        ("statement-mismatch", {
            let mut o = observed_matching();
            o.protocol_statement_hash = hex32(0x63);
            o
        }),
        ("proof-has-sorry", {
            let mut o = observed_matching();
            o.proof_has_sorry = true;
            o
        }),
        ("proof-new-axiom", {
            let mut o = observed_matching();
            o.proof_new_axioms = true;
            o
        }),
        ("checker-pin-mismatch", {
            let mut o = observed_matching();
            o.checker_hash = hex32(0x64);
            o
        }),
        ("verifier-pin-mismatch", {
            let mut o = observed_matching();
            o.verifier_hash = hex32(0x65);
            o
        }),
        ("canonicalizer-pin-mismatch", {
            let mut o = observed_matching();
            o.canonicalizer_hash = hex32(0x66);
            o
        }),
    ];
    for (expected_label, observed) in cases {
        let VerificationOutcome::Verdict(receipt) = audit_packet(&manifest(), &observed, &task_id)
        else {
            panic!("expected a verdict for {expected_label}");
        };
        assert!(!receipt.accepted(), "{expected_label} must reject");
        assert_eq!(receipt.reject_label(), Some(expected_label));
    }
}

#[test]
fn wall_clock_timeout_is_availability_not_a_reject_verdict() {
    // B4: budgets that decide verdicts are deterministic; wall-clock is
    // containment. A timeout produces NO receipt at all.
    let mut observed = observed_matching();
    observed.wall_clock_timeout = true;
    let outcome = audit_packet(&manifest(), &observed, &task().task_id());
    assert_eq!(outcome, VerificationOutcome::RetryableUnavailable);
}

#[test]
fn golden_product_fixture_is_stable() {
    let fixture: Value =
        serde_json::from_str(&std::fs::read_to_string(FIXTURE_PATH).expect("fixture readable"))
            .expect("fixture parses");
    let parsed = UsefulProductManifest::from_json_value(&fixture["manifest"]).expect("manifest");
    assert_eq!(
        parsed.artifact_root().to_hex(),
        fixture["expectedArtifactRoot"].as_str().expect("root")
    );
    let task_id = Hex32::from_hex(fixture["taskId"].as_str().expect("taskId")).unwrap();
    assert_eq!(
        parsed
            .submission_identity(&task_id)
            .submission_id()
            .to_hex(),
        fixture["expectedSubmissionId"]
            .as_str()
            .expect("submission id")
    );
    // Round-trip stability.
    let reparsed =
        UsefulProductManifest::from_json_value(&parsed.to_json_value()).expect("round-trip");
    assert_eq!(parsed, reparsed);
    // Receipt digest pin.
    let VerificationOutcome::Verdict(receipt) =
        audit_packet(&parsed, &observed_matching(), &task_id)
    else {
        panic!("expected verdict");
    };
    assert_eq!(
        receipt.receipt_digest().to_hex(),
        fixture["expectedReceiptDigest"]
            .as_str()
            .expect("receipt digest")
    );
    for case in fixture["rejectedManifests"].as_array().expect("rejected") {
        let err = UsefulProductManifest::from_json_value(&case["manifest"]).unwrap_err();
        assert_eq!(err.label(), case["reason"].as_str().expect("reason"));
    }
}

/// Regen helper mirroring repo conventions — rewrites the golden fixture
/// in place from the in-code cases.
#[test]
#[ignore = "regen helper: cargo test -p boole-core --test useful_product_manifest regen_product_golden_fixture -- --ignored"]
fn regen_product_golden_fixture() {
    let parsed = manifest();
    let task_id = task().task_id();
    let VerificationOutcome::Verdict(receipt) =
        audit_packet(&parsed, &observed_matching(), &task_id)
    else {
        panic!("expected verdict");
    };

    let mut missing_source = manifest_json();
    missing_source
        .as_object_mut()
        .unwrap()
        .remove("sourceTreeRoot");
    let mut self_ref = manifest_json();
    self_ref
        .as_object_mut()
        .unwrap()
        .insert("submissionId".into(), json!(digest(0x77)));

    let fixture = json!({
        "domain": "boole.useful-work.product.v0",
        "manifest": parsed.to_json_value(),
        "taskId": task_id.to_hex(),
        "expectedArtifactRoot": parsed.artifact_root().to_hex(),
        "expectedSubmissionId": parsed.submission_identity(&task_id).submission_id().to_hex(),
        "expectedReceiptDigest": receipt.receipt_digest().to_hex(),
        "rejectedManifests": [
            { "manifest": missing_source, "reason": "malformed-json" },
            { "manifest": self_ref, "reason": "self-referential-field" },
        ],
    });
    let pretty = format!("{}\n", serde_json::to_string_pretty(&fixture).unwrap());
    std::fs::create_dir_all(std::path::Path::new(FIXTURE_PATH).parent().unwrap()).unwrap();
    std::fs::write(FIXTURE_PATH, pretty).expect("write fixture");
}

#[test]
fn product_error_labels_are_stable() {
    assert_eq!(
        UsefulProductError::SelfReferentialField.label(),
        "self-referential-field"
    );
}
