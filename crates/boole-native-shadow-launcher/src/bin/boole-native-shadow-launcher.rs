#[cfg(target_os = "linux")]
use std::path::Path;

#[cfg(target_os = "linux")]
use boole_native_shadow_launcher::{
    active_execution::serve_qualified_three_fixed_unix_executions,
    closed_local_replay_startup::assemble_verified_closed_local_replay_startup,
    instance_id::acquire_fresh_launcher_instance,
    lifetime_lock::acquire_fixed_launcher_lifetime_lock,
    manager_cgroup::enter_fixed_manager_cgroup,
    runtime_rootfs_replay::verify_runtime_rootfs_replay,
    startup::verify_fixed_launcher_prelock_prerequisites,
    startup_recovery::recover_fixed_startup_orphans,
    toolchain_compatibility::verify_fixed_startup_toolchain_compatibility,
};

#[cfg(target_os = "linux")]
const FIXED_RUNTIME_ROOTFS: &str = "/var/lib/boole/native-shadow/runtime-rootfs";
#[cfg(target_os = "linux")]
const FIXED_RUNTIME_ROOTFS_MANIFEST: &str =
    "/var/lib/boole/native-shadow/ROOTFS-CONTENT-MANIFEST.json";

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let prerequisites = verify_fixed_launcher_prelock_prerequisites()?;
    let lifetime_lock = acquire_fixed_launcher_lifetime_lock(prerequisites)?;
    let instance = acquire_fresh_launcher_instance(lifetime_lock)?;
    let manager = enter_fixed_manager_cgroup(instance)?;
    let recovered = recover_fixed_startup_orphans(manager)?;
    let compatibility = verify_fixed_startup_toolchain_compatibility(recovered)?;
    let rootfs = verify_runtime_rootfs_replay(
        Path::new(FIXED_RUNTIME_ROOTFS),
        Path::new(FIXED_RUNTIME_ROOTFS_MANIFEST),
    )?;
    let startup = assemble_verified_closed_local_replay_startup(compatibility, rootfs)?;
    serve_qualified_three_fixed_unix_executions(startup)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("boole-native-shadow-launcher requires Linux");
    std::process::exit(1);
}
