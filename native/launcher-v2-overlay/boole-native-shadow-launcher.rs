#[cfg(target_os = "linux")]
use std::path::Path;

#[cfg(target_os = "linux")]
use boole_native_shadow_launcher::{
    active_execution::serve_qualified_three_fixed_unix_executions_with_listener_bound_console_evidence,
    closed_local_replay_startup::assemble_verified_closed_local_replay_startup,
    console_evidence::{observe_supervisor_privilege, Prerequisite, Record},
    instance_id::acquire_fresh_launcher_instance,
    lifetime_lock::acquire_fixed_launcher_lifetime_lock,
    manager_cgroup::enter_fixed_manager_cgroup,
    runtime_rootfs_replay::verify_runtime_rootfs_replay,
    startup::verify_fixed_launcher_prelock_prerequisites,
    startup_recovery::recover_fixed_startup_orphans,
    toolchain_compatibility::verify_fixed_startup_toolchain_compatibility,
};
#[cfg(target_os = "linux")]
use boole_native_shadow_protocol::sha256_hex;

#[cfg(target_os = "linux")]
const FIXED_RUNTIME_ROOTFS: &str = "/var/lib/boole/native-shadow/runtime-rootfs";
#[cfg(target_os = "linux")]
const FIXED_RUNTIME_ROOTFS_MANIFEST: &str =
    "/var/lib/boole/native-shadow/ROOTFS-CONTENT-MANIFEST.json";

#[cfg(target_os = "linux")]
const RESOLVED_PREREQUISITES: [&str; 9] = [
    "fixed-launcher-prelock-prerequisites",
    "fixed-launcher-lifetime-lock",
    "fresh-launcher-instance",
    "fixed-manager-cgroup",
    "fixed-startup-orphan-recovery",
    "fixed-startup-toolchain-compatibility",
    "runtime-rootfs-replay",
    "closed-local-replay-startup",
    "failed-unit-query",
];

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let executable = std::env::current_exe()?;
    let executable_bytes = std::fs::read(&executable)?;
    let executable_record = Record::LauncherExecutable {
        path: executable.to_string_lossy().into_owned(),
        sha256: sha256_hex(&executable_bytes),
    };

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

    let status = std::fs::read_to_string("/proc/thread-self/status")?;
    let supervisor = observe_supervisor_privilege(&status)?;
    let failed_units = observe_failed_systemd_units()?;
    let prerequisite_records = RESOLVED_PREREQUISITES
        .into_iter()
        .map(|name| Prerequisite {
            name: name.to_owned(),
            resolved: true,
        })
        .collect();
    let records = [
        executable_record,
        Record::LauncherPrerequisites(prerequisite_records),
        Record::SupervisorPrivilege(supervisor),
        Record::Readiness {
            ready: failed_units.is_empty(),
            failed_units,
        },
    ];
    serve_qualified_three_fixed_unix_executions_with_listener_bound_console_evidence(
        startup, records,
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn observe_failed_systemd_units() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("/usr/bin/systemctl")
        .args(["--failed", "--no-legend", "--plain", "--no-pager"])
        .env_clear()
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "fixed systemd failed-unit query exited with {:?}",
            output.status.code()
        )
        .into());
    }
    parse_failed_systemd_units(std::str::from_utf8(&output.stdout)?).map_err(Into::into)
}

#[cfg(any(target_os = "linux", test))]
fn parse_failed_systemd_units(text: &str) -> Result<Vec<String>, String> {
    let mut units = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let unit = match fields.as_slice() {
            ["●", unit, ..] => *unit,
            [unit, ..] => *unit,
            [] => continue,
        };
        if !unit.contains('.')
            || !unit
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"@_.:-\\".contains(&byte))
        {
            return Err("fixed systemd failed-unit output is malformed".to_owned());
        }
        units.push(unit.to_owned());
    }
    units.sort();
    units.dedup();
    Ok(units)
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("boole-native-shadow-launcher requires Linux");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::parse_failed_systemd_units;

    #[cfg(target_os = "linux")]
    #[test]
    fn listener_bound_evidence_api_consumes_exactly_four_records_by_value() {
        let _entrypoint: fn(
            boole_native_shadow_launcher::closed_local_replay_startup::VerifiedClosedLocalReplayStartup,
            [boole_native_shadow_launcher::console_evidence::Record; 4],
        ) -> Result<
            (),
            boole_native_shadow_launcher::active_execution::ActiveExecutionListenerError,
        > = boole_native_shadow_launcher::active_execution::serve_qualified_three_fixed_unix_executions_with_listener_bound_console_evidence;
    }

    #[test]
    fn empty_failed_unit_output_is_exactly_empty() {
        assert_eq!(
            parse_failed_systemd_units("").expect("empty output"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn plain_and_bulleted_systemd_rows_are_sorted_and_deduplicated() {
        let output = "z.service loaded failed failed Z\n\
                      ● a.service loaded failed failed A\n\
                      a.service loaded failed failed A again\n";
        assert_eq!(
            parse_failed_systemd_units(output).expect("fixed rows"),
            vec!["a.service".to_owned(), "z.service".to_owned()]
        );
    }

    #[test]
    fn prose_or_controlled_path_output_is_not_mistaken_for_a_unit() {
        for bad in [
            "0 loaded units listed.\n",
            "/tmp/fake.service loaded failed\n",
        ] {
            assert!(parse_failed_systemd_units(bad).is_err(), "accepted {bad:?}");
        }
    }
}
