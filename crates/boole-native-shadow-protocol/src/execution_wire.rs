//! Strict value and complete-frame codecs for the frozen native-shadow
//! execution wire. These types do not grant execution authority: the tracked
//! release remains qualification-only and rejects Execute frames before spawn.

use super::{
    complete_frame_payload, decode_strict_payload, encode_frame, read_frame_payload,
    require_wire_sha256, sha256_hex, write_frame, WireError, WireValidate, MAX_REQUEST_FRAME_BYTES,
    MAX_RESPONSE_FRAME_BYTES,
};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

const EXECUTION_REQUEST_SCHEMA: &str = "boole.native-shadow.launcher.execute.v1";
const EXECUTION_REPORT_SCHEMA: &str = "boole.native-shadow.launcher.report.v1";
const EXECUTION_HELLO_SCHEMA: &str = "boole.native-shadow.launcher.hello.v1";
const EXECUTION_READY_SCHEMA: &str = "boole.native-shadow.launcher.ready.v1";
const CHECKER_RESULT_SCHEMA: &str = "boole.native-shadow.checker-result.v1";
const REQUEST_DIGEST_DOMAIN: &[u8] = b"boole.native-shadow.launcher.request.v1\0";
const SUBMISSION_DIGEST_DOMAIN: &[u8] = b"boole.native-shadow.submission.v1\0";
const MAX_RAW_BYTES: usize = 16_384;

/// Node-owned commitment to one exact encoded Execute frame. The safe factory
/// derives its digest and payload length from validated frame bytes. A session
/// consumer must still compare a decoded inbound Hello with its own expected
/// request; decoding alone proves shape, not provenance.
///
/// ```compile_fail
/// let _: boole_native_shadow_protocol::ExecutionHello =
///     serde_json::from_slice(br#"{}"#).unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionHello {
    schema: String,
    nonce_hex: String,
    request_digest_hex: String,
    request_length_bytes: u32,
    execution_policy_digest_hex: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecutionHelloDto {
    schema: String,
    nonce_hex: String,
    request_digest_hex: String,
    request_length_bytes: u32,
    execution_policy_digest_hex: String,
}

/// Launcher readiness constructed from an `ExecutionHello`. It remains
/// disabled in this qualification release (`activationAllowed=false`). A
/// session consumer must compare decoded echo fields and identities with the
/// expected Hello and kernel-observed peer credentials.
///
/// ```compile_fail
/// let _: boole_native_shadow_protocol::ExecutionReady =
///     serde_json::from_slice(br#"{}"#).unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionReady {
    schema: String,
    nonce_hex: String,
    request_digest_hex: String,
    execution_policy_digest_hex: String,
    launcher_pid: u32,
    launcher_uid: u32,
    launcher_gid: u32,
    node_uid: u32,
    node_gid: u32,
    checker_uid: u32,
    checker_gid: u32,
    activation_allowed: bool,
    ready: bool,
}

/// Resolved process identities used by the launcher. Echoed hello fields and
/// readiness literals are deliberately absent so callers cannot forge them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReadyFields {
    pub launcher_pid: u32,
    pub launcher_uid: u32,
    pub launcher_gid: u32,
    pub node_uid: u32,
    pub node_gid: u32,
    pub checker_uid: u32,
    pub checker_gid: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecutionReadyDto {
    schema: String,
    nonce_hex: String,
    request_digest_hex: String,
    execution_policy_digest_hex: String,
    launcher_pid: u32,
    launcher_uid: u32,
    launcher_gid: u32,
    node_uid: u32,
    node_gid: u32,
    checker_uid: u32,
    checker_gid: u32,
    activation_allowed: bool,
    ready: bool,
}

/// Validated Execute message. Direct deserialization is intentionally absent.
///
/// ```compile_fail
/// let _: boole_native_shadow_protocol::ExecutionRequest =
///     serde_json::from_slice(br#"{}"#).unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRequest {
    schema: String,
    nonce_hex: String,
    operation_id_hex: String,
    family_version: String,
    template_id: String,
    challenge_sha256: String,
    epoch: u64,
    raw_answer_base64: String,
    submission_source_base64: String,
    submission_source_digest_hex: String,
    candidate_digest_hex: String,
    submission_digest_hex: String,
    registry_version: String,
    registry_digest_hex: String,
    anchor_digest_hex: String,
    task_digest_hex: String,
    checker_artifact_hash_hex: String,
    checker_policy_digest_hex: String,
    checker_release_manifest_digest_hex: String,
    toolchain_identity_digest_hex: String,
    execution_policy_digest_hex: String,
    intake_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionRequestFields {
    pub nonce_hex: String,
    pub operation_id_hex: String,
    pub family_version: String,
    pub template_id: String,
    pub challenge_sha256: String,
    pub epoch: u64,
    pub raw_answer_base64: String,
    pub submission_source_base64: String,
    pub submission_source_digest_hex: String,
    pub candidate_digest_hex: String,
    pub submission_digest_hex: String,
    pub registry_version: String,
    pub registry_digest_hex: String,
    pub anchor_digest_hex: String,
    pub task_digest_hex: String,
    pub checker_artifact_hash_hex: String,
    pub checker_policy_digest_hex: String,
    pub checker_release_manifest_digest_hex: String,
    pub toolchain_identity_digest_hex: String,
    pub execution_policy_digest_hex: String,
    pub intake_version: String,
}

