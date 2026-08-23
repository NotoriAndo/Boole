//! Minimal shared authority and wire contract for the disabled native-shadow
//! qualification handshake.

#[cfg(unix)]
pub mod installed_authority;
pub mod service_identities;
pub use service_identities::{
    resolve_fixed_service_identities, IdentityResolutionError, ResolvedServiceIdentities,
};

use std::collections::BTreeSet;
use std::fmt;
use std::io::{Read, Write};

use serde::de::{self, DeserializeOwned, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const NON_INTEGER_SENTINEL: &str = "native-shadow JSON requires integer-only numbers";

pub const TRACKED_REGISTRY_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/registry-v1.json"
));
pub const TRACKED_EXECUTION_POLICY_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/containment/native-shadow-execution-policy-v1.json"
));
pub const TRACKED_TOOLCHAIN_IDENTITY_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/containment/native-shadow-toolchain-identity-v1.json"
));

pub const INSTALLED_REGISTRY_PATH: &str = "/usr/share/boole/native-shadow/registry-v1.json";
pub const INSTALLED_EXECUTION_POLICY_PATH: &str =
    "/usr/share/boole/native-shadow/execution-policy-v1.json";
pub const INSTALLED_TOOLCHAIN_IDENTITY_PATH: &str =
    "/usr/share/boole/native-shadow/toolchain-identity-v1.json";
pub const MAX_REQUEST_FRAME_BYTES: usize = 131_072;
pub const MAX_RESPONSE_FRAME_BYTES: usize = 65_536;

/// A decoded qualification request. Callers must use this crate's validated
/// constructor/decoder; direct serde deserialization is deliberately absent.
///
/// ```compile_fail
/// let _: boole_native_shadow_protocol::QualificationHello =
///     serde_json::from_slice(br#"{}"#).unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualificationHello {
    schema: String,
    nonce_hex: String,
    execution_policy_digest_hex: String,
    toolchain_identity_digest_hex: String,
    registry_digest_hex: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QualificationHelloDto {
    schema: String,
    nonce_hex: String,
    execution_policy_digest_hex: String,
    toolchain_identity_digest_hex: String,
    registry_digest_hex: String,
}

/// A decoded launcher readiness response. Direct serde deserialization must
/// never bypass the readiness invariants enforced by this crate.
///
/// ```compile_fail
/// let _: boole_native_shadow_protocol::QualificationReady =
///     serde_json::from_slice(br#"{}"#).unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualificationReady {
    schema: String,
    nonce_hex: String,
    execution_policy_digest_hex: String,
    toolchain_identity_digest_hex: String,
    registry_digest_hex: String,
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
    activation_allowed: bool,
    ready: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QualificationReadyDto {
    schema: String,
    nonce_hex: String,
    execution_policy_digest_hex: String,
    toolchain_identity_digest_hex: String,
    registry_digest_hex: String,
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
    activation_allowed: bool,
    ready: bool,
}

/// Inputs used by the root launcher to construct a validated readiness frame.
/// This carrier is not serializable; only `QualificationReady::try_new` can
/// turn it into a wire message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualificationReadyFields {
    pub nonce_hex: String,
    pub execution_policy_digest_hex: String,
    pub toolchain_identity_digest_hex: String,
    pub registry_digest_hex: String,
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
    pub activation_allowed: bool,
    pub ready: bool,
}

