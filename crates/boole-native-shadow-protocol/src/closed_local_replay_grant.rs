//! Exact-byte authority for the bounded four-case closed-local verification
//! replay of the frozen real-history native checker fixture.
//!
//! Parsing JSON is not authority. The only production constructor lives in
//! `installed_authority` and first opens the one fixed, root-owned, read-only
//! file. This module then requires those bytes to equal the bytes compiled into
//! both node and launcher before producing a non-serializable capability.

#[cfg(any(target_os = "linux", test))]
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
#[cfg(any(target_os = "linux", test))]
use base64::Engine as _;
use serde::Deserialize;
use std::sync::Arc;
use thiserror::Error;

#[cfg(any(target_os = "linux", test))]
use crate::{
    execution_wire::ReplayRequestAuthority, submission_digest_hex, ExecutionRequest,
    ExecutionRequestFields,
};
use crate::{
    sha256_hex, validate_strict_json, AuthorityError, StrictJsonError, VerifiedAuthorityBundle,
    WireError,
};
#[cfg(any(target_os = "linux", test))]
use std::sync::atomic::{AtomicBool, Ordering};

pub const TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/containment/native-shadow-closed-local-replay-grant-v1.json"
));
pub const TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/containment/native-shadow-closed-local-replay-registry-overlay-v1.json"
));

const TRACKED_REAL_HISTORY_TASK_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/task.json"
));
const TRACKED_REAL_HISTORY_ANCHOR_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/anchor.rs"
));
const TRACKED_REAL_HISTORY_ACCEPTED_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/accepted.rs"
));
const TRACKED_REAL_HISTORY_TAMPERED_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/tampered.rs"
));
const TRACKED_REAL_HISTORY_CONSTANT_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/constant.rs"
));
const TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/replay-accepted.raw.txt"
));
const TRACKED_REAL_HISTORY_TAMPERED_RAW_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/replay-tampered.raw.txt"
));
const TRACKED_REAL_HISTORY_CONSTANT_RAW_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/replay-constant.raw.txt"
));
const TRACKED_REAL_HISTORY_EMPTY_RAW_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/native-shadow/a-rooted-native-mining-e2e-v1-real-history/replay-empty.raw.txt"
));
pub const TRACKED_CHECKER_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/checker/rust-tuple-struct-project-v1/checker.py"
));
pub const TRACKED_CHECKER_POLICY_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/checker/rust-tuple-struct-project-v1/policy.json"
));
pub const TRACKED_CHECKER_RELEASE_MANIFEST_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json"
));

pub const INSTALLED_CLOSED_LOCAL_REPLAY_GRANT_PATH: &str =
    "/usr/share/boole/native-shadow/closed-local-replay-grant-v1.json";
pub const INSTALLED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_PATH: &str =
    "/usr/share/boole/native-shadow/closed-local-replay-registry-overlay-v1.json";

const INSTALLED_PRODUCTION_REGISTRY_PATH: &str = "/usr/share/boole/native-shadow/registry-v1.json";
const INSTALLED_TASK_PATH: &str =
    "/usr/share/boole/native-shadow/fixtures/a-rooted-native-mining-e2e-v1-real-history/task.json";
const INSTALLED_ANCHOR_PATH: &str =
    "/usr/share/boole/native-shadow/fixtures/a-rooted-native-mining-e2e-v1-real-history/anchor.rs";
const INSTALLED_CHECKER_PATH: &str =
    "/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/checker.py";
const INSTALLED_CHECKER_RELEASE_MANIFEST_PATH: &str =
    "/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json";
const INSTALLED_CHECKER_POLICY_PATH: &str =
    "/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/policy.json";
const INSTALLED_EXECUTION_POLICY_PATH: &str =
    "/usr/share/boole/native-shadow/execution-policy-v1.json";
const INSTALLED_TOOLCHAIN_IDENTITY_PATH: &str =
    "/usr/share/boole/native-shadow/toolchain-identity-v1.json";

const GRANT_SCHEMA: &str = "boole.native-shadow.closed-local-replay-grant.v1";
const GRANT_VERSION: &str = "REAL-FROZEN-ACCEPT-NAMED-LINUX-REPLAY-V1";
const PRODUCTION_REGISTRY_VERSION: &str = "NATIVE-SHADOW-QUALIFICATION-REGISTRY-V1";
const REGISTRY_VERSION: &str = "REAL-FROZEN-ACCEPT-REPLAY-OVERLAY-V1";
const FAMILY_VERSION: &str = "TUPLE-STRUCT-PROJECT/RUST-TUPLE-STRUCT-PROJECT-V1";
const TEMPLATE_ID: &str = "800eee9c303c6a0e771e3a3db914eb15ea4ca68d10b19385d60fedd2c23e04b5";
const CHALLENGE_SHA256: &str = "0b32a406d00a858545b98c0d0937fd940dcfc368fe8a7ef171acc2159fa0f4c1";
const INTAKE_VERSION: &str = "RUST-TUPLE-STRUCT-NATIVE-PROOF-INTAKE-V1";
const CHECKER_ARTIFACT_HASH: &str =
    "19a43dbab7592953bfdf880f7c93ee6a5e2dafdc93b1b436ef779daa8ef9fa5d";

/// Exact, parsed grant loaded from the one fixed installed path. Deliberately
/// not `Serialize`, `Deserialize`, `Clone` or publicly constructible.
#[derive(Debug)]
pub struct VerifiedClosedLocalReplayGrant {
    parsed: Arc<ClosedLocalReplayGrantDto>,
    #[cfg(any(target_os = "linux", test))]
    authorized_cases: [AtomicBool; 4],
}

/// The result of matching one exact Execute request. This is the only
/// capability an executor may accept. It is deliberately not serializable,
/// cloneable or externally constructible.
#[derive(Debug)]
pub struct VerifiedClosedLocalReplayAuthorization {
    parsed: Arc<ClosedLocalReplayGrantDto>,
    case_index: usize,
}

/// Request-owned values available after strict JSON decoding and frozen
/// proof-intake, but before the node acquires its single execution slot.
/// These values are data, not authority; matching them can only produce the
/// opaque prepared capability below.
#[derive(Debug, Clone, Copy)]
pub struct ClosedLocalReplaySubmissionFields<'a> {
    pub family_version: &'a str,
    pub template_id: &'a str,
    pub challenge_sha256: &'a str,
    pub epoch: u64,
    pub candidate_digest_hex: &'a str,
    pub submission_source_digest_hex: &'a str,
}

/// Exact replay case selected without spending its one-shot authorization.
/// It is deliberately neither cloneable nor serializable. The node may hold
/// it while attempting the node-wide execution permit; only the later
/// `authorize_prepared_execution_request` call mutates grant state.
#[derive(Debug)]
pub struct VerifiedClosedLocalReplayPreparedCase {
    parsed: Arc<ClosedLocalReplayGrantDto>,
    case_index: usize,
}

/// The only proof-intake failure frozen into the four-case matrix. Keeping
/// this vocabulary closed prevents caller-controlled error text from becoming
/// replay authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosedLocalReplayPreIntakeReason {
    EmptyResponse,
}

#[derive(Debug, Clone, Copy)]
pub struct ClosedLocalReplayPreIntakeFields<'a> {
    pub family_version: &'a str,
    pub template_id: &'a str,
    pub challenge_sha256: &'a str,
    pub epoch: u64,
    pub candidate_digest_hex: &'a str,
    pub reason: ClosedLocalReplayPreIntakeReason,
}

