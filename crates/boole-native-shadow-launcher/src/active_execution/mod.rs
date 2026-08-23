#![cfg(target_os = "linux")]

//! Closed-local replay execution service.
//!
//! This module has no caller-supplied executor seam. Every accepted Execute
//! frame must consume the request-bound startup permit and run the one fixed
//! checker through the Linux containment implementation.

use std::io::{self, Write};

use boole_native_shadow_protocol::{
    decode_complete_execution_hello_frame, decode_complete_execution_request_frame,
    decode_exact_checker_stdout_line, encode_closed_local_replay_execution_ready_frame,
    encode_execution_report_frame, execution_request_digest_hex,
    validate_closed_local_replay_execution_session, AuthorityBindings, AuthorityBindingsFields,
    CheckerOutputStatus, CheckerResult, CheckerResultFields, Cleanup, CleanupFields,
    ClosedLocalReplayExecutionReady, ClosedLocalReplayExecutionReadyFields, ExecutionHello,
    ExecutionReport, ExecutionReportFields, ResourceObservations, ResourceObservationsFields,
    WaitStatus, WireError, MAX_REQUEST_FRAME_BYTES,
};
use thiserror::Error;

use crate::closed_local_replay_startup::{
    ClosedLocalReplayStartupError, VerifiedClosedLocalReplayStartup,
};
#[cfg(feature = "manager-cgroup-linux-gate")]
use crate::per_request_containment::containment_diagnostic_mode_is_selected;
use crate::per_request_containment::{
    execute_fixed_checker, ContainedExecution, ContainmentFailure, TerminalWait,
};
use crate::qualification::listener::{
    bind_listener_in_directory, require_fixed_umask, FixedQualificationListenerError,
    FIXED_SOCKET_PATH,
};

#[cfg(target_os = "linux")]
mod unix;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NodePeerCredentials {
    pid: u32,
    uid: u32,
    gid: u32,
}

trait ActiveExecutionSession: Write {
    fn peer_credentials(&mut self) -> io::Result<NodePeerCredentials>;
    fn read_frame(&mut self, cap: usize) -> io::Result<Option<Vec<u8>>>;
    fn shutdown_write(&mut self) -> io::Result<()>;
}

