use boole_native_shadow_launcher::{
    startup_recovery::VerifiedStartupCgroupRecovery,
    toolchain_compatibility::{
        verify_fixed_startup_toolchain_compatibility, ToolchainCompatibilityError,
        VerifiedStartupToolchainCompatibility,
    },
};

#[test]
fn production_probe_consumes_only_the_startup_recovery_proof() {
    let _entrypoint: fn(
        VerifiedStartupCgroupRecovery,
    ) -> Result<
        VerifiedStartupToolchainCompatibility,
        ToolchainCompatibilityError,
    > = verify_fixed_startup_toolchain_compatibility;
}
