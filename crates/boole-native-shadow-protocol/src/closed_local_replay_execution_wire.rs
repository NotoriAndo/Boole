//! Exact authority and Ready v3 wire contract for the bounded, named-Linux
//! replay of the frozen real-history native checker fixture.
//!
//! This authority does not enable production activation.  It is narrower
//! than the historical v2 authority and requires both the installed replay
//! authorities and the exact portable runtime-rootfs replay to be verified.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    complete_frame_payload, decode_strict_payload, encode_frame, read_frame_payload,
    validate_execution_session, validate_strict_json, write_frame, CheckerReason, CheckerVerdict,
    ExecutionHello, ExecutionReady, ExecutionReadyFields, ExecutionReport, ExecutionRequest,
    WireError, WireValidate, MAX_RESPONSE_FRAME_BYTES, TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
    TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES, TRACKED_EXECUTION_POLICY_BYTES,
    TRACKED_TOOLCHAIN_IDENTITY_BYTES,
};

const AUTHORITY_SCHEMA: &str = "boole.native-shadow.closed-local-replay-execution-authority.v1";
const AUTHORITY_VERSION: &str = "REAL-FROZEN-ACCEPT-NAMED-LINUX-EXECUTION-AUTHORITY-V1";
const READY_SCHEMA: &str = "boole.native-shadow.launcher.ready.v3";
const FIXED_SOCKET_PATH: &str = "/run/boole/native-shadow/launcher.sock";
const CHECKER_SHA256: &str = "d17dca244628bb55f6fbbf799c71adcae3d548169ef0655ca27c8eb1f7ba95d7";
const CHECKER_ARTIFACT_HASH: &str =
    "fa3fea6534d505a8dcce5eca38ecc2c4a60c5173ff19a310dd82cfd797a11598";
const CHECKER_POLICY_SHA256: &str =
    "940bc5d864a5ba488f4f3e85ea7b133afacfd1170e17a869233ee5724b25a685";
const CHECKER_RELEASE_MANIFEST_SHA256: &str =
    "9e3e6bd9d0ea716988829f0251cc9a5e9bc1b7c63b90c289f9dd4ae1f5345fd7";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthorityDto {
    schema: String,
    version: String,
    production_activation_allowed: bool,
    activation_allowed: bool,
    scope: String,
    base_execution_policy_sha256: String,
    closed_local_replay_grant_sha256: String,
    closed_local_replay_registry_overlay_sha256: String,
    checker_sha256: String,
    checker_artifact_hash_hex: String,
    checker_policy_sha256: String,
    checker_release_manifest_sha256: String,
    toolchain_identity_sha256: String,
    runtime_rootfs_portable_plan_sha256: String,
    runtime_rootfs_source_lock_sha256: String,
    runtime_rootfs_resolution_sha256: String,
    runtime_rootfs_replay_expectation_sha256: String,
    fixed_socket_path: String,
    hello_schema: String,
    ready_schema: String,
    execute_schema: String,
    report_schema: String,
    http_exposure: String,
    p2p_propagation_allowed: bool,
    consensus_allowed: bool,
    reward_mode: String,
    mineable_now: bool,
    requires_exact_linux_containment: bool,
    requires_installed_replay_authorities: bool,
    requires_runtime_rootfs_replay: bool,
    allows_degraded_containment: bool,
}

/// Exact-byte authority for the sole named-Linux replay service.  It is not
/// cloneable, serializable or caller-constructible.
#[derive(Debug)]
pub struct VerifiedClosedLocalReplayExecutionAuthority {
    digest_hex: String,
    base_execution_policy_sha256: String,
}

impl VerifiedClosedLocalReplayExecutionAuthority {
    pub fn digest_hex(&self) -> &str {
        &self.digest_hex
    }

    pub fn base_execution_policy_sha256(&self) -> &str {
        &self.base_execution_policy_sha256
    }

    pub fn fixed_socket_path(&self) -> &str {
        FIXED_SOCKET_PATH
    }

    pub fn activation_allowed(&self) -> bool {
        false
    }

