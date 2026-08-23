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

use crate::runtime_rootfs_replay::VerifiedRuntimeRootfsReplay;
use crate::toolchain_compatibility::{
    ToolchainProbeFailure, VerifiedStartupToolchainCompatibility,
};

#[cfg(target_os = "linux")]
use std::os::fd::OwnedFd;

#[derive(Debug, Error)]
pub enum ClosedLocalReplayStartupError {
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
    Ok(VerifiedClosedLocalReplayStartup::new(
        compatibility,
        installed,
        rootfs,
    ))
}

/// Complete startup authority retained for the whole bounded replay service.
/// No caller-selected path, checker or command is stored here.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub struct VerifiedClosedLocalReplayStartup {
    compatibility: VerifiedStartupToolchainCompatibility,
    installed: VerifiedInstalledClosedLocalReplayExecutionAuthorities,
    rootfs: VerifiedRuntimeRootfsReplay,
    poisoned: bool,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl VerifiedClosedLocalReplayStartup {
    #[cfg(target_os = "linux")]
    pub(crate) fn new(
        compatibility: VerifiedStartupToolchainCompatibility,
        installed: VerifiedInstalledClosedLocalReplayExecutionAuthorities,
        rootfs: VerifiedRuntimeRootfsReplay,
    ) -> Self {
        Self {
            compatibility,
            installed,
            rootfs,
            poisoned: false,
        }
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn execution_authority(&self) -> &VerifiedClosedLocalReplayExecutionAuthority {
        self.installed.execution_authority()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn identities(&self) -> ResolvedServiceIdentities {
        self.compatibility
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
            self.compatibility
                .recovery()
                .manager()
                .instance()
                .instance_id(),
        )
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn runtime_directory(&self) -> &std::fs::File {
        self.compatibility
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
        self.compatibility.reverify_for_execution()?;
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
            compatibility: &self.compatibility,
            authorization,
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
    authorization: VerifiedClosedLocalReplayAuthorization,
    installed_materials: VerifiedInstalledClosedLocalReplayExecutionMaterials,
    #[cfg(target_os = "linux")]
    rootfs: OwnedFd,
    submission: Vec<u8>,
}

pub(crate) struct ClosedLocalReplayExecutionPermitParts<'a> {
    pub(crate) compatibility: &'a VerifiedStartupToolchainCompatibility,
    pub(crate) authorization: VerifiedClosedLocalReplayAuthorization,
    pub(crate) installed_materials: VerifiedInstalledClosedLocalReplayExecutionMaterials,
    #[cfg(target_os = "linux")]
    pub(crate) rootfs: OwnedFd,
    pub(crate) submission: Vec<u8>,
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
