use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::rc::Rc;

use boole_native_shadow_protocol::{
    decode_complete_active_execution_ready_frame, decode_complete_execution_report_frame,
    encode_execution_hello_frame, encode_execution_request_frame, execution_request_digest_hex,
    sha256_hex, submission_digest_hex, AuthorityBindings, AuthorityBindingsFields,
    CheckerOutputStatus, CheckerParsedResult, CheckerParsedResultFields, CheckerReason,
    CheckerResult, CheckerResultFields, CheckerVerdict, Cleanup, CleanupFields, ExecutionHello,
    ExecutionReport, ExecutionReportFields, ExecutionRequest, ExecutionRequestFields,
    ResourceObservations, ResourceObservationsFields, WaitStatus, TRACKED_EXECUTION_POLICY_BYTES,
};

use super::{
    serve_active_execution_session, ActiveExecutionContext, ActiveExecutionServerError,
    ActiveExecutionSession, ContainedCheckerExecutor, NodePeerCredentials, ReplayGrantCapability,
};

const NODE_UID: u32 = 20_001;
const NODE_GID: u32 = 20_001;
const CHECKER_UID: u32 = 20_002;
const CHECKER_GID: u32 = 20_002;
const LAUNCHER_PID: u32 = 4_242;
const NODE_PID: u32 = 8_484;