/// Crate-private read-only view used by the exact replay-grant matcher. It is
/// not a wire type and cannot be constructed by launcher callers.
#[cfg(any(target_os = "linux", test))]
pub(crate) struct ReplayRequestAuthority<'a> {
    pub operation_id_hex: &'a str,
    pub family_version: &'a str,
    pub template_id: &'a str,
    pub challenge_sha256: &'a str,
    pub epoch: u64,
    pub candidate_digest_hex: &'a str,
    pub submission_source_digest_hex: &'a str,
    pub registry_version: &'a str,
    pub registry_digest_hex: &'a str,
    pub anchor_digest_hex: &'a str,
    pub task_digest_hex: &'a str,
    pub checker_artifact_hash_hex: &'a str,
    pub checker_policy_digest_hex: &'a str,
    pub checker_release_manifest_digest_hex: &'a str,
    pub toolchain_identity_digest_hex: &'a str,
    pub execution_policy_digest_hex: &'a str,
    pub intake_version: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecutionRequestDto {
    schema: String,
    nonce_hex: String,
    operation_id_hex: String,
    family_version: String,
    template_id: String,
    challenge_sha256: String,
    epoch: u64,
    raw_answer_base64: String,
    submission_source_base64: String,
    submission_source_digest_hex: String,
    candidate_digest_hex: String,
    submission_digest_hex: String,
    registry_version: String,
    registry_digest_hex: String,
    anchor_digest_hex: String,
    task_digest_hex: String,
    checker_artifact_hash_hex: String,
    checker_policy_digest_hex: String,
    checker_release_manifest_digest_hex: String,
    toolchain_identity_digest_hex: String,
    execution_policy_digest_hex: String,
    intake_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorityBindings {
    registry_version: String,
    registry_digest_hex: String,
    anchor_digest_hex: String,
    task_digest_hex: String,
    checker_artifact_hash_hex: String,
    checker_policy_digest_hex: String,
    checker_release_manifest_digest_hex: String,
    toolchain_identity_digest_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityBindingsFields {
    pub registry_version: String,
    pub registry_digest_hex: String,
    pub anchor_digest_hex: String,
    pub task_digest_hex: String,
    pub checker_artifact_hash_hex: String,
    pub checker_policy_digest_hex: String,
    pub checker_release_manifest_digest_hex: String,
    pub toolchain_identity_digest_hex: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthorityBindingsDto {
    registry_version: String,
    registry_digest_hex: String,
    anchor_digest_hex: String,
    task_digest_hex: String,
    checker_artifact_hash_hex: String,
    checker_policy_digest_hex: String,
    checker_release_manifest_digest_hex: String,
    toolchain_identity_digest_hex: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum WaitKind {
    Exited,
    Signaled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitStatus {
    kind: WaitKind,
    exit_code: Option<u8>,
    term_signal: Option<u8>,
    core_dumped: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WaitStatusDto {
    kind: String,
    exit_code: RequiredNullable<u8>,
    term_signal: RequiredNullable<u8>,
    core_dumped: bool,
}

#[derive(Debug, Deserialize)]
#[serde(transparent)]
struct RequiredNullable<T>(Option<T>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceObservations {
    memory_events_low_delta: u64,
    memory_events_high_delta: u64,
    memory_events_max_delta: u64,
    memory_events_oom_delta: u64,
    memory_events_oom_kill_delta: u64,
    memory_events_oom_group_kill_delta: u64,
    pids_events_max_delta: u64,
    cpu_usage_usec_delta: u64,
    output_limit_exceeded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceObservationsFields {
    pub memory_events_low_delta: u64,
    pub memory_events_high_delta: u64,
    pub memory_events_max_delta: u64,
    pub memory_events_oom_delta: u64,
    pub memory_events_oom_kill_delta: u64,
    pub memory_events_oom_group_kill_delta: u64,
    pub pids_events_max_delta: u64,
    pub cpu_usage_usec_delta: u64,
    pub output_limit_exceeded: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceObservationsDto {
    memory_events_low_delta: u64,
    memory_events_high_delta: u64,
    memory_events_max_delta: u64,
    memory_events_oom_delta: u64,
    memory_events_oom_kill_delta: u64,
    memory_events_oom_group_kill_delta: u64,
    pids_events_max_delta: u64,
    cpu_usage_usec_delta: u64,
    output_limit_exceeded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cleanup {
    child_reaped: bool,
    cgroup_populated_zero: bool,
    launcher_pidfd_and_namespace_fds_closed: bool,
    cgroup_leaf_removed: bool,
    completed_within_deadline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupFields {
    pub child_reaped: bool,
    pub cgroup_populated_zero: bool,
    pub launcher_pidfd_and_namespace_fds_closed: bool,
    pub cgroup_leaf_removed: bool,
    pub completed_within_deadline: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CleanupDto {
    child_reaped: bool,
    cgroup_populated_zero: bool,
    launcher_pidfd_and_namespace_fds_closed: bool,
    cgroup_leaf_removed: bool,
    completed_within_deadline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckerOutputStatus {
    ValidCheckerResult,
    InvalidOrNonconformingOutput,
    OutputLimitExceeded,
    NoCompleteOutput,
}

impl CheckerOutputStatus {
    fn parse(value: &str) -> Result<Self, WireError> {
        match value {
            "valid-checker-result" => Ok(Self::ValidCheckerResult),
            "invalid-or-nonconforming-output" => Ok(Self::InvalidOrNonconformingOutput),
            "output-limit-exceeded" => Ok(Self::OutputLimitExceeded),
            "no-complete-output" => Ok(Self::NoCompleteOutput),
            _ => Err(contract(
                "checkerResult.status is outside the frozen vocabulary",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckerVerdict {
    Accepted,
    DeterministicReject,
    RetryableUnavailable,
}

impl CheckerVerdict {
    fn parse(value: &str) -> Result<Self, WireError> {
        match value {
            "accepted" => Ok(Self::Accepted),
            "deterministic_reject" => Ok(Self::DeterministicReject),
            "retryable_unavailable" => Ok(Self::RetryableUnavailable),
            _ => Err(contract("checker verdict is outside the frozen vocabulary")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckerReason {
    Accepted,
    CompileOrHiddenTestFailed,
    ForbiddenConstruct,
    MalformedPatchRegion,
    OutsidePatchModified,
    PatchLineLimitExceeded,
    PatchSizeExceeded,
    SubmissionUnreadable,
    AnchorDigestMismatch,
    AnchorSizeExceeded,
    AnchorUnavailable,
    CheckerInternalError,
    ContainedProcessUnavailable,
    PolicyContractMismatch,
    PolicyUnavailable,
    ResourceMemoryLimit,
    ResourceOutputLimit,
    ResourceProcessLimit,
    ResourceProcessTerminated,
    ResourceWallLimit,
    ScratchRootRequired,
    ScratchRootUnavailable,
    ScratchWorkspaceUnavailable,
    TaskBindingMismatch,
    TaskContractInvalid,
    ToolchainIdentityMismatch,
    ToolchainProbeFailed,
    ToolchainUnavailable,
}

impl CheckerReason {
    fn parse(value: &str) -> Result<Self, WireError> {
        match value {
            "accepted" => Ok(Self::Accepted),
            "compile_or_hidden_test_failed" => Ok(Self::CompileOrHiddenTestFailed),
            "forbidden_construct" => Ok(Self::ForbiddenConstruct),
            "malformed_patch_region" => Ok(Self::MalformedPatchRegion),
            "outside_patch_modified" => Ok(Self::OutsidePatchModified),
            "patch_line_limit_exceeded" => Ok(Self::PatchLineLimitExceeded),
            "patch_size_exceeded" => Ok(Self::PatchSizeExceeded),
            "submission_unreadable" => Ok(Self::SubmissionUnreadable),
            "anchor_digest_mismatch" => Ok(Self::AnchorDigestMismatch),
            "anchor_size_exceeded" => Ok(Self::AnchorSizeExceeded),
            "anchor_unavailable" => Ok(Self::AnchorUnavailable),
            "checker_internal_error" => Ok(Self::CheckerInternalError),
            "contained_process_unavailable" => Ok(Self::ContainedProcessUnavailable),
            "policy_contract_mismatch" => Ok(Self::PolicyContractMismatch),
            "policy_unavailable" => Ok(Self::PolicyUnavailable),
            "resource_memory_limit" => Ok(Self::ResourceMemoryLimit),
            "resource_output_limit" => Ok(Self::ResourceOutputLimit),
            "resource_process_limit" => Ok(Self::ResourceProcessLimit),
            "resource_process_terminated" => Ok(Self::ResourceProcessTerminated),
            "resource_wall_limit" => Ok(Self::ResourceWallLimit),
            "scratch_root_required" => Ok(Self::ScratchRootRequired),
            "scratch_root_unavailable" => Ok(Self::ScratchRootUnavailable),
            "scratch_workspace_unavailable" => Ok(Self::ScratchWorkspaceUnavailable),
            "task_binding_mismatch" => Ok(Self::TaskBindingMismatch),
            "task_contract_invalid" => Ok(Self::TaskContractInvalid),
            "toolchain_identity_mismatch" => Ok(Self::ToolchainIdentityMismatch),
            "toolchain_probe_failed" => Ok(Self::ToolchainProbeFailed),
            "toolchain_unavailable" => Ok(Self::ToolchainUnavailable),
            _ => Err(contract(
                "checker reasonCode is outside the frozen vocabulary",
            )),
        }
    }

    fn belongs_to(self, verdict: CheckerVerdict) -> bool {
        match verdict {
            CheckerVerdict::Accepted => self == Self::Accepted,
            CheckerVerdict::DeterministicReject => matches!(
                self,
                Self::CompileOrHiddenTestFailed
                    | Self::ForbiddenConstruct
                    | Self::MalformedPatchRegion
                    | Self::OutsidePatchModified
                    | Self::PatchLineLimitExceeded
                    | Self::PatchSizeExceeded
                    | Self::SubmissionUnreadable
            ),
            CheckerVerdict::RetryableUnavailable => matches!(
                self,
                Self::AnchorDigestMismatch
                    | Self::AnchorSizeExceeded
                    | Self::AnchorUnavailable
                    | Self::CheckerInternalError
                    | Self::ContainedProcessUnavailable
                    | Self::PolicyContractMismatch
                    | Self::PolicyUnavailable
                    | Self::ResourceMemoryLimit
                    | Self::ResourceOutputLimit
                    | Self::ResourceProcessLimit
                    | Self::ResourceProcessTerminated
                    | Self::ResourceWallLimit
                    | Self::ScratchRootRequired
                    | Self::ScratchRootUnavailable
                    | Self::ScratchWorkspaceUnavailable
                    | Self::TaskBindingMismatch
                    | Self::TaskContractInvalid
                    | Self::ToolchainIdentityMismatch
                    | Self::ToolchainProbeFailed
                    | Self::ToolchainUnavailable
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckerParsedResult {
    schema: String,
    verdict: CheckerVerdict,
    reason_code: CheckerReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    checker_task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckerParsedResultFields {
    pub verdict: CheckerVerdict,
    pub reason_code: CheckerReason,
    pub checker_task_id: Option<String>,
    pub task_digest_hex: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckerParsedResultDto {
    schema: String,
    verdict: String,
    reason_code: String,
    #[serde(default, deserialize_with = "optional_non_null")]
    checker_task_id: Option<String>,
    #[serde(default, deserialize_with = "optional_non_null")]
    task_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckerResult {
    status: CheckerOutputStatus,
    stdout_sha256_hex: String,
    stderr_sha256_hex: String,
    stdout_bytes: u64,
    stderr_bytes: u64,
    parsed: Option<CheckerParsedResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckerResultFields {
    pub status: CheckerOutputStatus,
    pub stdout_sha256_hex: String,
    pub stderr_sha256_hex: String,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    pub parsed: Option<CheckerParsedResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckerResultDto {
    status: String,
    stdout_sha256_hex: String,
    stderr_sha256_hex: String,
    stdout_bytes: u64,
    stderr_bytes: u64,
    parsed: RequiredNullable<CheckerParsedResultDto>,
}

/// Validated Report message. Cross-message/session bindings still belong to
/// the node-owned session verifier and are not claimed by this value alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionReport {
    schema: String,
    nonce_hex: String,
    operation_id_hex: String,
    request_digest_hex: String,
    execution_policy_digest_hex: String,
    launcher_pid: u32,
    launcher_uid: u32,
    launcher_gid: u32,
    node_uid: u32,
    node_gid: u32,
    checker_uid: u32,
    checker_gid: u32,
    authority_bindings: AuthorityBindings,
    wait_status: WaitStatus,
    timed_out: bool,
    resource_observations: ResourceObservations,
    cleanup: Cleanup,
    checker_result: CheckerResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReportFields {
    pub nonce_hex: String,
    pub operation_id_hex: String,
    pub request_digest_hex: String,
    pub execution_policy_digest_hex: String,
    pub launcher_pid: u32,
    pub launcher_uid: u32,
    pub launcher_gid: u32,
    pub node_uid: u32,
    pub node_gid: u32,
    pub checker_uid: u32,
    pub checker_gid: u32,
    pub authority_bindings: AuthorityBindings,
    pub wait_status: WaitStatus,
    pub timed_out: bool,
    pub resource_observations: ResourceObservations,
    pub cleanup: Cleanup,
    pub checker_result: CheckerResult,
}

/// Read-only node adjudication facts from a fully decoded report. Keeping
/// this projection typed prevents the node from re-parsing launcher JSON or
/// trusting free-form checker text while it recomputes the frozen outcome
/// policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionAdjudicationView {
    pub timed_out: bool,
    pub signaled: bool,
    pub memory_events_max_delta: u64,
    pub pids_events_max_delta: u64,
    pub output_limit_exceeded: bool,
    pub checker_status: CheckerOutputStatus,
    pub checker_verdict: Option<CheckerVerdict>,
    pub checker_reason: Option<CheckerReason>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExecutionReportDto {
    schema: String,
    nonce_hex: String,
    operation_id_hex: String,
    request_digest_hex: String,
    execution_policy_digest_hex: String,
    launcher_pid: u32,
    launcher_uid: u32,
    launcher_gid: u32,
    node_uid: u32,
    node_gid: u32,
    checker_uid: u32,
    checker_gid: u32,
    authority_bindings: AuthorityBindingsDto,
    wait_status: WaitStatusDto,
    timed_out: bool,
    resource_observations: ResourceObservationsDto,
    cleanup: CleanupDto,
    checker_result: CheckerResultDto,
}

pub fn encode_execution_hello_frame(value: &ExecutionHello) -> Result<Vec<u8>, WireError> {
    encode_frame(value, MAX_REQUEST_FRAME_BYTES)
}

pub fn decode_complete_execution_hello_frame(frame: &[u8]) -> Result<ExecutionHello, WireError> {
    let payload = complete_frame_payload(frame, MAX_REQUEST_FRAME_BYTES)?;
    let dto: ExecutionHelloDto = decode_strict_payload(payload, MAX_REQUEST_FRAME_BYTES)?;
    ExecutionHello::try_from(dto)
}

pub fn encode_execution_ready_frame(value: &ExecutionReady) -> Result<Vec<u8>, WireError> {
    encode_frame(value, MAX_RESPONSE_FRAME_BYTES)
}

pub fn decode_complete_execution_ready_frame(frame: &[u8]) -> Result<ExecutionReady, WireError> {
    let payload = complete_frame_payload(frame, MAX_RESPONSE_FRAME_BYTES)?;
    let dto: ExecutionReadyDto = decode_strict_payload(payload, MAX_RESPONSE_FRAME_BYTES)?;
    ExecutionReady::try_from(dto)
}

pub fn encode_execution_request_frame(value: &ExecutionRequest) -> Result<Vec<u8>, WireError> {
    encode_frame(value, MAX_REQUEST_FRAME_BYTES)
}

pub fn decode_complete_execution_request_frame(
    frame: &[u8],
) -> Result<ExecutionRequest, WireError> {
    let payload = complete_frame_payload(frame, MAX_REQUEST_FRAME_BYTES)?;
    let dto: ExecutionRequestDto = decode_strict_payload(payload, MAX_REQUEST_FRAME_BYTES)?;
    ExecutionRequest::try_from(dto)
}

pub fn encode_execution_report_frame(value: &ExecutionReport) -> Result<Vec<u8>, WireError> {
    encode_frame(value, MAX_RESPONSE_FRAME_BYTES)
}

pub fn decode_complete_execution_report_frame(frame: &[u8]) -> Result<ExecutionReport, WireError> {
    let payload = complete_frame_payload(frame, MAX_RESPONSE_FRAME_BYTES)?;
    let dto: ExecutionReportDto = decode_strict_payload(payload, MAX_RESPONSE_FRAME_BYTES)?;
    ExecutionReport::try_from(dto)
}

/// Parse the checker's complete stdout contract: exactly one strict JSON
/// object followed by one LF byte and no other line break or surrounding
/// whitespace. This is intentionally narrower than generic JSON parsing so a
/// partial or multi-line diagnostic can never be mistaken for a verdict.
pub fn decode_exact_checker_stdout_line(stdout: &[u8]) -> Result<CheckerParsedResult, WireError> {
    let Some(payload) = stdout.strip_suffix(b"\n") else {
        return Err(contract("checker stdout must end with exactly one LF byte"));
    };
    if payload.first() != Some(&b'{')
        || payload.last() != Some(&b'}')
        || payload.contains(&b'\n')
        || payload.contains(&b'\r')
    {
        return Err(contract(
            "checker stdout must contain exactly one JSON object line",
        ));
    }
    let dto: CheckerParsedResultDto = decode_strict_payload(payload, MAX_RESPONSE_FRAME_BYTES)?;
    CheckerParsedResult::try_from(dto)
}

pub fn write_execution_hello<W: Write>(
    writer: &mut W,
    value: &ExecutionHello,
) -> Result<(), WireError> {
    write_frame(writer, value, MAX_REQUEST_FRAME_BYTES)
}

pub fn read_execution_hello<R: Read>(reader: &mut R) -> Result<Option<ExecutionHello>, WireError> {
    read_frame_payload(reader, MAX_REQUEST_FRAME_BYTES)?
        .map(|payload| {
            let dto: ExecutionHelloDto = decode_strict_payload(&payload, MAX_REQUEST_FRAME_BYTES)?;
            ExecutionHello::try_from(dto)
        })
        .transpose()
}

pub fn write_execution_ready<W: Write>(
    writer: &mut W,
    value: &ExecutionReady,
) -> Result<(), WireError> {
    write_frame(writer, value, MAX_RESPONSE_FRAME_BYTES)
}

pub fn read_execution_ready<R: Read>(reader: &mut R) -> Result<Option<ExecutionReady>, WireError> {
    read_frame_payload(reader, MAX_RESPONSE_FRAME_BYTES)?
        .map(|payload| {
            let dto: ExecutionReadyDto = decode_strict_payload(&payload, MAX_RESPONSE_FRAME_BYTES)?;
            ExecutionReady::try_from(dto)
        })
        .transpose()
}

pub fn write_execution_request<W: Write>(
    writer: &mut W,
    value: &ExecutionRequest,
) -> Result<(), WireError> {
    write_frame(writer, value, MAX_REQUEST_FRAME_BYTES)
}

pub fn read_execution_request<R: Read>(
    reader: &mut R,
) -> Result<Option<ExecutionRequest>, WireError> {
    read_frame_payload(reader, MAX_REQUEST_FRAME_BYTES)?
        .map(|payload| {
            let dto: ExecutionRequestDto =
                decode_strict_payload(&payload, MAX_REQUEST_FRAME_BYTES)?;
            ExecutionRequest::try_from(dto)
        })
        .transpose()
}

pub fn write_execution_report<W: Write>(
    writer: &mut W,
    value: &ExecutionReport,
) -> Result<(), WireError> {
    write_frame(writer, value, MAX_RESPONSE_FRAME_BYTES)
}

pub fn read_execution_report<R: Read>(
    reader: &mut R,
) -> Result<Option<ExecutionReport>, WireError> {
    read_frame_payload(reader, MAX_RESPONSE_FRAME_BYTES)?
        .map(|payload| {
            let dto: ExecutionReportDto =
                decode_strict_payload(&payload, MAX_RESPONSE_FRAME_BYTES)?;
            ExecutionReport::try_from(dto)
        })
        .transpose()
}

/// Validate the binding across one complete hello/ready/execute/report
/// exchange. The exact encoded execute frame is authoritative: its digest and
/// payload length are never reconstructed from a decoded value.
pub fn validate_execution_session(
    hello: &ExecutionHello,
    ready: &ExecutionReady,
    request_frame: &[u8],
    report: &ExecutionReport,
) -> Result<ExecutionRequest, WireError> {
    let request = decode_complete_execution_request_frame(request_frame)?;
    let derived_hello = ExecutionHello::try_from_execution_request_frame(request_frame)?;
    if hello != &derived_hello {
        return Err(contract("hello does not bind the exact execute frame"));
    }
    if ready.nonce_hex != hello.nonce_hex
        || ready.request_digest_hex != hello.request_digest_hex
        || ready.execution_policy_digest_hex != hello.execution_policy_digest_hex
    {
        return Err(contract("ready does not echo the exact hello bindings"));
    }
    if request.nonce_hex != hello.nonce_hex
        || request.execution_policy_digest_hex != hello.execution_policy_digest_hex
    {
        return Err(contract("execute does not match the hello bindings"));
    }
    if report.nonce_hex != hello.nonce_hex
        || report.operation_id_hex != request.operation_id_hex
        || report.request_digest_hex != hello.request_digest_hex
        || report.execution_policy_digest_hex != hello.execution_policy_digest_hex
    {
        return Err(contract("report does not match the execution session"));
    }
    if (
        report.launcher_pid,
        report.launcher_uid,
        report.launcher_gid,
        report.node_uid,
        report.node_gid,
        report.checker_uid,
        report.checker_gid,
    ) != (
        ready.launcher_pid,
        ready.launcher_uid,
        ready.launcher_gid,
        ready.node_uid,
        ready.node_gid,
        ready.checker_uid,
        ready.checker_gid,
    ) {
        return Err(contract("report service identities differ from ready"));
    }
    let authority = &report.authority_bindings;
    if authority.registry_version != request.registry_version
        || authority.registry_digest_hex != request.registry_digest_hex
        || authority.anchor_digest_hex != request.anchor_digest_hex
        || authority.task_digest_hex != request.task_digest_hex
        || authority.checker_artifact_hash_hex != request.checker_artifact_hash_hex
        || authority.checker_policy_digest_hex != request.checker_policy_digest_hex
        || authority.checker_release_manifest_digest_hex
            != request.checker_release_manifest_digest_hex
        || authority.toolchain_identity_digest_hex != request.toolchain_identity_digest_hex
    {
        return Err(contract(
            "report authority bindings differ from the execute request",
        ));
    }
    Ok(request)
}

/// Hash the exact received length prefix and JSON payload. Re-serializing a
/// decoded object here would lose meaningful byte identity such as whitespace.
pub fn execution_request_digest_hex(frame: &[u8]) -> Result<String, WireError> {
    let _ = decode_complete_execution_request_frame(frame)?;
    let mut digest = Sha256::new();
    digest.update(REQUEST_DIGEST_DOMAIN);
    digest.update(frame);
    Ok(hex::encode(digest.finalize()))
}

pub fn submission_digest_hex(
    family_version: &str,
    template_id: &str,
    challenge_sha256: &str,
    epoch: u64,
    raw_answer: &[u8],
) -> Result<String, WireError> {
    require_text("familyVersion", family_version, 256)?;
    require_wire_sha256("templateId", template_id)?;
    require_wire_sha256("challengeSha256", challenge_sha256)?;
    require_utf8("rawAnswer", raw_answer, MAX_RAW_BYTES, false)?;

    let mut digest = Sha256::new();
    digest.update(SUBMISSION_DIGEST_DOMAIN);
    digest.update(length_prefix(family_version.as_bytes())?);
    digest.update(family_version.as_bytes());
    digest.update(length_prefix(template_id.as_bytes())?);
    digest.update(template_id.as_bytes());
    digest.update(length_prefix(challenge_sha256.as_bytes())?);
    digest.update(challenge_sha256.as_bytes());
    digest.update(epoch.to_be_bytes());
    digest.update(length_prefix(raw_answer)?);
    digest.update(raw_answer);
    Ok(hex::encode(digest.finalize()))
}

impl ExecutionHello {
    pub fn try_from_execution_request_frame(frame: &[u8]) -> Result<Self, WireError> {
        let request = decode_complete_execution_request_frame(frame)?;
        let payload_length = frame
            .len()
            .checked_sub(4)
            .and_then(|length| u32::try_from(length).ok())
            .ok_or_else(|| contract("execution request payload length is invalid"))?;
        let mut digest = Sha256::new();
        digest.update(REQUEST_DIGEST_DOMAIN);
        digest.update(frame);
        Self {
            schema: EXECUTION_HELLO_SCHEMA.to_string(),
            nonce_hex: request.nonce_hex,
            request_digest_hex: hex::encode(digest.finalize()),
            request_length_bytes: payload_length,
            execution_policy_digest_hex: request.execution_policy_digest_hex,
        }
        .validated()
    }

    fn validated(self) -> Result<Self, WireError> {
        self.validate_wire()?;
        Ok(self)
    }

    pub fn nonce_hex(&self) -> &str {
        &self.nonce_hex
    }

    pub fn request_digest_hex(&self) -> &str {
        &self.request_digest_hex
    }

    pub fn request_length_bytes(&self) -> u32 {
        self.request_length_bytes
    }

    pub fn execution_policy_digest_hex(&self) -> &str {
        &self.execution_policy_digest_hex
    }
}

impl TryFrom<ExecutionHelloDto> for ExecutionHello {
    type Error = WireError;

    fn try_from(dto: ExecutionHelloDto) -> Result<Self, Self::Error> {
        Self {
            schema: dto.schema,
            nonce_hex: dto.nonce_hex,
            request_digest_hex: dto.request_digest_hex,
            request_length_bytes: dto.request_length_bytes,
            execution_policy_digest_hex: dto.execution_policy_digest_hex,
        }
        .validated()
    }
}

impl WireValidate for ExecutionHello {
    fn validate_wire(&self) -> Result<(), WireError> {
        if self.schema != EXECUTION_HELLO_SCHEMA {
            return Err(contract("execution hello schema literal mismatch"));
        }
        for (name, value) in [
            ("nonceHex", self.nonce_hex.as_str()),
            ("requestDigestHex", self.request_digest_hex.as_str()),
            (
                "executionPolicyDigestHex",
                self.execution_policy_digest_hex.as_str(),
            ),
        ] {
            require_wire_sha256(name, value)?;
        }
        if self.request_length_bytes == 0
            || self.request_length_bytes as usize > MAX_REQUEST_FRAME_BYTES
        {
            return Err(contract(
                "requestLengthBytes must be within the request payload cap",
            ));
        }
        Ok(())
    }
}

impl ExecutionReady {
    pub fn try_new(
        hello: &ExecutionHello,
        fields: ExecutionReadyFields,
    ) -> Result<Self, WireError> {
        hello.validate_wire()?;
        Self {
            schema: EXECUTION_READY_SCHEMA.to_string(),
            nonce_hex: hello.nonce_hex.clone(),
            request_digest_hex: hello.request_digest_hex.clone(),
            execution_policy_digest_hex: hello.execution_policy_digest_hex.clone(),
            launcher_pid: fields.launcher_pid,
            launcher_uid: fields.launcher_uid,
            launcher_gid: fields.launcher_gid,
            node_uid: fields.node_uid,
            node_gid: fields.node_gid,
            checker_uid: fields.checker_uid,
            checker_gid: fields.checker_gid,
            activation_allowed: false,
            ready: true,
        }
        .validated()
    }

    fn validated(self) -> Result<Self, WireError> {
        self.validate_wire()?;
        Ok(self)
    }

    pub fn nonce_hex(&self) -> &str {
        &self.nonce_hex
    }

    pub fn request_digest_hex(&self) -> &str {
        &self.request_digest_hex
    }

    pub fn execution_policy_digest_hex(&self) -> &str {
        &self.execution_policy_digest_hex
    }

    pub fn launcher_pid(&self) -> u32 {
        self.launcher_pid
    }

    pub fn launcher_uid(&self) -> u32 {
        self.launcher_uid
    }

    pub fn launcher_gid(&self) -> u32 {
        self.launcher_gid
    }

    pub fn node_uid(&self) -> u32 {
        self.node_uid
    }

    pub fn node_gid(&self) -> u32 {
        self.node_gid
    }

    pub fn checker_uid(&self) -> u32 {
        self.checker_uid
    }

    pub fn checker_gid(&self) -> u32 {
        self.checker_gid
    }

    pub fn activation_allowed(&self) -> bool {
        self.activation_allowed
    }

    pub fn ready(&self) -> bool {
        self.ready
    }
}

impl TryFrom<ExecutionReadyDto> for ExecutionReady {
    type Error = WireError;

    fn try_from(dto: ExecutionReadyDto) -> Result<Self, Self::Error> {
        Self {
            schema: dto.schema,
            nonce_hex: dto.nonce_hex,
            request_digest_hex: dto.request_digest_hex,
            execution_policy_digest_hex: dto.execution_policy_digest_hex,
            launcher_pid: dto.launcher_pid,
            launcher_uid: dto.launcher_uid,
            launcher_gid: dto.launcher_gid,
            node_uid: dto.node_uid,
            node_gid: dto.node_gid,
            checker_uid: dto.checker_uid,
            checker_gid: dto.checker_gid,
            activation_allowed: dto.activation_allowed,
            ready: dto.ready,
        }
        .validated()
    }
}

impl WireValidate for ExecutionReady {
    fn validate_wire(&self) -> Result<(), WireError> {
        if self.schema != EXECUTION_READY_SCHEMA {
            return Err(contract("execution ready schema literal mismatch"));
        }
        for (name, value) in [
            ("nonceHex", self.nonce_hex.as_str()),
            ("requestDigestHex", self.request_digest_hex.as_str()),
            (
                "executionPolicyDigestHex",
                self.execution_policy_digest_hex.as_str(),
            ),
        ] {
            require_wire_sha256(name, value)?;
        }
        validate_service_identities(
            self.launcher_pid,
            self.launcher_uid,
            self.launcher_gid,
            self.node_uid,
            self.node_gid,
            self.checker_uid,
            self.checker_gid,
        )?;
        if self.activation_allowed || !self.ready {
            return Err(contract(
                "execution readiness must be ready=true and activationAllowed=false",
            ));
        }
        Ok(())
    }
}

impl ExecutionRequest {
    pub fn try_new(fields: ExecutionRequestFields) -> Result<Self, WireError> {
        Self {
            schema: EXECUTION_REQUEST_SCHEMA.to_string(),
            nonce_hex: fields.nonce_hex,
            operation_id_hex: fields.operation_id_hex,
            family_version: fields.family_version,
            template_id: fields.template_id,
            challenge_sha256: fields.challenge_sha256,
            epoch: fields.epoch,
            raw_answer_base64: fields.raw_answer_base64,
            submission_source_base64: fields.submission_source_base64,
            submission_source_digest_hex: fields.submission_source_digest_hex,
            candidate_digest_hex: fields.candidate_digest_hex,
            submission_digest_hex: fields.submission_digest_hex,
            registry_version: fields.registry_version,
            registry_digest_hex: fields.registry_digest_hex,
            anchor_digest_hex: fields.anchor_digest_hex,
            task_digest_hex: fields.task_digest_hex,
            checker_artifact_hash_hex: fields.checker_artifact_hash_hex,
            checker_policy_digest_hex: fields.checker_policy_digest_hex,
            checker_release_manifest_digest_hex: fields.checker_release_manifest_digest_hex,
            toolchain_identity_digest_hex: fields.toolchain_identity_digest_hex,
            execution_policy_digest_hex: fields.execution_policy_digest_hex,
            intake_version: fields.intake_version,
        }
        .validated()
    }

    fn validated(self) -> Result<Self, WireError> {
        self.validate_wire()?;
        Ok(self)
    }

    pub fn nonce_hex(&self) -> &str {
        &self.nonce_hex
    }

    pub fn operation_id_hex(&self) -> &str {
        &self.operation_id_hex
    }

    pub fn family_version(&self) -> &str {
        &self.family_version
    }

    pub fn template_id(&self) -> &str {
        &self.template_id
    }

    pub fn challenge_sha256(&self) -> &str {
        &self.challenge_sha256
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn raw_answer(&self) -> Result<Vec<u8>, WireError> {
        decode_canonical_base64("rawAnswerBase64", &self.raw_answer_base64)
    }

    pub fn submission_source(&self) -> Result<Vec<u8>, WireError> {
        decode_canonical_base64("submissionSourceBase64", &self.submission_source_base64)
    }

    pub fn candidate_digest_hex(&self) -> &str {
        &self.candidate_digest_hex
    }

    pub fn submission_source_digest_hex(&self) -> &str {
        &self.submission_source_digest_hex
    }

    pub fn submission_digest_hex(&self) -> &str {
        &self.submission_digest_hex
    }

    pub fn registry_version(&self) -> &str {
        &self.registry_version
    }

    pub fn registry_digest_hex(&self) -> &str {
        &self.registry_digest_hex
    }

    pub fn anchor_digest_hex(&self) -> &str {
        &self.anchor_digest_hex
    }

    pub fn task_digest_hex(&self) -> &str {
        &self.task_digest_hex
    }

    pub fn checker_artifact_hash_hex(&self) -> &str {
        &self.checker_artifact_hash_hex
    }

    pub fn checker_policy_digest_hex(&self) -> &str {
        &self.checker_policy_digest_hex
    }

    pub fn checker_release_manifest_digest_hex(&self) -> &str {
        &self.checker_release_manifest_digest_hex
    }

    pub fn toolchain_identity_digest_hex(&self) -> &str {
        &self.toolchain_identity_digest_hex
    }

    pub fn execution_policy_digest_hex(&self) -> &str {
        &self.execution_policy_digest_hex
    }

    pub fn intake_version(&self) -> &str {
        &self.intake_version
    }

    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn replay_request_authority(&self) -> ReplayRequestAuthority<'_> {
        ReplayRequestAuthority {
            operation_id_hex: &self.operation_id_hex,
            family_version: &self.family_version,
            template_id: &self.template_id,
            challenge_sha256: &self.challenge_sha256,
            epoch: self.epoch,
            candidate_digest_hex: &self.candidate_digest_hex,
            submission_source_digest_hex: &self.submission_source_digest_hex,
            registry_version: &self.registry_version,
            registry_digest_hex: &self.registry_digest_hex,
            anchor_digest_hex: &self.anchor_digest_hex,
            task_digest_hex: &self.task_digest_hex,
            checker_artifact_hash_hex: &self.checker_artifact_hash_hex,
            checker_policy_digest_hex: &self.checker_policy_digest_hex,
            checker_release_manifest_digest_hex: &self.checker_release_manifest_digest_hex,
            toolchain_identity_digest_hex: &self.toolchain_identity_digest_hex,
            execution_policy_digest_hex: &self.execution_policy_digest_hex,
            intake_version: &self.intake_version,
        }
    }
}

impl TryFrom<ExecutionRequestDto> for ExecutionRequest {
    type Error = WireError;

    fn try_from(dto: ExecutionRequestDto) -> Result<Self, Self::Error> {
        Self {
            schema: dto.schema,
            nonce_hex: dto.nonce_hex,
            operation_id_hex: dto.operation_id_hex,
            family_version: dto.family_version,
            template_id: dto.template_id,
            challenge_sha256: dto.challenge_sha256,
            epoch: dto.epoch,
            raw_answer_base64: dto.raw_answer_base64,
            submission_source_base64: dto.submission_source_base64,
            submission_source_digest_hex: dto.submission_source_digest_hex,
            candidate_digest_hex: dto.candidate_digest_hex,
            submission_digest_hex: dto.submission_digest_hex,
            registry_version: dto.registry_version,
            registry_digest_hex: dto.registry_digest_hex,
            anchor_digest_hex: dto.anchor_digest_hex,
            task_digest_hex: dto.task_digest_hex,
            checker_artifact_hash_hex: dto.checker_artifact_hash_hex,
            checker_policy_digest_hex: dto.checker_policy_digest_hex,
            checker_release_manifest_digest_hex: dto.checker_release_manifest_digest_hex,
            toolchain_identity_digest_hex: dto.toolchain_identity_digest_hex,
            execution_policy_digest_hex: dto.execution_policy_digest_hex,
            intake_version: dto.intake_version,
        }
        .validated()
    }
}

impl WireValidate for ExecutionRequest {
    fn validate_wire(&self) -> Result<(), WireError> {
        if self.schema != EXECUTION_REQUEST_SCHEMA {
            return Err(contract("execution request schema literal mismatch"));
        }
        for (name, value) in [
            ("nonceHex", self.nonce_hex.as_str()),
            ("operationIdHex", self.operation_id_hex.as_str()),
            ("templateId", self.template_id.as_str()),
            ("challengeSha256", self.challenge_sha256.as_str()),
            (
                "submissionSourceDigestHex",
                self.submission_source_digest_hex.as_str(),
            ),
            ("candidateDigestHex", self.candidate_digest_hex.as_str()),
            ("submissionDigestHex", self.submission_digest_hex.as_str()),
            ("registryDigestHex", self.registry_digest_hex.as_str()),
            ("anchorDigestHex", self.anchor_digest_hex.as_str()),
            ("taskDigestHex", self.task_digest_hex.as_str()),
            (
                "checkerArtifactHashHex",
                self.checker_artifact_hash_hex.as_str(),
            ),
            (
                "checkerPolicyDigestHex",
                self.checker_policy_digest_hex.as_str(),
            ),
            (
                "checkerReleaseManifestDigestHex",
                self.checker_release_manifest_digest_hex.as_str(),
            ),
            (
                "toolchainIdentityDigestHex",
                self.toolchain_identity_digest_hex.as_str(),
            ),
            (
                "executionPolicyDigestHex",
                self.execution_policy_digest_hex.as_str(),
            ),
        ] {
            require_wire_sha256(name, value)?;
        }
        require_text("familyVersion", &self.family_version, 256)?;
        require_text("registryVersion", &self.registry_version, 256)?;
        require_text("intakeVersion", &self.intake_version, 128)?;

        let raw = decode_canonical_base64("rawAnswerBase64", &self.raw_answer_base64)?;
        require_utf8("rawAnswerBase64", &raw, MAX_RAW_BYTES, false)?;
        let source =
            decode_canonical_base64("submissionSourceBase64", &self.submission_source_base64)?;
        require_utf8("submissionSourceBase64", &source, MAX_RAW_BYTES, true)?;

        if sha256_hex(&raw) != self.candidate_digest_hex {
            return Err(contract("candidateDigestHex does not bind rawAnswerBase64"));
        }
        if sha256_hex(&source) != self.submission_source_digest_hex {
            return Err(contract(
                "submissionSourceDigestHex does not bind submissionSourceBase64",
            ));
        }
        let expected = submission_digest_hex(
            &self.family_version,
            &self.template_id,
            &self.challenge_sha256,
            self.epoch,
            &raw,
        )?;
        if expected != self.submission_digest_hex {
            return Err(contract("submissionDigestHex preimage mismatch"));
        }
        Ok(())
    }
}

impl AuthorityBindings {
    pub fn try_new(fields: AuthorityBindingsFields) -> Result<Self, WireError> {
        Self {
            registry_version: fields.registry_version,
            registry_digest_hex: fields.registry_digest_hex,
            anchor_digest_hex: fields.anchor_digest_hex,
            task_digest_hex: fields.task_digest_hex,
            checker_artifact_hash_hex: fields.checker_artifact_hash_hex,
            checker_policy_digest_hex: fields.checker_policy_digest_hex,
            checker_release_manifest_digest_hex: fields.checker_release_manifest_digest_hex,
            toolchain_identity_digest_hex: fields.toolchain_identity_digest_hex,
        }
        .validated()
    }

    fn validated(self) -> Result<Self, WireError> {
        self.validate_wire()?;
        Ok(self)
    }
}

impl TryFrom<AuthorityBindingsDto> for AuthorityBindings {
    type Error = WireError;

    fn try_from(dto: AuthorityBindingsDto) -> Result<Self, Self::Error> {
        Self::try_new(AuthorityBindingsFields {
            registry_version: dto.registry_version,
            registry_digest_hex: dto.registry_digest_hex,
            anchor_digest_hex: dto.anchor_digest_hex,
            task_digest_hex: dto.task_digest_hex,
            checker_artifact_hash_hex: dto.checker_artifact_hash_hex,
            checker_policy_digest_hex: dto.checker_policy_digest_hex,
            checker_release_manifest_digest_hex: dto.checker_release_manifest_digest_hex,
            toolchain_identity_digest_hex: dto.toolchain_identity_digest_hex,
        })
    }
}

impl WireValidate for AuthorityBindings {
    fn validate_wire(&self) -> Result<(), WireError> {
        require_text(
            "authorityBindings.registryVersion",
            &self.registry_version,
            256,
        )?;
        for (name, value) in [
            ("registryDigestHex", self.registry_digest_hex.as_str()),
            ("anchorDigestHex", self.anchor_digest_hex.as_str()),
            ("taskDigestHex", self.task_digest_hex.as_str()),
            (
                "checkerArtifactHashHex",
                self.checker_artifact_hash_hex.as_str(),
            ),
            (
                "checkerPolicyDigestHex",
                self.checker_policy_digest_hex.as_str(),
            ),
            (
                "checkerReleaseManifestDigestHex",
                self.checker_release_manifest_digest_hex.as_str(),
            ),
            (
                "toolchainIdentityDigestHex",
                self.toolchain_identity_digest_hex.as_str(),
            ),
        ] {
            require_wire_sha256(name, value)?;
        }
        Ok(())
    }
}

impl WaitStatus {
    pub fn exited(exit_code: u8) -> Self {
        Self {
            kind: WaitKind::Exited,
            exit_code: Some(exit_code),
            term_signal: None,
            core_dumped: false,
        }
    }

    pub fn signaled(term_signal: u8, core_dumped: bool) -> Self {
        Self {
            kind: WaitKind::Signaled,
            exit_code: None,
            term_signal: Some(term_signal),
            core_dumped,
        }
    }

    fn exited_zero(&self) -> bool {
        self.kind == WaitKind::Exited && self.exit_code == Some(0) && self.term_signal.is_none()
    }
}

impl TryFrom<WaitStatusDto> for WaitStatus {
    type Error = WireError;

    fn try_from(dto: WaitStatusDto) -> Result<Self, Self::Error> {
        let kind = match dto.kind.as_str() {
            "exited" => WaitKind::Exited,
            "signaled" => WaitKind::Signaled,
            _ => return Err(contract("waitStatus.kind must be exited or signaled")),
        };
        let value = Self {
            kind,
            exit_code: dto.exit_code.0,
            term_signal: dto.term_signal.0,
            core_dumped: dto.core_dumped,
        };
        value.validate_wire()?;
        Ok(value)
    }
}

impl WireValidate for WaitStatus {
    fn validate_wire(&self) -> Result<(), WireError> {
        match self.kind {
            WaitKind::Exited if self.exit_code.is_some() && self.term_signal.is_none() => Ok(()),
            WaitKind::Signaled if self.exit_code.is_none() && self.term_signal.is_some() => Ok(()),
            _ => Err(contract(
                "waitStatus requires exitCode only for exited and termSignal only for signaled",
            )),
        }
    }
}

impl ResourceObservations {
    pub fn try_new(fields: ResourceObservationsFields) -> Result<Self, WireError> {
        Ok(Self {
            memory_events_low_delta: fields.memory_events_low_delta,
            memory_events_high_delta: fields.memory_events_high_delta,
            memory_events_max_delta: fields.memory_events_max_delta,
            memory_events_oom_delta: fields.memory_events_oom_delta,
            memory_events_oom_kill_delta: fields.memory_events_oom_kill_delta,
            memory_events_oom_group_kill_delta: fields.memory_events_oom_group_kill_delta,
            pids_events_max_delta: fields.pids_events_max_delta,
            cpu_usage_usec_delta: fields.cpu_usage_usec_delta,
            output_limit_exceeded: fields.output_limit_exceeded,
        })
    }
}

impl TryFrom<ResourceObservationsDto> for ResourceObservations {
    type Error = WireError;

    fn try_from(dto: ResourceObservationsDto) -> Result<Self, Self::Error> {
        Self::try_new(ResourceObservationsFields {
            memory_events_low_delta: dto.memory_events_low_delta,
            memory_events_high_delta: dto.memory_events_high_delta,
            memory_events_max_delta: dto.memory_events_max_delta,
            memory_events_oom_delta: dto.memory_events_oom_delta,
            memory_events_oom_kill_delta: dto.memory_events_oom_kill_delta,
            memory_events_oom_group_kill_delta: dto.memory_events_oom_group_kill_delta,
            pids_events_max_delta: dto.pids_events_max_delta,
            cpu_usage_usec_delta: dto.cpu_usage_usec_delta,
            output_limit_exceeded: dto.output_limit_exceeded,
        })
    }
}

impl WireValidate for ResourceObservations {
    fn validate_wire(&self) -> Result<(), WireError> {
        Ok(())
    }
}

impl Cleanup {
    pub fn try_new(fields: CleanupFields) -> Result<Self, WireError> {
        let value = Self {
            child_reaped: fields.child_reaped,
            cgroup_populated_zero: fields.cgroup_populated_zero,
            launcher_pidfd_and_namespace_fds_closed: fields.launcher_pidfd_and_namespace_fds_closed,
            cgroup_leaf_removed: fields.cgroup_leaf_removed,
            completed_within_deadline: fields.completed_within_deadline,
        };
        value.validate_wire()?;
        Ok(value)
    }
}

impl TryFrom<CleanupDto> for Cleanup {
    type Error = WireError;

    fn try_from(dto: CleanupDto) -> Result<Self, Self::Error> {
        Self::try_new(CleanupFields {
            child_reaped: dto.child_reaped,
            cgroup_populated_zero: dto.cgroup_populated_zero,
            launcher_pidfd_and_namespace_fds_closed: dto.launcher_pidfd_and_namespace_fds_closed,
            cgroup_leaf_removed: dto.cgroup_leaf_removed,
            completed_within_deadline: dto.completed_within_deadline,
        })
    }
}

impl WireValidate for Cleanup {
    fn validate_wire(&self) -> Result<(), WireError> {
        if self.child_reaped
            && self.cgroup_populated_zero
            && self.launcher_pidfd_and_namespace_fds_closed
            && self.cgroup_leaf_removed
            && self.completed_within_deadline
        {
            Ok(())
        } else {
            Err(contract(
                "all cleanup fields must be true for an emitted report",
            ))
        }
    }
}

impl CheckerParsedResult {
    pub fn try_new(fields: CheckerParsedResultFields) -> Result<Self, WireError> {
        let value = Self {
            schema: CHECKER_RESULT_SCHEMA.to_string(),
            verdict: fields.verdict,
            reason_code: fields.reason_code,
            checker_task_id: fields.checker_task_id,
            task_digest: fields.task_digest_hex,
        };
        value.validate_wire()?;
        Ok(value)
    }
}

impl TryFrom<CheckerParsedResultDto> for CheckerParsedResult {
    type Error = WireError;

    fn try_from(dto: CheckerParsedResultDto) -> Result<Self, Self::Error> {
        if dto.schema != CHECKER_RESULT_SCHEMA {
            return Err(contract("parsed checker result schema literal mismatch"));
        }
        Self::try_new(CheckerParsedResultFields {
            verdict: CheckerVerdict::parse(&dto.verdict)?,
            reason_code: CheckerReason::parse(&dto.reason_code)?,
            checker_task_id: dto.checker_task_id,
            task_digest_hex: dto.task_digest,
        })
    }
}

impl WireValidate for CheckerParsedResult {
    fn validate_wire(&self) -> Result<(), WireError> {
        if self.schema != CHECKER_RESULT_SCHEMA {
            return Err(contract("parsed checker result schema literal mismatch"));
        }
        if !self.reason_code.belongs_to(self.verdict) {
            return Err(contract(
                "checker reasonCode does not belong to its verdict",
            ));
        }
        if let Some(task_id) = &self.checker_task_id {
            require_text("checkerTaskId", task_id, 256)?;
        }
        if let Some(digest) = &self.task_digest {
            require_wire_sha256("taskDigest", digest)?;
        }
        if matches!(
            self.verdict,
            CheckerVerdict::Accepted | CheckerVerdict::DeterministicReject
        ) && (self.checker_task_id.is_none() || self.task_digest.is_none())
        {
            return Err(contract(
                "accepted and deterministic_reject require checkerTaskId and taskDigest",
            ));
        }
        if self.checker_task_id.is_some() != self.task_digest.is_some() {
            return Err(contract(
                "checkerTaskId and taskDigest must be present together or both omitted",
            ));
        }
        Ok(())
    }
}

impl CheckerResult {
    pub fn try_new(fields: CheckerResultFields) -> Result<Self, WireError> {
        let value = Self {
            status: fields.status,
            stdout_sha256_hex: fields.stdout_sha256_hex,
            stderr_sha256_hex: fields.stderr_sha256_hex,
            stdout_bytes: fields.stdout_bytes,
            stderr_bytes: fields.stderr_bytes,
            parsed: fields.parsed,
        };
        value.validate_wire()?;
        Ok(value)
    }
}

impl TryFrom<CheckerResultDto> for CheckerResult {
    type Error = WireError;

    fn try_from(dto: CheckerResultDto) -> Result<Self, Self::Error> {
        Self::try_new(CheckerResultFields {
            status: CheckerOutputStatus::parse(&dto.status)?,
            stdout_sha256_hex: dto.stdout_sha256_hex,
            stderr_sha256_hex: dto.stderr_sha256_hex,
            stdout_bytes: dto.stdout_bytes,
            stderr_bytes: dto.stderr_bytes,
            parsed: dto
                .parsed
                .0
                .map(CheckerParsedResult::try_from)
                .transpose()?,
        })
    }
}

impl WireValidate for CheckerResult {
    fn validate_wire(&self) -> Result<(), WireError> {
        require_wire_sha256("stdoutSha256Hex", &self.stdout_sha256_hex)?;
        require_wire_sha256("stderrSha256Hex", &self.stderr_sha256_hex)?;
        if let Some(parsed) = &self.parsed {
            parsed.validate_wire()?;
        }
        if matches!(
            self.status,
            CheckerOutputStatus::InvalidOrNonconformingOutput
                | CheckerOutputStatus::NoCompleteOutput
        ) && self.parsed.is_some()
        {
            return Err(contract(
                "invalid-or-nonconforming and no-complete output require parsed=null",
            ));
        }
        Ok(())
    }
}

impl ExecutionReport {
    pub fn try_new(fields: ExecutionReportFields) -> Result<Self, WireError> {
        Self {
            schema: EXECUTION_REPORT_SCHEMA.to_string(),
            nonce_hex: fields.nonce_hex,
            operation_id_hex: fields.operation_id_hex,
            request_digest_hex: fields.request_digest_hex,
            execution_policy_digest_hex: fields.execution_policy_digest_hex,
            launcher_pid: fields.launcher_pid,
            launcher_uid: fields.launcher_uid,
            launcher_gid: fields.launcher_gid,
            node_uid: fields.node_uid,
            node_gid: fields.node_gid,
            checker_uid: fields.checker_uid,
            checker_gid: fields.checker_gid,
            authority_bindings: fields.authority_bindings,
            wait_status: fields.wait_status,
            timed_out: fields.timed_out,
            resource_observations: fields.resource_observations,
            cleanup: fields.cleanup,
            checker_result: fields.checker_result,
        }
        .validated()
    }

    fn validated(self) -> Result<Self, WireError> {
        self.validate_wire()?;
        Ok(self)
    }

    pub fn nonce_hex(&self) -> &str {
        &self.nonce_hex
    }

    pub fn operation_id_hex(&self) -> &str {
        &self.operation_id_hex
    }

    pub fn request_digest_hex(&self) -> &str {
        &self.request_digest_hex
    }

    pub fn execution_policy_digest_hex(&self) -> &str {
        &self.execution_policy_digest_hex
    }

    pub fn checker_verdict(&self) -> Option<CheckerVerdict> {
        self.checker_result
            .parsed
            .as_ref()
            .map(|value| value.verdict)
    }

    pub fn checker_reason(&self) -> Option<CheckerReason> {
        self.checker_result
            .parsed
            .as_ref()
            .map(|value| value.reason_code)
    }

    pub fn cleanup_complete(&self) -> bool {
        self.cleanup.child_reaped
            && self.cleanup.cgroup_populated_zero
            && self.cleanup.launcher_pidfd_and_namespace_fds_closed
            && self.cleanup.cgroup_leaf_removed
            && self.cleanup.completed_within_deadline
    }

    pub fn adjudication_view(&self) -> ExecutionAdjudicationView {
        ExecutionAdjudicationView {
            timed_out: self.timed_out,
            signaled: self.wait_status.kind == WaitKind::Signaled,
            memory_events_max_delta: self.resource_observations.memory_events_max_delta,
            pids_events_max_delta: self.resource_observations.pids_events_max_delta,
            output_limit_exceeded: self.resource_observations.output_limit_exceeded,
            checker_status: self.checker_result.status,
            checker_verdict: self.checker_verdict(),
            checker_reason: self.checker_reason(),
        }
    }
}

impl TryFrom<ExecutionReportDto> for ExecutionReport {
    type Error = WireError;

    fn try_from(dto: ExecutionReportDto) -> Result<Self, Self::Error> {
        Self {
            schema: dto.schema,
            nonce_hex: dto.nonce_hex,
            operation_id_hex: dto.operation_id_hex,
            request_digest_hex: dto.request_digest_hex,
            execution_policy_digest_hex: dto.execution_policy_digest_hex,
            launcher_pid: dto.launcher_pid,
            launcher_uid: dto.launcher_uid,
            launcher_gid: dto.launcher_gid,
            node_uid: dto.node_uid,
            node_gid: dto.node_gid,
            checker_uid: dto.checker_uid,
            checker_gid: dto.checker_gid,
            authority_bindings: AuthorityBindings::try_from(dto.authority_bindings)?,
            wait_status: WaitStatus::try_from(dto.wait_status)?,
            timed_out: dto.timed_out,
            resource_observations: ResourceObservations::try_from(dto.resource_observations)?,
            cleanup: Cleanup::try_from(dto.cleanup)?,
            checker_result: CheckerResult::try_from(dto.checker_result)?,
        }
        .validated()
    }
}

impl WireValidate for ExecutionReport {
    fn validate_wire(&self) -> Result<(), WireError> {
        if self.schema != EXECUTION_REPORT_SCHEMA {
            return Err(contract("execution report schema literal mismatch"));
        }
        for (name, value) in [
            ("nonceHex", self.nonce_hex.as_str()),
            ("operationIdHex", self.operation_id_hex.as_str()),
            ("requestDigestHex", self.request_digest_hex.as_str()),
            (
                "executionPolicyDigestHex",
                self.execution_policy_digest_hex.as_str(),
            ),
        ] {
            require_wire_sha256(name, value)?;
        }
        validate_service_identities(
            self.launcher_pid,
            self.launcher_uid,
            self.launcher_gid,
            self.node_uid,
            self.node_gid,
            self.checker_uid,
            self.checker_gid,
        )?;
        self.authority_bindings.validate_wire()?;
        self.wait_status.validate_wire()?;
        self.resource_observations.validate_wire()?;
        self.cleanup.validate_wire()?;
        self.checker_result.validate_wire()?;

        let output_status = self.checker_result.status == CheckerOutputStatus::OutputLimitExceeded;
        if output_status != self.resource_observations.output_limit_exceeded {
            return Err(contract(
                "output-limit checker status must exactly match resource observation",
            ));
        }
        if self.checker_result.status == CheckerOutputStatus::ValidCheckerResult
            && (!self.wait_status.exited_zero()
                || self.timed_out
                || self.checker_result.stdout_bytes == 0
                || self.checker_result.stderr_bytes != 0
                || self.checker_result.stderr_sha256_hex != sha256_hex(b"")
                || self.checker_result.parsed.is_none())
        {
            return Err(contract(
                "valid-checker-result requires exit zero, no timeout, empty stderr and parsed result",
            ));
        }
        if let Some(parsed) = &self.checker_result.parsed {
            if matches!(
                parsed.verdict,
                CheckerVerdict::Accepted | CheckerVerdict::DeterministicReject
            ) && parsed.task_digest.as_deref()
                != Some(self.authority_bindings.task_digest_hex.as_str())
            {
                return Err(contract(
                    "parsed taskDigest does not match report authorityBindings.taskDigestHex",
                ));
            }
        }
        Ok(())
    }
}

fn validate_service_identities(
    launcher_pid: u32,
    launcher_uid: u32,
    launcher_gid: u32,
    node_uid: u32,
    node_gid: u32,
    checker_uid: u32,
    checker_gid: u32,
) -> Result<(), WireError> {
    if launcher_pid == 0 {
        return Err(contract("launcherPid must be non-zero"));
    }
    if launcher_uid != 0 || launcher_gid != 0 {
        return Err(contract("launcher UID and GID must both be root (0)"));
    }
    if node_uid == 0
        || node_gid == 0
        || checker_uid == 0
        || checker_gid == 0
        || node_uid == checker_uid
        || node_gid == checker_gid
    {
        return Err(contract(
            "node/checker IDs must be non-root and mutually distinct",
        ));
    }
    Ok(())
}

fn decode_canonical_base64(name: &str, value: &str) -> Result<Vec<u8>, WireError> {
    let decoded = BASE64_STANDARD
        .decode(value)
        .map_err(|_| contract(format!("{name} must be canonical RFC 4648 base64")))?;
    if BASE64_STANDARD.encode(&decoded) != value {
        return Err(contract(format!(
            "{name} must use canonical RFC 4648 base64 encoding"
        )));
    }
    Ok(decoded)
}

fn require_utf8(name: &str, bytes: &[u8], max: usize, reject_nul: bool) -> Result<(), WireError> {
    if bytes.len() > max {
        return Err(contract(format!(
            "{name} decoded bytes exceed {max}: {}",
            bytes.len()
        )));
    }
    std::str::from_utf8(bytes)
        .map_err(|_| contract(format!("{name} decoded bytes must be valid UTF-8")))?;
    if reject_nul && bytes.contains(&0) {
        return Err(contract(format!(
            "{name} decoded bytes must not contain NUL"
        )));
    }
    Ok(())
}

fn require_text(name: &str, value: &str, max: usize) -> Result<(), WireError> {
    if value.is_empty() || value.len() > max {
        return Err(contract(format!(
            "{name} must be nonempty UTF-8 of at most {max} bytes"
        )));
    }
    Ok(())
}

fn length_prefix(bytes: &[u8]) -> Result<[u8; 4], WireError> {
    let length =
        u32::try_from(bytes.len()).map_err(|_| contract("digest field length exceeds u32"))?;
    Ok(length.to_be_bytes())
}

fn optional_non_null<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    T::deserialize(deserializer).map(Some)
}

fn contract(message: impl Into<String>) -> WireError {
    WireError::Contract(message.into())
}
