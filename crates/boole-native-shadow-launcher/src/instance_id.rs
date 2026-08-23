//! One-shot launcher-instance identity bound to the lifetime lock.

use std::io;

use thiserror::Error;

use crate::lifetime_lock::LauncherLifetimeLockGuard;

const LAUNCHER_INSTANCE_ID_BYTES: usize = 32;

/// Opaque proof that one fresh launcher-instance ID was obtained while this
/// process still owns the fixed launcher lifetime lock.
///
/// ```compile_fail
/// use boole_native_shadow_launcher::instance_id::VerifiedLauncherInstance;
/// let _forged = VerifiedLauncherInstance {};
/// ```
///
/// ```compile_fail
/// use boole_native_shadow_launcher::{
///     instance_id::acquire_fresh_launcher_instance,
///     lifetime_lock::acquire_fixed_launcher_lifetime_lock,
///     startup::verify_fixed_launcher_prelock_prerequisites,
/// };
/// let proof = verify_fixed_launcher_prelock_prerequisites().unwrap();
/// let guard = acquire_fixed_launcher_lifetime_lock(proof).unwrap();
/// let _first = acquire_fresh_launcher_instance(guard);
/// let _second = acquire_fresh_launcher_instance(guard);
/// ```
///
/// ```compile_fail
/// use boole_native_shadow_launcher::{
///     instance_id::acquire_fresh_launcher_instance,
///     lifetime_lock::acquire_fixed_launcher_lifetime_lock,
///     startup::verify_fixed_launcher_prelock_prerequisites,
/// };
/// let proof = verify_fixed_launcher_prelock_prerequisites().unwrap();
/// let guard = acquire_fixed_launcher_lifetime_lock(proof).unwrap();
/// let instance = acquire_fresh_launcher_instance(guard).unwrap();
/// std::thread::spawn(move || drop(instance));
/// ```
///
/// ```compile_fail
/// use boole_native_shadow_launcher::{
///     instance_id::acquire_fresh_launcher_instance,
///     lifetime_lock::acquire_fixed_launcher_lifetime_lock,
///     startup::verify_fixed_launcher_prelock_prerequisites,
/// };
/// let proof = verify_fixed_launcher_prelock_prerequisites().unwrap();
/// let guard = acquire_fixed_launcher_lifetime_lock(proof).unwrap();
/// let instance = acquire_fresh_launcher_instance(guard).unwrap();
/// println!("{instance:?}");
/// ```
#[must_use]
#[allow(dead_code)]
pub struct VerifiedLauncherInstance {
    lifetime_lock: LauncherLifetimeLockGuard,
    instance_id: [u8; LAUNCHER_INSTANCE_ID_BYTES],
}

#[derive(Debug, Error)]
pub enum LauncherInstanceIdError {
    #[error("native-shadow launcher instance identity requires Linux")]
    UnsupportedPlatform,
    #[error("getrandom(2) failed while creating the launcher instance identity: {0}")]
    Getrandom(#[source] io::Error),
    #[error("getrandom(2) returned {actual} bytes instead of exactly 32")]
    ShortRead { actual: usize },
}

/// Consume the lifetime lock and obtain one 32-byte launcher-instance ID from
/// one `getrandom(2)` call with flags zero. There is no retry or fallback.
pub fn acquire_fresh_launcher_instance(
    lifetime_lock: LauncherLifetimeLockGuard,
) -> Result<VerifiedLauncherInstance, LauncherInstanceIdError> {
    #[cfg(target_os = "linux")]
    {
        let instance_id = instance_id_from_one_call(getrandom_once)?;
        Ok(VerifiedLauncherInstance {
            lifetime_lock,
            instance_id,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        drop(lifetime_lock);
        Err(LauncherInstanceIdError::UnsupportedPlatform)
    }
}

#[cfg(any(target_os = "linux", test))]
fn instance_id_from_one_call(
    read_once: impl FnOnce(&mut [u8; LAUNCHER_INSTANCE_ID_BYTES], u32) -> io::Result<usize>,
) -> Result<[u8; LAUNCHER_INSTANCE_ID_BYTES], LauncherInstanceIdError> {
    let mut instance_id = [0_u8; LAUNCHER_INSTANCE_ID_BYTES];
    let actual = read_once(&mut instance_id, 0).map_err(LauncherInstanceIdError::Getrandom)?;
    if actual != instance_id.len() {
        return Err(LauncherInstanceIdError::ShortRead { actual });
    }
    Ok(instance_id)
}

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn getrandom_once(output: &mut [u8; LAUNCHER_INSTANCE_ID_BYTES], flags: u32) -> io::Result<usize> {
    // SAFETY: `output` exposes exactly 32 writable bytes for the duration of
    // the syscall, and getrandom neither retains nor aliases that pointer.
    let result = unsafe {
        libc::getrandom(
            output.as_mut_ptr().cast::<libc::c_void>(),
            output.len(),
            flags,
        )
    };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result as usize)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io;

    use boole_native_shadow_protocol::TRACKED_EXECUTION_POLICY_BYTES;
    use serde_json::Value;

    use super::{instance_id_from_one_call, LauncherInstanceIdError};

    #[test]
    fn one_exact_call_uses_a_32_byte_buffer_and_flags_zero() {
        let calls = Cell::new(0_u8);
        let expected = std::array::from_fn(|index| index as u8);
        let actual = instance_id_from_one_call(|output, flags| {
            calls.set(calls.get() + 1);
            assert_eq!(output.len(), 32);
            assert_eq!(flags, 0);
            output.copy_from_slice(&expected);
            Ok(output.len())
        })
        .expect("one exact read must issue an instance ID");

        assert_eq!(calls.get(), 1);
        assert_eq!(actual, expected);
    }

    #[test]
    fn syscall_failure_is_not_retried_or_replaced() {
        let calls = Cell::new(0_u8);
        let error = instance_id_from_one_call(|_, flags| {
            calls.set(calls.get() + 1);
            assert_eq!(flags, 0);
            Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted"))
        })
        .expect_err("an interrupted syscall must fail without retry");

        assert_eq!(calls.get(), 1);
        assert!(matches!(error, LauncherInstanceIdError::Getrandom(_)));
    }

    #[test]
    fn every_short_read_fails_without_a_second_call() {
        for short in [0_usize, 1, 31] {
            let calls = Cell::new(0_u8);
            let error = instance_id_from_one_call(|_, flags| {
                calls.set(calls.get() + 1);
                assert_eq!(flags, 0);
                Ok(short)
            })
            .expect_err("a short getrandom read must fail closed");

            assert_eq!(calls.get(), 1);
            assert!(matches!(
                error,
                LauncherInstanceIdError::ShortRead { actual } if actual == short
            ));
        }
    }

    #[test]
    fn compile_time_instance_id_shape_matches_the_tracked_policy() {
        let policy: Value = serde_json::from_slice(TRACKED_EXECUTION_POLICY_BYTES)
            .expect("tracked execution policy JSON");
        assert_eq!(
            policy.pointer("/ipc/qualificationHandshake/launcherInstanceIdSource"),
            Some(&Value::String(
                "getrandom:32-bytes:no-fallback-at-launcher-startup".to_string()
            ))
        );
        assert_eq!(
            policy.pointer("/ipc/messages/qualificationReady/requiredFields/launcherInstanceIdHex"),
            Some(&Value::String("lower-hex:32-bytes".to_string()))
        );
    }
}