pub trait WireValidate {
    fn validate_wire(&self) -> Result<(), WireError>;
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WireError {
    #[error(transparent)]
    StrictJson(#[from] StrictJsonError),
    #[error("frame payload exceeds cap {cap}: {actual} bytes")]
    FrameTooLarge { cap: usize, actual: usize },
    #[error("wire contract violation: {0}")]
    Contract(String),
    #[error("wire JSON does not match its strict schema: {0}")]
    Schema(String),
    #[error("frame header truncated: expected 4 bytes, got {actual}")]
    TruncatedHeader { actual: usize },
    #[error("frame body truncated: declared {declared} bytes, got {actual}")]
    TruncatedBody { declared: usize, actual: usize },
    #[error("frame has {actual} trailing bytes after the declared payload")]
    TrailingBytes { actual: usize },
    #[error("wire JSON encoding failed: {0}")]
    Encode(String),
    #[error("frame I/O failed: {0}")]
    Io(String),
}

fn encode_frame<T>(value: &T, cap: usize) -> Result<Vec<u8>, WireError>
where
    T: Serialize + WireValidate,
{
    value.validate_wire()?;
    let payload =
        serde_json::to_vec(value).map_err(|error| WireError::Encode(error.to_string()))?;
    if payload.len() > cap || payload.len() > u32::MAX as usize {
        return Err(WireError::FrameTooLarge {
            cap,
            actual: payload.len(),
        });
    }
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn complete_frame_payload(frame: &[u8], cap: usize) -> Result<&[u8], WireError> {
    if frame.len() < 4 {
        return Err(WireError::TruncatedHeader {
            actual: frame.len(),
        });
    }
    let declared = u32::from_be_bytes(frame[..4].try_into().expect("length checked")) as usize;
    if declared > cap {
        return Err(WireError::FrameTooLarge {
            cap,
            actual: declared,
        });
    }
    let actual = frame.len() - 4;
    if actual < declared {
        return Err(WireError::TruncatedBody { declared, actual });
    }
    if actual > declared {
        return Err(WireError::TrailingBytes {
            actual: actual - declared,
        });
    }
    Ok(&frame[4..])
}

fn write_frame<W, T>(writer: &mut W, value: &T, cap: usize) -> Result<(), WireError>
where
    W: Write,
    T: Serialize + WireValidate,
{
    let encoded = encode_frame(value, cap)?;
    writer
        .write_all(&encoded)
        .map_err(|error| WireError::Io(error.to_string()))
}

/// Read one raw length-prefixed payload. A clean EOF before any header byte is
/// `Ok(None)`; partial headers and bodies are typed truncation failures. The
/// cap is checked immediately after four bytes, before body allocation/read.
fn read_frame_payload<R>(reader: &mut R, cap: usize) -> Result<Option<Vec<u8>>, WireError>
where
    R: Read,
{
    let mut header = [0_u8; 4];
    let header_read = read_up_to(reader, &mut header)?;
    if header_read == 0 {
        return Ok(None);
    }
    if header_read < header.len() {
        return Err(WireError::TruncatedHeader {
            actual: header_read,
        });
    }

    let declared = u32::from_be_bytes(header) as usize;
    if declared > cap {
        return Err(WireError::FrameTooLarge {
            cap,
            actual: declared,
        });
    }
    let mut payload = vec![0_u8; declared];
    let actual = read_up_to(reader, &mut payload)?;
    if actual < declared {
        return Err(WireError::TruncatedBody { declared, actual });
    }
    Ok(Some(payload))
}

fn read_up_to<R>(reader: &mut R, buffer: &mut [u8]) -> Result<usize, WireError>
where
    R: Read,
{
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(WireError::Io(error.to_string())),
        }
    }
    Ok(filled)
}

fn decode_strict_payload<T>(payload: &[u8], cap: usize) -> Result<T, WireError>
where
    T: DeserializeOwned,
{
    if payload.len() > cap {
        return Err(WireError::FrameTooLarge {
            cap,
            actual: payload.len(),
        });
    }
    validate_strict_json(payload)?;
    serde_json::from_slice(payload).map_err(|error| WireError::Schema(error.to_string()))
}

pub fn encode_qualification_hello_frame(value: &QualificationHello) -> Result<Vec<u8>, WireError> {
    encode_frame(value, MAX_REQUEST_FRAME_BYTES)
}

pub fn encode_qualification_ready_frame(value: &QualificationReady) -> Result<Vec<u8>, WireError> {
    encode_frame(value, MAX_RESPONSE_FRAME_BYTES)
}

pub fn decode_complete_qualification_hello_frame(
    frame: &[u8],
) -> Result<QualificationHello, WireError> {
    decode_qualification_hello_payload(complete_frame_payload(frame, MAX_REQUEST_FRAME_BYTES)?)
}

pub fn decode_complete_qualification_ready_frame(
    frame: &[u8],
) -> Result<QualificationReady, WireError> {
    decode_qualification_ready_payload(complete_frame_payload(frame, MAX_RESPONSE_FRAME_BYTES)?)
}

fn decode_qualification_hello_payload(payload: &[u8]) -> Result<QualificationHello, WireError> {
    let dto: QualificationHelloDto = decode_strict_payload(payload, MAX_REQUEST_FRAME_BYTES)?;
    QualificationHello::try_from(dto)
}

fn decode_qualification_ready_payload(payload: &[u8]) -> Result<QualificationReady, WireError> {
    let dto: QualificationReadyDto = decode_strict_payload(payload, MAX_RESPONSE_FRAME_BYTES)?;
    QualificationReady::try_from(dto)
}

pub fn write_qualification_hello<W: Write>(
    writer: &mut W,
    value: &QualificationHello,
) -> Result<(), WireError> {
    write_frame(writer, value, MAX_REQUEST_FRAME_BYTES)
}

pub fn write_qualification_ready<W: Write>(
    writer: &mut W,
    value: &QualificationReady,
) -> Result<(), WireError> {
    write_frame(writer, value, MAX_RESPONSE_FRAME_BYTES)
}

pub fn read_qualification_hello<R: Read>(
    reader: &mut R,
) -> Result<Option<QualificationHello>, WireError> {
    read_frame_payload(reader, MAX_REQUEST_FRAME_BYTES)?
        .map(|payload| decode_qualification_hello_payload(&payload))
        .transpose()
}

pub fn read_qualification_ready<R: Read>(
    reader: &mut R,
) -> Result<Option<QualificationReady>, WireError> {
    read_frame_payload(reader, MAX_RESPONSE_FRAME_BYTES)?
        .map(|payload| decode_qualification_ready_payload(&payload))
        .transpose()
}

impl QualificationHello {
    pub fn try_new(
        nonce_hex: String,
        execution_policy_digest_hex: String,
        toolchain_identity_digest_hex: String,
        registry_digest_hex: String,
    ) -> Result<Self, WireError> {
        Self {
            schema: "boole.native-shadow.launcher.qualification-hello.v1".to_string(),
            nonce_hex,
            execution_policy_digest_hex,
            toolchain_identity_digest_hex,
            registry_digest_hex,
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

    pub fn execution_policy_digest_hex(&self) -> &str {
        &self.execution_policy_digest_hex
    }

    pub fn toolchain_identity_digest_hex(&self) -> &str {
        &self.toolchain_identity_digest_hex
    }

    pub fn registry_digest_hex(&self) -> &str {
        &self.registry_digest_hex
    }
}

impl TryFrom<QualificationHelloDto> for QualificationHello {
    type Error = WireError;

    fn try_from(dto: QualificationHelloDto) -> Result<Self, Self::Error> {
        Self {
            schema: dto.schema,
            nonce_hex: dto.nonce_hex,
            execution_policy_digest_hex: dto.execution_policy_digest_hex,
            toolchain_identity_digest_hex: dto.toolchain_identity_digest_hex,
            registry_digest_hex: dto.registry_digest_hex,
        }
        .validated()
    }
}

impl QualificationReady {
    pub fn try_new(fields: QualificationReadyFields) -> Result<Self, WireError> {
        Self {
            schema: "boole.native-shadow.launcher.qualification-ready.v1".to_string(),
            nonce_hex: fields.nonce_hex,
            execution_policy_digest_hex: fields.execution_policy_digest_hex,
            toolchain_identity_digest_hex: fields.toolchain_identity_digest_hex,
            registry_digest_hex: fields.registry_digest_hex,
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
            activation_allowed: fields.activation_allowed,
            ready: fields.ready,
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

    pub fn execution_policy_digest_hex(&self) -> &str {
        &self.execution_policy_digest_hex
    }

    pub fn toolchain_identity_digest_hex(&self) -> &str {
        &self.toolchain_identity_digest_hex
    }

    pub fn registry_digest_hex(&self) -> &str {
        &self.registry_digest_hex
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

    pub fn activation_allowed(&self) -> bool {
        self.activation_allowed
    }

    pub fn ready(&self) -> bool {
        self.ready
    }
}

impl TryFrom<QualificationReadyDto> for QualificationReady {
    type Error = WireError;

    fn try_from(dto: QualificationReadyDto) -> Result<Self, Self::Error> {
        Self {
            schema: dto.schema,
            nonce_hex: dto.nonce_hex,
            execution_policy_digest_hex: dto.execution_policy_digest_hex,
            toolchain_identity_digest_hex: dto.toolchain_identity_digest_hex,
            registry_digest_hex: dto.registry_digest_hex,
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
            activation_allowed: dto.activation_allowed,
            ready: dto.ready,
        }
        .validated()
    }
}

impl WireValidate for QualificationHello {
    fn validate_wire(&self) -> Result<(), WireError> {
        if self.schema != "boole.native-shadow.launcher.qualification-hello.v1" {
            return Err(WireError::Contract(
                "qualification hello schema literal mismatch".to_string(),
            ));
        }
        for (name, value) in [
            ("nonceHex", self.nonce_hex.as_str()),
            (
                "executionPolicyDigestHex",
                self.execution_policy_digest_hex.as_str(),
            ),
            (
                "toolchainIdentityDigestHex",
                self.toolchain_identity_digest_hex.as_str(),
            ),
            ("registryDigestHex", self.registry_digest_hex.as_str()),
        ] {
            require_wire_sha256(name, value)?;
        }
        Ok(())
    }
}

impl WireValidate for QualificationReady {
    fn validate_wire(&self) -> Result<(), WireError> {
        if self.schema != "boole.native-shadow.launcher.qualification-ready.v1" {
            return Err(WireError::Contract(
                "qualification ready schema literal mismatch".to_string(),
            ));
        }
        for (name, value) in [
            ("nonceHex", self.nonce_hex.as_str()),
            (
                "executionPolicyDigestHex",
                self.execution_policy_digest_hex.as_str(),
            ),
            (
                "toolchainIdentityDigestHex",
                self.toolchain_identity_digest_hex.as_str(),
            ),
            ("registryDigestHex", self.registry_digest_hex.as_str()),
            (
                "launcherInstanceIdHex",
                self.launcher_instance_id_hex.as_str(),
            ),
        ] {
            require_wire_sha256(name, value)?;
        }
        if self.launcher_pid == 0 {
            return Err(WireError::Contract(
                "launcherPid must be non-zero".to_string(),
            ));
        }
        if self.launcher_uid != 0 || self.launcher_gid != 0 {
            return Err(WireError::Contract(
                "launcher UID and GID must both be root (0)".to_string(),
            ));
        }
        if self.node_uid == 0
            || self.node_gid == 0
            || self.checker_uid == 0
            || self.checker_gid == 0
            || self.node_uid == self.checker_uid
            || self.node_gid == self.checker_gid
        {
            return Err(WireError::Contract(
                "node/checker IDs must be non-root and mutually distinct".to_string(),
            ));
        }
        if !self.startup_recovery_complete
            || self.active_execution_leaves != 0
            || self.unexpected_direct_cgroup_children != 0
            || !self.manager_subgroup_verified
        {
            return Err(WireError::Contract(
                "qualification readiness requires completed zero-leaf recovery".to_string(),
            ));
        }
        if self.activation_allowed || !self.ready {
            return Err(WireError::Contract(
                "qualification readiness must be ready=true and activationAllowed=false"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

fn require_wire_sha256(name: &str, value: &str) -> Result<(), WireError> {
    if is_lower_sha256(value) {
        Ok(())
    } else {
        Err(WireError::Contract(format!(
            "{name} must be 64 lowercase hexadecimal characters"
        )))
    }
}

/// Exact-byte and semantic verification result. Its fields are intentionally
/// not externally constructible.
///
/// ```compile_fail
/// let _forged = boole_native_shadow_protocol::VerifiedAuthorityBundle {
///     registry: panic!("not evaluated"),
///     registry_digest: String::new(),
///     execution_policy_digest: String::new(),
///     toolchain_identity_digest: String::new(),
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedAuthorityBundle {
    registry: StrictNativeShadowRegistry,
    registry_digest: String,
    execution_policy_digest: String,
    toolchain_identity_digest: String,
}

/// Full installed registry schema. Unlike the node lifecycle's intentionally
/// small test model, this authority model accepts no unknown or omitted field.
/// It can only be obtained from a verified authority bundle.
///
/// ```compile_fail
/// let _: boole_native_shadow_protocol::StrictNativeShadowRegistry =
///     serde_json::from_slice(br#"{}"#).unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrictNativeShadowRegistry {
    schema: String,
    version: String,
    activation_allowed: bool,
    purpose: String,
    execution_policy_sha256: String,
    toolchain_identity_sha256: String,
    templates: Vec<StrictNativeShadowTemplate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrictNativeShadowTemplate {
    family_version: String,
    template_id: String,
    semantic_locator: String,
    anchor_sha256: String,
    task_path: String,
    task_sha256: String,
    checker_release: String,
    checker_release_manifest_sha256: String,
    checker_artifact_hash: String,
    policy_sha256: String,
    toolchain_channel: String,
    intake_version: String,
    challenge_sha256: String,
    epoch: u64,
    non_issuable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StrictNativeShadowRegistryDto {
    schema: String,
    version: String,
    activation_allowed: bool,
    purpose: String,
    execution_policy_sha256: String,
    toolchain_identity_sha256: String,
    templates: Vec<StrictNativeShadowTemplateDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StrictNativeShadowTemplateDto {
    family_version: String,
    template_id: String,
    semantic_locator: String,
    anchor_sha256: String,
    task_path: String,
    task_sha256: String,
    checker_release: String,
    checker_release_manifest_sha256: String,
    checker_artifact_hash: String,
    policy_sha256: String,
    toolchain_channel: String,
    intake_version: String,
    challenge_sha256: String,
    epoch: u64,
    non_issuable: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthorityError {
    #[error(transparent)]
    StrictJson(#[from] StrictJsonError),
    #[error("authority JSON does not match the frozen schema: {0}")]
    Schema(String),
    #[error("authority invariant failed: {0}")]
    Invariant(String),
    #[error("installed {0} bytes differ from the compiled tracked authority")]
    ByteMismatch(&'static str),
    #[error("registry {field} does not bind the supplied authority bytes")]
    DigestBinding { field: &'static str },
}

/// Verify the exact byte bundle both the node and launcher compile in. This is
/// intentionally path-free: callers open their fixed root-owned paths, then
/// pass the bytes here; requests, environment and CWD cannot select authority.
pub fn verify_authority_bundle(
    registry_raw: &[u8],
    execution_policy_raw: &[u8],
    toolchain_identity_raw: &[u8],
) -> Result<VerifiedAuthorityBundle, AuthorityError> {
    require_exact_bytes("registry", registry_raw, TRACKED_REGISTRY_BYTES)?;
    require_exact_bytes(
        "execution policy",
        execution_policy_raw,
        TRACKED_EXECUTION_POLICY_BYTES,
    )?;
    require_exact_bytes(
        "toolchain identity",
        toolchain_identity_raw,
        TRACKED_TOOLCHAIN_IDENTITY_BYTES,
    )?;

    validate_strict_json(registry_raw)?;
    validate_strict_json(execution_policy_raw)?;
    validate_strict_json(toolchain_identity_raw)?;

    let registry = StrictNativeShadowRegistry::parse(registry_raw)?;
    let registry_digest = sha256_hex(registry_raw);
    let execution_policy_digest = sha256_hex(execution_policy_raw);
    let toolchain_identity_digest = sha256_hex(toolchain_identity_raw);
    if registry.execution_policy_sha256 != execution_policy_digest {
        return Err(AuthorityError::DigestBinding {
            field: "executionPolicySha256",
        });
    }
    if registry.toolchain_identity_sha256 != toolchain_identity_digest {
        return Err(AuthorityError::DigestBinding {
            field: "toolchainIdentitySha256",
        });
    }

    Ok(VerifiedAuthorityBundle {
        registry,
        registry_digest,
        execution_policy_digest,
        toolchain_identity_digest,
    })
}

fn require_exact_bytes(
    label: &'static str,
    actual: &[u8],
    tracked: &[u8],
) -> Result<(), AuthorityError> {
    if actual != tracked {
        return Err(AuthorityError::ByteMismatch(label));
    }
    Ok(())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

impl VerifiedAuthorityBundle {
    pub fn registry(&self) -> &StrictNativeShadowRegistry {
        &self.registry
    }

    pub fn registry_digest(&self) -> &str {
        &self.registry_digest
    }

    pub fn execution_policy_digest(&self) -> &str {
        &self.execution_policy_digest
    }

    pub fn toolchain_identity_digest(&self) -> &str {
        &self.toolchain_identity_digest
    }
}

impl StrictNativeShadowRegistry {
    fn parse(raw: &[u8]) -> Result<Self, AuthorityError> {
        validate_strict_json(raw)?;
        let dto: StrictNativeShadowRegistryDto = serde_json::from_slice(raw)
            .map_err(|error| AuthorityError::Schema(error.to_string()))?;
        let registry = Self {
            schema: dto.schema,
            version: dto.version,
            activation_allowed: dto.activation_allowed,
            purpose: dto.purpose,
            execution_policy_sha256: dto.execution_policy_sha256,
            toolchain_identity_sha256: dto.toolchain_identity_sha256,
            templates: dto
                .templates
                .into_iter()
                .map(StrictNativeShadowTemplate::from)
                .collect(),
        };
        registry.validate()?;
        Ok(registry)
    }

    pub fn schema(&self) -> &str {
        &self.schema
    }

    pub fn activation_allowed(&self) -> bool {
        self.activation_allowed
    }

    pub fn templates(&self) -> &[StrictNativeShadowTemplate] {
        &self.templates
    }

    fn validate(&self) -> Result<(), AuthorityError> {
        if self.schema != "boole.native-shadow.registry.v1" {
            return Err(AuthorityError::Invariant(
                "registry schema must be boole.native-shadow.registry.v1".to_string(),
            ));
        }
        if self.version != "NATIVE-SHADOW-QUALIFICATION-REGISTRY-V1" {
            return Err(AuthorityError::Invariant(
                "registry version must be NATIVE-SHADOW-QUALIFICATION-REGISTRY-V1".to_string(),
            ));
        }
        if self.activation_allowed {
            return Err(AuthorityError::Invariant(
                "disabled qualification registry must keep activationAllowed=false".to_string(),
            ));
        }
        require_nonempty("purpose", &self.purpose)?;
        require_lower_sha256("executionPolicySha256", &self.execution_policy_sha256)?;
        require_lower_sha256("toolchainIdentitySha256", &self.toolchain_identity_sha256)?;
        if self.templates.is_empty() {
            return Err(AuthorityError::Invariant(
                "registry must contain at least one template".to_string(),
            ));
        }

        let mut identities = BTreeSet::new();
        for template in &self.templates {
            template.validate()?;
            let identity = (
                template.family_version.as_str(),
                template.template_id.as_str(),
                template.challenge_sha256.as_str(),
                template.epoch,
            );
            if !identities.insert(identity) {
                return Err(AuthorityError::Invariant(
                    "registry contains a duplicate four-tuple template identity".to_string(),
                ));
            }
        }
        Ok(())
    }
}

impl StrictNativeShadowTemplate {
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

    pub fn non_issuable(&self) -> bool {
        self.non_issuable
    }

    fn validate(&self) -> Result<(), AuthorityError> {
        for (name, value) in [
            ("familyVersion", self.family_version.as_str()),
            ("semanticLocator", self.semantic_locator.as_str()),
            ("checkerRelease", self.checker_release.as_str()),
            ("toolchainChannel", self.toolchain_channel.as_str()),
            ("intakeVersion", self.intake_version.as_str()),
        ] {
            require_nonempty(name, value)?;
        }
        for (name, value) in [
            ("templateId", self.template_id.as_str()),
            ("anchorSha256", self.anchor_sha256.as_str()),
            ("taskSha256", self.task_sha256.as_str()),
            (
                "checkerReleaseManifestSha256",
                self.checker_release_manifest_sha256.as_str(),
            ),
            ("checkerArtifactHash", self.checker_artifact_hash.as_str()),
            ("policySha256", self.policy_sha256.as_str()),
            ("challengeSha256", self.challenge_sha256.as_str()),
        ] {
            require_lower_sha256(name, value)?;
        }
        require_relative_authority_path("taskPath", &self.task_path)?;
        if !self.non_issuable {
            return Err(AuthorityError::Invariant(
                "disabled qualification templates must keep nonIssuable=true".to_string(),
            ));
        }
        Ok(())
    }
}

impl From<StrictNativeShadowTemplateDto> for StrictNativeShadowTemplate {
    fn from(dto: StrictNativeShadowTemplateDto) -> Self {
        Self {
            family_version: dto.family_version,
            template_id: dto.template_id,
            semantic_locator: dto.semantic_locator,
            anchor_sha256: dto.anchor_sha256,
            task_path: dto.task_path,
            task_sha256: dto.task_sha256,
            checker_release: dto.checker_release,
            checker_release_manifest_sha256: dto.checker_release_manifest_sha256,
            checker_artifact_hash: dto.checker_artifact_hash,
            policy_sha256: dto.policy_sha256,
            toolchain_channel: dto.toolchain_channel,
            intake_version: dto.intake_version,
            challenge_sha256: dto.challenge_sha256,
            epoch: dto.epoch,
            non_issuable: dto.non_issuable,
        }
    }
}

fn require_nonempty(name: &str, value: &str) -> Result<(), AuthorityError> {
    if value.is_empty() || value.contains('\0') {
        return Err(AuthorityError::Invariant(format!(
            "{name} must be non-empty and contain no NUL"
        )));
    }
    Ok(())
}

fn require_lower_sha256(name: &str, value: &str) -> Result<(), AuthorityError> {
    if !is_lower_sha256(value) {
        return Err(AuthorityError::Invariant(format!(
            "{name} must be 64 lowercase hexadecimal characters"
        )));
    }
    Ok(())
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn require_relative_authority_path(name: &str, value: &str) -> Result<(), AuthorityError> {
    require_nonempty(name, value)?;
    if value.starts_with('/')
        || value.contains('\\')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(AuthorityError::Invariant(format!(
            "{name} must be a normalized relative slash-separated path"
        )));
    }
    Ok(())
}

/// Strict syntax errors shared by installed authority files and IPC frames.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum StrictJsonError {
    #[error("UTF-8 BOM is forbidden")]
    Bom,
    #[error("duplicate object key: {0}")]
    DuplicateKey(String),
    #[error("floating-point JSON numbers are forbidden")]
    NonIntegerNumber,
    #[error("invalid JSON: {0}")]
    Parse(String),
}

/// A syntax-only value that preserves object pairs until duplicate checking.
/// Scalar contents are deliberately discarded: typed deserialization follows
/// this preflight and owns their field-level meaning.
enum SyntaxValue {
    Scalar,
    Array(Vec<SyntaxValue>),
    Object(Vec<(String, SyntaxValue)>),
}

impl<'de> Deserialize<'de> for SyntaxValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SyntaxVisitor;

        impl<'de> Visitor<'de> for SyntaxVisitor {
            type Value = SyntaxValue;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("strict JSON with integer-only numbers")
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(SyntaxValue::Scalar)
            }

            fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
                Ok(SyntaxValue::Scalar)
            }

            fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
                Ok(SyntaxValue::Scalar)
            }

            fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
                Ok(SyntaxValue::Scalar)
            }

            fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Err(E::custom(NON_INTEGER_SENTINEL))
            }

            fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E> {
                Ok(SyntaxValue::Scalar)
            }

            fn visit_string<E>(self, _value: String) -> Result<Self::Value, E> {
                Ok(SyntaxValue::Scalar)
            }

            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(SyntaxValue::Scalar)
            }

            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                SyntaxValue::deserialize(deserializer)
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<SyntaxValue>()? {
                    values.push(value);
                }
                Ok(SyntaxValue::Array(values))
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, SyntaxValue>()? {
                    entries.push(entry);
                }
                Ok(SyntaxValue::Object(entries))
            }
        }

        deserializer.deserialize_any(SyntaxVisitor)
    }
}

/// Validate the syntax accepted at the native-shadow authority and wire
/// boundary before any typed serde model is allowed to see the document.
pub fn validate_strict_json(raw: &[u8]) -> Result<(), StrictJsonError> {
    if raw.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(StrictJsonError::Bom);
    }

    let mut deserializer = serde_json::Deserializer::from_slice(raw);
    let value = SyntaxValue::deserialize(&mut deserializer).map_err(map_json_error)?;
    deserializer.end().map_err(map_json_error)?;
    reject_duplicate_keys(&value)
}

fn map_json_error(error: serde_json::Error) -> StrictJsonError {
    if error.to_string().contains(NON_INTEGER_SENTINEL) {
        StrictJsonError::NonIntegerNumber
    } else {
        StrictJsonError::Parse(error.to_string())
    }
}

fn reject_duplicate_keys(value: &SyntaxValue) -> Result<(), StrictJsonError> {
    match value {
        SyntaxValue::Object(entries) => {
            let mut seen = BTreeSet::new();
            for (key, _) in entries {
                if !seen.insert(key.as_str()) {
                    return Err(StrictJsonError::DuplicateKey(key.clone()));
                }
            }
            for (_, value) in entries {
                reject_duplicate_keys(value)?;
            }
        }
        SyntaxValue::Array(values) => {
            for value in values {
                reject_duplicate_keys(value)?;
            }
        }
        SyntaxValue::Scalar => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qualification_hello_json() -> serde_json::Value {
        serde_json::json!({
            "schema": "boole.native-shadow.launcher.qualification-hello.v1",
            "nonceHex": "11".repeat(32),
            "executionPolicyDigestHex": sha256_hex(TRACKED_EXECUTION_POLICY_BYTES),
            "toolchainIdentityDigestHex": sha256_hex(TRACKED_TOOLCHAIN_IDENTITY_BYTES),
            "registryDigestHex": sha256_hex(TRACKED_REGISTRY_BYTES),
        })
    }

    fn qualification_ready_json() -> serde_json::Value {
        serde_json::json!({
            "schema": "boole.native-shadow.launcher.qualification-ready.v1",
            "nonceHex": "11".repeat(32),
            "executionPolicyDigestHex": sha256_hex(TRACKED_EXECUTION_POLICY_BYTES),
            "toolchainIdentityDigestHex": sha256_hex(TRACKED_TOOLCHAIN_IDENTITY_BYTES),
            "registryDigestHex": sha256_hex(TRACKED_REGISTRY_BYTES),
            "launcherPid": 42,
            "launcherUid": 0,
            "launcherGid": 0,
            "nodeUid": 1001,
            "nodeGid": 1001,
            "checkerUid": 1002,
            "checkerGid": 1002,
            "startupRecoveryComplete": true,
            "activeExecutionLeaves": 0,
            "unexpectedDirectCgroupChildren": 0,
            "managerSubgroupVerified": true,
            "launcherInstanceIdHex": "22".repeat(32),
            "activationAllowed": false,
            "ready": true,
        })
    }

    #[test]
    fn strict_json_rejects_nested_duplicate_keys() {
        let duplicate = br#"{"outer":{"digest":"first","digest":"second"}}"#;

        assert_eq!(
            validate_strict_json(duplicate),
            Err(StrictJsonError::DuplicateKey("digest".to_string()))
        );
    }

    #[test]
    fn strict_json_rejects_bom_floats_and_trailing_documents() {
        assert_eq!(
            validate_strict_json(b"\xef\xbb\xbf{}"),
            Err(StrictJsonError::Bom)
        );
        assert_eq!(
            validate_strict_json(br#"{"epoch":0.5}"#),
            Err(StrictJsonError::NonIntegerNumber)
        );
        assert!(matches!(
            validate_strict_json(br#"{}{}"#),
            Err(StrictJsonError::Parse(_))
        ));
    }

    #[test]
    fn tracked_registry_parses_as_the_full_strict_schema() {
        let registry = StrictNativeShadowRegistry::parse(TRACKED_REGISTRY_BYTES)
            .expect("tracked registry must satisfy the full installed schema");

        assert_eq!(registry.templates.len(), 1);
        assert!(!registry.activation_allowed);
        assert!(registry.templates[0].non_issuable);
    }

    #[test]
    fn strict_registry_rejects_unknown_and_missing_fields() {
        let tracked = std::str::from_utf8(TRACKED_REGISTRY_BYTES).expect("tracked UTF-8");
        let unknown = tracked.replacen('{', "{\"unexpected\":1,", 1);
        assert!(StrictNativeShadowRegistry::parse(unknown.as_bytes()).is_err());

        let mut missing: serde_json::Value =
            serde_json::from_slice(TRACKED_REGISTRY_BYTES).expect("tracked JSON");
        missing["templates"][0]
            .as_object_mut()
            .expect("template object")
            .remove("nonIssuable");
        let missing = serde_json::to_vec(&missing).expect("serialize test mutation");
        assert!(StrictNativeShadowRegistry::parse(&missing).is_err());
    }

    #[test]
    fn strict_registry_rejects_wrong_literals_digests_and_float_epoch() {
        let mut wrong_schema: serde_json::Value =
            serde_json::from_slice(TRACKED_REGISTRY_BYTES).expect("tracked JSON");
        wrong_schema["schema"] = serde_json::json!("boole.native-shadow.registry.v2");
        assert!(StrictNativeShadowRegistry::parse(
            &serde_json::to_vec(&wrong_schema).expect("schema mutation")
        )
        .is_err());

        let mut uppercase_digest: serde_json::Value =
            serde_json::from_slice(TRACKED_REGISTRY_BYTES).expect("tracked JSON");
        uppercase_digest["executionPolicySha256"] = serde_json::json!("A".repeat(64));
        assert!(StrictNativeShadowRegistry::parse(
            &serde_json::to_vec(&uppercase_digest).expect("digest mutation")
        )
        .is_err());

        let tracked = std::str::from_utf8(TRACKED_REGISTRY_BYTES).expect("tracked UTF-8");
        let float_epoch = tracked.replacen("\"epoch\": 0", "\"epoch\": 0.5", 1);
        assert!(matches!(
            StrictNativeShadowRegistry::parse(float_epoch.as_bytes()),
            Err(AuthorityError::StrictJson(
                StrictJsonError::NonIntegerNumber
            ))
        ));
    }

    #[test]
    fn tracked_authority_bundle_is_byte_exact_and_digest_bound() {
        let verified = verify_authority_bundle(
            TRACKED_REGISTRY_BYTES,
            TRACKED_EXECUTION_POLICY_BYTES,
            TRACKED_TOOLCHAIN_IDENTITY_BYTES,
        )
        .expect("tracked authority bundle must verify");
        assert_eq!(verified.registry.templates.len(), 1);

        let mut changed_policy = TRACKED_EXECUTION_POLICY_BYTES.to_vec();
        let last = changed_policy.last_mut().expect("non-empty policy");
        *last ^= 1;
        assert!(verify_authority_bundle(
            TRACKED_REGISTRY_BYTES,
            &changed_policy,
            TRACKED_TOOLCHAIN_IDENTITY_BYTES,
        )
        .is_err());
    }

    #[test]
    fn byte_mismatch_precedes_interpreting_untrusted_authority_bytes() {
        for (registry, policy, toolchain, expected) in [
            (
                b"not-json-and-not-the-tracked-registry".as_slice(),
                TRACKED_EXECUTION_POLICY_BYTES,
                TRACKED_TOOLCHAIN_IDENTITY_BYTES,
                "registry",
            ),
            (
                TRACKED_REGISTRY_BYTES,
                b"not-json-and-not-the-tracked-policy".as_slice(),
                TRACKED_TOOLCHAIN_IDENTITY_BYTES,
                "execution policy",
            ),
            (
                TRACKED_REGISTRY_BYTES,
                TRACKED_EXECUTION_POLICY_BYTES,
                b"not-json-and-not-the-tracked-toolchain".as_slice(),
                "toolchain identity",
            ),
        ] {
            assert_eq!(
                verify_authority_bundle(registry, policy, toolchain),
                Err(AuthorityError::ByteMismatch(expected)),
                "installed authority must match the compiled bytes before any parser interprets it"
            );
        }
    }

    #[test]
    fn qualification_messages_are_strict_and_literal_bound() {
        let hello_bytes = serde_json::to_vec(&qualification_hello_json()).expect("hello JSON");
        let hello = decode_qualification_hello_payload(&hello_bytes).expect("valid hello");
        assert_eq!(hello.nonce_hex(), "11".repeat(32));

        let ready_bytes = serde_json::to_vec(&qualification_ready_json()).expect("ready JSON");
        let ready = decode_qualification_ready_payload(&ready_bytes).expect("valid ready");
        assert!(ready.ready());

        let mut unknown = qualification_ready_json();
        unknown["unexpected"] = serde_json::json!(true);
        assert!(decode_qualification_ready_payload(
            &serde_json::to_vec(&unknown).expect("unknown JSON")
        )
        .is_err());

        let duplicate = format!(
            "{{\"schema\":\"boole.native-shadow.launcher.qualification-hello.v1\",\"nonceHex\":\"{}\",\"nonceHex\":\"{}\",\"executionPolicyDigestHex\":\"{}\",\"toolchainIdentityDigestHex\":\"{}\",\"registryDigestHex\":\"{}\"}}",
            "11".repeat(32),
            "22".repeat(32),
            sha256_hex(TRACKED_EXECUTION_POLICY_BYTES),
            sha256_hex(TRACKED_TOOLCHAIN_IDENTITY_BYTES),
            sha256_hex(TRACKED_REGISTRY_BYTES),
        );
        assert!(decode_qualification_hello_payload(duplicate.as_bytes()).is_err());
    }

    #[test]
    fn safe_constructors_and_directional_stream_apis_preserve_the_wire_contract() {
        let hello = QualificationHello::try_new(
            "11".repeat(32),
            sha256_hex(TRACKED_EXECUTION_POLICY_BYTES),
            sha256_hex(TRACKED_TOOLCHAIN_IDENTITY_BYTES),
            sha256_hex(TRACKED_REGISTRY_BYTES),
        )
        .expect("valid hello constructor");
        assert!(QualificationHello::try_new(
            "not-a-digest".to_string(),
            sha256_hex(TRACKED_EXECUTION_POLICY_BYTES),
            sha256_hex(TRACKED_TOOLCHAIN_IDENTITY_BYTES),
            sha256_hex(TRACKED_REGISTRY_BYTES),
        )
        .is_err());

        let mut hello_stream = Vec::new();
        write_qualification_hello(&mut hello_stream, &hello).expect("write hello");
        let decoded_hello = read_qualification_hello(&mut std::io::Cursor::new(hello_stream))
            .expect("read hello")
            .expect("hello frame");
        assert_eq!(decoded_hello, hello);

        let ready = QualificationReady::try_new(QualificationReadyFields {
            nonce_hex: "11".repeat(32),
            execution_policy_digest_hex: sha256_hex(TRACKED_EXECUTION_POLICY_BYTES),
            toolchain_identity_digest_hex: sha256_hex(TRACKED_TOOLCHAIN_IDENTITY_BYTES),
            registry_digest_hex: sha256_hex(TRACKED_REGISTRY_BYTES),
            launcher_pid: 42,
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
            launcher_instance_id_hex: "22".repeat(32),
            activation_allowed: false,
            ready: true,
        })
        .expect("valid ready constructor");
        let mut ready_stream = Vec::new();
        write_qualification_ready(&mut ready_stream, &ready).expect("write ready");
        let decoded_ready = read_qualification_ready(&mut std::io::Cursor::new(ready_stream))
            .expect("read ready")
            .expect("ready frame");
        assert_eq!(decoded_ready, ready);
    }

    #[test]
    fn qualification_ready_rejects_nonliteral_readiness_state() {
        for (field, invalid) in [
            ("startupRecoveryComplete", serde_json::json!(false)),
            ("activeExecutionLeaves", serde_json::json!(1)),
            ("unexpectedDirectCgroupChildren", serde_json::json!(1)),
            ("managerSubgroupVerified", serde_json::json!(false)),
            ("activationAllowed", serde_json::json!(true)),
            ("ready", serde_json::json!(false)),
        ] {
            let mut ready = qualification_ready_json();
            ready[field] = invalid;
            let raw = serde_json::to_vec(&ready).expect("ready mutation");
            assert!(
                decode_qualification_ready_payload(&raw).is_err(),
                "invalid readiness field must fail: {field}"
            );
        }
    }

    #[test]
    fn complete_frame_codec_enforces_big_endian_cap_and_exact_length() {
        let hello_bytes = serde_json::to_vec(&qualification_hello_json()).expect("hello JSON");
        let hello = decode_qualification_hello_payload(&hello_bytes).expect("valid hello");
        let encoded = encode_qualification_hello_frame(&hello).expect("encode frame");

        assert_eq!(
            u32::from_be_bytes(encoded[..4].try_into().expect("four-byte prefix")) as usize,
            encoded.len() - 4
        );
        let decoded = decode_complete_qualification_hello_frame(&encoded).expect("decode frame");
        assert_eq!(decoded, hello);

        let oversized_prefix = ((MAX_REQUEST_FRAME_BYTES + 1) as u32)
            .to_be_bytes()
            .to_vec();
        assert!(matches!(
            decode_complete_qualification_hello_frame(&oversized_prefix),
            Err(WireError::FrameTooLarge { .. })
        ));
        assert!(matches!(
            decode_complete_qualification_hello_frame(&[0, 0, 1]),
            Err(WireError::TruncatedHeader { .. })
        ));

        let mut truncated = encoded.clone();
        truncated.pop();
        assert!(matches!(
            decode_complete_qualification_hello_frame(&truncated),
            Err(WireError::TruncatedBody { .. })
        ));

        let mut trailing = encoded;
        trailing.push(0);
        assert!(matches!(
            decode_complete_qualification_hello_frame(&trailing),
            Err(WireError::TrailingBytes { .. })
        ));
    }

    #[test]
    fn stream_codec_rejects_oversize_before_reading_body_and_distinguishes_eof() {
        let mut empty = std::io::Cursor::new(Vec::<u8>::new());
        assert_eq!(
            read_frame_payload(&mut empty, MAX_REQUEST_FRAME_BYTES).expect("clean EOF"),
            None
        );

        let mut oversized = ((MAX_REQUEST_FRAME_BYTES + 1) as u32)
            .to_be_bytes()
            .to_vec();
        oversized.extend_from_slice(&[7; 32]);
        let mut oversized = std::io::Cursor::new(oversized);
        assert!(matches!(
            read_frame_payload(&mut oversized, MAX_REQUEST_FRAME_BYTES),
            Err(WireError::FrameTooLarge { .. })
        ));
        assert_eq!(
            oversized.position(),
            4,
            "oversized length must fail before reading or allocating its body"
        );

        let hello_raw = serde_json::to_vec(&qualification_hello_json()).expect("hello JSON");
        let hello = decode_qualification_hello_payload(&hello_raw).expect("valid hello");
        let mut written = Vec::new();
        write_qualification_hello(&mut written, &hello).expect("write frame");
        let mut written = std::io::Cursor::new(written);
        let payload = read_frame_payload(&mut written, MAX_REQUEST_FRAME_BYTES)
            .expect("read frame")
            .expect("one frame");
        let decoded = decode_qualification_hello_payload(&payload).expect("decode payload");
        assert_eq!(decoded, hello);
    }
}
