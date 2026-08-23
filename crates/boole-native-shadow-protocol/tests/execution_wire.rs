use boole_native_shadow_protocol::{
    decode_complete_execution_hello_frame, decode_complete_execution_ready_frame,
    decode_complete_execution_report_frame, decode_complete_execution_request_frame,
    decode_complete_qualification_ready_frame, encode_execution_hello_frame,
    encode_execution_ready_frame, encode_execution_report_frame, encode_execution_request_frame,
    execution_request_digest_hex, read_execution_hello, read_execution_ready,
    read_execution_report, read_execution_request, sha256_hex, submission_digest_hex,
    validate_execution_session, write_execution_hello, write_execution_ready,
    write_execution_report, write_execution_request, AuthorityBindings, AuthorityBindingsFields,
    CheckerOutputStatus, CheckerParsedResult, CheckerParsedResultFields, CheckerReason,
    CheckerResult, CheckerResultFields, CheckerVerdict, Cleanup, CleanupFields, ExecutionHello,
    ExecutionReady, ExecutionReadyFields, ExecutionReport, ExecutionReportFields, ExecutionRequest,
    ExecutionRequestFields, ResourceObservations, ResourceObservationsFields, WaitStatus,
};
use serde_json::{json, Value};
use std::io::Cursor;

const RAW: &[u8] = b"answer";
const SOURCE: &[u8] = b"fn main() {}\n";
const RAW_B64: &str = "YW5zd2Vy";
const SOURCE_B64: &str = "Zm4gbWFpbigpIHt9Cg==";

