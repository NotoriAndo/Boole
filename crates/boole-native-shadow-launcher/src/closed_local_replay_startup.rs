//! Request-bound startup proof for the exact closed-local replay executor.
//!
//! The fixed executor accepts only the non-cloneable permit minted here.  The
//! permit combines startup toolchain identity, installed replay authority,
//! per-request checker-release revalidation and a duplicated descriptor for
//! the exact verified runtime rootfs.  A wire request alone is never execution
//! authority.

use boole_native_shadow_protocol::installed_authority::{
    InstalledAuthorityError, VerifiedInstalledClosedLocalReplayExecutionAuthorities,
    VerifiedInstalledClosedLocalReplayExecutionMaterials,
};
use boole_native_shadow_protocol::{
    ClosedLocalReplayGrantError, VerifiedClosedLocalReplayAuthorization, WireError,
};
use thiserror::Error;

#[cfg(target_os = "linux")]
use boole_native_shadow_protocol::installed_authority::open_verified_installed_closed_local_replay_execution_authorities;
#[cfg(target_os = "linux")]
use boole_native_shadow_protocol::{
    sha256_hex, ClosedLocalReplaySubmissionFields, ExecutionRequest, ResolvedServiceIdentities,
    VerifiedClosedLocalReplayExecutionAuthority,
};

use crate::qualification::VerifiedQualificationStartup;
use crate::runtime_rootfs_replay::VerifiedRuntimeRootfsReplay;
use crate::toolchain_compatibility::{
    ToolchainProbeFailure, VerifiedStartupToolchainCompatibility,
};

#[cfg(target_os = "linux")]
use std::os::fd::OwnedFd;

