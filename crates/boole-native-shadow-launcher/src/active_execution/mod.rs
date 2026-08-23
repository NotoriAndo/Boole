//! Closed-local active execution service core.

// This module is deliberately not wired into a production listener until the
// launcher owns an exact per-request Linux containment implementation and a
// non-forgeable runtime-rootfs replay proof.  Keeping the core unreachable is
// the compile-time fail-closed boundary; its in-module tests exercise the wire
// sequence without making an uncontained executor injectable by callers.
#![allow(dead_code)]

use std::io::{self, Write};

use boole_native_shadow_protocol::{
    decode_complete_execution_hello_frame, decode_complete_execution_request_frame,
    encode_active_execution_ready_frame, encode_execution_report_frame,
    validate_active_execution_session, ActiveExecutionReady, ActiveExecutionReadyFields,
    ExecutionHello, ExecutionReport, ExecutionRequest, VerifiedAuthorityBundle,
    VerifiedLocalExecutionAuthority, WireError, MAX_REQUEST_FRAME_BYTES,
};
#[cfg(test)]
use boole_native_shadow_protocol::{
    verify_authority_bundle, verify_local_execution_authority_bytes,
    TRACKED_EXECUTION_POLICY_BYTES, TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES,
    TRACKED_REGISTRY_BYTES, TRACKED_TOOLCHAIN_IDENTITY_BYTES,
};
use thiserror::Error;

#[cfg(target_os = "linux")]
mod unix;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NodePeerCredentials {
    pid: u32,
    uid: u32,
    gid: u32,
}

/// The service core owns one authenticated stream and never returns it after
/// any error.  Implementations are crate-private so an external caller cannot
/// substitute claimed peer credentials for `SO_PEERCRED`.
trait ActiveExecutionSession: Write {
    fn peer_credentials(&mut self) -> io::Result<NodePeerCredentials>;
    fn read_frame(&mut self, cap: usize) -> io::Result<Option<Vec<u8>>>;
    fn shutdown_write(&mut self) -> io::Result<()>;
}

/// Deliberately crate-private execution boundary.  A later Linux containment
/// module may implement it only after it owns the exact clone3/cgroup/tmpfs/
/// seccomp/Landlock lifecycle.  Tests use an in-module deterministic engine;
/// external code cannot inject an uncontained `Command` implementation.
trait ContainedCheckerExecutor {
    fn execute(
        &mut self,
        request: &ExecutionRequest,
        exact_request_frame: &[u8],
    ) -> Result<ExecutionReport, String>;
}

/// Temporary compile-time boundary while the exact fixed-case replay-grant
/// API is integrated. Production code has no constructor for this type, so it
/// cannot start a session from the disabled installed authority alone. The
/// test-only constructor exercises transport sequencing without creating a
/// public fake-grant seam.
struct ReplayGrantCapability {
    _private: (),
}

impl ReplayGrantCapability {
    #[cfg(test)]
    fn for_test() -> Self {
        Self { _private: () }
    }
}

struct ActiveExecutionContext {
    base_authority: VerifiedAuthorityBundle,
    local_authority: VerifiedLocalExecutionAuthority,
    launcher_pid: u32,
    node_uid: u32,
    node_gid: u32,
    checker_uid: u32,
    checker_gid: u32,
    launcher_instance_id_hex: String,
    runtime_rootfs_replay_verified: bool,
}