fn h(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn request() -> ExecutionRequest {
    let family = "rust-tuple-struct-project-v1".to_string();
    let template = h(3);
    let challenge = h(4);
    let submission = submission_digest_hex(&family, &template, &challenge, 7, RAW).unwrap();
    ExecutionRequest::try_new(ExecutionRequestFields {
        nonce_hex: h(1),
        operation_id_hex: h(2),
        family_version: family,
        template_id: template,
        challenge_sha256: challenge,
        epoch: 7,
        raw_answer_base64: RAW_B64.to_string(),
        submission_source_base64: SOURCE_B64.to_string(),
        submission_source_digest_hex: sha256_hex(SOURCE),
        candidate_digest_hex: sha256_hex(RAW),
        submission_digest_hex: submission,
        registry_version: "native-shadow-registry-v1".to_string(),
        registry_digest_hex: h(5),
        anchor_digest_hex: h(6),
        task_digest_hex: h(7),
        checker_artifact_hash_hex: h(8),
        checker_policy_digest_hex: h(9),
        checker_release_manifest_digest_hex: h(10),
        toolchain_identity_digest_hex: h(11),
        execution_policy_digest_hex: h(12),
        intake_version: "RUST-TUPLE-STRUCT-NATIVE-PROOF-INTAKE-V1".to_string(),
    })
    .unwrap()
}

fn authority() -> AuthorityBindings {
    AuthorityBindings::try_new(AuthorityBindingsFields {
        registry_version: "native-shadow-registry-v1".to_string(),
        registry_digest_hex: h(5),
        anchor_digest_hex: h(6),
        task_digest_hex: h(7),
        checker_artifact_hash_hex: h(8),
        checker_policy_digest_hex: h(9),
        checker_release_manifest_digest_hex: h(10),
        toolchain_identity_digest_hex: h(11),
    })
    .unwrap()
}

fn resources(output_limit_exceeded: bool) -> ResourceObservations {
    ResourceObservations::try_new(ResourceObservationsFields {
        memory_events_low_delta: 0,
        memory_events_high_delta: 0,
        memory_events_max_delta: 0,
        memory_events_oom_delta: 0,
        memory_events_oom_kill_delta: 0,
        memory_events_oom_group_kill_delta: 0,
        pids_events_max_delta: 0,
        cpu_usage_usec_delta: 42,
        output_limit_exceeded,
    })
    .unwrap()
}

fn cleanup() -> Cleanup {
    Cleanup::try_new(CleanupFields {
        child_reaped: true,
        cgroup_populated_zero: true,
        launcher_pidfd_and_namespace_fds_closed: true,
        cgroup_leaf_removed: true,
        completed_within_deadline: true,
    })
    .unwrap()
}

fn accepted_result() -> CheckerResult {
    let parsed = CheckerParsedResult::try_new(CheckerParsedResultFields {
        verdict: CheckerVerdict::Accepted,
        reason_code: CheckerReason::Accepted,
        checker_task_id: Some("fixture-task-1".to_string()),
        task_digest_hex: Some(h(7)),
    })
    .unwrap();
    CheckerResult::try_new(CheckerResultFields {
        status: CheckerOutputStatus::ValidCheckerResult,
        stdout_sha256_hex: h(13),
        stderr_sha256_hex: sha256_hex(b""),
        stdout_bytes: 128,
        stderr_bytes: 0,
        parsed: Some(parsed),
    })
    .unwrap()
}

fn report(request_digest_hex: String) -> ExecutionReport {
    ExecutionReport::try_new(ExecutionReportFields {
        nonce_hex: h(1),
        operation_id_hex: h(2),
        request_digest_hex,
        execution_policy_digest_hex: h(12),
        launcher_pid: 1234,
        launcher_uid: 0,
        launcher_gid: 0,
        node_uid: 1001,
        node_gid: 1001,
        checker_uid: 1002,
        checker_gid: 1002,
        authority_bindings: authority(),
        wait_status: WaitStatus::exited(0),
        timed_out: false,
        resource_observations: resources(false),
        cleanup: cleanup(),
        checker_result: accepted_result(),
    })
    .unwrap()
}

fn payload_value(frame: &[u8]) -> Value {
    serde_json::from_slice(&frame[4..]).unwrap()
}

fn frame(value: &Value) -> Vec<u8> {
    let payload = serde_json::to_vec(value).unwrap();
    let mut result = Vec::with_capacity(4 + payload.len());
    result.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    result.extend_from_slice(&payload);
    result
}

fn execution_ready_fields() -> ExecutionReadyFields {
    ExecutionReadyFields {
        launcher_pid: 1234,
        launcher_uid: 0,
        launcher_gid: 0,
        node_uid: 1001,
        node_gid: 1001,
        checker_uid: 1002,
        checker_gid: 1002,
    }
}

#[test]
fn hello_is_derived_from_the_exact_execute_frame_and_round_trips() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();

    assert_eq!(hello.nonce_hex(), h(1));
    assert_eq!(
        hello.request_digest_hex(),
        execution_request_digest_hex(&request_frame).unwrap()
    );
    assert_eq!(hello.request_length_bytes(), request_frame.len() as u32 - 4);
    assert_eq!(hello.execution_policy_digest_hex(), h(12));

    let encoded = encode_execution_hello_frame(&hello).unwrap();
    assert_eq!(
        decode_complete_execution_hello_frame(&encoded).unwrap(),
        hello
    );
    let mut whitespace_payload = Vec::from(&request_frame[4..]);
    whitespace_payload.insert(1, b' ');
    let mut whitespace_frame = Vec::new();
    whitespace_frame.extend_from_slice(&(whitespace_payload.len() as u32).to_be_bytes());
    whitespace_frame.extend_from_slice(&whitespace_payload);
    let whitespace_hello =
        ExecutionHello::try_from_execution_request_frame(&whitespace_frame).unwrap();
    assert_ne!(
        hello.request_digest_hex(),
        whitespace_hello.request_digest_hex()
    );
}

