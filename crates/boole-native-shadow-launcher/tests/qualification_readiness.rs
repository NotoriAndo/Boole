use boole_native_shadow_launcher::{
    qualification::VerifiedQualificationStartup,
    readiness::{assemble_fixed_qualification_startup, QualificationStartupError},
    toolchain_compatibility::VerifiedStartupToolchainCompatibility,
};

#[test]
fn production_readiness_requires_the_complete_toolchain_compatibility_proof() {
    let _entrypoint: fn(
        VerifiedStartupToolchainCompatibility,
    ) -> Result<VerifiedQualificationStartup, QualificationStartupError> =
        assemble_fixed_qualification_startup;
}
