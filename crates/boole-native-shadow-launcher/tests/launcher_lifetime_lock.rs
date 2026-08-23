use boole_native_shadow_launcher::{
    lifetime_lock::{
        acquire_fixed_launcher_lifetime_lock, LauncherLifetimeLockError, LauncherLifetimeLockGuard,
    },
    startup::VerifiedLauncherPrelockPrerequisites,
};

#[test]
fn production_lock_entrypoint_consumes_only_the_opaque_prelock_proof() {
    let _entrypoint: fn(
        VerifiedLauncherPrelockPrerequisites,
    ) -> Result<LauncherLifetimeLockGuard, LauncherLifetimeLockError> =
        acquire_fixed_launcher_lifetime_lock;
}