#[derive(Debug, Error)]
pub enum ClosedLocalReplayStartupError {
    #[cfg(feature = "fresh-answer-canary")]
    #[error(transparent)]
    Canary(#[from] boole_native_shadow_protocol::fresh_answer_canary::CanaryError),
    #[error("closed-local replay launcher is permanently poisoned")]
    Poisoned,
    #[error(transparent)]
    InstalledAuthority(#[from] InstalledAuthorityError),
    #[error(transparent)]
    ReplayGrant(#[from] ClosedLocalReplayGrantError),
    #[error(transparent)]
    Wire(#[from] WireError),
    #[error("execution-time toolchain or manager identity drifted: {0}")]
    Toolchain(#[from] ToolchainProbeFailure),
    #[error("runtime rootfs replay identity drifted: {0}")]
    Rootfs(String),
    #[error("closed-local replay qualification startup failed: {0}")]
    Qualification(#[from] crate::readiness::QualificationStartupError),
}

/// Assemble the only complete startup proof accepted by the bounded replay
/// listener. Installed grant/checker authority is opened from its fixed
/// root-owned path; callers can provide neither paths nor checker commands.
#[cfg(target_os = "linux")]
pub fn assemble_verified_closed_local_replay_startup(
    compatibility: VerifiedStartupToolchainCompatibility,
    rootfs: VerifiedRuntimeRootfsReplay,
) -> Result<VerifiedClosedLocalReplayStartup, ClosedLocalReplayStartupError> {
    let installed = open_verified_installed_closed_local_replay_execution_authorities()?;
    let qualification = VerifiedQualificationStartup::from_verified_toolchain(compatibility)?;
    Ok(VerifiedClosedLocalReplayStartup::new(
        qualification,
        installed,
        rootfs,
    ))
}

/// Complete startup authority retained for the whole bounded replay service.
/// No caller-selected path, checker or command is stored here.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub struct VerifiedClosedLocalReplayStartup {
    qualification: VerifiedQualificationStartup,
    installed: VerifiedInstalledClosedLocalReplayExecutionAuthorities,
    rootfs: VerifiedRuntimeRootfsReplay,
    poisoned: bool,
    #[cfg(all(target_os = "linux", feature = "fresh-answer-canary"))]
    canary: Option<CanaryStartup>,
}

#[cfg(all(target_os = "linux", feature = "fresh-answer-canary"))]
struct CanaryStartup {
    installed: boole_native_shadow_protocol::installed_authority::InstalledCanaryAuthority,
    budget: boole_native_shadow_protocol::fresh_answer_canary::CanaryBudget,
}

#[cfg(all(target_os = "linux", feature = "fresh-answer-canary"))]
pub struct VerifiedFreshAnswerCanaryStartup(VerifiedClosedLocalReplayStartup);

#[cfg(all(target_os = "linux", feature = "fresh-answer-canary"))]
impl VerifiedFreshAnswerCanaryStartup {
    pub(crate) fn into_inner(self) -> VerifiedClosedLocalReplayStartup {
        self.0
    }
}

/// Explicit development entrypoint. It keeps every installed replay material,
/// toolchain and containment check, but selects a separate signed canary grant.
#[cfg(all(target_os = "linux", feature = "fresh-answer-canary"))]
pub fn assemble_verified_fresh_answer_canary_startup(
    compatibility: VerifiedStartupToolchainCompatibility,
    rootfs: VerifiedRuntimeRootfsReplay,
) -> Result<VerifiedFreshAnswerCanaryStartup, ClosedLocalReplayStartupError> {
    use boole_native_shadow_protocol::{
        fresh_answer_canary::CanaryBudgetRole, installed_authority::open_installed_canary,
    };
    let mut startup = assemble_verified_closed_local_replay_startup(compatibility, rootfs)?;
    let installed = open_installed_canary()?;
    let (budget, _) = installed.open_budget(CanaryBudgetRole::Launcher, 0, 0)?;
    startup.canary = Some(CanaryStartup { installed, budget });
    Ok(VerifiedFreshAnswerCanaryStartup(startup))
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl VerifiedClosedLocalReplayStartup {
    #[cfg(target_os = "linux")]
    pub(crate) fn new(
        qualification: VerifiedQualificationStartup,
        installed: VerifiedInstalledClosedLocalReplayExecutionAuthorities,
        rootfs: VerifiedRuntimeRootfsReplay,
    ) -> Self {
        Self {
            qualification,
            installed,
            rootfs,
            poisoned: false,
            #[cfg(feature = "fresh-answer-canary")]
            canary: None,
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn execution_authority(&self) -> &VerifiedClosedLocalReplayExecutionAuthority {
        self.installed.execution_authority()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn identities(&self) -> ResolvedServiceIdentities {
        self.qualification
            .verified_toolchain()
            .recovery()
            .manager()
            .instance()
            .lifetime_lock()
            .prerequisites()
            .identities()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn launcher_instance_id_hex(&self) -> String {
        hex::encode(
            self.qualification
                .verified_toolchain()
                .recovery()
                .manager()
                .instance()
                .instance_id(),
        )
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn runtime_directory(&self) -> &std::fs::File {
        self.qualification
            .verified_toolchain()
            .recovery()
            .manager()
            .instance()
            .lifetime_lock()
            .runtime_directory()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn poison(&mut self) {
        self.poisoned = true;
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn qualification_startup(&self) -> &VerifiedQualificationStartup {
        &self.qualification
    }

    /// Mint the only value accepted by the fixed executor.  Every retryable
    /// revalidation occurs before the one-shot grant case is spent.
    #[cfg(target_os = "linux")]
    pub(crate) fn authorize_for_execution<'a>(
        &'a mut self,
        request: &ExecutionRequest,
    ) -> Result<VerifiedClosedLocalReplayExecutionPermit<'a>, ClosedLocalReplayStartupError> {
        if self.poisoned {
            return Err(ClosedLocalReplayStartupError::Poisoned);
        }
        self.qualification
            .verified_toolchain()
            .reverify_for_execution()?;
        let installed_materials = self.installed.reverify_execution_materials()?;
        self.rootfs
            .reverify_for_execution()
            .map_err(|error| ClosedLocalReplayStartupError::Rootfs(error.to_string()))?;
        let rootfs = self
            .rootfs
            .duplicate_directory_fd()
            .map_err(|error| ClosedLocalReplayStartupError::Rootfs(error.to_string()))?;
        let submission = request.submission_source()?;
        let submission_source_digest = sha256_hex(&submission);
        #[cfg(feature = "fresh-answer-canary")]
        if let Some(canary) = self.canary.as_mut() {
            let authorization = canary
                .budget
                .reserve_execution(canary.installed.grant(), request)?;
            return Ok(VerifiedClosedLocalReplayExecutionPermit {
                compatibility: self.qualification.verified_toolchain(),
                authorization: CheckerAuthorization::Canary(authorization),
                installed_materials,
                rootfs,
                submission,
            });
        }
        let prepared =
            self.installed
                .grant()
                .prepare_execution_case(ClosedLocalReplaySubmissionFields {
                    family_version: request.family_version(),
                    template_id: request.template_id(),
                    challenge_sha256: request.challenge_sha256(),
                    epoch: request.epoch(),
                    candidate_digest_hex: request.candidate_digest_hex(),
                    submission_source_digest_hex: &submission_source_digest,
                })?;
        let authorization = self
            .installed
            .grant()
            .authorize_prepared_execution_request(prepared, request)?;
        Ok(VerifiedClosedLocalReplayExecutionPermit {
            compatibility: self.qualification.verified_toolchain(),
            authorization: CheckerAuthorization::Replay(authorization),
            installed_materials,
            rootfs,
            submission,
        })
    }
}

/// Non-cloneable one-request execution authority.  Its fields are private and
/// the value is consumed by the executor.
pub(crate) struct VerifiedClosedLocalReplayExecutionPermit<'a> {
    compatibility: &'a VerifiedStartupToolchainCompatibility,
    authorization: CheckerAuthorization,
    installed_materials: VerifiedInstalledClosedLocalReplayExecutionMaterials,
    #[cfg(target_os = "linux")]
    rootfs: OwnedFd,
    submission: Vec<u8>,
}

pub(crate) struct ClosedLocalReplayExecutionPermitParts<'a> {
    pub(crate) compatibility: &'a VerifiedStartupToolchainCompatibility,
    pub(crate) authorization: CheckerAuthorization,
    pub(crate) installed_materials: VerifiedInstalledClosedLocalReplayExecutionMaterials,
    #[cfg(target_os = "linux")]
    pub(crate) rootfs: OwnedFd,
    pub(crate) submission: Vec<u8>,
}

/// The fresh capability is not converted into a replay authorization. Both
/// variants require the same request-bound containment startup proof.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) enum CheckerAuthorization {
    Replay(VerifiedClosedLocalReplayAuthorization),
    #[cfg(all(unix, feature = "fresh-answer-canary"))]
    Canary(boole_native_shadow_protocol::fresh_answer_canary::VerifiedCanaryExecutionAuthorization),
}

impl CheckerAuthorization {
    pub(crate) fn operation_id_hex(&self) -> &str {
        match self {
            Self::Replay(a) => a.operation_id_hex(),
            #[cfg(all(unix, feature = "fresh-answer-canary"))]
            Self::Canary(a) => a.request().operation_id_hex(),
        }
    }
    pub(crate) fn submission_source_digest_hex(&self) -> &str {
        match self {
            Self::Replay(a) => a.submission_source_digest_hex(),
            #[cfg(all(unix, feature = "fresh-answer-canary"))]
            Self::Canary(a) => a.request().submission_source_digest_hex(),
        }
    }
    pub(crate) fn max_checker_executions(&self) -> u8 {
        match self {
            Self::Replay(a) => a.max_checker_executions(),
            #[cfg(all(unix, feature = "fresh-answer-canary"))]
            Self::Canary(_) => 1,
        }
    }
    pub(crate) fn task_bytes(&self) -> &[u8] {
        match self {
            Self::Replay(a) => a.task_bytes(),
            #[cfg(all(unix, feature = "fresh-answer-canary"))]
            Self::Canary(a) => a.task_bytes(),
        }
    }
    pub(crate) fn anchor_bytes(&self) -> &[u8] {
        match self {
            Self::Replay(a) => a.anchor_bytes(),
            #[cfg(all(unix, feature = "fresh-answer-canary"))]
            Self::Canary(a) => a.anchor_bytes(),
        }
    }
}

impl<'a> VerifiedClosedLocalReplayExecutionPermit<'a> {
    pub(crate) fn into_parts(self) -> ClosedLocalReplayExecutionPermitParts<'a> {
        ClosedLocalReplayExecutionPermitParts {
            compatibility: self.compatibility,
            authorization: self.authorization,
            installed_materials: self.installed_materials,
            #[cfg(target_os = "linux")]
            rootfs: self.rootfs,
            submission: self.submission,
        }
    }
}