fn h(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

pub(super) fn request() -> ExecutionRequest {
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
        execution_policy_digest_hex: sha256_hex(TRACKED_EXECUTION_POLICY_BYTES),
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

pub(super) fn report(request_digest_hex: String) -> ExecutionReport {
    report_with_identities(
        request_digest_hex,
        NODE_UID,
        NODE_GID,
        CHECKER_UID,
        CHECKER_GID,
    )
}

pub(super) fn report_with_identities(
    request_digest_hex: String,
    node_uid: u32,
    node_gid: u32,
    checker_uid: u32,
    checker_gid: u32,
) -> ExecutionReport {
    let parsed = CheckerParsedResult::try_new(CheckerParsedResultFields {
        verdict: CheckerVerdict::Accepted,
        reason_code: CheckerReason::Accepted,
        checker_task_id: Some("fixture-task-1".to_string()),
        task_digest_hex: Some(h(7)),
    })
    .unwrap();
    ExecutionReport::try_new(ExecutionReportFields {
        nonce_hex: h(1),
        operation_id_hex: h(2),
        request_digest_hex,
        execution_policy_digest_hex: sha256_hex(TRACKED_EXECUTION_POLICY_BYTES),
        launcher_pid: LAUNCHER_PID,
        launcher_uid: 0,
        launcher_gid: 0,
        node_uid,
        node_gid,
        checker_uid,
        checker_gid,
        authority_bindings: authority(),
        wait_status: WaitStatus::exited(0),
        timed_out: false,
        resource_observations: ResourceObservations::try_new(ResourceObservationsFields {
            memory_events_low_delta: 0,
            memory_events_high_delta: 0,
            memory_events_max_delta: 0,
            memory_events_oom_delta: 0,
            memory_events_oom_kill_delta: 0,
            memory_events_oom_group_kill_delta: 0,
            pids_events_max_delta: 0,
            cpu_usage_usec_delta: 42,
            output_limit_exceeded: false,
        })
        .unwrap(),
        cleanup: Cleanup::try_new(CleanupFields {
            child_reaped: true,
            cgroup_populated_zero: true,
            launcher_pidfd_and_namespace_fds_closed: true,
            cgroup_leaf_removed: true,
            completed_within_deadline: true,
        })
        .unwrap(),
        checker_result: CheckerResult::try_new(CheckerResultFields {
            status: CheckerOutputStatus::ValidCheckerResult,
            stdout_sha256_hex: h(13),
            stderr_sha256_hex: sha256_hex(b""),
            stdout_bytes: 128,
            stderr_bytes: 0,
            parsed: Some(parsed),
        })
        .unwrap(),
    })
    .unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Event {
    PeerCredentials,
    ReadHello,
    WriteReady,
    FlushReady,
    ReadExecute,
    ReadNodeEof,
    ExecuteContained,
    WriteReport,
    FlushReport,
    ShutdownWrite,
}

#[derive(Default)]
struct Observation {
    events: Vec<Event>,
    output: Vec<u8>,
    read_calls: usize,
    write_calls: usize,
}

struct MockSession {
    input: VecDeque<Vec<u8>>,
    observation: Rc<RefCell<Observation>>,
}

impl Write for MockSession {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let mut observation = self.observation.borrow_mut();
        observation.write_calls += 1;
        let event = if observation.write_calls == 1 {
            Event::WriteReady
        } else {
            Event::WriteReport
        };
        observation.events.push(event);
        observation.output.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut observation = self.observation.borrow_mut();
        let event = if observation.write_calls == 1 {
            Event::FlushReady
        } else {
            Event::FlushReport
        };
        observation.events.push(event);
        Ok(())
    }
}

impl ActiveExecutionSession for MockSession {
    fn peer_credentials(&mut self) -> io::Result<NodePeerCredentials> {
        self.observation
            .borrow_mut()
            .events
            .push(Event::PeerCredentials);
        Ok(NodePeerCredentials {
            pid: NODE_PID,
            uid: NODE_UID,
            gid: NODE_GID,
        })
    }

    fn read_frame(&mut self, _cap: usize) -> io::Result<Option<Vec<u8>>> {
        let mut observation = self.observation.borrow_mut();
        observation.read_calls += 1;
        let event = match observation.read_calls {
            1 => Event::ReadHello,
            2 => Event::ReadExecute,
            _ => Event::ReadNodeEof,
        };
        observation.events.push(event);
        drop(observation);
        Ok(self.input.pop_front())
    }

    fn shutdown_write(&mut self) -> io::Result<()> {
        self.observation
            .borrow_mut()
            .events
            .push(Event::ShutdownWrite);
        Ok(())
    }
}

struct MockExecutor {
    observation: Rc<RefCell<Observation>>,
}

impl ContainedCheckerExecutor for MockExecutor {
    fn execute(
        &mut self,
        _request: &ExecutionRequest,
        exact_request_frame: &[u8],
    ) -> Result<ExecutionReport, String> {
        self.observation
            .borrow_mut()
            .events
            .push(Event::ExecuteContained);
        Ok(report(
            execution_request_digest_hex(exact_request_frame).unwrap(),
        ))
    }
}

#[test]
fn no_replay_grant_cannot_emit_ready_or_report_or_run_the_checker() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();
    let observation = Rc::new(RefCell::new(Observation::default()));
    let session = MockSession {
        input: VecDeque::from([encode_execution_hello_frame(&hello).unwrap(), request_frame]),
        observation: Rc::clone(&observation),
    };
    let mut executor = MockExecutor {
        observation: Rc::clone(&observation),
    };
    let context = ActiveExecutionContext::for_test(
        LAUNCHER_PID,
        NODE_UID,
        NODE_GID,
        CHECKER_UID,
        CHECKER_GID,
    );

    let error = serve_active_execution_session(session, &context, &mut executor, None)
        .expect_err("an absent replay grant must fail before any session I/O");

    assert_eq!(error, ActiveExecutionServerError::ReplayGrantRequired);
    let observation = observation.borrow();
    assert!(observation.events.is_empty());
    assert!(observation.output.is_empty());
}

#[test]
fn one_authenticated_session_runs_after_node_eof_and_returns_one_clean_report() {
    let request_frame = encode_execution_request_frame(&request()).unwrap();
    let hello = ExecutionHello::try_from_execution_request_frame(&request_frame).unwrap();
    let observation = Rc::new(RefCell::new(Observation::default()));
    let session = MockSession {
        input: VecDeque::from([encode_execution_hello_frame(&hello).unwrap(), request_frame]),
        observation: Rc::clone(&observation),
    };
    let mut executor = MockExecutor {
        observation: Rc::clone(&observation),
    };
    let context = ActiveExecutionContext::for_test(
        LAUNCHER_PID,
        NODE_UID,
        NODE_GID,
        CHECKER_UID,
        CHECKER_GID,
    );

    let replay_grant = ReplayGrantCapability::for_test();
    serve_active_execution_session(session, &context, &mut executor, Some(&replay_grant))
        .expect("one strict active execution session succeeds");

    let observation = observation.borrow();
    assert_eq!(
        observation.events,
        [
            Event::PeerCredentials,
            Event::ReadHello,
            Event::WriteReady,
            Event::FlushReady,
            Event::ReadExecute,
            Event::ReadNodeEof,
            Event::ExecuteContained,
            Event::WriteReport,
            Event::FlushReport,
            Event::ShutdownWrite,
        ]
    );

    let ready_len = 4 + u32::from_be_bytes(observation.output[..4].try_into().unwrap()) as usize;
    let ready = decode_complete_active_execution_ready_frame(&observation.output[..ready_len])
        .expect("first response is strict active ready v2");
    assert!(!ready.activation_allowed());
    let decoded_report = decode_complete_execution_report_frame(&observation.output[ready_len..])
        .expect("second response is one strict execution report");
    assert_eq!(
        serde_json::to_value(decoded_report).unwrap()["checkerResult"]["parsed"]["verdict"],
        "accepted"
    );
}