impl ActiveExecutionContext {
    #[cfg(test)]
    fn for_test(
        launcher_pid: u32,
        node_uid: u32,
        node_gid: u32,
        checker_uid: u32,
        checker_gid: u32,
    ) -> Self {
        Self {
            base_authority: verify_authority_bundle(
                TRACKED_REGISTRY_BYTES,
                TRACKED_EXECUTION_POLICY_BYTES,
                TRACKED_TOOLCHAIN_IDENTITY_BYTES,
            )
            .expect("tracked qualification authority verifies in tests"),
            local_authority: verify_local_execution_authority_bytes(
                TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES,
            )
            .expect("tracked local execution authority verifies in tests"),
            launcher_pid,
            node_uid,
            node_gid,
            checker_uid,
            checker_gid,
            launcher_instance_id_hex: "5a".repeat(32),
            runtime_rootfs_replay_verified: true,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
enum ActiveExecutionServerError {
    #[error("a consumed exact replay-grant authorization is required")]
    ReplayGrantRequired,
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
    #[error("exact contained checker execution failed: {0}")]
    ContainedExecution(String),
    #[error("failed to write or flush {stage}: {reason}")]
    ResponseIo { stage: &'static str, reason: String },
    #[error("failed to shut down launcher write half: {0}")]
    ShutdownWrite(String),
}

fn serve_active_execution_session<S, E>(
    mut session: S,
    context: &ActiveExecutionContext,
    executor: &mut E,
    replay_grant: Option<&ReplayGrantCapability>,
) -> Result<(), ActiveExecutionServerError>
where
    S: ActiveExecutionSession,
    E: ContainedCheckerExecutor,
{
    let replay_grant = replay_grant.ok_or(ActiveExecutionServerError::ReplayGrantRequired)?;
    serve_granted_active_execution_session(&mut session, context, executor, replay_grant)
}

/// The execution core itself has a grant capability in its type signature.
/// Once the shared replay-grant patch lands, `ReplayGrantCapability` is
/// replaced by its opaque request-bound authorization type rather than by a
/// boolean or caller-implementable trait.
fn serve_granted_active_execution_session<S, E>(
    session: &mut S,
    context: &ActiveExecutionContext,
    executor: &mut E,
    _replay_grant: &ReplayGrantCapability,
) -> Result<(), ActiveExecutionServerError>
where
    S: ActiveExecutionSession,
    E: ContainedCheckerExecutor,
{
    let peer = session.peer_credentials().map_err(|error| {
        ActiveExecutionServerError::PeerCredentialsUnavailable(error.to_string())
    })?;
    if peer.pid == 0 || peer.uid != context.node_uid || peer.gid != context.node_gid {
        return Err(ActiveExecutionServerError::UntrustedNodePeer);
    }

    let hello_frame = session
        .read_frame(MAX_REQUEST_FRAME_BYTES)
        .map_err(|error| ActiveExecutionServerError::FrameIo(error.to_string()))?
        .ok_or(ActiveExecutionServerError::PrematureEof("execution Hello"))?;
    let hello = decode_complete_execution_hello_frame(&hello_frame)?;
    require_binding(
        "executionPolicyDigestHex",
        context.base_authority.execution_policy_digest(),
        hello.execution_policy_digest_hex(),
    )?;

    let ready = ActiveExecutionReady::try_new(
        &hello,
        &context.local_authority,
        ActiveExecutionReadyFields {
            launcher_pid: context.launcher_pid,
            launcher_uid: 0,
            launcher_gid: 0,
            node_uid: context.node_uid,
            node_gid: context.node_gid,
            checker_uid: context.checker_uid,
            checker_gid: context.checker_gid,
            startup_recovery_complete: true,
            active_execution_leaves: 0,
            unexpected_direct_cgroup_children: 0,
            manager_subgroup_verified: true,
            launcher_instance_id_hex: context.launcher_instance_id_hex.clone(),
            runtime_rootfs_replay_verified: context.runtime_rootfs_replay_verified,
        },
    )?;
    write_response(
        session,
        &encode_active_execution_ready_frame(&ready)?,
        "active ready",
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

    let report = executor
        .execute(&request, &request_frame)
        .map_err(ActiveExecutionServerError::ContainedExecution)?;
    let _validated_request =
        validate_active_execution_session(&hello, &ready, &request_frame, &report)?;
    write_response(
        session,
        &encode_execution_report_frame(&report)?,
        "execution report",
    )?;
    session
        .shutdown_write()
        .map_err(|error| ActiveExecutionServerError::ShutdownWrite(error.to_string()))?;
    Ok(())
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
mod tests;
