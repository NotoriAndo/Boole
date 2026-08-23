use boole_native_shadow_launcher::{
    manager_cgroup::VerifiedManagerCgroup,
    startup_recovery::{
        recover_fixed_startup_orphans, StartupCgroupRecoveryError, VerifiedStartupCgroupRecovery,
    },
};

#[test]
fn production_recovery_consumes_only_the_opaque_manager_proof() {
    let _entrypoint: fn(
        VerifiedManagerCgroup,
    ) -> Result<VerifiedStartupCgroupRecovery, StartupCgroupRecoveryError> =
        recover_fixed_startup_orphans;
}