#[test]
fn hello_rejects_invalid_execute_frames_and_strict_json_drift() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    assert!(ExecutionHello::try_from_execution_request_frame(&request_frame[..3]).is_err());
    assert!(ExecutionHello::try_from_execution_request_frame(
        &request_frame[..request_frame.len() - 1]
    )
    .is_err());
    let mut trailing = request_frame.clone();
    trailing.push(0);
    assert!(ExecutionHello::try_from_execution_request_frame(&trailing).is_err());

    let mut wrong_execute = payload_value(&request_frame);
    wrong_execute["schema"] = json!("boole.native-shadow.launcher.not-execute.v1");
    assert!(ExecutionHello::try_from_execution_request_frame(&frame(&wrong_execute)).is_err());
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();
    let encoded = encode_execution_hello_frame(&hello).unwrap();

    let mut value = payload_value(&encoded);
    value["surprise"] = json!(true);
    assert!(decode_complete_execution_hello_frame(&frame(&value)).is_err());

    let mut value = payload_value(&encoded);
    value.as_object_mut().unwrap().remove("requestDigestHex");
    assert!(decode_complete_execution_hello_frame(&frame(&value)).is_err());

    let mut value = payload_value(&encoded);
    value["requestLengthBytes"] = json!(1.0);
    assert!(decode_complete_execution_hello_frame(&frame(&value)).is_err());
    let payload = String::from_utf8(encoded[4..].to_vec()).unwrap();
    let duplicated = payload.replacen("{", &format!("{{\"requestDigestHex\":\"{}\",", h(20)), 1);
    let mut duplicate_frame = Vec::new();
    duplicate_frame.extend_from_slice(&(duplicated.len() as u32).to_be_bytes());
    duplicate_frame.extend_from_slice(duplicated.as_bytes());
    assert!(decode_complete_execution_hello_frame(&duplicate_frame).is_err());

    for invalid_length in [0_u32, 131_073] {
        let mut value = payload_value(&encoded);
        value["requestLengthBytes"] = json!(invalid_length);
        assert!(decode_complete_execution_hello_frame(&frame(&value)).is_err());
    }
}

#[test]
fn ready_echoes_hello_and_validates_fixed_service_identities() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();
    let ready = ExecutionReady::try_new(&hello, execution_ready_fields()).unwrap();

    assert_eq!(ready.nonce_hex(), hello.nonce_hex());
    assert_eq!(ready.request_digest_hex(), hello.request_digest_hex());
    assert_eq!(
        ready.execution_policy_digest_hex(),
        hello.execution_policy_digest_hex()
    );
    assert_eq!(ready.launcher_pid(), 1234);
    assert!(!ready.activation_allowed());
    assert!(ready.ready());

    let encoded = encode_execution_ready_frame(&ready).unwrap();
    assert_eq!(
        decode_complete_execution_ready_frame(&encoded).unwrap(),
        ready
    );

    for invalid in [
        ExecutionReadyFields {
            launcher_pid: 0,
            ..execution_ready_fields()
        },
        ExecutionReadyFields {
            launcher_uid: 1,
            ..execution_ready_fields()
        },
        ExecutionReadyFields {
            checker_uid: 1001,
            ..execution_ready_fields()
        },
        ExecutionReadyFields {
            checker_gid: 1001,
            ..execution_ready_fields()
        },
    ] {
        assert!(ExecutionReady::try_new(&hello, invalid).is_err());
    }
}

#[test]
fn ready_inbound_decoder_rejects_strict_json_and_literal_drift() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();
    let ready = ExecutionReady::try_new(&hello, execution_ready_fields()).unwrap();
    let encoded = encode_execution_ready_frame(&ready).unwrap();

    let mut value = payload_value(&encoded);
    value["surprise"] = json!(true);
    assert!(decode_complete_execution_ready_frame(&frame(&value)).is_err());

    let mut value = payload_value(&encoded);
    value.as_object_mut().unwrap().remove("launcherPid");
    assert!(decode_complete_execution_ready_frame(&frame(&value)).is_err());

    let payload = String::from_utf8(encoded[4..].to_vec()).unwrap();
    let duplicated = payload.replacen("{", "{\"ready\":true,", 1);
    let mut duplicate_frame = Vec::new();
    duplicate_frame.extend_from_slice(&(duplicated.len() as u32).to_be_bytes());
    duplicate_frame.extend_from_slice(duplicated.as_bytes());
    assert!(decode_complete_execution_ready_frame(&duplicate_frame).is_err());

    let mut value = payload_value(&encoded);
    value["launcherPid"] = json!(1234.0);
    assert!(decode_complete_execution_ready_frame(&frame(&value)).is_err());

    for (field, invalid) in [("activationAllowed", json!(true)), ("ready", json!(false))] {
        let mut value = payload_value(&encoded);
        value[field] = invalid;
        assert!(decode_complete_execution_ready_frame(&frame(&value)).is_err());
    }

    let mut trailing = encoded;
    trailing.push(0);
    assert!(decode_complete_execution_ready_frame(&trailing).is_err());
}

