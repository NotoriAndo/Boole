//! BF.6 pure contract tracer: stable task identity is distinct from an
//! epoch/challenge/registry-bound task instance.

use boole_core::useful_product::{ReceiptVerdict, VerificationReceipt};
use boole_core::useful_work_bf6::{
    decode_signed_assignment, decode_signed_commit, decode_signed_reveal, validate_commit,
    validate_native_receipt_bridge, validate_reveal, NativeTaskIdentity, TaskInstanceBinding,
    UsefulWorkAssignment, UsefulWorkBf6Error, UsefulWorkCommit, UsefulWorkReveal,
};
use boole_core::{Hex32, SigningKeyV2};

fn digest(byte: u8) -> Hex32 {
    Hex32::from_bytes([byte; 32])
}

#[test]
fn stable_task_id_is_distinct_from_challenge_bound_instance_id() {
    let native =
        NativeTaskIdentity::try_new("rust-tuple-struct-project-v1", digest(0x11), digest(0x22))
            .expect("valid native task identity");
    let stable = native.task_id();
    let first = TaskInstanceBinding::new(stable, digest(0x31), 7, digest(0x41));
    let second = TaskInstanceBinding::new(stable, digest(0x32), 7, digest(0x41));
    let next_epoch = TaskInstanceBinding::new(stable, digest(0x31), 8, digest(0x41));
    let next_registry = TaskInstanceBinding::new(stable, digest(0x31), 7, digest(0x42));

    assert_eq!(
        native.task_id(),
        stable,
        "the task identity must stay stable"
    );
    assert_ne!(
        first.task_instance_id(),
        second.task_instance_id(),
        "a fresh challenge must create a distinct task instance"
    );
    assert_ne!(first.task_instance_id(), next_epoch.task_instance_id());
    assert_ne!(first.task_instance_id(), next_registry.task_instance_id());
    assert_ne!(
        stable.as_hex32(),
        first.task_instance_id().as_hex32(),
        "task_id and task_instance_id use distinct domains and types"
    );
}

#[test]
fn assignment_json_round_trip_preserves_stable_task_and_instance_bindings() {
    let native =
        NativeTaskIdentity::try_new("rust-tuple-struct-project-v1", digest(0x11), digest(0x22))
            .expect("valid native task identity");
    let assignment = UsefulWorkAssignment::try_new(
        native,
        digest(0x31),
        7,
        digest(0x41),
        digest(0x51),
        "boole-testnet",
        digest(0x61),
        digest(0x71),
    )
    .expect("valid assignment");

    let encoded = assignment.to_canonical_json_bytes();
    let decoded = UsefulWorkAssignment::from_json_bytes(&encoded).expect("strict round trip");
    assert_eq!(decoded, assignment);
    assert_eq!(decoded.task_id(), assignment.native_task().task_id());
    assert_eq!(
        decoded.task_instance_id(),
        TaskInstanceBinding::new(decoded.task_id(), digest(0x31), 7, digest(0x41),)
            .task_instance_id()
    );
    assert_eq!(decoded.assignment_digest(), assignment.assignment_digest());
}

#[test]
fn assignment_json_rejects_unknown_and_duplicate_fields() {
    let native = NativeTaskIdentity::try_new("family-v1", digest(0x11), digest(0x22)).unwrap();
    let assignment = UsefulWorkAssignment::try_new(
        native,
        digest(0x31),
        7,
        digest(0x41),
        digest(0x51),
        "boole-testnet",
        digest(0x61),
        digest(0x71),
    )
    .unwrap();
    let mut unknown = assignment.to_json_value();
    unknown["answerHint"] = serde_json::json!("forbidden");
    assert!(UsefulWorkAssignment::from_json_bytes(&serde_json::to_vec(&unknown).unwrap()).is_err());

    let canonical = String::from_utf8(assignment.to_canonical_json_bytes()).unwrap();
    let duplicate = canonical.replacen(
        "{",
        &format!("{{\"taskId\":\"{}\",", assignment.task_id().to_hex()),
        1,
    );
    assert!(
        UsefulWorkAssignment::from_json_bytes(duplicate.as_bytes()).is_err(),
        "duplicate object keys must fail before typed decoding"
    );
}

