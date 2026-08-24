//! Successor constraints for closed-local native-shadow execution.
//!
//! The qualification policy and its v1 readiness frame stay permanently
//! disabled.  This module verifies a separate, exact-byte authority whose
//! scope is deliberately narrower than production mining: loopback HTTP only,
//! no P2P propagation, no consensus, no protocol reward, and
//! `mineableNow=false`.  This document never activates execution; a separate
//! exact replay-grant authorization capability is required by the launcher.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    complete_frame_payload, decode_strict_payload, encode_frame, read_frame_payload,
    validate_execution_session, validate_strict_json, write_frame, ExecutionHello, ExecutionReady,
    ExecutionReadyFields, ExecutionReport, ExecutionRequest, WireError, WireValidate,
    MAX_RESPONSE_FRAME_BYTES, TRACKED_EXECUTION_POLICY_BYTES,
};

const AUTHORITY_SCHEMA: &str = "boole.native-shadow.local-execution-authority.v1";
const AUTHORITY_VERSION: &str = "NATIVE-SHADOW-LOCAL-EXECUTION-AUTHORITY-V1";
const FIXED_SOCKET_PATH: &str = "/run/boole/native-shadow/launcher.sock";
const ACTIVE_READY_SCHEMA: &str = "boole.native-shadow.launcher.ready.v2";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LocalExecutionAuthorityDto {
    schema: String,
    version: String,
    activation_allowed: bool,
    scope: String,
    base_execution_policy_sha256: String,
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
    requires_verified_runtime_rootfs_replay: bool,
    allows_degraded_containment: bool,
}

/// Exact-byte verified successor constraint bundle. Fields are private so
/// callers cannot synthesize a different execution scope in memory. In
/// particular, this bundle is permanently `activationAllowed=false`.
#[derive(Debug)]
pub struct VerifiedLocalExecutionAuthority {
    activation_allowed: bool,
    scope: String,
    base_execution_policy_sha256: String,
    http_exposure: String,
    p2p_propagation_allowed: bool,
    consensus_allowed: bool,
    reward_mode: String,
    mineable_now: bool,
    requires_exact_linux_containment: bool,
    requires_verified_runtime_rootfs_replay: bool,
    allows_degraded_containment: bool,
    digest_hex: String,
}

impl VerifiedLocalExecutionAuthority {
    pub fn activation_allowed(&self) -> bool {
        self.activation_allowed
    }

    pub fn scope(&self) -> &str {
        &self.scope
    }