#[test]
fn ready_inbound_decoder_rejects_every_invalid_service_identity_class() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();
    let ready = ExecutionReady::try_new(&hello, execution_ready_fields()).unwrap();
    let encoded = encode_execution_ready_frame(&ready).unwrap();

    for (field, invalid) in [
        ("launcherPid", json!(0)),
        ("launcherUid", json!(1)),
        ("launcherGid", json!(1)),
        ("nodeUid", json!(0)),
        ("nodeGid", json!(0)),
        ("checkerUid", json!(0)),
        ("checkerGid", json!(0)),
        ("checkerUid", json!(1001)),
        ("checkerGid", json!(1001)),
    ] {
        let mut value = payload_value(&encoded);
        value[field] = invalid;
        assert!(decode_complete_execution_ready_frame(&frame(&value)).is_err());
    }
}

#[test]
fn execution_and_qualification_ready_frames_cannot_be_reinterpreted() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();
    let ready = ExecutionReady::try_new(&hello, execution_ready_fields()).unwrap();
    let encoded = encode_execution_ready_frame(&ready).unwrap();
    assert!(decode_complete_qualification_ready_frame(&encoded).is_err());

    let qualification = boole_native_shadow_protocol::QualificationReady::try_new(
        boole_native_shadow_protocol::QualificationReadyFields {
            nonce_hex: h(1),
            execution_policy_digest_hex: h(12),
            toolchain_identity_digest_hex: h(11),
            registry_digest_hex: h(5),
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
            activation_allowed: false,
            ready: true,
        },
    )
    .unwrap();
    let encoded =
        boole_native_shadow_protocol::encode_qualification_ready_frame(&qualification).unwrap();
    assert!(decode_complete_execution_ready_frame(&encoded).is_err());
}

#[test]
fn execute_round_trip_and_exact_frame_digest_are_stable() {
    let encoded = encode_execution_request_frame(&request()).unwrap();
    let decoded = decode_complete_execution_request_frame(&encoded).unwrap();
    assert_eq!(decoded, request());

    let digest = execution_request_digest_hex(&encoded).unwrap();
    let mut whitespace_payload = Vec::from(&encoded[4..]);
    whitespace_payload.insert(1, b' ');
    let mut whitespace_frame = Vec::new();
    whitespace_frame.extend_from_slice(&(whitespace_payload.len() as u32).to_be_bytes());
    whitespace_frame.extend_from_slice(&whitespace_payload);
    assert_ne!(
        digest,
        execution_request_digest_hex(&whitespace_frame).unwrap()
    );
}

#[test]
fn execute_rejects_unknown_missing_duplicate_float_and_trailing_bytes() {
    let encoded = encode_execution_request_frame(&request()).unwrap();
    let mut value = payload_value(&encoded);
    value["surprise"] = json!(true);
    assert!(decode_complete_execution_request_frame(&frame(&value)).is_err());

    let mut value = payload_value(&encoded);
    value.as_object_mut().unwrap().remove("epoch");
    assert!(decode_complete_execution_request_frame(&frame(&value)).is_err());

    let payload = String::from_utf8(encoded[4..].to_vec()).unwrap();
    let duplicated = payload.replacen("{", &format!("{{\"nonceHex\":\"{}\",", h(1)), 1);
    let mut duplicate_frame = Vec::new();
    duplicate_frame.extend_from_slice(&(duplicated.len() as u32).to_be_bytes());
    duplicate_frame.extend_from_slice(duplicated.as_bytes());
    assert!(decode_complete_execution_request_frame(&duplicate_frame).is_err());

    let mut value = payload_value(&encoded);
    value["epoch"] = json!(7.0);
    assert!(decode_complete_execution_request_frame(&frame(&value)).is_err());

    let mut trailing = encoded;
    trailing.push(0);
    assert!(decode_complete_execution_request_frame(&trailing).is_err());
}