#[test]
fn commit_and_reveal_round_trip_under_one_assignment() {
    let native = NativeTaskIdentity::try_new("family-v1", digest(0x11), digest(0x22)).unwrap();
    let assignment = UsefulWorkAssignment::try_new(
        native,
        digest(0x31),
        7,
        digest(0x41),
        digest(0x51),
        "boole-testnet",
        digest(0x61),
        digest(0x71),
    )
    .unwrap();
    let submission_id = digest(0x81);
    let candidate_digest = digest(0x82);
    let nonce = digest(0x83);
    let commit = UsefulWorkCommit::try_new(&assignment, submission_id, candidate_digest, nonce)
        .expect("commit derives from the assigned reveal inputs");
    let reveal =
        UsefulWorkReveal::try_new(&assignment, &commit, submission_id, candidate_digest, nonce)
            .expect("reveal matches commitment");

    assert_eq!(
        UsefulWorkCommit::from_json_bytes(&commit.to_canonical_json_bytes()).unwrap(),
        commit
    );
    assert_eq!(
        UsefulWorkReveal::from_json_bytes(&reveal.to_canonical_json_bytes()).unwrap(),
        reveal
    );
    validate_reveal(&assignment, &commit, &reveal).expect("one bound flow validates");
}

fn signed_wire(key: &SigningKeyV2, payload: serde_json::Value, network_id: &str) -> Vec<u8> {
    let signed = key
        .sign_for_network(&payload, Some(network_id))
        .expect("sign network-bound payload");
    serde_json::to_vec(&serde_json::json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
        "network_id": network_id,
    }))
    .unwrap()
}

#[test]
fn signed_commit_and_reveal_bind_the_assignee_and_network() {
    let key = SigningKeyV2::from_dev_id("bf6-assignee");
    let assignee = Hex32::from_hex(&key.pk_hex()).unwrap();
    let native = NativeTaskIdentity::try_new("family-v1", digest(0x11), digest(0x22)).unwrap();
    let assignment = UsefulWorkAssignment::try_new(
        native,
        digest(0x31),
        7,
        digest(0x41),
        digest(0x51),
        "boole-testnet",
        assignee,
        digest(0x71),
    )
    .unwrap();
    let commit =
        UsefulWorkCommit::try_new(&assignment, digest(0x81), digest(0x82), digest(0x83)).unwrap();
    let reveal = UsefulWorkReveal::try_new(
        &assignment,
        &commit,
        digest(0x81),
        digest(0x82),
        digest(0x83),
    )
    .unwrap();

    let signed_commit = signed_wire(&key, commit.to_json_value(), "boole-testnet");
    let signed_reveal = signed_wire(&key, reveal.to_json_value(), "boole-testnet");
    assert_eq!(
        decode_signed_commit(&signed_commit, &assignment).unwrap(),
        commit
    );
    assert_eq!(
        decode_signed_reveal(&signed_reveal, &assignment, &commit).unwrap(),
        reveal
    );
}

fn assignment_for(
    challenge: u8,
    network_id: &str,
    assignee: Hex32,
    reward: u8,
) -> UsefulWorkAssignment {
    UsefulWorkAssignment::try_new(
        NativeTaskIdentity::try_new("family-v1", digest(0x11), digest(0x22)).unwrap(),
        digest(challenge),
        7,
        digest(0x41),
        digest(0x51),
        network_id,
        assignee,
        digest(reward),
    )
    .unwrap()
}

#[test]
fn challenge_network_and_reward_swaps_are_rejected() {
    let key = SigningKeyV2::from_dev_id("bf6-swap-matrix");
    let assignee = Hex32::from_hex(&key.pk_hex()).unwrap();
    let assigned = assignment_for(0x31, "boole-testnet", assignee, 0x71);
    let commit =
        UsefulWorkCommit::try_new(&assigned, digest(0x81), digest(0x82), digest(0x83)).unwrap();

    let challenge_swapped = assignment_for(0x32, "boole-testnet", assignee, 0x71);
    assert!(validate_commit(&challenge_swapped, &commit).is_err());

    let network_swapped = assignment_for(0x31, "boole-mainnet", assignee, 0x71);
    assert!(validate_commit(&network_swapped, &commit).is_err());

    let reward_swapped = assignment_for(0x31, "boole-testnet", assignee, 0x72);
    assert!(validate_commit(&reward_swapped, &commit).is_err());
}

