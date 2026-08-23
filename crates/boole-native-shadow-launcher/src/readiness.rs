//! Assembly of request-free qualification readiness after every fixed
//! launcher startup prerequisite has passed.

use std::num::NonZeroU32;

use thiserror::Error;

use crate::qualification::VerifiedQualificationStartup;
use crate::toolchain_compatibility::VerifiedStartupToolchainCompatibility;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum QualificationStartupError {
    #[error("kernel returned an impossible zero launcher PID")]
    ZeroLauncherPid,
}

/// Consume the complete fixed startup/toolchain proof and assemble the only
/// token that may serve the disabled qualification handshake.
///
/// The returned token retains the complete proof chain, including the
/// lifetime lock and verified cgroup directory descriptors. No caller can
/// select a path, identity, digest, PID or activation policy.
pub fn assemble_fixed_qualification_startup(
    compatibility: VerifiedStartupToolchainCompatibility,
) -> Result<VerifiedQualificationStartup, QualificationStartupError> {
    let startup = VerifiedQualificationStartup::from_verified_toolchain(compatibility)?;
    let _retained_proof = startup.verified_toolchain();
    Ok(startup)
}

pub(crate) fn require_nonzero_launcher_pid(
    pid: u32,
) -> Result<NonZeroU32, QualificationStartupError> {
    NonZeroU32::new(pid).ok_or(QualificationStartupError::ZeroLauncherPid)
}

#[cfg(test)]
mod tests {
    use super::{require_nonzero_launcher_pid, QualificationStartupError};

    #[test]
    fn launcher_pid_must_be_nonzero_without_a_fallback() {
        assert_eq!(
            require_nonzero_launcher_pid(0),
            Err(QualificationStartupError::ZeroLauncherPid)
        );
        assert_eq!(
            require_nonzero_launcher_pid(42)
                .expect("a non-zero kernel PID is retained")
                .get(),
            42
        );
    }
}
