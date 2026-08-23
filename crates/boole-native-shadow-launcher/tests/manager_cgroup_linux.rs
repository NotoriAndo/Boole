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
    startup_recovery::{recover_fixed_startup_orphans, StartupCgroupRecoveryError},
};

#[cfg(target_os = "linux")]
const MODE_PATH: &str = "/run/boole/native-shadow/manager-cgroup-gate-mode";
#[cfg(target_os = "linux")]
const SERVICE_ROOT: &str = "/sys/fs/cgroup/system.slice/boole-native-shadow-launcher.service";
#[cfg(target_os = "linux")]
const RECOVERY_RELEASE_PATH: &str = "/run/boole/native-shadow/startup-recovery-release";
#[cfg(target_os = "linux")]
const INVENTORY_TEST_LEAF: &str =
    "run-0000000000000000000000000000000000000000000000000000000000000001";

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
        "startup-recovery" => {
            let manager = enter_fixed_manager_cgroup(instance).map_err(format_manager_error)?;
            announce("native-shadow-startup-recovery-prepared")?;
            wait_for_recovery_release()?;
            let recovered =
                recover_fixed_startup_orphans(manager).map_err(format_startup_recovery_error)?;
            if recovered.recovered_orphan_count() != 3 {
                return Err(format!(
                    "startup recovery removed {} leaves instead of 3",
                    recovered.recovered_orphan_count()
                ));
            }
            announce_and_wait("native-shadow-startup-recovery-complete:3")
        }
        "startup-inventory-reject" => {
            let manager = enter_fixed_manager_cgroup(instance).map_err(format_manager_error)?;
            announce("native-shadow-startup-inventory-prepared")?;
            wait_for_recovery_release()?;
            let before = snapshot_inventory_test_leaf()?;
            match recover_fixed_startup_orphans(manager) {
                Err(StartupCgroupRecoveryError::PostMoveFatal {
                    stage: "validate startup cgroup inventory",
                    ..
                }) => {
                    let after = snapshot_inventory_test_leaf()?;
                    if after != before {
                        return Err(format!(
                            "inventory rejection mutated valid leaf: before={before:?}, after={after:?}"
                        ));
                    }
                    if !Path::new(&format!("{SERVICE_ROOT}/zzz-unexpected")).is_dir() {
                        return Err("inventory rejection removed the unexpected child".to_string());
                    }
                    announce_and_wait("native-shadow-startup-inventory-untouched")
                }
                Err(error) => Err(format!(
                    "startup inventory rejection had wrong error phase: {error}"
                )),
                Ok(_) => Err("unexpected startup inventory was accepted".to_string()),
            }
        }
        other => Err(format!("unknown manager gate mode: {other}")),
    }
}

#[cfg(target_os = "linux")]
fn wait_for_recovery_release() -> Result<(), String> {
    for _ in 0..4_000 {
        match fs::remove_file(RECOVERY_RELEASE_PATH) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(format!("consume startup recovery release failed: {error}")),
        }
    }
    Err("startup recovery release was not provided".to_string())
}

#[cfg(target_os = "linux")]
#[derive(Debug, Eq, PartialEq)]
struct LeafSnapshot {
    frozen: u8,
    populated: u8,
    processes: Vec<u32>,
    threads: Vec<u32>,
}

#[cfg(target_os = "linux")]
fn snapshot_inventory_test_leaf() -> Result<LeafSnapshot, String> {
    let leaf = format!("{SERVICE_ROOT}/{INVENTORY_TEST_LEAF}");
    let events = fs::read_to_string(format!("{leaf}/cgroup.events"))
        .map_err(|error| format!("read untouched leaf events failed: {error}"))?;
    let frozen = parse_cgroup_event(&events, "frozen")?;
    let populated = parse_cgroup_event(&events, "populated")?;
    let processes = read_sorted_ids(&format!("{leaf}/cgroup.procs"))?;
    let threads = read_sorted_ids(&format!("{leaf}/cgroup.threads"))?;
    if frozen != 0 || populated != 1 || processes.is_empty() || threads.is_empty() {
        return Err(format!(
            "inventory rejection fixture is not live and unfrozen: frozen={frozen}, populated={populated}, processes={processes:?}, threads={threads:?}"
        ));
    }
    Ok(LeafSnapshot {
        frozen,
        populated,
        processes,
        threads,
    })
}

#[cfg(target_os = "linux")]
fn read_sorted_ids(path: &str) -> Result<Vec<u32>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("read {path} failed: {error}"))?;
    let mut ids = text
        .split_whitespace()
        .map(|value| {
            value
                .parse::<u32>()
                .map_err(|_| format!("{path} contains malformed ID"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    ids.sort_unstable();
    Ok(ids)
}

#[cfg(target_os = "linux")]
fn parse_cgroup_event(events: &str, key: &str) -> Result<u8, String> {
    let mut found = None;
    for line in events.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.first().copied() != Some(key) {
            continue;
        }
        if fields.len() != 2 || found.is_some() {
            return Err(format!("malformed or duplicate cgroup event: {key}"));
        }
        found = Some(
            fields[1]
                .parse::<u8>()
                .map_err(|_| format!("malformed cgroup event value: {key}"))?,
        );
    }
    found.ok_or_else(|| format!("missing cgroup event: {key}"))
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
fn format_startup_recovery_error(error: StartupCgroupRecoveryError) -> String {
    format!("fatal startup cgroup recovery failure: {error}")
}

#[cfg(target_os = "linux")]
fn announce(marker: &str) -> Result<(), String> {
    println!("{marker}");
    io::stdout()
        .flush()
        .map_err(|error| format!("flush marker failed: {error}"))
}

#[cfg(target_os = "linux")]
fn announce_and_wait(marker: &str) -> Result<(), String> {
    announce(marker)?;
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
