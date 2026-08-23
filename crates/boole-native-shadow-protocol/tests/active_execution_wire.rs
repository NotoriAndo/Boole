use boole_native_shadow_protocol::{
    decode_complete_active_execution_ready_frame,
    decode_complete_closed_local_replay_execution_ready_frame, encode_active_execution_ready_frame,
    encode_closed_local_replay_execution_ready_frame, encode_execution_request_frame, sha256_hex,
    submission_digest_hex, verify_closed_local_replay_execution_authority_bytes,
    verify_local_execution_authority_bytes, ActiveExecutionReady, ActiveExecutionReadyFields,
    ClosedLocalReplayExecutionAuthorityError, ClosedLocalReplayExecutionReady,
    ClosedLocalReplayExecutionReadyFields, ExecutionHello, ExecutionRequest,
    ExecutionRequestFields, LocalExecutionAuthorityError,
    TRACKED_CLOSED_LOCAL_REPLAY_EXECUTION_AUTHORITY_BYTES, TRACKED_EXECUTION_POLICY_BYTES,
    TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES,
};
use sha2::{Digest, Sha256};

fn h(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn request() -> ExecutionRequest {
    let family = "rust-tuple-struct-project-v1".to_string();
    let template = h(3);
    let challenge = h(4);
    let raw = b"answer";
    ExecutionRequest::try_new(ExecutionRequestFields {
        nonce_hex: h(1),
        operation_id_hex: h(2),
        family_version: family.clone(),
        template_id: template.clone(),
        challenge_sha256: challenge.clone(),
        epoch: 7,
        raw_answer_base64: "YW5zd2Vy".to_string(),
        submission_source_base64: "Zm4gbWFpbigpIHt9Cg==".to_string(),
        submission_source_digest_hex: sha256_hex(b"fn main() {}\n"),
        candidate_digest_hex: sha256_hex(raw),
        submission_digest_hex: submission_digest_hex(&family, &template, &challenge, 7, raw)
            .unwrap(),
        registry_version: "native-shadow-registry-v1".to_string(),
        registry_digest_hex: h(5),
        anchor_digest_hex: h(6),
        task_digest_hex: h(7),
        checker_artifact_hash_hex: h(8),
        checker_policy_digest_hex: h(9),
        checker_release_manifest_digest_hex: h(10),
        toolchain_identity_digest_hex: h(11),
        execution_policy_digest_hex: hex::encode(Sha256::digest(TRACKED_EXECUTION_POLICY_BYTES)),
        intake_version: "RUST-TUPLE-STRUCT-NATIVE-PROOF-INTAKE-V1".to_string(),
    })
    .unwrap()
}

#[test]
fn successor_authority_is_exact_byte_bound_and_keeps_v1_disabled() {
    let authority = verify_local_execution_authority_bytes(TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES)
        .expect("the tracked local authority must verify");

    assert!(!authority.activation_allowed());
    assert_eq!(authority.scope(), "closed-local-loopback-only");
    assert!(authority.loopback_only());
    assert!(!authority.p2p_propagation_allowed());
    assert!(!authority.consensus_allowed());
    assert_eq!(authority.reward_mode(), "no_protocol_reward");
    assert!(!authority.mineable_now());
    assert!(authority.requires_exact_linux_containment());
    assert!(authority.requires_verified_runtime_rootfs_replay());
    assert!(!authority.allows_degraded_containment());
    assert_eq!(
        authority.base_execution_policy_sha256(),
        hex::encode(Sha256::digest(TRACKED_EXECUTION_POLICY_BYTES))
    );

    let v1: serde_json::Value = serde_json::from_slice(TRACKED_EXECUTION_POLICY_BYTES).unwrap();
    assert_eq!(v1["activationAllowed"], false);
    assert_eq!(
        v1["checkerInvocation"]["executionAllowedUnderThisRelease"],
        false
    );

    let mut drifted = TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES.to_vec();
    drifted.push(b'\n');
    assert!(matches!(
        verify_local_execution_authority_bytes(&drifted),
        Err(LocalExecutionAuthorityError::ByteMismatch)
    ));
}

#[test]
fn local_ready_v2_stays_disabled_while_binding_closed_local_constraints() {
    let authority =
        verify_local_execution_authority_bytes(TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES).unwrap();
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();
    let ready = ActiveExecutionReady::try_new(
        &hello,
        &authority,
        ActiveExecutionReadyFields {
            launcher_pid: 1234,
            launcher_uid: 0,
            launcher_gid: 0,
            node_uid: 1001,
            node_gid: 1001,
            checker_uid: 1002,
            checker_gid: 1002,
            startup_recovery_complete: true,
            active_execution_leaves: 0,
            unexpected_direct_cgroup_children: 0,
            manager_subgroup_verified: true,
            launcher_instance_id_hex: h(14),
            runtime_rootfs_replay_verified: true,
        },
    )
    .unwrap();

    assert_eq!(ready.nonce_hex(), hello.nonce_hex());
    assert_eq!(ready.request_digest_hex(), hello.request_digest_hex());
    assert_eq!(
        ready.execution_policy_digest_hex(),
        hello.execution_policy_digest_hex()
    );
    assert!(!ready.activation_allowed());
    assert!(ready.local_only());
    assert!(!ready.p2p_propagation_allowed());
    assert!(!ready.consensus_allowed());
    assert_eq!(ready.reward_mode(), "no_protocol_reward");
    assert!(!ready.mineable_now());
    assert!(ready.exact_linux_containment_required());
    assert!(ready.startup_recovery_complete());
    assert_eq!(ready.active_execution_leaves(), 0);
    assert_eq!(ready.unexpected_direct_cgroup_children(), 0);
    assert!(ready.manager_subgroup_verified());
    assert!(ready.runtime_rootfs_replay_verified());
    assert!(ready.ready());

    let encoded = encode_active_execution_ready_frame(&ready).unwrap();
    assert_eq!(
        decode_complete_active_execution_ready_frame(&encoded).unwrap(),
        ready
    );

    let v1 = boole_native_shadow_protocol::ExecutionReady::try_new(
        &hello,
        boole_native_shadow_protocol::ExecutionReadyFields {
            launcher_pid: 1234,
            launcher_uid: 0,
            launcher_gid: 0,
            node_uid: 1001,
            node_gid: 1001,
            checker_uid: 1002,
            checker_gid: 1002,
        },
    )
    .unwrap();
    assert!(!v1.activation_allowed());
}

#[test]
fn active_ready_v2_rejects_claimed_readiness_without_rootfs_replay() {
    let authority =
        verify_local_execution_authority_bytes(TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES).unwrap();
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();
    let ready = ActiveExecutionReady::try_new(
        &hello,
        &authority,
        ActiveExecutionReadyFields {
            launcher_pid: 1234,
            launcher_uid: 0,
            launcher_gid: 0,
            node_uid: 1001,
            node_gid: 1001,
            checker_uid: 1002,
            checker_gid: 1002,
            startup_recovery_complete: true,
            active_execution_leaves: 0,
            unexpected_direct_cgroup_children: 0,
            manager_subgroup_verified: true,
            launcher_instance_id_hex: h(14),
            runtime_rootfs_replay_verified: true,
        },
    )
    .unwrap();
    let encoded = encode_active_execution_ready_frame(&ready).unwrap();
    let declared = u32::from_be_bytes(encoded[..4].try_into().unwrap()) as usize;
    let mut payload: serde_json::Value = serde_json::from_slice(&encoded[4..]).unwrap();
    payload["runtimeRootfsReplayVerified"] = serde_json::Value::Bool(false);
    let payload = serde_json::to_vec(&payload).unwrap();
    assert_ne!(payload.len(), declared);
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);

    assert!(decode_complete_active_execution_ready_frame(&frame).is_err());
}

