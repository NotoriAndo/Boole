#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::io::{self, Write};
#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "linux")]
use std::sync::{Arc, Barrier};
#[cfg(target_os = "linux")]
use std::time::Duration;

#[cfg(target_os = "linux")]
use boole_native_shadow_launcher::{
    instance_id::acquire_fresh_launcher_instance,
    lifetime_lock::acquire_fixed_launcher_lifetime_lock,
    manager_cgroup::{enter_fixed_manager_cgroup, ManagerCgroupError},
    startup::verify_fixed_launcher_prelock_prerequisites,
};

#[cfg(target_os = "linux")]
const MODE_PATH: &str = "/run/boole/native-shadow/manager-cgroup-gate-mode";
#[cfg(target_os = "linux")]
const SERVICE_ROOT: &str = "/sys/fs/cgroup/system.slice/boole-native-shadow-launcher.service";

fn main() {
    #[cfg(target_os = "linux")]
    if let Err(error) = run_linux() {
        eprintln!("native-shadow manager cgroup Linux harness failed: {error}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("native-shadow manager cgroup Linux harness requires Linux");
        std::process::exit(1);
    }
}

#[cfg(target_os = "linux")]
fn run_linux() -> Result<(), String> {
    let prerequisites = verify_fixed_launcher_prelock_prerequisites()
        .map_err(|error| format!("pre-lock verification failed: {error}"))?;
    let lifetime_lock = acquire_fixed_launcher_lifetime_lock(prerequisites)
        .map_err(|error| format!("lifetime lock failed: {error}"))?;
    let instance = acquire_fresh_launcher_instance(lifetime_lock)
        .map_err(|error| format!("instance identity failed: {error}"))?;
    let mode = match fs::read_to_string(MODE_PATH) {
        Ok(value) => value.trim().to_string(),
        Err(error) if error.kind() == io::ErrorKind::NotFound => "normal".to_string(),
        Err(error) => return Err(format!("read gate mode failed: {error}")),
    };

    match mode.as_str() {
        "normal" => {
            let _manager = enter_fixed_manager_cgroup(instance).map_err(format_manager_error)?;
            announce_and_wait("native-shadow-manager-normal-ready")
        }
        "safe-reuse" => {
            create_exact_manager_directory(&format!("{SERVICE_ROOT}/manager"))?;
            let _manager = enter_fixed_manager_cgroup(instance).map_err(format_manager_error)?;
            announce_and_wait("native-shadow-manager-safe-reuse-ready")
        }
        "nested-reject" => {
            let manager = format!("{SERVICE_ROOT}/manager");
            let nested = format!("{manager}/nested");
            create_exact_manager_directory(&manager)?;
            fs::create_dir(&nested)
                .map_err(|error| format!("precreate nested manager child failed: {error}"))?;
            let result = enter_fixed_manager_cgroup(instance);
            fs::remove_dir(&nested)
                .map_err(|error| format!("remove nested child failed: {error}"))?;
            fs::remove_dir(&manager)
                .map_err(|error| format!("remove rejected manager failed: {error}"))?;
            match result {
                Err(ManagerCgroupError::PreMove { .. }) => {
                    println!("native-shadow-manager-nested-rejected");
                    Ok(())
                }
                Err(error) => Err(format!("nested manager had wrong error phase: {error}")),
                Ok(_) => Err("nested manager was accepted".to_string()),
            }
        }
        "frozen-reject" => {
            let manager = format!("{SERVICE_ROOT}/manager");
            create_exact_manager_directory(&manager)?;
            fs::write(format!("{manager}/cgroup.freeze"), "1\n")
                .map_err(|error| format!("freeze manager failed: {error}"))?;
            wait_for_cgroup_event(&manager, "frozen", 1)?;
            let result = enter_fixed_manager_cgroup(instance);
            fs::write(format!("{manager}/cgroup.freeze"), "0\n")
                .map_err(|error| format!("unfreeze rejected manager failed: {error}"))?;
            wait_for_cgroup_event(&manager, "frozen", 0)?;
            fs::remove_dir(&manager)
                .map_err(|error| format!("remove rejected manager failed: {error}"))?;
            match result {
                Err(ManagerCgroupError::PreMove { .. }) => {
                    println!("native-shadow-manager-frozen-rejected");
                    Ok(())
                }
                Err(error) => Err(format!("frozen manager had wrong error phase: {error}")),
                Ok(_) => Err("frozen manager was accepted".to_string()),
            }
        }
        "multithread-reject" => {
            let barrier = Arc::new(Barrier::new(2));
            let child_barrier = Arc::clone(&barrier);
            let child = std::thread::spawn(move || {
                child_barrier.wait();
                std::thread::park();
            });
            barrier.wait();
            let result = enter_fixed_manager_cgroup(instance);
            child.thread().unpark();
            child
                .join()
                .map_err(|_| "gate helper thread panicked".to_string())?;
            match result {
                Err(ManagerCgroupError::PreMove { .. }) => {
                    println!("native-shadow-manager-multithread-rejected");
                    Ok(())
                }
                Err(error) => Err(format!(
                    "multithread launcher had wrong error phase: {error}"
                )),
                Ok(_) => Err("multithread launcher was accepted".to_string()),
            }
        }
        other => Err(format!("unknown manager gate mode: {other}")),
    }
}

#[cfg(target_os = "linux")]
fn create_exact_manager_directory(path: &str) -> Result<(), String> {
    fs::create_dir(path).map_err(|error| format!("precreate manager failed: {error}"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("set precreated manager mode failed: {error}"))
}

#[cfg(target_os = "linux")]
fn wait_for_cgroup_event(manager: &str, key: &str, expected: u8) -> Result<(), String> {
    let events_path = format!("{manager}/cgroup.events");
    for _ in 0..200 {
        let events = fs::read_to_string(&events_path)
            .map_err(|error| format!("read manager cgroup.events failed: {error}"))?;
        let observed = events.lines().find_map(|line| {
            let mut fields = line.split_whitespace();
            match (fields.next(), fields.next(), fields.next()) {
                (Some(name), Some(value), None) if name == key => value.parse::<u8>().ok(),
                _ => None,
            }
        });
        if observed == Some(expected) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Err(format!(
        "manager cgroup event {key} did not reach {expected}"
    ))
}

#[cfg(target_os = "linux")]
fn format_manager_error(error: ManagerCgroupError) -> String {
    match error {
        ManagerCgroupError::PostMoveFatal { .. } => {
            format!("fatal manager state after move attempt: {error}")
        }
        _ => format!("manager setup failed: {error}"),
    }
}

#[cfg(target_os = "linux")]
fn announce_and_wait(marker: &str) -> Result<(), String> {
    println!("{marker}");
    io::stdout()
        .flush()
        .map_err(|error| format!("flush ready marker failed: {error}"))?;
    loop {
        // SAFETY: `pause` has no pointer or ownership preconditions. The
        // tracked systemd unit supplies the terminating signal.
        #[allow(unsafe_code)]
        unsafe {
            libc::pause();
        }
        if !Path::new(SERVICE_ROOT).exists() {
            return Err("service cgroup disappeared without process termination".to_string());
        }
    }
}
