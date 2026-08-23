//! Fixed launcher startup prerequisites that must pass before the lifetime
//! lock, cgroup recovery, or socket work can begin.

use boole_native_shadow_protocol::{ResolvedServiceIdentities, VerifiedAuthorityBundle};
use thiserror::Error;

use crate::privilege::VerifiedLauncherPrivilege;

/// Opaque proof that the calling launcher thread passed the three fixed,
/// input-free checks allowed before acquiring the launcher lifetime lock.
///
/// The privilege proof is retained so this aggregate remains bound to the OS
/// thread that performed the checks. It deliberately exposes no constructor
/// or fields and is not a readiness or recovery proof.
///
/// ```compile_fail
/// use boole_native_shadow_launcher::startup::VerifiedLauncherPrelockPrerequisites;
/// let _forged = VerifiedLauncherPrelockPrerequisites {};
/// ```
///
/// ```compile_fail
/// let proof = boole_native_shadow_launcher::startup::verify_fixed_launcher_prelock_prerequisites()
///     .expect("launcher pre-lock prerequisites");
/// std::thread::spawn(move || drop(proof));
/// ```
#[must_use]
#[derive(Debug)]
#[allow(dead_code)]
pub struct VerifiedLauncherPrelockPrerequisites {
    privilege: VerifiedLauncherPrivilege,
    authority: VerifiedAuthorityBundle,
    identities: ResolvedServiceIdentities,
}

impl VerifiedLauncherPrelockPrerequisites {
    #[cfg(target_os = "linux")]
    pub(crate) fn node_gid(&self) -> u32 {
        self.identities.node_gid()
    }
}

#[derive(Debug, Error)]
pub enum LauncherPrelockError {
    #[error("native-shadow launcher pre-lock verification requires Linux")]
    UnsupportedPlatform,
    #[cfg(target_os = "linux")]
    #[error("launcher privilege prerequisite failed: {0}")]
    Privilege(#[source] crate::privilege::LauncherPrivilegeError),
    #[cfg(target_os = "linux")]
    #[error("installed launcher authority prerequisite failed: {0}")]
    Authority(#[source] boole_native_shadow_protocol::installed_authority::InstalledAuthorityError),
    #[cfg(target_os = "linux")]
    #[error("fixed launcher service-identity prerequisite failed: {0}")]
    Identity(#[source] boole_native_shadow_protocol::IdentityResolutionError),
}

/// Verify the three fixed launcher prerequisites, in order, without accepting
/// a path, account name, numeric identity, policy, or capability set.
///
/// This function performs no lock, random-ID, cgroup, listener, journal,
/// route, or child-process operation.
pub fn verify_fixed_launcher_prelock_prerequisites(
) -> Result<VerifiedLauncherPrelockPrerequisites, LauncherPrelockError> {
    #[cfg(target_os = "linux")]
    {
        let (privilege, authority, identities) = run_prelock_steps(
            || {
                crate::privilege::verify_fixed_launcher_privilege()
                    .map_err(LauncherPrelockError::Privilege)
            },
            || {
                boole_native_shadow_protocol::installed_authority::open_verified_installed_authority_bundle()
                    .map_err(LauncherPrelockError::Authority)
            },
            || {
                boole_native_shadow_protocol::resolve_fixed_service_identities()
                    .map_err(LauncherPrelockError::Identity)
            },
        )?;
        Ok(VerifiedLauncherPrelockPrerequisites {
            privilege,
            authority,
            identities,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(LauncherPrelockError::UnsupportedPlatform)
    }
}

#[cfg(any(target_os = "linux", test))]
fn run_prelock_steps<P, A, I, E>(
    verify_privilege: impl FnOnce() -> Result<P, E>,
    verify_authority: impl FnOnce() -> Result<A, E>,
    resolve_identities: impl FnOnce() -> Result<I, E>,
) -> Result<(P, A, I), E> {
    let privilege = verify_privilege()?;
    let authority = verify_authority()?;
    let identities = resolve_identities()?;
    Ok((privilege, authority, identities))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::run_prelock_steps;

    #[test]
    fn prerequisites_run_exactly_once_in_the_frozen_order() {
        let events = RefCell::new(Vec::new());
        let result: Result<_, ()> = run_prelock_steps(
            || {
                events.borrow_mut().push("privilege");
                Ok(1_u8)
            },
            || {
                events.borrow_mut().push("authority");
                Ok(2_u8)
            },
            || {
                events.borrow_mut().push("identities");
                Ok(3_u8)
            },
        );

        assert_eq!(result, Ok((1, 2, 3)));
        assert_eq!(
            events.into_inner(),
            ["privilege", "authority", "identities"]
        );
    }

    #[test]
    fn privilege_failure_prevents_authority_and_identity_work() {
        let events = RefCell::new(Vec::new());
        let result = run_prelock_steps(
            || {
                events.borrow_mut().push("privilege");
                Err::<(), _>("privilege")
            },
            || {
                events.borrow_mut().push("authority");
                Ok(())
            },
            || {
                events.borrow_mut().push("identities");
                Ok(())
            },
        );

        assert_eq!(result, Err("privilege"));
        assert_eq!(events.into_inner(), ["privilege"]);
    }

    #[test]
    fn authority_failure_prevents_identity_work() {
        let events = RefCell::new(Vec::new());
        let result = run_prelock_steps(
            || {
                events.borrow_mut().push("privilege");
                Ok(())
            },
            || {
                events.borrow_mut().push("authority");
                Err::<(), _>("authority")
            },
            || {
                events.borrow_mut().push("identities");
                Ok(())
            },
        );

        assert_eq!(result, Err("authority"));
        assert_eq!(events.into_inner(), ["privilege", "authority"]);
    }

    #[test]
    fn identity_failure_does_not_issue_a_token() {
        let events = RefCell::new(Vec::new());
        let result = run_prelock_steps(
            || {
                events.borrow_mut().push("privilege");
                Ok(())
            },
            || {
                events.borrow_mut().push("authority");
                Ok(())
            },
            || {
                events.borrow_mut().push("identities");
                Err::<(), _>("identities")
            },
        );

        assert_eq!(result, Err("identities"));
        assert_eq!(
            events.into_inner(),
            ["privilege", "authority", "identities"]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires exact root capabilities, installed authority, and fixed NSS accounts"]
    fn real_linux_prelock_prerequisites_match_the_frozen_host_contract() {
        let _proof = super::verify_fixed_launcher_prelock_prerequisites()
            .unwrap_or_else(|error| panic!("launcher pre-lock verification failed: {error}"));
    }
}