#[test]
fn replay_ready_v3_is_byte_bound_and_requires_real_rootfs_replay_without_activation() {
    let authority = verify_closed_local_replay_execution_authority_bytes(
        TRACKED_CLOSED_LOCAL_REPLAY_EXECUTION_AUTHORITY_BYTES,
    )
    .expect("the tracked closed-local replay authority must verify");
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();
    let ready = ClosedLocalReplayExecutionReady::try_new(
        &hello,
        &authority,
        ClosedLocalReplayExecutionReadyFields {
            launcher_pid: 1234,
            launcher_uid: 0,
            launcher_gid: 0,
            node_uid: 1001,
            node_gid: 1001,
            checker_uid: 1002,
            checker_gid: 1002,
            startup_recovery_complete: true,
            active_execution_leaves: 0,
            unexpected_direct_cgroup_children: 0,
            manager_subgroup_verified: true,
            launcher_instance_id_hex: h(14),
            installed_replay_authorities_verified: true,
            runtime_rootfs_replay_verified: true,
            production_activation_allowed: false,
        },
    )
    .unwrap();

    assert!(ready.installed_replay_authorities_verified());
    assert!(ready.runtime_rootfs_replay_verified());
    assert!(!ready.production_activation_allowed());
    assert!(!ready.activation_allowed());
    assert!(ready.ready());

    let encoded = encode_closed_local_replay_execution_ready_frame(&ready).unwrap();
    assert_eq!(
        decode_complete_closed_local_replay_execution_ready_frame(&encoded).unwrap(),
        ready
    );

    let mut drifted = TRACKED_CLOSED_LOCAL_REPLAY_EXECUTION_AUTHORITY_BYTES.to_vec();
    drifted.push(b'\n');
    assert!(matches!(
        verify_closed_local_replay_execution_authority_bytes(&drifted),
        Err(ClosedLocalReplayExecutionAuthorityError::ByteMismatch)
    ));
}

#[test]
fn replay_ready_v3_rejects_missing_installed_authority_proof_or_claimed_activation() {
    let authority = verify_closed_local_replay_execution_authority_bytes(
        TRACKED_CLOSED_LOCAL_REPLAY_EXECUTION_AUTHORITY_BYTES,
    )
    .unwrap();
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();

    for (installed, rootfs, activation) in [
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        assert!(ClosedLocalReplayExecutionReady::try_new(
            &hello,
            &authority,
            ClosedLocalReplayExecutionReadyFields {
                launcher_pid: 1234,
                launcher_uid: 0,
                launcher_gid: 0,
                node_uid: 1001,
                node_gid: 1001,
                checker_uid: 1002,
                checker_gid: 1002,
                startup_recovery_complete: true,
                active_execution_leaves: 0,
                unexpected_direct_cgroup_children: 0,
                manager_subgroup_verified: true,
                launcher_instance_id_hex: h(14),
                installed_replay_authorities_verified: installed,
                runtime_rootfs_replay_verified: rootfs,
                production_activation_allowed: activation,
            },
        )
        .is_err());
    }
}