#[test]
fn execute_rejects_noncanonical_base64_nul_source_and_digest_drift() {
    let encoded = encode_execution_request_frame(&request()).unwrap();
    let mut value = payload_value(&encoded);
    value["rawAnswerBase64"] = json!("YW5zd2Vy=");
    assert!(decode_complete_execution_request_frame(&frame(&value)).is_err());

    let mut value = payload_value(&encoded);
    value["submissionSourceBase64"] = json!("AGZuIG1haW4oKSB7fQo=");
    value["submissionSourceDigestHex"] = json!(sha256_hex(b"\0fn main() {}\n"));
    assert!(decode_complete_execution_request_frame(&frame(&value)).is_err());

    let mut value = payload_value(&encoded);
    value["candidateDigestHex"] = json!(h(30));
    assert!(decode_complete_execution_request_frame(&frame(&value)).is_err());
}

#[test]
fn submission_digest_uses_ascii_hex_not_decoded_digest_bytes() {
    let digest = submission_digest_hex("f", &h(3), &h(4), 7, b"x").unwrap();
    assert_eq!(
        digest,
        "0a15c2aba6342f38b36fab7a8a870247d59084cb5b5002909cf657707681cd90"
    );
}

#[test]
fn report_round_trip_preserves_validated_nested_contract() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let report = report(execution_request_digest_hex(&request_frame).unwrap());
    let encoded = encode_execution_report_frame(&report).unwrap();
    assert_eq!(
        decode_complete_execution_report_frame(&encoded).unwrap(),
        report
    );
}

#[test]
fn report_requires_nullable_keys_and_rejects_unknown_nested_fields() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let encoded = encode_execution_report_frame(&report(
        execution_request_digest_hex(&request_frame).unwrap(),
    ))
    .unwrap();

    let mut value = payload_value(&encoded);
    value["waitStatus"]
        .as_object_mut()
        .unwrap()
        .remove("exitCode");
    assert!(decode_complete_execution_report_frame(&frame(&value)).is_err());

    let mut value = payload_value(&encoded);
    value["checkerResult"]
        .as_object_mut()
        .unwrap()
        .remove("parsed");
    assert!(decode_complete_execution_report_frame(&frame(&value)).is_err());

    let mut value = payload_value(&encoded);
    value["cleanup"]["unexpected"] = json!(true);
    assert!(decode_complete_execution_report_frame(&frame(&value)).is_err());
}

#[test]
fn report_rejects_invalid_wait_identity_and_cleanup_combinations() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let encoded = encode_execution_report_frame(&report(
        execution_request_digest_hex(&request_frame).unwrap(),
    ))
    .unwrap();

    let mut value = payload_value(&encoded);
    value["waitStatus"]["termSignal"] = json!(9);
    assert!(decode_complete_execution_report_frame(&frame(&value)).is_err());

    let mut value = payload_value(&encoded);
    value["launcherUid"] = json!(1000);
    assert!(decode_complete_execution_report_frame(&frame(&value)).is_err());

    let mut value = payload_value(&encoded);
    value["cleanup"]["cgroupLeafRemoved"] = json!(false);
    assert!(decode_complete_execution_report_frame(&frame(&value)).is_err());
}