#[test]
fn nonce_swap_after_commit_is_rejected_even_when_the_signer_resigns() {
    let key = SigningKeyV2::from_dev_id("bf6-nonce-swap");
    let assignee = Hex32::from_hex(&key.pk_hex()).unwrap();
    let assignment = assignment_for(0x31, "boole-testnet", assignee, 0x71);
    let commit =
        UsefulWorkCommit::try_new(&assignment, digest(0x81), digest(0x82), digest(0x83)).unwrap();
    let reveal = UsefulWorkReveal::try_new(
        &assignment,
        &commit,
        digest(0x81),
        digest(0x82),
        digest(0x83),
    )
    .unwrap();
    let mut swapped = reveal.to_json_value();
    swapped["nonce"] = serde_json::json!(digest(0x84).to_hex());
    let resigned = signed_wire(&key, swapped, "boole-testnet");

    assert!(decode_signed_reveal(&resigned, &assignment, &commit).is_err());
}

#[test]
fn copied_reveal_and_task_task_instance_confusion_are_rejected() {
    let key = SigningKeyV2::from_dev_id("bf6-copy-reveal");
    let assignee = Hex32::from_hex(&key.pk_hex()).unwrap();
    let first = assignment_for(0x31, "boole-testnet", assignee, 0x71);
    let second = assignment_for(0x32, "boole-testnet", assignee, 0x71);
    let first_commit =
        UsefulWorkCommit::try_new(&first, digest(0x81), digest(0x82), digest(0x83)).unwrap();
    let second_commit =
        UsefulWorkCommit::try_new(&second, digest(0x81), digest(0x82), digest(0x83)).unwrap();
    let first_reveal = UsefulWorkReveal::try_new(
        &first,
        &first_commit,
        digest(0x81),
        digest(0x82),
        digest(0x83),
    )
    .unwrap();

    let copied = signed_wire(&key, first_reveal.to_json_value(), "boole-testnet");
    assert!(decode_signed_reveal(&copied, &second, &second_commit).is_err());

    let mut confused = first_reveal.to_json_value();
    let stable = confused["taskId"].clone();
    confused["taskId"] = confused["taskInstanceId"].clone();
    confused["taskInstanceId"] = stable;
    let resigned = signed_wire(&key, confused, "boole-testnet");
    assert!(decode_signed_reveal(&resigned, &first, &first_commit).is_err());
}

#[test]
fn bf3_receipt_bridge_rejects_an_unassigned_stable_task() {
    let key = SigningKeyV2::from_dev_id("bf6-receipt-bridge");
    let assignee = Hex32::from_hex(&key.pk_hex()).unwrap();
    let assignment = assignment_for(0x31, "boole-testnet", assignee, 0x71);
    let commit =
        UsefulWorkCommit::try_new(&assignment, digest(0x81), digest(0x82), digest(0x83)).unwrap();
    let reveal = UsefulWorkReveal::try_new(
        &assignment,
        &commit,
        digest(0x81),
        digest(0x82),
        digest(0x83),
    )
    .unwrap();
    let assigned_receipt = VerificationReceipt {
        task_id: assignment.task_id().as_hex32(),
        submission_id: reveal.submission_id(),
        artifact_root: digest(0x91),
        checker_hash: digest(0x92),
        verdict: ReceiptVerdict::Accepted,
    };
    let bridge = validate_native_receipt_bridge(&assignment, &commit, &reveal, &assigned_receipt)
        .expect("the assigned stable task bridges");
    assert_eq!(bridge.task_id(), assignment.task_id());
    assert_eq!(bridge.task_instance_id(), assignment.task_instance_id());

    let unassigned = VerificationReceipt {
        task_id: digest(0xaa),
        ..assigned_receipt
    };
    assert!(
        validate_native_receipt_bridge(&assignment, &commit, &reveal, &unassigned).is_err(),
        "a receipt for any other stable task must not enter this assignment"
    );
}