    pub fn production_activation_allowed(&self) -> bool {
        false
    }

    pub fn requires_exact_linux_containment(&self) -> bool {
        true
    }

    pub fn requires_installed_replay_authorities(&self) -> bool {
        true
    }

    pub fn requires_runtime_rootfs_replay(&self) -> bool {
        true
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ClosedLocalReplayExecutionAuthorityError {
    #[error("installed closed-local replay execution authority differs from tracked bytes")]
    ByteMismatch,
    #[error("closed-local replay execution authority is not strict JSON: {0}")]
    StrictJson(String),
    #[error("closed-local replay execution authority schema mismatch: {0}")]
    Schema(String),
    #[error("closed-local replay execution authority contract differs: {0}")]
    Contract(&'static str),
}

pub fn verify_closed_local_replay_execution_authority_bytes(
    bytes: &[u8],
) -> Result<VerifiedClosedLocalReplayExecutionAuthority, ClosedLocalReplayExecutionAuthorityError> {
    if bytes != crate::TRACKED_CLOSED_LOCAL_REPLAY_EXECUTION_AUTHORITY_BYTES {
        return Err(ClosedLocalReplayExecutionAuthorityError::ByteMismatch);
    }
    validate_strict_json(bytes)
        .map_err(|error| ClosedLocalReplayExecutionAuthorityError::StrictJson(error.to_string()))?;
    let dto: AuthorityDto = serde_json::from_slice(bytes)
        .map_err(|error| ClosedLocalReplayExecutionAuthorityError::Schema(error.to_string()))?;
    validate_authority_contract(&dto)?;
    Ok(VerifiedClosedLocalReplayExecutionAuthority {
        digest_hex: hex::encode(Sha256::digest(bytes)),
        base_execution_policy_sha256: dto.base_execution_policy_sha256,
    })
}

fn validate_authority_contract(
    value: &AuthorityDto,
) -> Result<(), ClosedLocalReplayExecutionAuthorityError> {
    let exact_literals = value.schema == AUTHORITY_SCHEMA
        && value.version == AUTHORITY_VERSION
        && value.scope == "closed-local-named-linux-replay-only"
        && value.base_execution_policy_sha256
            == hex::encode(Sha256::digest(TRACKED_EXECUTION_POLICY_BYTES))
        && value.closed_local_replay_grant_sha256
            == hex::encode(Sha256::digest(TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES))
        && value.closed_local_replay_registry_overlay_sha256
            == hex::encode(Sha256::digest(
                TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
            ))
        && value.checker_sha256 == CHECKER_SHA256
        && value.checker_artifact_hash_hex == CHECKER_ARTIFACT_HASH
        && value.checker_policy_sha256 == CHECKER_POLICY_SHA256
        && value.checker_release_manifest_sha256 == CHECKER_RELEASE_MANIFEST_SHA256
        && value.toolchain_identity_sha256
            == hex::encode(Sha256::digest(TRACKED_TOOLCHAIN_IDENTITY_BYTES))
        && value.runtime_rootfs_portable_plan_sha256
            == "fa4119964d87f30ad9fde496f509f0dbcc641f33ea52a345b19c1d2296cabb42"
        && value.runtime_rootfs_source_lock_sha256
            == "01b2180a5d9a2274076630775729904448a0894b05cfaaccec142d0d476e12e1"
        && value.runtime_rootfs_resolution_sha256
            == "5ff55eb8193ef8e5236b7401264bac08144b3431fd1cf0d378c8130d0d602af5"
        && value.runtime_rootfs_replay_expectation_sha256
            == "ce1597ce06ed7a89d3293e69997c3c129085e326ee90e8fb1d17cb6e92d2518b"
        && value.fixed_socket_path == FIXED_SOCKET_PATH
        && value.hello_schema == "boole.native-shadow.launcher.hello.v1"
        && value.ready_schema == READY_SCHEMA
        && value.execute_schema == "boole.native-shadow.launcher.execute.v1"
        && value.report_schema == "boole.native-shadow.launcher.report.v1"
        && value.http_exposure == "loopback-only"
        && value.reward_mode == "no_protocol_reward";
    if !exact_literals {
        return Err(ClosedLocalReplayExecutionAuthorityError::Contract(
            "one or more fixed identity literals differ",
        ));
    }
    if value.production_activation_allowed
        || value.activation_allowed
        || value.p2p_propagation_allowed
        || value.consensus_allowed
        || value.mineable_now
        || !value.requires_exact_linux_containment
        || !value.requires_installed_replay_authorities
        || !value.requires_runtime_rootfs_replay
        || value.allows_degraded_containment
    {
        return Err(ClosedLocalReplayExecutionAuthorityError::Contract(
            "activation, containment or non-economic boundary differs",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedLocalReplayExecutionReadyFields {
    pub launcher_pid: u32,
    pub launcher_uid: u32,
    pub launcher_gid: u32,
    pub node_uid: u32,
    pub node_gid: u32,
    pub checker_uid: u32,
    pub checker_gid: u32,
    pub startup_recovery_complete: bool,
    pub active_execution_leaves: u32,
    pub unexpected_direct_cgroup_children: u32,
    pub manager_subgroup_verified: bool,
    pub launcher_instance_id_hex: String,
    pub installed_replay_authorities_verified: bool,
    pub runtime_rootfs_replay_verified: bool,
    pub production_activation_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosedLocalReplayExecutionReady {
    schema: String,
    nonce_hex: String,
    request_digest_hex: String,
    execution_policy_digest_hex: String,
    closed_local_replay_execution_authority_digest_hex: String,
    launcher_pid: u32,
    launcher_uid: u32,
    launcher_gid: u32,
    node_uid: u32,
    node_gid: u32,
    checker_uid: u32,
    checker_gid: u32,
    startup_recovery_complete: bool,
    active_execution_leaves: u32,
    unexpected_direct_cgroup_children: u32,
    manager_subgroup_verified: bool,
    launcher_instance_id_hex: String,
    installed_replay_authorities_verified: bool,
    runtime_rootfs_replay_verified: bool,
    production_activation_allowed: bool,
    activation_allowed: bool,
    local_only: bool,
    p2p_propagation_allowed: bool,
    consensus_allowed: bool,
    reward_mode: String,
    mineable_now: bool,
    exact_linux_containment_required: bool,
    ready: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReadyDto {
    schema: String,
    nonce_hex: String,
    request_digest_hex: String,
    execution_policy_digest_hex: String,
    closed_local_replay_execution_authority_digest_hex: String,
    launcher_pid: u32,
    launcher_uid: u32,
    launcher_gid: u32,
    node_uid: u32,
    node_gid: u32,
    checker_uid: u32,
    checker_gid: u32,
    startup_recovery_complete: bool,
    active_execution_leaves: u32,
    unexpected_direct_cgroup_children: u32,
    manager_subgroup_verified: bool,
    launcher_instance_id_hex: String,
    installed_replay_authorities_verified: bool,
    runtime_rootfs_replay_verified: bool,
    production_activation_allowed: bool,
    activation_allowed: bool,
    local_only: bool,
    p2p_propagation_allowed: bool,
    consensus_allowed: bool,
    reward_mode: String,
    mineable_now: bool,
    exact_linux_containment_required: bool,
    ready: bool,
}

impl ClosedLocalReplayExecutionReady {
    pub fn try_new(
        hello: &ExecutionHello,
        authority: &VerifiedClosedLocalReplayExecutionAuthority,
        fields: ClosedLocalReplayExecutionReadyFields,
    ) -> Result<Self, WireError> {
        let _identity_check = ExecutionReady::try_new(
            hello,
            ExecutionReadyFields {
                launcher_pid: fields.launcher_pid,
                launcher_uid: fields.launcher_uid,
                launcher_gid: fields.launcher_gid,
                node_uid: fields.node_uid,
                node_gid: fields.node_gid,
                checker_uid: fields.checker_uid,
                checker_gid: fields.checker_gid,
            },
        )?;
        let value = Self {
            schema: READY_SCHEMA.to_string(),
            nonce_hex: hello.nonce_hex().to_string(),
            request_digest_hex: hello.request_digest_hex().to_string(),
            execution_policy_digest_hex: hello.execution_policy_digest_hex().to_string(),
            closed_local_replay_execution_authority_digest_hex: authority.digest_hex().to_string(),
            launcher_pid: fields.launcher_pid,
            launcher_uid: fields.launcher_uid,
            launcher_gid: fields.launcher_gid,
            node_uid: fields.node_uid,
            node_gid: fields.node_gid,
            checker_uid: fields.checker_uid,
            checker_gid: fields.checker_gid,
            startup_recovery_complete: fields.startup_recovery_complete,
            active_execution_leaves: fields.active_execution_leaves,
            unexpected_direct_cgroup_children: fields.unexpected_direct_cgroup_children,
            manager_subgroup_verified: fields.manager_subgroup_verified,
            launcher_instance_id_hex: fields.launcher_instance_id_hex,
            installed_replay_authorities_verified: fields.installed_replay_authorities_verified,
            runtime_rootfs_replay_verified: fields.runtime_rootfs_replay_verified,
            production_activation_allowed: fields.production_activation_allowed,
            activation_allowed: false,
            local_only: true,
            p2p_propagation_allowed: false,
            consensus_allowed: false,
            reward_mode: "no_protocol_reward".to_string(),
            mineable_now: false,
            exact_linux_containment_required: true,
            ready: true,
        };
        value.validate_wire()?;
        Ok(value)
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
    pub fn authority_digest_hex(&self) -> &str {
        &self.closed_local_replay_execution_authority_digest_hex
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
    pub fn startup_recovery_complete(&self) -> bool {
        self.startup_recovery_complete
    }
    pub fn active_execution_leaves(&self) -> u32 {
        self.active_execution_leaves
    }
    pub fn unexpected_direct_cgroup_children(&self) -> u32 {
        self.unexpected_direct_cgroup_children
    }
    pub fn manager_subgroup_verified(&self) -> bool {
        self.manager_subgroup_verified
    }
    pub fn launcher_instance_id_hex(&self) -> &str {
        &self.launcher_instance_id_hex
    }
    pub fn installed_replay_authorities_verified(&self) -> bool {
        self.installed_replay_authorities_verified
    }
    pub fn runtime_rootfs_replay_verified(&self) -> bool {
        self.runtime_rootfs_replay_verified
    }
    pub fn production_activation_allowed(&self) -> bool {
        self.production_activation_allowed
    }
    pub fn activation_allowed(&self) -> bool {
        self.activation_allowed
    }
    pub fn local_only(&self) -> bool {
        self.local_only
    }
    pub fn p2p_propagation_allowed(&self) -> bool {
        self.p2p_propagation_allowed
    }
    pub fn consensus_allowed(&self) -> bool {
        self.consensus_allowed
    }
    pub fn reward_mode(&self) -> &str {
        &self.reward_mode
    }
    pub fn mineable_now(&self) -> bool {
        self.mineable_now
    }
    pub fn exact_linux_containment_required(&self) -> bool {
        self.exact_linux_containment_required
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
}

impl TryFrom<ReadyDto> for ClosedLocalReplayExecutionReady {
    type Error = WireError;

    fn try_from(dto: ReadyDto) -> Result<Self, Self::Error> {
        let value = Self {
            schema: dto.schema,
            nonce_hex: dto.nonce_hex,
            request_digest_hex: dto.request_digest_hex,
            execution_policy_digest_hex: dto.execution_policy_digest_hex,
            closed_local_replay_execution_authority_digest_hex: dto
                .closed_local_replay_execution_authority_digest_hex,
            launcher_pid: dto.launcher_pid,
            launcher_uid: dto.launcher_uid,
            launcher_gid: dto.launcher_gid,
            node_uid: dto.node_uid,
            node_gid: dto.node_gid,
            checker_uid: dto.checker_uid,
            checker_gid: dto.checker_gid,
            startup_recovery_complete: dto.startup_recovery_complete,
            active_execution_leaves: dto.active_execution_leaves,
            unexpected_direct_cgroup_children: dto.unexpected_direct_cgroup_children,
            manager_subgroup_verified: dto.manager_subgroup_verified,
            launcher_instance_id_hex: dto.launcher_instance_id_hex,
            installed_replay_authorities_verified: dto.installed_replay_authorities_verified,
            runtime_rootfs_replay_verified: dto.runtime_rootfs_replay_verified,
            production_activation_allowed: dto.production_activation_allowed,
            activation_allowed: dto.activation_allowed,
            local_only: dto.local_only,
            p2p_propagation_allowed: dto.p2p_propagation_allowed,
            consensus_allowed: dto.consensus_allowed,
            reward_mode: dto.reward_mode,
            mineable_now: dto.mineable_now,
            exact_linux_containment_required: dto.exact_linux_containment_required,
            ready: dto.ready,
        };
        value.validate_wire()?;
        Ok(value)
    }
}

impl WireValidate for ClosedLocalReplayExecutionReady {
    fn validate_wire(&self) -> Result<(), WireError> {
        if self.schema != READY_SCHEMA {
            return Err(WireError::Contract(
                "closed-local replay ready schema literal mismatch".to_string(),
            ));
        }
        let authority = verify_closed_local_replay_execution_authority_bytes(
            crate::TRACKED_CLOSED_LOCAL_REPLAY_EXECUTION_AUTHORITY_BYTES,
        )
        .map_err(|error| WireError::Contract(error.to_string()))?;
        if self.closed_local_replay_execution_authority_digest_hex != authority.digest_hex() {
            return Err(WireError::Contract(
                "closed-local replay ready authority digest mismatch".to_string(),
            ));
        }
        crate::require_wire_sha256("launcherInstanceIdHex", &self.launcher_instance_id_hex)?;
        let hello = hello_for_validation(self)?;
        let _identity_check = ExecutionReady::try_new(
            &hello,
            ExecutionReadyFields {
                launcher_pid: self.launcher_pid,
                launcher_uid: self.launcher_uid,
                launcher_gid: self.launcher_gid,
                node_uid: self.node_uid,
                node_gid: self.node_gid,
                checker_uid: self.checker_uid,
                checker_gid: self.checker_gid,
            },
        )?;
        if self.production_activation_allowed
            || self.activation_allowed
            || !self.local_only
            || self.p2p_propagation_allowed
            || self.consensus_allowed
            || self.reward_mode != "no_protocol_reward"
            || self.mineable_now
            || !self.exact_linux_containment_required
            || !self.startup_recovery_complete
            || self.active_execution_leaves != 0
            || self.unexpected_direct_cgroup_children != 0
            || !self.manager_subgroup_verified
            || !self.installed_replay_authorities_verified
            || !self.runtime_rootfs_replay_verified
            || !self.ready
        {
            return Err(WireError::Contract(
                "closed-local replay readiness widened or weakened its authority".to_string(),
            ));
        }
        Ok(())
    }
}

fn hello_for_validation(
    value: &ClosedLocalReplayExecutionReady,
) -> Result<ExecutionHello, WireError> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Hello<'a> {
        schema: &'static str,
        nonce_hex: &'a str,
        request_digest_hex: &'a str,
        request_length_bytes: u32,
        execution_policy_digest_hex: &'a str,
    }
    let payload = serde_json::to_vec(&Hello {
        schema: "boole.native-shadow.launcher.hello.v1",
        nonce_hex: &value.nonce_hex,
        request_digest_hex: &value.request_digest_hex,
        request_length_bytes: 1,
        execution_policy_digest_hex: &value.execution_policy_digest_hex,
    })
    .map_err(|error| WireError::Encode(error.to_string()))?;
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    crate::decode_complete_execution_hello_frame(&frame)
}

/// Proof that the complete closed-local replay execution session passed every
/// v3 readiness check and every shared Hello/Execute/Report binding. Its
/// fields are private, it has no public constructor, and it is intentionally
/// not cloneable so callers cannot manufacture or duplicate validation proof.
#[derive(Debug)]
pub struct ValidatedClosedLocalReplayExecutionSession {
    request: ExecutionRequest,
    report: ExecutionReport,
}

impl ValidatedClosedLocalReplayExecutionSession {
    pub fn request(&self) -> &ExecutionRequest {
        &self.request
    }

    pub fn report(&self) -> &ExecutionReport {
        &self.report
    }

    pub fn checker_verdict(&self) -> Option<CheckerVerdict> {
        self.report.checker_verdict()
    }

    pub fn checker_reason(&self) -> Option<CheckerReason> {
        self.report.checker_reason()
    }

    pub fn cleanup_complete(&self) -> bool {
        self.report.cleanup_complete()
    }
}

pub fn validate_closed_local_replay_execution_session(
    hello: &ExecutionHello,
    ready: &ClosedLocalReplayExecutionReady,
    request_frame: &[u8],
    report: &ExecutionReport,
) -> Result<ValidatedClosedLocalReplayExecutionSession, WireError> {
    ready.validate_wire()?;
    if ready.nonce_hex != hello.nonce_hex()
        || ready.request_digest_hex != hello.request_digest_hex()
        || ready.execution_policy_digest_hex != hello.execution_policy_digest_hex()
    {
        return Err(WireError::Contract(
            "closed-local replay ready does not echo the exact hello bindings".to_string(),
        ));
    }
    let frozen_ready = ExecutionReady::try_new(
        hello,
        ExecutionReadyFields {
            launcher_pid: ready.launcher_pid,
            launcher_uid: ready.launcher_uid,
            launcher_gid: ready.launcher_gid,
            node_uid: ready.node_uid,
            node_gid: ready.node_gid,
            checker_uid: ready.checker_uid,
            checker_gid: ready.checker_gid,
        },
    )?;
    let request = validate_execution_session(hello, &frozen_ready, request_frame, report)?;
    Ok(ValidatedClosedLocalReplayExecutionSession {
        request,
        report: report.clone(),
    })
}

pub fn encode_closed_local_replay_execution_ready_frame(
    value: &ClosedLocalReplayExecutionReady,
) -> Result<Vec<u8>, WireError> {
    encode_frame(value, MAX_RESPONSE_FRAME_BYTES)
}

pub fn decode_complete_closed_local_replay_execution_ready_frame(
    frame: &[u8],
) -> Result<ClosedLocalReplayExecutionReady, WireError> {
    let payload = complete_frame_payload(frame, MAX_RESPONSE_FRAME_BYTES)?;
    let dto: ReadyDto = decode_strict_payload(payload, MAX_RESPONSE_FRAME_BYTES)?;
    ClosedLocalReplayExecutionReady::try_from(dto)
}

pub fn write_closed_local_replay_execution_ready<W: Write>(
    writer: &mut W,
    value: &ClosedLocalReplayExecutionReady,
) -> Result<(), WireError> {
    write_frame(writer, value, MAX_RESPONSE_FRAME_BYTES)
}

pub fn read_closed_local_replay_execution_ready<R: Read>(
    reader: &mut R,
) -> Result<Option<ClosedLocalReplayExecutionReady>, WireError> {
    read_frame_payload(reader, MAX_RESPONSE_FRAME_BYTES)?
        .map(|payload| {
            let dto: ReadyDto = decode_strict_payload(&payload, MAX_RESPONSE_FRAME_BYTES)?;
            ClosedLocalReplayExecutionReady::try_from(dto)
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::ValidatedClosedLocalReplayExecutionSession;

    #[test]
    fn validated_replay_session_proof_is_not_cloneable() {
        struct Invalid;
        trait AmbiguousIfClone<A> {
            fn marker() {}
        }
        impl<T: ?Sized> AmbiguousIfClone<()> for T {}
        impl<T: Clone> AmbiguousIfClone<Invalid> for T {}

        let _ = <ValidatedClosedLocalReplayExecutionSession as AmbiguousIfClone<_>>::marker;
    }
}
