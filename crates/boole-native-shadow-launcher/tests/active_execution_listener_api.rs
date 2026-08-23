#[cfg(target_os = "linux")]
use boole_native_shadow_launcher::{
    active_execution::{serve_three_fixed_unix_executions, ActiveExecutionListenerError},
    closed_local_replay_startup::{
        assemble_verified_closed_local_replay_startup, ClosedLocalReplayStartupError,
        VerifiedClosedLocalReplayStartup,
    },
    runtime_rootfs_replay::VerifiedRuntimeRootfsReplay,
    toolchain_compatibility::VerifiedStartupToolchainCompatibility,
};

#[cfg(target_os = "linux")]
#[test]
fn active_listener_api_requires_the_complete_startup_proof_by_value() {
    let _assemble: fn(
        VerifiedStartupToolchainCompatibility,
        VerifiedRuntimeRootfsReplay,
    )
        -> Result<VerifiedClosedLocalReplayStartup, ClosedLocalReplayStartupError> =
        assemble_verified_closed_local_replay_startup;
    let _serve: fn(VerifiedClosedLocalReplayStartup) -> Result<(), ActiveExecutionListenerError> =
        serve_three_fixed_unix_executions;
}