#[test]
fn signed_assignment_binds_the_authority_and_network() {
    let authority = SigningKeyV2::from_dev_id("bf6-assignment-authority");
    let authority_pk = Hex32::from_hex(&authority.pk_hex()).unwrap();
    let assignee = SigningKeyV2::from_dev_id("bf6-assignment-recipient");
    let assignment = assignment_for(
        0x31,
        "boole-testnet",
        Hex32::from_hex(&assignee.pk_hex()).unwrap(),
        0x71,
    );
    let wire = signed_wire(&authority, assignment.to_json_value(), "boole-testnet");

    assert_eq!(
        decode_signed_assignment(&wire, "boole-testnet", authority_pk).unwrap(),
        assignment
    );
    assert!(decode_signed_assignment(&wire, "boole-mainnet", authority_pk).is_err());
    assert!(decode_signed_assignment(&wire, "boole-testnet", digest(0xaa)).is_err());
}

#[test]
fn canonical_bf6_derivations_match_the_frozen_contract() {
    let assignment = assignment_for(0x31, "boole-testnet", digest(0x61), 0x71);
    let commit =
        UsefulWorkCommit::try_new(&assignment, digest(0x81), digest(0x82), digest(0x83)).unwrap();
    let reveal = UsefulWorkReveal::try_new(
        &assignment,
        &commit,
        digest(0x81),
        digest(0x82),
        digest(0x83),
    )
    .unwrap();
    let receipt = VerificationReceipt {
        task_id: assignment.task_id().as_hex32(),
        submission_id: reveal.submission_id(),
        artifact_root: digest(0x91),
        checker_hash: digest(0x92),
        verdict: ReceiptVerdict::Accepted,
    };
    let bridge = validate_native_receipt_bridge(&assignment, &commit, &reveal, &receipt).unwrap();
    assert_eq!(
        [
            assignment.task_id().to_hex(),
            assignment.task_instance_id().to_hex(),
            assignment.assignment_digest().to_hex(),
            commit.result_commitment().to_hex(),
            commit.commit_digest().to_hex(),
            bridge.receipt_digest().to_hex(),
            bridge.bridge_digest().to_hex(),
        ],
        [
            "856fda5f156a000368efd0f47c71ec4c2713aca5a8877b192e848a212b5b34b3".to_string(),
            "22819fbc62a569032268ce2de596db930194756d5a7b7c1c441d6c390d919371".to_string(),
            "60c31c1e08a523133850fc183d48262d78beea47e5cae56ca86e991c1eda08c4".to_string(),
            "170d932b864fb991b60db7d0792124605c0f0659bfb375e0045719cfa5ebf22e".to_string(),
            "cfc24a424b6c8ac8a0684b64e5a524958ed6d497e24b1387d13a7a8abe2918f1".to_string(),
            "6b4601e713ff727c3fa27334ba99d76607a75130089d1d0823004a4edf4700fb".to_string(),
            "00ed8340505deeebef59aa955b8a33f749d476d6c548f6828ebf628d1a2f867c".to_string(),
        ]
    );
}

#[test]
fn bf6_rejection_labels_are_stable() {
    assert_eq!(
        UsefulWorkBf6Error::StableTaskIdMismatch.label(),
        "task-id-mismatch"
    );
    assert_eq!(
        UsefulWorkBf6Error::TaskInstanceIdMismatch.label(),
        "task-instance-id-mismatch"
    );
    assert_eq!(
        UsefulWorkBf6Error::ReceiptTaskNotAssigned.label(),
        "unassigned-task"
    );
    assert_eq!(
        UsefulWorkBf6Error::NetworkBindingMismatch.label(),
        "network-binding-mismatch"
    );
    assert_eq!(
        UsefulWorkBf6Error::SignerBindingMismatch.label(),
        "signer-binding-mismatch"
    );
    assert_eq!(
        UsefulWorkBf6Error::CommitmentMismatch.label(),
        "commitment-mismatch"
    );
}
