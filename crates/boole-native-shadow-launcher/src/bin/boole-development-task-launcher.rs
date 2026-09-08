//! Explicit opt-in task admission; neither historical binary selects it.
#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use boole_native_shadow_launcher::{
        active_execution::serve_qualified_one_canary_execution,
        closed_local_replay_startup::assemble_verified_development_task_startup,
        instance_id::acquire_fresh_launcher_instance,
        lifetime_lock::acquire_fixed_launcher_lifetime_lock,
        manager_cgroup::enter_fixed_manager_cgroup,
        runtime_rootfs_replay::verify_runtime_rootfs_replay,
        startup::verify_fixed_launcher_prelock_prerequisites,
        startup_recovery::recover_fixed_startup_orphans,
        toolchain_compatibility::verify_fixed_startup_toolchain_compatibility,
    };
    use std::path::Path;
    if std::env::args_os().len() != 1 {
        return Err("development task launcher takes no request-selected configuration".into());
    }
    let prerequisites = verify_fixed_launcher_prelock_prerequisites()?;
    let lifetime_lock = acquire_fixed_launcher_lifetime_lock(prerequisites)?;
    let instance = acquire_fresh_launcher_instance(lifetime_lock)?;
    let manager = enter_fixed_manager_cgroup(instance)?;
    let recovered = recover_fixed_startup_orphans(manager)?;
    let compatibility = verify_fixed_startup_toolchain_compatibility(recovered)?;
    let rootfs = verify_runtime_rootfs_replay(
        Path::new("/var/lib/boole/native-shadow/runtime-rootfs"),
        Path::new("/var/lib/boole/native-shadow/ROOTFS-CONTENT-MANIFEST.json"),
    )?;
    serve_qualified_one_canary_execution(assemble_verified_development_task_startup(
        compatibility,
        rootfs,
    )?)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("development task admission requires qualified Linux; no host fallback");
    std::process::exit(2);
}