    pub fn loopback_only(&self) -> bool {
        self.http_exposure == "loopback-only"
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

    pub fn requires_exact_linux_containment(&self) -> bool {
        self.requires_exact_linux_containment
    }

    pub fn requires_verified_runtime_rootfs_replay(&self) -> bool {
        self.requires_verified_runtime_rootfs_replay
    }

    pub fn allows_degraded_containment(&self) -> bool {
        self.allows_degraded_containment
    }

    pub fn base_execution_policy_sha256(&self) -> &str {
        &self.base_execution_policy_sha256
    }

    pub fn fixed_socket_path(&self) -> &str {
        FIXED_SOCKET_PATH
    }

    pub fn digest_hex(&self) -> &str {
        &self.digest_hex
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LocalExecutionAuthorityError {
    #[error("installed local execution authority differs from the tracked bytes")]
    ByteMismatch,
    #[error("local execution authority is not strict JSON: {0}")]
    StrictJson(String),
    #[error("local execution authority schema mismatch: {0}")]
    Schema(String),
    #[error("local execution authority violates the closed-local contract: {0}")]
    Contract(&'static str),
}

/// Verify the exact tracked successor constraint bundle. A caller cannot use
/// path-selected or reserialized bytes: any byte difference is rejected before
/// JSON interpretation. Verification does not grant execution authority.
pub fn verify_local_execution_authority_bytes(
    bytes: &[u8],
) -> Result<VerifiedLocalExecutionAuthority, LocalExecutionAuthorityError> {
    if bytes != crate::TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES {
        return Err(LocalExecutionAuthorityError::ByteMismatch);
    }
    validate_strict_json(bytes)
        .map_err(|error| LocalExecutionAuthorityError::StrictJson(error.to_string()))?;
    let dto: LocalExecutionAuthorityDto = serde_json::from_slice(bytes)
        .map_err(|error| LocalExecutionAuthorityError::Schema(error.to_string()))?;
    validate_contract(&dto)?;
    Ok(VerifiedLocalExecutionAuthority {
        activation_allowed: dto.activation_allowed,
        scope: dto.scope,
        base_execution_policy_sha256: dto.base_execution_policy_sha256,
        http_exposure: dto.http_exposure,
        p2p_propagation_allowed: dto.p2p_propagation_allowed,
        consensus_allowed: dto.consensus_allowed,
        reward_mode: dto.reward_mode,
        mineable_now: dto.mineable_now,
        requires_exact_linux_containment: dto.requires_exact_linux_containment,
        requires_verified_runtime_rootfs_replay: dto.requires_verified_runtime_rootfs_replay,
        allows_degraded_containment: dto.allows_degraded_containment,
        digest_hex: hex::encode(Sha256::digest(bytes)),
    })
}

fn validate_contract(
    authority: &LocalExecutionAuthorityDto,
) -> Result<(), LocalExecutionAuthorityError> {
    let base_digest = hex::encode(Sha256::digest(TRACKED_EXECUTION_POLICY_BYTES));
    let literals_match = authority.schema == AUTHORITY_SCHEMA
        && authority.version == AUTHORITY_VERSION
        && authority.scope == "closed-local-loopback-only"
        && authority.base_execution_policy_sha256 == base_digest
        && authority.runtime_rootfs_portable_plan_sha256
            == "c325450a15d96bfc13fac66fadf3d4df9249283ed466005da4945be951000016"
        && authority.runtime_rootfs_source_lock_sha256
            == "01b2180a5d9a2274076630775729904448a0894b05cfaaccec142d0d476e12e1"
        && authority.runtime_rootfs_resolution_sha256
            == "f5e289a78de3ae0ac98b07343f2af077bb463d2eb6aa9eaa45098b356d284ebd"
        && authority.runtime_rootfs_replay_expectation_sha256
            == "4169f2a6236536245ed54b70bc4cef21d1bc3bbd29bcc1736c4e5d1ae46b7bc1"
        && authority.fixed_socket_path == FIXED_SOCKET_PATH
        && authority.hello_schema == "boole.native-shadow.launcher.hello.v1"
        && authority.ready_schema == "boole.native-shadow.launcher.ready.v2"
        && authority.execute_schema == "boole.native-shadow.launcher.execute.v1"
        && authority.report_schema == "boole.native-shadow.launcher.report.v1"
        && authority.http_exposure == "loopback-only"
        && authority.reward_mode == "no_protocol_reward";
    if !literals_match {
        return Err(LocalExecutionAuthorityError::Contract(
            "one or more fixed identity literals differ",
        ));
    }
    if authority.activation_allowed
        || authority.p2p_propagation_allowed
        || authority.consensus_allowed
        || authority.mineable_now
        || !authority.requires_exact_linux_containment
        || !authority.requires_verified_runtime_rootfs_replay
        || authority.allows_degraded_containment
    {
        return Err(LocalExecutionAuthorityError::Contract(
            "activation or non-economic boundary differs",
        ));
    }
    Ok(())
}

/// Resolved fixed identities used to construct one v2 active readiness frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveExecutionReadyFields {
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
    pub runtime_rootfs_replay_verified: bool,
}

/// Closed-local runtime readiness. This is a distinct schema from the frozen
/// v1 qualification-ready value, but it remains `activationAllowed=false` and
/// therefore cannot grant execution by itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveExecutionReady {
    schema: String,
    nonce_hex: String,
    request_digest_hex: String,
    execution_policy_digest_hex: String,
    local_execution_authority_digest_hex: String,
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
    runtime_rootfs_replay_verified: bool,
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
struct ActiveExecutionReadyDto {
    schema: String,
    nonce_hex: String,
    request_digest_hex: String,
    execution_policy_digest_hex: String,
    local_execution_authority_digest_hex: String,
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
    runtime_rootfs_replay_verified: bool,
    activation_allowed: bool,
    local_only: bool,
    p2p_propagation_allowed: bool,
    consensus_allowed: bool,
    reward_mode: String,
    mineable_now: bool,
    exact_linux_containment_required: bool,
    ready: bool,
}

impl ActiveExecutionReady {
    pub fn try_new(
        hello: &ExecutionHello,
        authority: &VerifiedLocalExecutionAuthority,
        fields: ActiveExecutionReadyFields,
    ) -> Result<Self, WireError> {
        // Reuse v1's fixed service-identity and Hello validation, while
        // discarding its disabled readiness value.  The v1 type itself stays
        // byte-for-byte and semantically disabled.
        let _validated_v1 = ExecutionReady::try_new(
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
            schema: ACTIVE_READY_SCHEMA.to_string(),
            nonce_hex: hello.nonce_hex().to_string(),
            request_digest_hex: hello.request_digest_hex().to_string(),
            execution_policy_digest_hex: hello.execution_policy_digest_hex().to_string(),
            local_execution_authority_digest_hex: authority.digest_hex().to_string(),
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
            runtime_rootfs_replay_verified: fields.runtime_rootfs_replay_verified,
            activation_allowed: false,
            local_only: authority.loopback_only(),
            p2p_propagation_allowed: authority.p2p_propagation_allowed(),
            consensus_allowed: authority.consensus_allowed(),
            reward_mode: authority.reward_mode().to_string(),
            mineable_now: authority.mineable_now(),
            exact_linux_containment_required: authority.requires_exact_linux_containment(),
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
    pub fn local_execution_authority_digest_hex(&self) -> &str {
        &self.local_execution_authority_digest_hex
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
    pub fn runtime_rootfs_replay_verified(&self) -> bool {
        self.runtime_rootfs_replay_verified
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

impl TryFrom<ActiveExecutionReadyDto> for ActiveExecutionReady {
    type Error = WireError;

    fn try_from(dto: ActiveExecutionReadyDto) -> Result<Self, Self::Error> {
        let value = Self {
            schema: dto.schema,
            nonce_hex: dto.nonce_hex,
            request_digest_hex: dto.request_digest_hex,
            execution_policy_digest_hex: dto.execution_policy_digest_hex,
            local_execution_authority_digest_hex: dto.local_execution_authority_digest_hex,
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
            runtime_rootfs_replay_verified: dto.runtime_rootfs_replay_verified,
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

impl WireValidate for ActiveExecutionReady {
    fn validate_wire(&self) -> Result<(), WireError> {
        if self.schema != ACTIVE_READY_SCHEMA {
            return Err(WireError::Contract(
                "active execution ready schema literal mismatch".to_string(),
            ));
        }
        let authority =
            verify_local_execution_authority_bytes(crate::TRACKED_LOCAL_EXECUTION_AUTHORITY_BYTES)
                .map_err(|error| WireError::Contract(error.to_string()))?;
        if self.local_execution_authority_digest_hex != authority.digest_hex() {
            return Err(WireError::Contract(
                "active readiness authority digest mismatch".to_string(),
            ));
        }
        crate::require_wire_sha256("launcherInstanceIdHex", &self.launcher_instance_id_hex)?;
        let synthetic_hello = execution_hello_for_validation(self)?;
        let _validated_identities = ExecutionReady::try_new(
            &synthetic_hello,
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
        if self.activation_allowed
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
            || !self.runtime_rootfs_replay_verified
            || !self.ready
        {
            return Err(WireError::Contract(
                "active readiness widened the closed-local authority".to_string(),
            ));
        }
        Ok(())
    }
}

/// Validate one complete successor session by first projecting its fixed
/// service identities onto the frozen v1 readiness contract, then delegating
/// every Hello/Execute/Report binding to the shared validator.  The projection
/// never widens v1: both readiness values remain
/// `activationAllowed=false`. The separately verified v2 value supplies only
/// closed-local constraints and runtime-rootfs proof. The launcher must hold a
/// request-bound replay-grant authorization before it can emit this frame.
pub fn validate_active_execution_session(
    hello: &ExecutionHello,
    ready: &ActiveExecutionReady,
    request_frame: &[u8],
    report: &ExecutionReport,
) -> Result<ExecutionRequest, WireError> {
    ready.validate_wire()?;
    if ready.nonce_hex != hello.nonce_hex()
        || ready.request_digest_hex != hello.request_digest_hex()
        || ready.execution_policy_digest_hex != hello.execution_policy_digest_hex()
    {
        return Err(WireError::Contract(
            "active ready does not echo the exact hello bindings".to_string(),
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
    validate_execution_session(hello, &frozen_ready, request_frame, report)
}

// ExecutionHello has no caller-selected constructor.  For inbound v2
// validation we encode the three bound fields through a minimal strict Hello
// DTO and use the existing decoder, preserving one validation authority.
fn execution_hello_for_validation(
    value: &ActiveExecutionReady,
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

pub fn encode_active_execution_ready_frame(
    value: &ActiveExecutionReady,
) -> Result<Vec<u8>, WireError> {
    encode_frame(value, MAX_RESPONSE_FRAME_BYTES)
}

pub fn decode_complete_active_execution_ready_frame(
    frame: &[u8],
) -> Result<ActiveExecutionReady, WireError> {
    let payload = complete_frame_payload(frame, MAX_RESPONSE_FRAME_BYTES)?;
    let dto: ActiveExecutionReadyDto = decode_strict_payload(payload, MAX_RESPONSE_FRAME_BYTES)?;
    ActiveExecutionReady::try_from(dto)
}

pub fn write_active_execution_ready<W: Write>(
    writer: &mut W,
    value: &ActiveExecutionReady,
) -> Result<(), WireError> {
    write_frame(writer, value, MAX_RESPONSE_FRAME_BYTES)
}

pub fn read_active_execution_ready<R: Read>(
    reader: &mut R,
) -> Result<Option<ActiveExecutionReady>, WireError> {
    read_frame_payload(reader, MAX_RESPONSE_FRAME_BYTES)?
        .map(|payload| {
            let dto: ActiveExecutionReadyDto =
                decode_strict_payload(&payload, MAX_RESPONSE_FRAME_BYTES)?;
            ActiveExecutionReady::try_from(dto)
        })
        .transpose()
}