/// Opaque proof that the exact empty matrix row was consumed before checker
/// execution. It carries no Execute authority and cannot be converted into
/// the checker authorization type.
#[derive(Debug)]
pub struct VerifiedClosedLocalReplayPreIntakeAuthorization {
    parsed: Arc<ClosedLocalReplayGrantDto>,
    case_index: usize,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ClosedLocalReplayGrantError {
    #[error(transparent)]
    StrictJson(#[from] StrictJsonError),
    #[error("closed-local replay grant bytes differ from the compiled grant")]
    ByteMismatch,
    #[error("closed-local replay grant schema mismatch: {0}")]
    Schema(String),
    #[error("closed-local replay grant invariant failed: {0}")]
    Invariant(&'static str),
    #[error("closed-local replay grant authority bundle failed: {0}")]
    Authority(#[from] AuthorityError),
    #[error("execution request does not match replay grant field {0}")]
    RequestBinding(&'static str),
    #[error("closed-local replay case was already authorized by this grant")]
    CaseAlreadyAuthorized,
    #[error("prepared closed-local replay case belongs to another grant instance")]
    PreparedGrantMismatch,
    #[error("closed-local replay case must stop at node proof intake")]
    PreIntakeOnlyCase,
    #[error(transparent)]
    Wire(#[from] WireError),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClosedLocalReplayGrantDto {
    schema: String,
    version: String,
    purpose: String,
    scope: ReplayScopeDto,
    cases: Vec<ReplayCaseDto>,
    production_registry: RegistryGrantDto,
    registry: RegistryGrantDto,
    task: TaskGrantDto,
    anchor: FileGrantDto,
    checker: CheckerGrantDto,
    execution_policy: FileGrantDto,
    toolchain_identity: FileGrantDto,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReplayScopeDto {
    named_linux_verification_replay_only: bool,
    max_matrix_requests_total: u8,
    max_checker_executions_total: u8,
    loopback_only: bool,
    p2p_allowed: bool,
    consensus_allowed: bool,
    reward_allowed: bool,
    mineable_now: bool,
    activation_allowed: bool,
    non_issuable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReplayCaseDto {
    case_id: String,
    operation_id_hex: String,
    raw_answer_sha256: String,
    submission_source_sha256: String,
    epoch: u64,
    pre_intake_only: bool,
    max_checker_executions: u8,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RegistryGrantDto {
    path: String,
    version: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskGrantDto {
    path: String,
    sha256: String,
    family_version: String,
    template_id: String,
    challenge_sha256: String,
    intake_version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FileGrantDto {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CheckerGrantDto {
    path: String,
    sha256: String,
    artifact_hash: String,
    release_manifest_path: String,
    release_manifest_sha256: String,
    policy_path: String,
    policy_sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReplayRegistryOverlayDto {
    schema: String,
    version: String,
    activation_allowed: bool,
    purpose: String,
    base_registry_sha256: String,
    execution_policy_sha256: String,
    toolchain_identity_sha256: String,
    templates: Vec<ReplayRegistryTemplateDto>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReplayRegistryTemplateDto {
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

pub(crate) fn verify_closed_local_replay_grant_bytes(
    raw: &[u8],
    overlay_raw: &[u8],
    authority: &VerifiedAuthorityBundle,
) -> Result<VerifiedClosedLocalReplayGrant, ClosedLocalReplayGrantError> {
    if raw != TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES {
        return Err(ClosedLocalReplayGrantError::ByteMismatch);
    }
    if overlay_raw != TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES {
        return Err(ClosedLocalReplayGrantError::ByteMismatch);
    }
    validate_strict_json(raw)?;
    validate_strict_json(overlay_raw)?;
    let parsed: ClosedLocalReplayGrantDto = serde_json::from_slice(raw)
        .map_err(|error| ClosedLocalReplayGrantError::Schema(error.to_string()))?;
    let overlay: ReplayRegistryOverlayDto = serde_json::from_slice(overlay_raw)
        .map_err(|error| ClosedLocalReplayGrantError::Schema(error.to_string()))?;
    validate_grant(&parsed, &overlay, authority)?;
    Ok(VerifiedClosedLocalReplayGrant {
        parsed: Arc::new(parsed),
        #[cfg(any(target_os = "linux", test))]
        authorized_cases: std::array::from_fn(|_| AtomicBool::new(false)),
    })
}

fn validate_grant(
    grant: &ClosedLocalReplayGrantDto,
    overlay: &ReplayRegistryOverlayDto,
    authority: &VerifiedAuthorityBundle,
) -> Result<(), ClosedLocalReplayGrantError> {
    require_eq(&grant.schema, GRANT_SCHEMA, "schema")?;
    require_eq(&grant.version, GRANT_VERSION, "version")?;
    if grant.purpose.is_empty() || grant.purpose.contains('\0') {
        return Err(ClosedLocalReplayGrantError::Invariant("purpose"));
    }
    if !grant.scope.named_linux_verification_replay_only
        || grant.scope.max_matrix_requests_total != 4
        || grant.scope.max_checker_executions_total != 3
        || !grant.scope.loopback_only
        || grant.scope.p2p_allowed
        || grant.scope.consensus_allowed
        || grant.scope.reward_allowed
        || grant.scope.mineable_now
        || grant.scope.activation_allowed
        || !grant.scope.non_issuable
    {
        return Err(ClosedLocalReplayGrantError::Invariant("closed-local scope"));
    }
    if authority.registry().activation_allowed()
        || authority
            .registry()
            .templates()
            .iter()
            .any(|template| !template.non_issuable())
    {
        return Err(ClosedLocalReplayGrantError::Invariant(
            "production registry must remain disabled",
        ));
    }
    validate_cases(&grant.cases)?;
    require_eq(
        &grant.production_registry.path,
        INSTALLED_PRODUCTION_REGISTRY_PATH,
        "productionRegistry.path",
    )?;
    require_eq(
        &grant.production_registry.version,
        PRODUCTION_REGISTRY_VERSION,
        "productionRegistry.version",
    )?;
    require_eq(
        &grant.production_registry.sha256,
        authority.registry_digest(),
        "productionRegistry.sha256",
    )?;
    require_eq(
        authority.registry().version(),
        PRODUCTION_REGISTRY_VERSION,
        "authority registry version",
    )?;
    require_eq(
        &grant.registry.path,
        INSTALLED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_PATH,
        "registry.path",
    )?;
    require_eq(
        &grant.registry.version,
        REGISTRY_VERSION,
        "registry.version",
    )?;
    require_eq(
        &grant.registry.sha256,
        &sha256_hex(TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES),
        "registry.sha256",
    )?;
    validate_overlay(overlay, authority)?;
    require_eq(&grant.task.path, INSTALLED_TASK_PATH, "task.path")?;
    require_eq(
        &grant.task.sha256,
        &sha256_hex(TRACKED_REAL_HISTORY_TASK_BYTES),
        "task.sha256",
    )?;
    require_eq(
        &grant.task.family_version,
        FAMILY_VERSION,
        "task.familyVersion",
    )?;
    require_eq(&grant.task.template_id, TEMPLATE_ID, "task.templateId")?;
    require_eq(
        &grant.task.challenge_sha256,
        CHALLENGE_SHA256,
        "task.challengeSha256",
    )?;
    require_eq(
        &grant.task.intake_version,
        INTAKE_VERSION,
        "task.intakeVersion",
    )?;
    require_eq(&grant.anchor.path, INSTALLED_ANCHOR_PATH, "anchor.path")?;
    require_eq(
        &grant.anchor.sha256,
        &sha256_hex(TRACKED_REAL_HISTORY_ANCHOR_BYTES),
        "anchor.sha256",
    )?;
    require_eq(&grant.checker.path, INSTALLED_CHECKER_PATH, "checker.path")?;
    require_eq(
        &grant.checker.sha256,
        &sha256_hex(TRACKED_CHECKER_BYTES),
        "checker.sha256",
    )?;
    require_eq(
        &grant.checker.artifact_hash,
        CHECKER_ARTIFACT_HASH,
        "checker.artifactHash",
    )?;
    require_eq(
        &grant.checker.release_manifest_path,
        INSTALLED_CHECKER_RELEASE_MANIFEST_PATH,
        "checker.releaseManifestPath",
    )?;
    require_eq(
        &grant.checker.release_manifest_sha256,
        &sha256_hex(TRACKED_CHECKER_RELEASE_MANIFEST_BYTES),
        "checker.releaseManifestSha256",
    )?;
    require_eq(
        &grant.checker.policy_path,
        INSTALLED_CHECKER_POLICY_PATH,
        "checker.policyPath",
    )?;
    require_eq(
        &grant.checker.policy_sha256,
        &sha256_hex(TRACKED_CHECKER_POLICY_BYTES),
        "checker.policySha256",
    )?;
    require_eq(
        &grant.execution_policy.path,
        INSTALLED_EXECUTION_POLICY_PATH,
        "executionPolicy.path",
    )?;
    require_eq(
        &grant.execution_policy.sha256,
        authority.execution_policy_digest(),
        "executionPolicy.sha256",
    )?;
    require_eq(
        &grant.toolchain_identity.path,
        INSTALLED_TOOLCHAIN_IDENTITY_PATH,
        "toolchainIdentity.path",
    )?;
    require_eq(
        &grant.toolchain_identity.sha256,
        authority.toolchain_identity_digest(),
        "toolchainIdentity.sha256",
    )?;
    Ok(())
}

fn validate_overlay(
    overlay: &ReplayRegistryOverlayDto,
    authority: &VerifiedAuthorityBundle,
) -> Result<(), ClosedLocalReplayGrantError> {
    require_eq(
        &overlay.schema,
        "boole.native-shadow.closed-local-replay-registry-overlay.v1",
        "overlay.schema",
    )?;
    require_eq(&overlay.version, REGISTRY_VERSION, "overlay.version")?;
    if overlay.activation_allowed {
        return Err(ClosedLocalReplayGrantError::Invariant(
            "overlay.activationAllowed",
        ));
    }
    if overlay.purpose.is_empty() || overlay.purpose.contains('\0') {
        return Err(ClosedLocalReplayGrantError::Invariant("overlay.purpose"));
    }
    require_eq(
        &overlay.base_registry_sha256,
        authority.registry_digest(),
        "overlay.baseRegistrySha256",
    )?;
    require_eq(
        &overlay.execution_policy_sha256,
        authority.execution_policy_digest(),
        "overlay.executionPolicySha256",
    )?;
    require_eq(
        &overlay.toolchain_identity_sha256,
        authority.toolchain_identity_digest(),
        "overlay.toolchainIdentitySha256",
    )?;
    if overlay.templates.len() != 4 {
        return Err(ClosedLocalReplayGrantError::Invariant(
            "overlay.templates length",
        ));
    }
    let anchor_digest = sha256_hex(TRACKED_REAL_HISTORY_ANCHOR_BYTES);
    let task_digest = sha256_hex(TRACKED_REAL_HISTORY_TASK_BYTES);
    let release_manifest_digest = sha256_hex(TRACKED_CHECKER_RELEASE_MANIFEST_BYTES);
    let checker_policy_digest = sha256_hex(TRACKED_CHECKER_POLICY_BYTES);
    // This is a custom replay-only schema, not the production registry
    // schema. Its four rows deliberately repeat family/template/challenge and
    // differ only by fixed epoch, yielding four distinct node state keys.
    for (expected_epoch, template) in overlay.templates.iter().enumerate() {
        for (actual, expected, field) in [
            (
                template.family_version.as_str(),
                FAMILY_VERSION,
                "overlay.familyVersion",
            ),
            (
                template.template_id.as_str(),
                TEMPLATE_ID,
                "overlay.templateId",
            ),
            (
                template.semantic_locator.as_str(),
                "rustc-corpus/e7795af6d2449fb05a6393c3320ced873a999eb3/tests/ui/consts/transmute-const.rs:Foo",
                "overlay.semanticLocator",
            ),
            (
                template.anchor_sha256.as_str(),
                anchor_digest.as_str(),
                "overlay.anchorSha256",
            ),
            (
                template.task_path.as_str(),
                "a-rooted-native-mining-e2e-v1-real-history/task.json",
                "overlay.taskPath",
            ),
            (
                template.task_sha256.as_str(),
                task_digest.as_str(),
                "overlay.taskSha256",
            ),
            (
                template.checker_release.as_str(),
                "RUST-TUPLE-STRUCT-CHECKER-V1-QUALIFICATION",
                "overlay.checkerRelease",
            ),
            (
                template.checker_release_manifest_sha256.as_str(),
                release_manifest_digest.as_str(),
                "overlay.checkerReleaseManifestSha256",
            ),
            (
                template.checker_artifact_hash.as_str(),
                CHECKER_ARTIFACT_HASH,
                "overlay.checkerArtifactHash",
            ),
            (
                template.policy_sha256.as_str(),
                checker_policy_digest.as_str(),
                "overlay.policySha256",
            ),
            (
                template.toolchain_channel.as_str(),
                "rust-lang-ci-e7795af6d2449fb05a6393c3320ced873a999eb3",
                "overlay.toolchainChannel",
            ),
            (
                template.intake_version.as_str(),
                INTAKE_VERSION,
                "overlay.intakeVersion",
            ),
            (
                template.challenge_sha256.as_str(),
                CHALLENGE_SHA256,
                "overlay.challengeSha256",
            ),
        ] {
            require_eq(actual, expected, field)?;
        }
        if template.epoch != expected_epoch as u64 || !template.non_issuable {
            return Err(ClosedLocalReplayGrantError::Invariant(
                "overlay distinct epoch nonIssuable identity",
            ));
        }
    }
    Ok(())
}

fn validate_cases(cases: &[ReplayCaseDto]) -> Result<(), ClosedLocalReplayGrantError> {
    let accepted_source = validated_tracked_source(
        TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES,
        TRACKED_REAL_HISTORY_ACCEPTED_BYTES,
    )?;
    let tampered_source = validated_tracked_source(
        TRACKED_REAL_HISTORY_TAMPERED_RAW_BYTES,
        TRACKED_REAL_HISTORY_TAMPERED_BYTES,
    )?;
    let constant_source = validated_tracked_source(
        TRACKED_REAL_HISTORY_CONSTANT_RAW_BYTES,
        TRACKED_REAL_HISTORY_CONSTANT_BYTES,
    )?;
    if extract_replay_source(TRACKED_REAL_HISTORY_EMPTY_RAW_BYTES)?.is_some() {
        return Err(ClosedLocalReplayGrantError::Invariant(
            "empty replay envelope must fail proof intake",
        ));
    }
    let expected = [
        (
            "accepted",
            "746bba0847458159f16dfe79d19958d2f44d1de7b67f946b1831207586b978be",
            sha256_hex(TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES),
            sha256_hex(accepted_source),
            0,
            false,
            1,
        ),
        (
            "tampered",
            "99407a29b50c4c378f7b49d423515eb584f961ae005ed510723e676d1c7121cd",
            sha256_hex(TRACKED_REAL_HISTORY_TAMPERED_RAW_BYTES),
            sha256_hex(tampered_source),
            1,
            false,
            1,
        ),
        (
            "constant",
            "3558b66ce8d5ce7549e7ddb951ed6452cfa9ca575515bd8bb30598eda6f00b61",
            sha256_hex(TRACKED_REAL_HISTORY_CONSTANT_RAW_BYTES),
            sha256_hex(constant_source),
            2,
            false,
            1,
        ),
        (
            "empty",
            "c5d9705f2367515cedc06639217b95a40f5a98e377089b49ed97c0e7ec8b3038",
            sha256_hex(TRACKED_REAL_HISTORY_EMPTY_RAW_BYTES),
            sha256_hex(b""),
            3,
            true,
            0,
        ),
    ];
    if cases.len() != expected.len() {
        return Err(ClosedLocalReplayGrantError::Invariant("cases length"));
    }
    for (
        case,
        (
            expected_id,
            expected_operation,
            expected_raw_answer,
            expected_submission_source,
            expected_epoch,
            expected_pre_intake_only,
            expected_checker_executions,
        ),
    ) in cases.iter().zip(expected)
    {
        require_eq(&case.case_id, expected_id, "cases.caseId")?;
        require_eq(
            &case.operation_id_hex,
            expected_operation,
            "cases.operationIdHex",
        )?;
        require_eq(
            &case.raw_answer_sha256,
            &expected_raw_answer,
            "cases.rawAnswerSha256",
        )?;
        require_eq(
            &case.submission_source_sha256,
            &expected_submission_source,
            "cases.submissionSourceSha256",
        )?;
        if case.epoch != expected_epoch
            || case.pre_intake_only != expected_pre_intake_only
            || case.max_checker_executions != expected_checker_executions
        {
            return Err(ClosedLocalReplayGrantError::Invariant(
                "cases epoch/intake/execution limits",
            ));
        }
    }
    Ok(())
}

fn trimmed_utf8(raw: &[u8]) -> Result<&[u8], ClosedLocalReplayGrantError> {
    std::str::from_utf8(raw)
        .map(|text| text.trim().as_bytes())
        .map_err(|_| ClosedLocalReplayGrantError::Invariant("tracked replay source UTF-8"))
}

fn validated_tracked_source<'a>(
    raw_answer: &'a [u8],
    tracked_source: &[u8],
) -> Result<&'a [u8], ClosedLocalReplayGrantError> {
    let extracted = extract_replay_source(raw_answer)?.ok_or(
        ClosedLocalReplayGrantError::Invariant("checker replay envelope must pass proof intake"),
    )?;
    if extracted != trimmed_utf8(tracked_source)? {
        return Err(ClosedLocalReplayGrantError::Invariant(
            "replay envelope/source parity",
        ));
    }
    Ok(extracted)
}

/// Frozen parity copy of the public intake algorithm. It exists only to
/// prove that the tracked raw envelopes bind the separately tracked source
/// digests; it never repairs or normalizes candidate code.
fn extract_replay_source(raw: &[u8]) -> Result<Option<&[u8]>, ClosedLocalReplayGrantError> {
    let text = std::str::from_utf8(raw)
        .map_err(|_| ClosedLocalReplayGrantError::Invariant("tracked replay rawAnswer UTF-8"))?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    let Some(start) = text.find("```") else {
        return Ok(None);
    };
    let after_open = &text[start + 3..];
    let after_language = after_open.strip_prefix("rust").unwrap_or(after_open);
    let body_start = after_language
        .char_indices()
        .find(|(_, character)| !character.is_whitespace())
        .map(|(index, _)| index)
        .unwrap_or(after_language.len());
    let after_whitespace = &after_language[body_start..];
    let Some(close) = after_whitespace.find("```") else {
        return Ok(None);
    };
    let body = after_whitespace[..close].trim();
    if body.is_empty() {
        Ok(None)
    } else {
        Ok(Some(body.as_bytes()))
    }
}

fn require_eq(
    actual: &str,
    expected: &str,
    field: &'static str,
) -> Result<(), ClosedLocalReplayGrantError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ClosedLocalReplayGrantError::Invariant(field))
    }
}

impl VerifiedClosedLocalReplayGrant {
    /// Consume the one exact matrix row that is expected to fail frozen
    /// proof-intake. No Execute request or checker authorization is produced.
    #[cfg(any(target_os = "linux", test))]
    pub fn authorize_pre_intake_case(
        &self,
        fields: ClosedLocalReplayPreIntakeFields<'_>,
    ) -> Result<VerifiedClosedLocalReplayPreIntakeAuthorization, ClosedLocalReplayGrantError> {
        require_request(
            fields.family_version,
            &self.parsed.task.family_version,
            "familyVersion",
        )?;
        require_request(
            fields.template_id,
            &self.parsed.task.template_id,
            "templateId",
        )?;
        require_request(
            fields.challenge_sha256,
            &self.parsed.task.challenge_sha256,
            "challengeSha256",
        )?;
        let (case_index, replay_case) = self
            .parsed
            .cases
            .iter()
            .enumerate()
            .find(|(_, replay_case)| replay_case.epoch == fields.epoch)
            .ok_or(ClosedLocalReplayGrantError::RequestBinding("epoch"))?;
        require_request(
            fields.candidate_digest_hex,
            &replay_case.raw_answer_sha256,
            "candidateDigestHex",
        )?;
        if !replay_case.pre_intake_only
            || replay_case.max_checker_executions != 0
            || fields.reason != ClosedLocalReplayPreIntakeReason::EmptyResponse
            || replay_case.raw_answer_sha256 != replay_case.submission_source_sha256
        {
            return Err(ClosedLocalReplayGrantError::RequestBinding("preIntakeOnly"));
        }
        self.authorized_cases[case_index]
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ClosedLocalReplayGrantError::CaseAlreadyAuthorized)?;
        Ok(VerifiedClosedLocalReplayPreIntakeAuthorization {
            parsed: Arc::clone(&self.parsed),
            case_index,
        })
    }

    /// Select one exact checker-executing replay case from values already
    /// derived by the node. This is intentionally a read-only operation so a
    /// concurrently busy node cannot burn the fixed case merely by looking it
    /// up before acquiring the global execution permit.
    #[cfg(any(target_os = "linux", test))]
    pub fn prepare_execution_case(
        &self,
        fields: ClosedLocalReplaySubmissionFields<'_>,
    ) -> Result<VerifiedClosedLocalReplayPreparedCase, ClosedLocalReplayGrantError> {
        let case_index = match_submission(&self.parsed, fields)?;
        if self.parsed.cases[case_index].pre_intake_only {
            return Err(ClosedLocalReplayGrantError::PreIntakeOnlyCase);
        }
        Ok(VerifiedClosedLocalReplayPreparedCase {
            parsed: Arc::clone(&self.parsed),
            case_index,
        })
    }

    /// Spend one previously prepared case after the node owns its global
    /// execution permit. The complete Execute frame is still checked here,
    /// so preparation cannot authorize a request whose node-owned authority
    /// fields drifted before journal mutation.
    #[cfg(any(target_os = "linux", test))]
    pub fn authorize_prepared_execution_request(
        &self,
        prepared: VerifiedClosedLocalReplayPreparedCase,
        request: &ExecutionRequest,
    ) -> Result<VerifiedClosedLocalReplayAuthorization, ClosedLocalReplayGrantError> {
        if !Arc::ptr_eq(&self.parsed, &prepared.parsed) {
            return Err(ClosedLocalReplayGrantError::PreparedGrantMismatch);
        }
        let request = request.replay_request_authority();
        let case_index = match_request(&self.parsed, &request)?;
        if case_index != prepared.case_index {
            return Err(ClosedLocalReplayGrantError::RequestBinding("preparedCase"));
        }
        self.authorize_case_index(case_index)
    }

    /// Match every node-owned authority field in one validated Execute
    /// request before producing the sole capability accepted by the private
    /// executor. Each fixed case can authorize once per loaded grant. Calling
    /// this method, or reopening the fixed file, is not retry authority: the
    /// durable journal must also consume each fixed operation ID exactly once.
    /// This method does not exist in non-Linux production builds.
    #[cfg(any(target_os = "linux", test))]
    pub fn authorize_execution_request(
        &self,
        request: &ExecutionRequest,
    ) -> Result<VerifiedClosedLocalReplayAuthorization, ClosedLocalReplayGrantError> {
        let request = request.replay_request_authority();
        let case_index = match_request(&self.parsed, &request)?;
        self.authorize_case_index(case_index)
    }

    #[cfg(any(target_os = "linux", test))]
    fn authorize_case_index(
        &self,
        case_index: usize,
    ) -> Result<VerifiedClosedLocalReplayAuthorization, ClosedLocalReplayGrantError> {
        self.authorized_cases[case_index]
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| ClosedLocalReplayGrantError::CaseAlreadyAuthorized)?;
        Ok(VerifiedClosedLocalReplayAuthorization {
            parsed: Arc::clone(&self.parsed),
            case_index,
        })
    }

    pub fn max_matrix_requests_total(&self) -> u8 {
        self.parsed.scope.max_matrix_requests_total
    }

    pub fn max_checker_executions_total(&self) -> u8 {
        self.parsed.scope.max_checker_executions_total
    }

    pub fn named_linux_verification_replay_only(&self) -> bool {
        self.parsed.scope.named_linux_verification_replay_only
    }

    pub fn loopback_only(&self) -> bool {
        self.parsed.scope.loopback_only
    }

    pub fn p2p_allowed(&self) -> bool {
        self.parsed.scope.p2p_allowed
    }

    pub fn consensus_allowed(&self) -> bool {
        self.parsed.scope.consensus_allowed
    }

    pub fn reward_allowed(&self) -> bool {
        self.parsed.scope.reward_allowed
    }

    pub fn mineable_now(&self) -> bool {
        self.parsed.scope.mineable_now
    }

    pub fn activation_allowed(&self) -> bool {
        self.parsed.scope.activation_allowed
    }

    pub fn non_issuable(&self) -> bool {
        self.parsed.scope.non_issuable
    }

    pub fn registry_digest_hex(&self) -> &str {
        &self.parsed.registry.sha256
    }

    pub fn production_registry_digest_hex(&self) -> &str {
        &self.parsed.production_registry.sha256
    }
}

#[cfg(any(target_os = "linux", test))]
fn match_submission(
    grant: &ClosedLocalReplayGrantDto,
    fields: ClosedLocalReplaySubmissionFields<'_>,
) -> Result<usize, ClosedLocalReplayGrantError> {
    require_request(
        fields.family_version,
        &grant.task.family_version,
        "familyVersion",
    )?;
    require_request(fields.template_id, &grant.task.template_id, "templateId")?;
    require_request(
        fields.challenge_sha256,
        &grant.task.challenge_sha256,
        "challengeSha256",
    )?;
    let (case_index, replay_case) = grant
        .cases
        .iter()
        .enumerate()
        .find(|(_, replay_case)| replay_case.epoch == fields.epoch)
        .ok_or(ClosedLocalReplayGrantError::RequestBinding("epoch"))?;
    require_request(
        fields.candidate_digest_hex,
        &replay_case.raw_answer_sha256,
        "candidateDigestHex",
    )?;
    require_request(
        fields.submission_source_digest_hex,
        &replay_case.submission_source_sha256,
        "submissionSourceDigestHex",
    )?;
    Ok(case_index)
}

impl VerifiedClosedLocalReplayPreparedCase {
    pub fn case_id(&self) -> &str {
        &self.parsed.cases[self.case_index].case_id
    }

    pub fn operation_id_hex(&self) -> &str {
        &self.parsed.cases[self.case_index].operation_id_hex
    }

    /// Build the complete node-to-launcher Execute request from the selected
    /// fixed case. Only the session nonce is fresh input here; all execution
    /// authority fields come from the verified grant and the supplied answer
    /// bytes must match the case digests before a wire value is produced.
    #[cfg(any(target_os = "linux", test))]
    pub fn build_execution_request(
        &self,
        nonce_hex: &str,
        raw_answer: &[u8],
        submission_source: &[u8],
    ) -> Result<ExecutionRequest, ClosedLocalReplayGrantError> {
        let replay_case = &self.parsed.cases[self.case_index];
        require_request(
            &sha256_hex(raw_answer),
            &replay_case.raw_answer_sha256,
            "candidateDigestHex",
        )?;
        require_request(
            &sha256_hex(submission_source),
            &replay_case.submission_source_sha256,
            "submissionSourceDigestHex",
        )?;
        let submission_digest_hex = submission_digest_hex(
            &self.parsed.task.family_version,
            &self.parsed.task.template_id,
            &self.parsed.task.challenge_sha256,
            replay_case.epoch,
            raw_answer,
        )?;

        Ok(ExecutionRequest::try_new(ExecutionRequestFields {
            nonce_hex: nonce_hex.to_string(),
            operation_id_hex: replay_case.operation_id_hex.clone(),
            family_version: self.parsed.task.family_version.clone(),
            template_id: self.parsed.task.template_id.clone(),
            challenge_sha256: self.parsed.task.challenge_sha256.clone(),
            epoch: replay_case.epoch,
            raw_answer_base64: BASE64_STANDARD.encode(raw_answer),
            submission_source_base64: BASE64_STANDARD.encode(submission_source),
            submission_source_digest_hex: replay_case.submission_source_sha256.clone(),
            candidate_digest_hex: replay_case.raw_answer_sha256.clone(),
            submission_digest_hex,
            registry_version: self.parsed.registry.version.clone(),
            registry_digest_hex: self.parsed.registry.sha256.clone(),
            anchor_digest_hex: self.parsed.anchor.sha256.clone(),
            task_digest_hex: self.parsed.task.sha256.clone(),
            checker_artifact_hash_hex: self.parsed.checker.artifact_hash.clone(),
            checker_policy_digest_hex: self.parsed.checker.policy_sha256.clone(),
            checker_release_manifest_digest_hex: self
                .parsed
                .checker
                .release_manifest_sha256
                .clone(),
            toolchain_identity_digest_hex: self.parsed.toolchain_identity.sha256.clone(),
            execution_policy_digest_hex: self.parsed.execution_policy.sha256.clone(),
            intake_version: self.parsed.task.intake_version.clone(),
        })?)
    }
}

impl VerifiedClosedLocalReplayPreIntakeAuthorization {
    pub fn case_id(&self) -> &str {
        &self.parsed.cases[self.case_index].case_id
    }

    pub fn epoch(&self) -> u64 {
        self.parsed.cases[self.case_index].epoch
    }

    pub fn max_checker_executions(&self) -> u8 {
        self.parsed.cases[self.case_index].max_checker_executions
    }
}

#[cfg(any(target_os = "linux", test))]
fn match_request(
    grant: &ClosedLocalReplayGrantDto,
    request: &ReplayRequestAuthority<'_>,
) -> Result<usize, ClosedLocalReplayGrantError> {
    let (case_index, replay_case) = grant
        .cases
        .iter()
        .enumerate()
        .find(|(_, case)| case.operation_id_hex == request.operation_id_hex)
        .ok_or(ClosedLocalReplayGrantError::RequestBinding(
            "operationIdHex",
        ))?;
    require_request(
        request.family_version,
        &grant.task.family_version,
        "familyVersion",
    )?;
    require_request(request.template_id, &grant.task.template_id, "templateId")?;
    require_request(
        request.challenge_sha256,
        &grant.task.challenge_sha256,
        "challengeSha256",
    )?;
    require_request(
        request.candidate_digest_hex,
        &replay_case.raw_answer_sha256,
        "candidateDigestHex",
    )?;
    require_request(
        request.submission_source_digest_hex,
        &replay_case.submission_source_sha256,
        "submissionSourceDigestHex",
    )?;
    if request.epoch != replay_case.epoch {
        return Err(ClosedLocalReplayGrantError::RequestBinding("epoch"));
    }
    if replay_case.pre_intake_only {
        return Err(ClosedLocalReplayGrantError::PreIntakeOnlyCase);
    }
    require_request(
        request.registry_version,
        &grant.registry.version,
        "registryVersion",
    )?;
    require_request(
        request.registry_digest_hex,
        &grant.registry.sha256,
        "registryDigestHex",
    )?;
    require_request(
        request.anchor_digest_hex,
        &grant.anchor.sha256,
        "anchorDigestHex",
    )?;
    require_request(request.task_digest_hex, &grant.task.sha256, "taskDigestHex")?;
    require_request(
        request.checker_artifact_hash_hex,
        &grant.checker.artifact_hash,
        "checkerArtifactHashHex",
    )?;
    require_request(
        request.checker_policy_digest_hex,
        &grant.checker.policy_sha256,
        "checkerPolicyDigestHex",
    )?;
    require_request(
        request.checker_release_manifest_digest_hex,
        &grant.checker.release_manifest_sha256,
        "checkerReleaseManifestDigestHex",
    )?;
    require_request(
        request.toolchain_identity_digest_hex,
        &grant.toolchain_identity.sha256,
        "toolchainIdentityDigestHex",
    )?;
    require_request(
        request.execution_policy_digest_hex,
        &grant.execution_policy.sha256,
        "executionPolicyDigestHex",
    )?;
    require_request(
        request.intake_version,
        &grant.task.intake_version,
        "intakeVersion",
    )?;
    Ok(case_index)
}

#[cfg(any(target_os = "linux", test))]
fn require_request(
    actual: &str,
    expected: &str,
    field: &'static str,
) -> Result<(), ClosedLocalReplayGrantError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ClosedLocalReplayGrantError::RequestBinding(field))
    }
}

impl VerifiedClosedLocalReplayAuthorization {
    /// Exact task bytes compiled into both sides of the closed-local replay.
    /// The executor must use these bytes instead of reopening a request path.
    pub fn task_bytes(&self) -> &'static [u8] {
        TRACKED_REAL_HISTORY_TASK_BYTES
    }

    /// Exact anchor bytes compiled into both sides of the closed-local replay.
    /// The executor must use these bytes instead of reopening a request path.
    pub fn anchor_bytes(&self) -> &'static [u8] {
        TRACKED_REAL_HISTORY_ANCHOR_BYTES
    }

    pub fn operation_id_hex(&self) -> &str {
        &self.parsed.cases[self.case_index].operation_id_hex
    }

    pub fn task_path(&self) -> &str {
        &self.parsed.task.path
    }

    pub fn family_version(&self) -> &str {
        &self.parsed.task.family_version
    }

    pub fn template_id(&self) -> &str {
        &self.parsed.task.template_id
    }

    pub fn challenge_sha256(&self) -> &str {
        &self.parsed.task.challenge_sha256
    }

    pub fn epoch(&self) -> u64 {
        self.parsed.cases[self.case_index].epoch
    }

    pub fn intake_version(&self) -> &str {
        &self.parsed.task.intake_version
    }

    pub fn anchor_path(&self) -> &str {
        &self.parsed.anchor.path
    }

    pub fn checker_path(&self) -> &str {
        &self.parsed.checker.path
    }

    pub fn checker_sha256(&self) -> &str {
        &self.parsed.checker.sha256
    }

    pub fn checker_artifact_hash_hex(&self) -> &str {
        &self.parsed.checker.artifact_hash
    }

    pub fn checker_release_manifest_path(&self) -> &str {
        &self.parsed.checker.release_manifest_path
    }

    pub fn checker_policy_path(&self) -> &str {
        &self.parsed.checker.policy_path
    }

    pub fn candidate_digest_hex(&self) -> &str {
        &self.parsed.cases[self.case_index].raw_answer_sha256
    }

    pub fn submission_source_digest_hex(&self) -> &str {
        &self.parsed.cases[self.case_index].submission_source_sha256
    }

    pub fn case_id(&self) -> &str {
        &self.parsed.cases[self.case_index].case_id
    }

    pub fn max_checker_executions(&self) -> u8 {
        self.parsed.cases[self.case_index].max_checker_executions
    }

    pub fn registry_path(&self) -> &str {
        &self.parsed.registry.path
    }

    pub fn registry_version(&self) -> &str {
        &self.parsed.registry.version
    }

    pub fn registry_digest_hex(&self) -> &str {
        &self.parsed.registry.sha256
    }

    pub fn task_digest_hex(&self) -> &str {
        &self.parsed.task.sha256
    }

    pub fn anchor_digest_hex(&self) -> &str {
        &self.parsed.anchor.sha256
    }

    pub fn checker_release_manifest_digest_hex(&self) -> &str {
        &self.parsed.checker.release_manifest_sha256
    }

    pub fn checker_policy_digest_hex(&self) -> &str {
        &self.parsed.checker.policy_sha256
    }

    pub fn execution_policy_path(&self) -> &str {
        &self.parsed.execution_policy.path
    }

    pub fn execution_policy_digest_hex(&self) -> &str {
        &self.parsed.execution_policy.sha256
    }

    pub fn toolchain_identity_path(&self) -> &str {
        &self.parsed.toolchain_identity.path
    }

    pub fn toolchain_identity_digest_hex(&self) -> &str {
        &self.parsed.toolchain_identity.sha256
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        submission_digest_hex, verify_authority_bundle, ExecutionRequest, ExecutionRequestFields,
        TRACKED_EXECUTION_POLICY_BYTES, TRACKED_REGISTRY_BYTES, TRACKED_TOOLCHAIN_IDENTITY_BYTES,
    };
    fn authority() -> VerifiedAuthorityBundle {
        verify_authority_bundle(
            TRACKED_REGISTRY_BYTES,
            TRACKED_EXECUTION_POLICY_BYTES,
            TRACKED_TOOLCHAIN_IDENTITY_BYTES,
        )
        .expect("tracked disabled authority")
    }

    fn exact_request_fields(
        raw: &[u8],
        source: &[u8],
        operation_id_hex: &str,
        epoch: u64,
    ) -> ExecutionRequestFields {
        ExecutionRequestFields {
            nonce_hex: "11".repeat(32),
            operation_id_hex: operation_id_hex.to_string(),
            family_version: FAMILY_VERSION.to_string(),
            template_id: TEMPLATE_ID.to_string(),
            challenge_sha256: CHALLENGE_SHA256.to_string(),
            epoch,
            raw_answer_base64: BASE64_STANDARD.encode(raw),
            submission_source_base64: BASE64_STANDARD.encode(source),
            submission_source_digest_hex: sha256_hex(source),
            candidate_digest_hex: sha256_hex(raw),
            submission_digest_hex: String::new(),
            registry_version: REGISTRY_VERSION.to_string(),
            registry_digest_hex: sha256_hex(TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES),
            anchor_digest_hex: sha256_hex(TRACKED_REAL_HISTORY_ANCHOR_BYTES),
            task_digest_hex: sha256_hex(TRACKED_REAL_HISTORY_TASK_BYTES),
            checker_artifact_hash_hex: CHECKER_ARTIFACT_HASH.to_string(),
            checker_policy_digest_hex: sha256_hex(TRACKED_CHECKER_POLICY_BYTES),
            checker_release_manifest_digest_hex: sha256_hex(TRACKED_CHECKER_RELEASE_MANIFEST_BYTES),
            toolchain_identity_digest_hex: sha256_hex(TRACKED_TOOLCHAIN_IDENTITY_BYTES),
            execution_policy_digest_hex: sha256_hex(TRACKED_EXECUTION_POLICY_BYTES),
            intake_version: INTAKE_VERSION.to_string(),
        }
    }

    fn request_from_fields(mut fields: ExecutionRequestFields, raw: &[u8]) -> ExecutionRequest {
        fields.submission_digest_hex = submission_digest_hex(
            &fields.family_version,
            &fields.template_id,
            &fields.challenge_sha256,
            fields.epoch,
            raw,
        )
        .expect("submission digest");
        ExecutionRequest::try_new(fields).expect("exact replay request")
    }

    fn exact_request(
        raw: &[u8],
        source: &[u8],
        operation_id_hex: &str,
        epoch: u64,
    ) -> ExecutionRequest {
        request_from_fields(
            exact_request_fields(raw, source, operation_id_hex, epoch),
            raw,
        )
    }

    #[test]
    fn exact_grant_yields_a_closed_local_bounded_replay_capability() {
        let grant = verify_closed_local_replay_grant_bytes(
            TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
            TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
            &authority(),
        )
        .expect("exact grant");

        assert_eq!(grant.max_matrix_requests_total(), 4);
        assert_eq!(grant.max_checker_executions_total(), 3);
        assert!(grant.named_linux_verification_replay_only());
        assert!(grant.loopback_only());
        assert!(!grant.p2p_allowed());
        assert!(!grant.consensus_allowed());
        assert!(!grant.reward_allowed());
        assert!(!grant.mineable_now());
        assert!(!grant.activation_allowed());
        assert!(grant.non_issuable());
        assert_eq!(
            grant.registry_digest_hex(),
            sha256_hex(TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES)
        );
        assert_eq!(
            grant.production_registry_digest_hex(),
            sha256_hex(TRACKED_REGISTRY_BYTES)
        );
    }

    #[test]
    fn exact_execute_request_yields_request_bound_executor_capability() {
        let grant = verify_closed_local_replay_grant_bytes(
            TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
            TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
            &authority(),
        )
        .expect("exact grant");

        let authorization = grant
            .authorize_execution_request(&exact_request(
                TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES,
                trimmed_utf8(TRACKED_REAL_HISTORY_ACCEPTED_BYTES).expect("accepted source"),
                "746bba0847458159f16dfe79d19958d2f44d1de7b67f946b1831207586b978be",
                0,
            ))
            .expect("all authority fields must match before executor invocation");

        assert_eq!(authorization.case_id(), "accepted");
        assert_eq!(authorization.max_checker_executions(), 1);
        assert_eq!(
            authorization.operation_id_hex(),
            "746bba0847458159f16dfe79d19958d2f44d1de7b67f946b1831207586b978be"
        );
        assert_eq!(authorization.task_path(), INSTALLED_TASK_PATH);
        assert_eq!(authorization.anchor_path(), INSTALLED_ANCHOR_PATH);
        assert_eq!(authorization.checker_path(), INSTALLED_CHECKER_PATH);
        assert_eq!(authorization.task_bytes(), TRACKED_REAL_HISTORY_TASK_BYTES);
        assert_eq!(
            authorization.anchor_bytes(),
            TRACKED_REAL_HISTORY_ANCHOR_BYTES
        );
        assert_eq!(
            authorization.candidate_digest_hex(),
            sha256_hex(TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES)
        );
        assert_eq!(
            authorization.submission_source_digest_hex(),
            sha256_hex(trimmed_utf8(TRACKED_REAL_HISTORY_ACCEPTED_BYTES).expect("accepted source"))
        );
    }

    #[test]
    fn node_can_prepare_an_exact_case_without_spending_its_one_shot_authorization() {
        let grant = verify_closed_local_replay_grant_bytes(
            TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
            TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
            &authority(),
        )
        .expect("exact grant");
        let source = trimmed_utf8(TRACKED_REAL_HISTORY_ACCEPTED_BYTES).expect("accepted source");
        let request = exact_request(
            TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES,
            source,
            "746bba0847458159f16dfe79d19958d2f44d1de7b67f946b1831207586b978be",
            0,
        );

        let prepared = grant
            .prepare_execution_case(ClosedLocalReplaySubmissionFields {
                family_version: FAMILY_VERSION,
                template_id: TEMPLATE_ID,
                challenge_sha256: CHALLENGE_SHA256,
                epoch: 0,
                candidate_digest_hex: &sha256_hex(TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES),
                submission_source_digest_hex: &sha256_hex(source),
            })
            .expect("exact raw submission prepares the accepted replay case");

        assert_eq!(prepared.case_id(), "accepted");
        assert_eq!(
            prepared.operation_id_hex(),
            "746bba0847458159f16dfe79d19958d2f44d1de7b67f946b1831207586b978be"
        );

        let first = grant
            .authorize_prepared_execution_request(prepared, &request)
            .expect("the permit-stage authorization spends the case once");
        assert_eq!(first.case_id(), "accepted");
        assert!(matches!(
            grant.authorize_execution_request(&request),
            Err(ClosedLocalReplayGrantError::CaseAlreadyAuthorized)
        ));
    }

    #[test]
    fn prepared_case_builds_the_complete_node_owned_execute_authority() {
        let grant = verify_closed_local_replay_grant_bytes(
            TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
            TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
            &authority(),
        )
        .expect("exact grant");
        let source = trimmed_utf8(TRACKED_REAL_HISTORY_ACCEPTED_BYTES).expect("accepted source");
        let prepared = grant
            .prepare_execution_case(ClosedLocalReplaySubmissionFields {
                family_version: FAMILY_VERSION,
                template_id: TEMPLATE_ID,
                challenge_sha256: CHALLENGE_SHA256,
                epoch: 0,
                candidate_digest_hex: &sha256_hex(TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES),
                submission_source_digest_hex: &sha256_hex(source),
            })
            .expect("exact case prepares");

        let request = prepared
            .build_execution_request(
                &"11".repeat(32),
                TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES,
                source,
            )
            .expect("prepared authority builds the only exact Execute request");

        assert_eq!(request.operation_id_hex(), prepared.operation_id_hex());
        assert_eq!(request.family_version(), FAMILY_VERSION);
        assert_eq!(request.epoch(), 0);
        assert_eq!(
            request.registry_digest_hex(),
            sha256_hex(TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES)
        );
        grant
            .authorize_prepared_execution_request(prepared, &request)
            .expect("the derived request authorizes once");
    }

    #[test]
    fn one_grant_authorizes_each_fixed_case_at_most_once() {
        let grant = verify_closed_local_replay_grant_bytes(
            TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
            TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
            &authority(),
        )
        .expect("exact grant");
        let request = exact_request(
            TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES,
            trimmed_utf8(TRACKED_REAL_HISTORY_ACCEPTED_BYTES).expect("accepted source"),
            "746bba0847458159f16dfe79d19958d2f44d1de7b67f946b1831207586b978be",
            0,
        );

        grant
            .authorize_execution_request(&request)
            .expect("first authorization");
        assert!(matches!(
            grant.authorize_execution_request(&request),
            Err(ClosedLocalReplayGrantError::CaseAlreadyAuthorized)
        ));
    }

    #[test]
    fn only_three_checker_cases_are_authorized_and_empty_stops_at_pre_intake() {
        let grant = verify_closed_local_replay_grant_bytes(
            TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
            TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
            &authority(),
        )
        .expect("exact grant");
        for (case_id, operation_id, raw, source, epoch) in [
            (
                "accepted",
                "746bba0847458159f16dfe79d19958d2f44d1de7b67f946b1831207586b978be",
                TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES,
                trimmed_utf8(TRACKED_REAL_HISTORY_ACCEPTED_BYTES).expect("accepted source"),
                0,
            ),
            (
                "tampered",
                "99407a29b50c4c378f7b49d423515eb584f961ae005ed510723e676d1c7121cd",
                TRACKED_REAL_HISTORY_TAMPERED_RAW_BYTES,
                trimmed_utf8(TRACKED_REAL_HISTORY_TAMPERED_BYTES).expect("tampered source"),
                1,
            ),
            (
                "constant",
                "3558b66ce8d5ce7549e7ddb951ed6452cfa9ca575515bd8bb30598eda6f00b61",
                TRACKED_REAL_HISTORY_CONSTANT_RAW_BYTES,
                trimmed_utf8(TRACKED_REAL_HISTORY_CONSTANT_BYTES).expect("constant source"),
                2,
            ),
        ] {
            let authorization = grant
                .authorize_execution_request(&exact_request(raw, source, operation_id, epoch))
                .expect("pre-registered case");
            assert_eq!(authorization.case_id(), case_id);
            assert_eq!(authorization.epoch(), epoch);
            assert_eq!(authorization.max_checker_executions(), 1);
        }

        let empty = exact_request(
            TRACKED_REAL_HISTORY_EMPTY_RAW_BYTES,
            b"",
            "c5d9705f2367515cedc06639217b95a40f5a98e377089b49ed97c0e7ec8b3038",
            3,
        );
        assert!(matches!(
            grant.authorize_execution_request(&empty),
            Err(ClosedLocalReplayGrantError::PreIntakeOnlyCase)
        ));

        let grant = verify_closed_local_replay_grant_bytes(
            TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
            TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
            &authority(),
        )
        .expect("exact grant");
        let unregistered = exact_request(
            b"not one of the frozen matrix candidates",
            b"not one of the frozen matrix candidates",
            "ff6bba0847458159f16dfe79d19958d2f44d1de7b67f946b1831207586b978be",
            9,
        );
        assert_eq!(
            grant
                .authorize_execution_request(&unregistered)
                .expect_err("unregistered operation must fail"),
            ClosedLocalReplayGrantError::RequestBinding("operationIdHex")
        );

        let grant = verify_closed_local_replay_grant_bytes(
            TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
            TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
            &authority(),
        )
        .expect("exact grant");
        let crossed = exact_request(
            TRACKED_REAL_HISTORY_TAMPERED_RAW_BYTES,
            trimmed_utf8(TRACKED_REAL_HISTORY_TAMPERED_BYTES).expect("tampered source"),
            "746bba0847458159f16dfe79d19958d2f44d1de7b67f946b1831207586b978be",
            0,
        );
        assert_eq!(
            grant
                .authorize_execution_request(&crossed)
                .expect_err("operation/candidate cross-pair must fail"),
            ClosedLocalReplayGrantError::RequestBinding("candidateDigestHex")
        );
    }

    #[test]
    fn exact_empty_case_is_consumed_at_pre_intake_without_an_execute_request() {
        let grant = verify_closed_local_replay_grant_bytes(
            TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
            TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
            &authority(),
        )
        .expect("exact grant");
        let fields = ClosedLocalReplayPreIntakeFields {
            family_version: FAMILY_VERSION,
            template_id: TEMPLATE_ID,
            challenge_sha256: CHALLENGE_SHA256,
            epoch: 3,
            candidate_digest_hex: &sha256_hex(TRACKED_REAL_HISTORY_EMPTY_RAW_BYTES),
            reason: ClosedLocalReplayPreIntakeReason::EmptyResponse,
        };

        let authorization = grant
            .authorize_pre_intake_case(fields)
            .expect("the fixed empty response is the sole pre-intake case");
        assert_eq!(authorization.case_id(), "empty");
        assert_eq!(authorization.epoch(), 3);
        assert_eq!(authorization.max_checker_executions(), 0);

        assert!(matches!(
            grant.authorize_pre_intake_case(fields),
            Err(ClosedLocalReplayGrantError::CaseAlreadyAuthorized)
        ));
    }

    #[test]
    fn every_execute_authority_field_must_match_before_authorization() {
        type Mutation = fn(&mut ExecutionRequestFields);
        let mutations: [(&str, Mutation); 15] = [
            ("operationIdHex", |fields| {
                fields.operation_id_hex = "aa".repeat(32)
            }),
            ("familyVersion", |fields| {
                fields.family_version.push_str("-drift")
            }),
            ("templateId", |fields| fields.template_id = "aa".repeat(32)),
            ("challengeSha256", |fields| {
                fields.challenge_sha256 = "aa".repeat(32)
            }),
            ("epoch", |fields| fields.epoch = 1),
            ("submissionSourceDigestHex", |fields| {
                let source = b"different extracted source";
                fields.submission_source_base64 = BASE64_STANDARD.encode(source);
                fields.submission_source_digest_hex = sha256_hex(source);
            }),
            ("registryVersion", |fields| {
                fields.registry_version.push_str("-drift")
            }),
            ("registryDigestHex", |fields| {
                fields.registry_digest_hex = "aa".repeat(32)
            }),
            ("anchorDigestHex", |fields| {
                fields.anchor_digest_hex = "aa".repeat(32)
            }),
            ("taskDigestHex", |fields| {
                fields.task_digest_hex = "aa".repeat(32)
            }),
            ("checkerArtifactHashHex", |fields| {
                fields.checker_artifact_hash_hex = "aa".repeat(32)
            }),
            ("checkerPolicyDigestHex", |fields| {
                fields.checker_policy_digest_hex = "aa".repeat(32)
            }),
            ("checkerReleaseManifestDigestHex", |fields| {
                fields.checker_release_manifest_digest_hex = "aa".repeat(32)
            }),
            ("toolchainIdentityDigestHex", |fields| {
                fields.toolchain_identity_digest_hex = "aa".repeat(32)
            }),
            ("executionPolicyDigestHex", |fields| {
                fields.execution_policy_digest_hex = "aa".repeat(32)
            }),
        ];
        let operation = "746bba0847458159f16dfe79d19958d2f44d1de7b67f946b1831207586b978be";
        for (expected_field, mutate) in mutations {
            let mut fields = exact_request_fields(
                TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES,
                trimmed_utf8(TRACKED_REAL_HISTORY_ACCEPTED_BYTES).expect("accepted source"),
                operation,
                0,
            );
            mutate(&mut fields);
            let request = request_from_fields(fields, TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES);
            let grant = verify_closed_local_replay_grant_bytes(
                TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
                TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
                &authority(),
            )
            .expect("exact grant");
            assert_eq!(
                grant
                    .authorize_execution_request(&request)
                    .expect_err("authority drift must fail"),
                ClosedLocalReplayGrantError::RequestBinding(expected_field),
                "drifted field: {expected_field}"
            );
        }

        let mut intake = exact_request_fields(
            TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES,
            trimmed_utf8(TRACKED_REAL_HISTORY_ACCEPTED_BYTES).expect("accepted source"),
            operation,
            0,
        );
        intake.intake_version.push_str("-drift");
        let request = request_from_fields(intake, TRACKED_REAL_HISTORY_ACCEPTED_RAW_BYTES);
        let grant = verify_closed_local_replay_grant_bytes(
            TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES,
            TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
            &authority(),
        )
        .expect("exact grant");
        assert_eq!(
            grant
                .authorize_execution_request(&request)
                .expect_err("intake drift must fail"),
            ClosedLocalReplayGrantError::RequestBinding("intakeVersion")
        );
    }

    #[test]
    fn any_grant_byte_or_json_field_mutation_is_rejected_before_parsing() {
        let mut byte_drift = TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES.to_vec();
        *byte_drift.last_mut().expect("grant bytes") ^= 1;
        assert_eq!(
            verify_closed_local_replay_grant_bytes(
                &byte_drift,
                TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
                &authority(),
            )
            .expect_err("byte drift must fail"),
            ClosedLocalReplayGrantError::ByteMismatch
        );

        let mut field_drift: serde_json::Value =
            serde_json::from_slice(TRACKED_CLOSED_LOCAL_REPLAY_GRANT_BYTES).expect("grant JSON");
        field_drift["scope"]["p2pAllowed"] = serde_json::json!(true);
        assert_eq!(
            verify_closed_local_replay_grant_bytes(
                &serde_json::to_vec(&field_drift).expect("field drift"),
                TRACKED_CLOSED_LOCAL_REPLAY_REGISTRY_OVERLAY_BYTES,
                &authority(),
            )
            .expect_err("field drift must fail exact-byte verification"),
            ClosedLocalReplayGrantError::ByteMismatch
        );
    }
}
