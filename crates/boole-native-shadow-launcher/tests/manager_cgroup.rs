use boole_native_shadow_launcher::{
    instance_id::VerifiedLauncherInstance,
    manager_cgroup::{enter_fixed_manager_cgroup, ManagerCgroupError, VerifiedManagerCgroup},
};

#[test]
fn production_manager_entrypoint_consumes_only_the_opaque_launcher_instance() {
    let _entrypoint: fn(
        VerifiedLauncherInstance,
    ) -> Result<VerifiedManagerCgroup, ManagerCgroupError> = enter_fixed_manager_cgroup;
}