#[derive(Debug, Error)]
pub(crate) enum ActiveExecutionServerError {
    #[error("node peer credentials are unavailable: {0}")]
    PeerCredentialsUnavailable(String),
    #[error("active execution peer does not match the fixed boole-node identity")]
    UntrustedNodePeer,
    #[error("node closed before sending {0}")]
    PrematureEof(&'static str),
    #[error("active execution frame I/O failed: {0}")]
    FrameIo(String),
    #[error(transparent)]
    Wire(#[from] WireError),
    #[error("active execution binding mismatch for {0}")]
    BindingMismatch(&'static str),
    #[error("node sent a second Execute frame or trailing data")]
    UnexpectedPostExecuteFrame,
    #[error("request-bound replay startup authorization failed: {0}")]
    Startup(#[from] ClosedLocalReplayStartupError),
    #[error("exact contained checker execution failed: {0}")]
    ContainedExecution(String),
    #[error("fatal containment cleanup poisoned the launcher: {0}")]
    FatalContainmentCleanup(String),
    #[error("failed to write or flush {stage}: {reason}")]
    ResponseIo { stage: &'static str, reason: String },
    #[error("failed to shut down launcher write half: {0}")]
    ShutdownWrite(String),
}

#[derive(Debug, Error)]
pub enum ActiveExecutionListenerError {
    #[error(transparent)]
    Listener(#[from] FixedQualificationListenerError),
    #[error("active execution connection {connection} failed: {reason}")]
    Session { connection: usize, reason: String },
}

/// Serve the exact three closed-local replay cases installed by the reviewed
/// authority bundle.  The listener is not opened until the complete startup
/// proof is owned, and no caller can supply an executor or checker command.
pub fn serve_three_fixed_unix_executions(
    startup: VerifiedClosedLocalReplayStartup,
) -> Result<(), ActiveExecutionListenerError> {
    serve_fixed_unix_executions(startup, 3)
}

/// CI-only one-request listener used to compare the exact accepted replay
/// across individual containment layers. It is absent from production builds.
#[cfg(feature = "manager-cgroup-linux-gate")]
#[doc(hidden)]
pub fn serve_one_diagnostic_unix_execution(
    startup: VerifiedClosedLocalReplayStartup,
) -> Result<(), ActiveExecutionListenerError> {
    serve_fixed_unix_executions(startup, 1)
}

fn serve_fixed_unix_executions(
    mut startup: VerifiedClosedLocalReplayStartup,
    connection_count: usize,
) -> Result<(), ActiveExecutionListenerError> {
    require_fixed_umask()?;
    let identities = startup.identities();
    let runtime_directory = startup.runtime_directory().try_clone().map_err(|source| {
        FixedQualificationListenerError::Io {
            stage: "duplicate verified runtime directory for active listener",
            source,
        }
    })?;
    let mut listener = bind_listener_in_directory(
        &runtime_directory,
        std::path::Path::new(FIXED_SOCKET_PATH),
        0,
        identities.node_gid(),
    )?;
    for connection in 1..=connection_count {
        let stream = listener.accept_one()?;
        unix::serve_connected_unix_execution(stream, &mut startup).map_err(|source| {
            ActiveExecutionListenerError::Session {
                connection,
                reason: source.to_string(),
            }
        })?;
    }
    listener.remove_exact_bound_entry()?;
    Ok(())
}

fn serve_active_execution_session<S: ActiveExecutionSession>(
    mut session: S,
    startup: &mut VerifiedClosedLocalReplayStartup,
) -> Result<(), ActiveExecutionServerError> {
    if startup.is_poisoned() {
        return Err(ActiveExecutionServerError::FatalContainmentCleanup(
            "launcher was already poisoned".to_string(),
        ));
    }
    let identities = startup.identities();
    let peer = session.peer_credentials().map_err(|error| {
        ActiveExecutionServerError::PeerCredentialsUnavailable(error.to_string())
    })?;
    // The frozen v1 trust boundary is one dedicated boole-node UID and its
    // primary GID behind a root:boole-node 2750 directory and 0660 socket.
    // Every process with that identity is intentionally inside the boundary;
    // no executable-path or parent-PID contract is invented here.
    if peer.pid == 0 || peer.uid != identities.node_uid() || peer.gid != identities.node_gid() {
        return Err(ActiveExecutionServerError::UntrustedNodePeer);
    }
    let hello_frame = session
        .read_frame(MAX_REQUEST_FRAME_BYTES)
        .map_err(|error| ActiveExecutionServerError::FrameIo(error.to_string()))?
        .ok_or(ActiveExecutionServerError::PrematureEof("execution Hello"))?;
    let hello = decode_complete_execution_hello_frame(&hello_frame)?;
    require_binding(
        "executionPolicyDigestHex",
        startup.execution_authority().base_execution_policy_sha256(),
        hello.execution_policy_digest_hex(),
    )?;

    let ready = ClosedLocalReplayExecutionReady::try_new(
        &hello,
        startup.execution_authority(),
        ClosedLocalReplayExecutionReadyFields {
            launcher_pid: std::process::id(),
            launcher_uid: 0,
            launcher_gid: 0,
            node_uid: identities.node_uid(),
            node_gid: identities.node_gid(),
            checker_uid: identities.checker_uid(),
            checker_gid: identities.checker_gid(),
            startup_recovery_complete: true,
            active_execution_leaves: 0,
            unexpected_direct_cgroup_children: 0,
            manager_subgroup_verified: true,
            launcher_instance_id_hex: startup.launcher_instance_id_hex(),
            installed_replay_authorities_verified: true,
            runtime_rootfs_replay_verified: true,
            production_activation_allowed: false,
        },
    )?;
    write_response(
        &mut session,
        &encode_closed_local_replay_execution_ready_frame(&ready)?,
        "closed-local replay ready",
    )?;

    let request_frame = session
        .read_frame(MAX_REQUEST_FRAME_BYTES)
        .map_err(|error| ActiveExecutionServerError::FrameIo(error.to_string()))?
        .ok_or(ActiveExecutionServerError::PrematureEof("Execute"))?;
    let request = decode_complete_execution_request_frame(&request_frame)?;
    let derived_hello = ExecutionHello::try_from_execution_request_frame(&request_frame)?;
    if derived_hello != hello {
        return Err(ActiveExecutionServerError::BindingMismatch(
            "Hello-to-Execute exact frame binding",
        ));
    }
    if session
        .read_frame(MAX_REQUEST_FRAME_BYTES)
        .map_err(|error| ActiveExecutionServerError::FrameIo(error.to_string()))?
        .is_some()
    {
        return Err(ActiveExecutionServerError::UnexpectedPostExecuteFrame);
    }

    let permit = startup.authorize_for_execution(&request)?;
    let execution = match execute_fixed_checker(permit) {
        Ok(execution) => execution,
        Err(ContainmentFailure::FatalCleanup(reason)) => {
            startup.poison();
            return Err(ActiveExecutionServerError::FatalContainmentCleanup(reason));
        }
        Err(error) => {
            return Err(ActiveExecutionServerError::ContainedExecution(
                error.to_string(),
            ));
        }
    };
    #[cfg(feature = "manager-cgroup-linux-gate")]
    if containment_diagnostic_mode_is_selected() {
        emit_operator_checker_diagnostic(&execution);
    }
    let report = build_execution_report(&request, &request_frame, execution, identities)?;
    let _validated =
        validate_closed_local_replay_execution_session(&hello, &ready, &request_frame, &report)?;
    write_response(
        &mut session,
        &encode_execution_report_frame(&report)?,
        "execution report",
    )?;
    session
        .shutdown_write()
        .map_err(|error| ActiveExecutionServerError::ShutdownWrite(error.to_string()))?;
    Ok(())
}

#[cfg(feature = "manager-cgroup-linux-gate")]
const OPERATOR_DIAGNOSTIC_PREFIX: &[u8] =
    b"boole-native-shadow-checker-cargo-diagnostic:v1;category=";

#[cfg(feature = "manager-cgroup-linux-gate")]
const OPERATOR_DIAGNOSTIC_CATEGORIES: &[&[u8]] = &[
    b"success",
    b"wall_limit",
    b"output_limit",
    b"authority_unavailable",
    b"rustc_version_permission_denied",
    b"rustc_version_failed",
    b"rustc_metadata_permission_denied",
    b"rustc_metadata_failed",
    b"rustc_link_permission_denied",
    b"rustc_linker_failed",
    b"rustc_link_failed",
    b"rustc_probe_permission_denied",
    b"rustc_probe_linker_failed",
    b"rustc_probe_failed",
    b"workspace_execute_denied",
    b"workspace_execute_failed",
    b"cargo_test_execute_denied",
    b"cargo_rustc_execute_denied",
    b"cargo_linker_permission_denied",
    b"cargo_temp_permission_denied",
    b"cargo_directory_permission_denied",
    b"permission_denied",
    b"read_only_filesystem",
    b"missing_file",
    b"cargo_lock_wait",
    b"process_spawn_failed",
    b"linker_failed",
    b"temporary_directory_failed",
    b"hidden_test_failed",
    b"compiler_error",
    b"unknown_nonzero",
];

#[cfg(feature = "manager-cgroup-linux-gate")]
fn validated_operator_checker_diagnostic_line(stderr: &[u8]) -> Option<&[u8]> {
    let line = stderr.strip_suffix(b"\n")?;
    if line.contains(&b'\n') || line.contains(&b'\r') || line.len() > 96 {
        return None;
    }
    let category = line.strip_prefix(OPERATOR_DIAGNOSTIC_PREFIX)?;
    OPERATOR_DIAGNOSTIC_CATEGORIES
        .contains(&category)
        .then_some(line)
}

#[cfg(feature = "manager-cgroup-linux-gate")]
fn emit_operator_checker_diagnostic(execution: &ContainedExecution) {
    if let Some(line) = validated_operator_checker_diagnostic_line(execution.stderr()) {
        eprintln!("{}", String::from_utf8_lossy(line));
    } else {
        eprintln!("boole-native-shadow-checker-cargo-diagnostic-invalid:v1");
    }
}

fn build_execution_report(
    request: &boole_native_shadow_protocol::ExecutionRequest,
    request_frame: &[u8],
    execution: ContainedExecution,
    identities: boole_native_shadow_protocol::ResolvedServiceIdentities,
) -> Result<ExecutionReport, WireError> {
    let wait_status = match execution.wait() {
        TerminalWait::Exited(code) => WaitStatus::exited(code),
        TerminalWait::Signaled {
            signal,
            core_dumped,
        } => WaitStatus::signaled(signal, core_dumped),
    };
    let resources = execution.resources();
    let resource_observations = ResourceObservations::try_new(ResourceObservationsFields {
        memory_events_low_delta: resources.memory_events_low,
        memory_events_high_delta: resources.memory_events_high,
        memory_events_max_delta: resources.memory_events_max,
        memory_events_oom_delta: resources.memory_events_oom,
        memory_events_oom_kill_delta: resources.memory_events_oom_kill,
        memory_events_oom_group_kill_delta: resources.memory_events_oom_group_kill,
        pids_events_max_delta: resources.pids_events_max,
        cpu_usage_usec_delta: resources.cpu_usage_usec,
        output_limit_exceeded: execution.output_overflow(),
    })?;
    let parsed = if !execution.output_overflow()
        && execution.stdout_bytes() != 0
        && execution.stderr_bytes() == 0
        && !execution.timed_out()
        && execution.wait() == TerminalWait::Exited(0)
    {
        decode_exact_checker_stdout_line(execution.stdout()).ok()
    } else {
        None
    };
    let status = if execution.output_overflow() {
        CheckerOutputStatus::OutputLimitExceeded
    } else if execution.stdout_bytes() == 0 {
        CheckerOutputStatus::NoCompleteOutput
    } else if parsed.is_some() {
        CheckerOutputStatus::ValidCheckerResult
    } else {
        CheckerOutputStatus::InvalidOrNonconformingOutput
    };
    let checker_result = CheckerResult::try_new(CheckerResultFields {
        status,
        stdout_sha256_hex: hex::encode(execution.stdout_sha256()),
        stderr_sha256_hex: hex::encode(execution.stderr_sha256()),
        stdout_bytes: execution.stdout_bytes(),
        stderr_bytes: execution.stderr_bytes(),
        parsed,
    })?;
    let authority_bindings = AuthorityBindings::try_new(AuthorityBindingsFields {
        registry_version: request.registry_version().to_string(),
        registry_digest_hex: request.registry_digest_hex().to_string(),
        anchor_digest_hex: request.anchor_digest_hex().to_string(),
        task_digest_hex: request.task_digest_hex().to_string(),
        checker_artifact_hash_hex: request.checker_artifact_hash_hex().to_string(),
        checker_policy_digest_hex: request.checker_policy_digest_hex().to_string(),
        checker_release_manifest_digest_hex: request
            .checker_release_manifest_digest_hex()
            .to_string(),
        toolchain_identity_digest_hex: request.toolchain_identity_digest_hex().to_string(),
    })?;
    let cleanup = Cleanup::try_new(CleanupFields {
        child_reaped: true,
        cgroup_populated_zero: true,
        launcher_pidfd_and_namespace_fds_closed: true,
        cgroup_leaf_removed: true,
        completed_within_deadline: true,
    })?;
    ExecutionReport::try_new(ExecutionReportFields {
        nonce_hex: request.nonce_hex().to_string(),
        operation_id_hex: request.operation_id_hex().to_string(),
        request_digest_hex: execution_request_digest_hex(request_frame)?,
        execution_policy_digest_hex: request.execution_policy_digest_hex().to_string(),
        launcher_pid: std::process::id(),
        launcher_uid: 0,
        launcher_gid: 0,
        node_uid: identities.node_uid(),
        node_gid: identities.node_gid(),
        checker_uid: identities.checker_uid(),
        checker_gid: identities.checker_gid(),
        authority_bindings,
        wait_status,
        timed_out: execution.timed_out(),
        resource_observations,
        cleanup,
        checker_result,
    })
}

fn write_response<S: ActiveExecutionSession>(
    session: &mut S,
    frame: &[u8],
    stage: &'static str,
) -> Result<(), ActiveExecutionServerError> {
    session
        .write_all(frame)
        .and_then(|()| session.flush())
        .map_err(|error| ActiveExecutionServerError::ResponseIo {
            stage,
            reason: error.to_string(),
        })
}

fn require_binding(
    field: &'static str,
    expected: &str,
    actual: &str,
) -> Result<(), ActiveExecutionServerError> {
    if expected == actual {
        Ok(())
    } else {
        Err(ActiveExecutionServerError::BindingMismatch(field))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_execution_report, serve_three_fixed_unix_executions, ActiveExecutionListenerError,
    };

    #[cfg(feature = "manager-cgroup-linux-gate")]
    use super::validated_operator_checker_diagnostic_line;

    #[test]
    fn report_builder_has_no_caller_supplied_executor_or_verdict_boolean() {
        let _builder = build_execution_report;
    }

    #[test]
    fn bounded_listener_consumes_one_complete_startup_and_has_no_executor_seam() {
        let _entrypoint: fn(
            crate::closed_local_replay_startup::VerifiedClosedLocalReplayStartup,
        ) -> Result<(), ActiveExecutionListenerError> = serve_three_fixed_unix_executions;
    }

    #[cfg(feature = "manager-cgroup-linux-gate")]
    #[test]
    fn operator_diagnostic_accepts_only_the_fixed_categorical_record() {
        let marker =
            b"boole-native-shadow-checker-cargo-diagnostic:v1;category=permission_denied\n";
        assert_eq!(
            validated_operator_checker_diagnostic_line(marker),
            Some(&marker[..marker.len() - 1])
        );

        assert_eq!(
            validated_operator_checker_diagnostic_line(
                b"boole-native-shadow-checker-cargo-diagnostic:v1;category=permission_denied\n/private/submission.rs\n"
            ),
            None
        );
        assert_eq!(
            validated_operator_checker_diagnostic_line(
                b"boole-native-shadow-checker-cargo-diagnostic:v1;category=raw_output\n"
            ),
            None
        );
    }
}