#[test]
fn report_rejects_checker_cross_field_and_reason_contradictions() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let encoded = encode_execution_report_frame(&report(
        execution_request_digest_hex(&request_frame).unwrap(),
    ))
    .unwrap();

    let mut value = payload_value(&encoded);
    value["timedOut"] = json!(true);
    assert!(decode_complete_execution_report_frame(&frame(&value)).is_err());

    let mut value = payload_value(&encoded);
    value["checkerResult"]["parsed"]["reasonCode"] = json!("compile_or_hidden_test_failed");
    assert!(decode_complete_execution_report_frame(&frame(&value)).is_err());

    let mut value = payload_value(&encoded);
    value["checkerResult"]["parsed"]["taskDigest"] = json!(h(31));
    assert!(decode_complete_execution_report_frame(&frame(&value)).is_err());

    let mut value = payload_value(&encoded);
    value["resourceObservations"]["outputLimitExceeded"] = json!(true);
    assert!(decode_complete_execution_report_frame(&frame(&value)).is_err());
}

#[test]
fn report_accepts_policy_permitted_output_limit_parsed_object() {
    let parsed = CheckerParsedResult::try_new(CheckerParsedResultFields {
        verdict: CheckerVerdict::RetryableUnavailable,
        reason_code: CheckerReason::ResourceOutputLimit,
        checker_task_id: None,
        task_digest_hex: None,
    })
    .unwrap();
    let checker = CheckerResult::try_new(CheckerResultFields {
        status: CheckerOutputStatus::OutputLimitExceeded,
        stdout_sha256_hex: h(13),
        stderr_sha256_hex: sha256_hex(b""),
        stdout_bytes: 65_537,
        stderr_bytes: 0,
        parsed: Some(parsed),
    })
    .unwrap();
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let report = ExecutionReport::try_new(ExecutionReportFields {
        nonce_hex: h(1),
        operation_id_hex: h(2),
        request_digest_hex: execution_request_digest_hex(&request_frame).unwrap(),
        execution_policy_digest_hex: h(12),
        launcher_pid: 1234,
        launcher_uid: 0,
        launcher_gid: 0,
        node_uid: 1001,
        node_gid: 1001,
        checker_uid: 1002,
        checker_gid: 1002,
        authority_bindings: authority(),
        wait_status: WaitStatus::signaled(9, true),
        timed_out: false,
        resource_observations: resources(true),
        cleanup: cleanup(),
        checker_result: checker,
    })
    .unwrap();
    assert!(decode_complete_execution_report_frame(
        &encode_execution_report_frame(&report).unwrap()
    )
    .is_ok());
}

#[test]
fn execution_messages_cannot_be_reinterpreted_as_qualification_readiness() {
    let encoded = encode_execution_request_frame(&request()).unwrap();
    assert!(decode_complete_qualification_ready_frame(&encoded).is_err());
}

#[test]
fn optional_checker_identity_rejects_explicit_null() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let encoded = encode_execution_report_frame(&report(
        execution_request_digest_hex(&request_frame).unwrap(),
    ))
    .unwrap();
    let mut value = payload_value(&encoded);
    value["checkerResult"]["parsed"]["checkerTaskId"] = Value::Null;
    assert!(decode_complete_execution_report_frame(&frame(&value)).is_err());
}

#[test]
fn valid_checker_result_requires_nonempty_stdout() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let encoded = encode_execution_report_frame(&report(
        execution_request_digest_hex(&request_frame).unwrap(),
    ))
    .unwrap();
    let mut value = payload_value(&encoded);
    value["checkerResult"]["stdoutBytes"] = json!(0);
    assert!(decode_complete_execution_report_frame(&frame(&value)).is_err());
}

#[test]
fn retryable_checker_identity_is_present_as_a_complete_pair_or_omitted() {
    let parsed = CheckerParsedResult::try_new(CheckerParsedResultFields {
        verdict: CheckerVerdict::RetryableUnavailable,
        reason_code: CheckerReason::CheckerInternalError,
        checker_task_id: Some("fixture-task-1".to_string()),
        task_digest_hex: None,
    });
    assert!(parsed.is_err());
}

