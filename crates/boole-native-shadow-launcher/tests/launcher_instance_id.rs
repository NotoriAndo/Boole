use boole_native_shadow_launcher::{
    instance_id::{
        acquire_fresh_launcher_instance, LauncherInstanceIdError, VerifiedLauncherInstance,
    },
    lifetime_lock::LauncherLifetimeLockGuard,
};

#[test]
fn production_instance_entrypoint_consumes_only_the_opaque_lifetime_lock() {
    let _entrypoint: fn(
        LauncherLifetimeLockGuard,
    ) -> Result<VerifiedLauncherInstance, LauncherInstanceIdError> =
        acquire_fresh_launcher_instance;
}