#[test]
fn execution_stream_helpers_round_trip_one_frame_then_clean_eof() {
    let request = request();
    let request_frame = encode_execution_request_frame(&request).unwrap();
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();
    let ready = ExecutionReady::try_new(&hello, execution_ready_fields()).unwrap();
    let report = report(execution_request_digest_hex(&request_frame).unwrap());

    let mut hello_wire = Vec::new();
    write_execution_hello(&mut hello_wire, &hello).unwrap();
    let mut hello_reader = Cursor::new(hello_wire);
    assert_eq!(
        read_execution_hello(&mut hello_reader).unwrap(),
        Some(hello)
    );
    assert_eq!(read_execution_hello(&mut hello_reader).unwrap(), None);

    let mut ready_wire = Vec::new();
    write_execution_ready(&mut ready_wire, &ready).unwrap();
    let mut ready_reader = Cursor::new(ready_wire);
    assert_eq!(
        read_execution_ready(&mut ready_reader).unwrap(),
        Some(ready)
    );
    assert_eq!(read_execution_ready(&mut ready_reader).unwrap(), None);

    let mut request_wire = Vec::new();
    write_execution_request(&mut request_wire, &request).unwrap();
    let mut request_reader = Cursor::new(request_wire);
    assert_eq!(
        read_execution_request(&mut request_reader).unwrap(),
        Some(request)
    );
    assert_eq!(read_execution_request(&mut request_reader).unwrap(), None);

    let mut report_wire = Vec::new();
    write_execution_report(&mut report_wire, &report).unwrap();
    let mut report_reader = Cursor::new(report_wire);
    assert_eq!(
        read_execution_report(&mut report_reader).unwrap(),
        Some(report)
    );
    assert_eq!(read_execution_report(&mut report_reader).unwrap(), None);
}

#[test]
fn validated_execution_values_expose_only_bound_session_fields() {
    let request = request();
    let request_frame = encode_execution_request_frame(&request).unwrap();
    let report = report(execution_request_digest_hex(&request_frame).unwrap());

    assert_eq!(request.nonce_hex(), h(1));
    assert_eq!(request.operation_id_hex(), h(2));
    assert_eq!(request.family_version(), "rust-tuple-struct-project-v1");
    assert_eq!(request.template_id(), h(3));
    assert_eq!(request.challenge_sha256(), h(4));
    assert_eq!(request.epoch(), 7);
    assert_eq!(request.raw_answer().unwrap(), RAW);
    assert_eq!(request.submission_source().unwrap(), SOURCE);
    assert_eq!(request.candidate_digest_hex(), sha256_hex(RAW));
    assert_eq!(request.registry_digest_hex(), h(5));
    assert_eq!(request.task_digest_hex(), h(7));
    assert_eq!(request.checker_artifact_hash_hex(), h(8));
    assert_eq!(request.checker_policy_digest_hex(), h(9));
    assert_eq!(request.toolchain_identity_digest_hex(), h(11));
    assert_eq!(request.execution_policy_digest_hex(), h(12));

    assert_eq!(report.nonce_hex(), request.nonce_hex());
    assert_eq!(report.operation_id_hex(), request.operation_id_hex());
    assert_eq!(report.execution_policy_digest_hex(), h(12));
    assert_eq!(report.checker_verdict(), Some(CheckerVerdict::Accepted));
    assert_eq!(report.checker_reason(), Some(CheckerReason::Accepted));
    assert!(report.cleanup_complete());
}

#[test]
fn complete_session_validation_binds_all_four_messages() {
    let request = request();
    let request_frame = encode_execution_request_frame(&request).unwrap();
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();
    let ready = ExecutionReady::try_new(&hello, execution_ready_fields()).unwrap();
    let execution_report = report(execution_request_digest_hex(&request_frame).unwrap());

    let validated =
        validate_execution_session(&hello, &ready, &request_frame, &execution_report).unwrap();
    assert_eq!(validated, request);

    let other_frame = {
        let mut value = payload_value(&request_frame);
        value["operationIdHex"] = json!(h(30));
        frame(&value)
    };
    assert!(validate_execution_session(&hello, &ready, &other_frame, &execution_report).is_err());

    let wrong_report = report(h(31));
    assert!(validate_execution_session(&hello, &ready, &request_frame, &wrong_report).is_err());
}
