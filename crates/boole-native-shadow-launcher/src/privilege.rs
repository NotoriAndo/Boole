use std::marker::PhantomData;
use std::rc::Rc;

use thiserror::Error;

#[cfg(any(target_os = "linux", test))]
const FIXED_LAUNCHER_CAPABILITY_MASK: u64 =
    (1_u64 << 6) | (1_u64 << 7) | (1_u64 << 8) | (1_u64 << 21);

/// Proof that the calling launcher thread matched the frozen privilege shape.
///
/// Fields are private and the marker keeps the proof on the OS thread that
/// performed the check.
///
/// ```compile_fail
/// let proof = boole_native_shadow_launcher::privilege::verify_fixed_launcher_privilege()
///     .expect("launcher privilege");
/// std::thread::spawn(move || drop(proof));
/// ```
#[must_use]
#[derive(Debug)]
pub struct VerifiedLauncherPrivilege {
    _thread_bound: PhantomData<Rc<()>>,
}

#[derive(Debug, Error)]
pub enum LauncherPrivilegeError {
    #[error("native-shadow launcher privilege verification requires Linux")]
    UnsupportedPlatform,
    #[error("failed to read the current launcher thread status: {0}")]
    ReadStatus(#[source] std::io::Error),
    #[error("launcher status is missing required field {0}")]
    MissingField(&'static str),
    #[error("launcher status repeats required field {0}")]
    DuplicateField(&'static str),
    #[error("launcher status field {0} is malformed")]
    MalformedField(&'static str),
    #[error("launcher {kind} identity is not root in every kernel slot: {actual:?}")]
    RootIdentityMismatch {
        kind: &'static str,
        actual: [u32; 4],
    },
    #[error(
        "launcher {set} capability set mismatch: expected {expected:#018x}, actual {actual:#018x}"
    )]
    CapabilityMismatch {
        set: &'static str,
        expected: u64,
        actual: u64,
    },
    #[error("launcher NoNewPrivs mismatch: expected 0, actual {actual}")]
    NoNewPrivilegesMismatch { actual: u32 },
}

/// Verify the fixed launcher privilege contract without caller-selected input.
pub fn verify_fixed_launcher_privilege() -> Result<VerifiedLauncherPrivilege, LauncherPrivilegeError>
{
    #[cfg(target_os = "linux")]
    {
        linux::verify()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(LauncherPrivilegeError::UnsupportedPlatform)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs;

    use super::{LauncherPrivilegeError, VerifiedLauncherPrivilege};

    pub(super) fn verify() -> Result<VerifiedLauncherPrivilege, LauncherPrivilegeError> {
        let status = fs::read_to_string("/proc/thread-self/status")
            .map_err(LauncherPrivilegeError::ReadStatus)?;
        super::verify_status_snapshot(&status)
    }
}

#[cfg(any(target_os = "linux", test))]
fn verify_status_snapshot(
    status: &str,
) -> Result<VerifiedLauncherPrivilege, LauncherPrivilegeError> {
    let mut uids = None;
    let mut gids = None;
    let mut cap_inheritable = None;
    let mut cap_permitted = None;
    let mut cap_effective = None;
    let mut cap_bounding = None;
    let mut cap_ambient = None;
    let mut no_new_privileges = None;

    for line in status.lines() {
        let Some((field, value)) = line.split_once(':') else {
            continue;
        };
        match field {
            "Uid" => set_once(&mut uids, "Uid", parse_id_slots("Uid", value)?)?,
            "Gid" => set_once(&mut gids, "Gid", parse_id_slots("Gid", value)?)?,
            "CapInh" => set_once(
                &mut cap_inheritable,
                "CapInh",
                parse_capability("CapInh", value)?,
            )?,
            "CapPrm" => set_once(
                &mut cap_permitted,
                "CapPrm",
                parse_capability("CapPrm", value)?,
            )?,
            "CapEff" => set_once(
                &mut cap_effective,
                "CapEff",
                parse_capability("CapEff", value)?,
            )?,
            "CapBnd" => set_once(
                &mut cap_bounding,
                "CapBnd",
                parse_capability("CapBnd", value)?,
            )?,
            "CapAmb" => set_once(
                &mut cap_ambient,
                "CapAmb",
                parse_capability("CapAmb", value)?,
            )?,
            "NoNewPrivs" => set_once(
                &mut no_new_privileges,
                "NoNewPrivs",
                parse_decimal("NoNewPrivs", value)?,
            )?,
            _ => {}
        }
    }

    let uids = required(uids, "Uid")?;
    let gids = required(gids, "Gid")?;
    require_root("UID", uids)?;
    require_root("GID", gids)?;
    require_capability(
        "effective",
        required(cap_effective, "CapEff")?,
        FIXED_LAUNCHER_CAPABILITY_MASK,
    )?;
    require_capability(
        "permitted",
        required(cap_permitted, "CapPrm")?,
        FIXED_LAUNCHER_CAPABILITY_MASK,
    )?;
    require_capability(
        "bounding",
        required(cap_bounding, "CapBnd")?,
        FIXED_LAUNCHER_CAPABILITY_MASK,
    )?;
    require_capability("inheritable", required(cap_inheritable, "CapInh")?, 0)?;
    require_capability("ambient", required(cap_ambient, "CapAmb")?, 0)?;

    let no_new_privileges = required(no_new_privileges, "NoNewPrivs")?;
    if no_new_privileges != 0 {
        return Err(LauncherPrivilegeError::NoNewPrivilegesMismatch {
            actual: no_new_privileges,
        });
    }

    Ok(VerifiedLauncherPrivilege {
        _thread_bound: PhantomData,
    })
}

#[cfg(any(target_os = "linux", test))]
fn parse_id_slots(field: &'static str, value: &str) -> Result<[u32; 4], LauncherPrivilegeError> {
    let parsed = value
        .split_whitespace()
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| LauncherPrivilegeError::MalformedField(field))?;
    parsed
        .try_into()
        .map_err(|_| LauncherPrivilegeError::MalformedField(field))
}

#[cfg(any(target_os = "linux", test))]
fn parse_capability(field: &'static str, value: &str) -> Result<u64, LauncherPrivilegeError> {
    let value = value.trim();
    if value.len() != 16 {
        return Err(LauncherPrivilegeError::MalformedField(field));
    }
    u64::from_str_radix(value, 16).map_err(|_| LauncherPrivilegeError::MalformedField(field))
}

#[cfg(any(target_os = "linux", test))]
fn parse_decimal(field: &'static str, value: &str) -> Result<u32, LauncherPrivilegeError> {
    value
        .trim()
        .parse()
        .map_err(|_| LauncherPrivilegeError::MalformedField(field))
}

#[cfg(any(target_os = "linux", test))]
fn required<T>(value: Option<T>, field: &'static str) -> Result<T, LauncherPrivilegeError> {
    value.ok_or(LauncherPrivilegeError::MissingField(field))
}

#[cfg(any(target_os = "linux", test))]
fn set_once<T>(
    slot: &mut Option<T>,
    field: &'static str,
    value: T,
) -> Result<(), LauncherPrivilegeError> {
    if slot.replace(value).is_some() {
        return Err(LauncherPrivilegeError::DuplicateField(field));
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn require_root(kind: &'static str, actual: [u32; 4]) -> Result<(), LauncherPrivilegeError> {
    if actual != [0; 4] {
        return Err(LauncherPrivilegeError::RootIdentityMismatch { kind, actual });
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn require_capability(
    set: &'static str,
    actual: u64,
    expected: u64,
) -> Result<(), LauncherPrivilegeError> {
    if actual != expected {
        return Err(LauncherPrivilegeError::CapabilityMismatch {
            set,
            expected,
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[cfg(target_os = "linux")]
    use super::verify_fixed_launcher_privilege;
    use super::{verify_status_snapshot, LauncherPrivilegeError, FIXED_LAUNCHER_CAPABILITY_MASK};

    const EXACT_STATUS: &str = "\
Name:\tboole-native-s\n\
Uid:\t0\t0\t0\t0\n\
Gid:\t0\t0\t0\t0\n\
CapInh:\t0000000000000000\n\
CapPrm:\t00000000002001c0\n\
CapEff:\t00000000002001c0\n\
CapBnd:\t00000000002001c0\n\
CapAmb:\t0000000000000000\n\
NoNewPrivs:\t0\n";

    #[test]
    fn exact_frozen_privilege_snapshot_issues_a_thread_bound_proof() {
        let _proof =
            verify_status_snapshot(EXACT_STATUS).expect("exact frozen launcher privilege snapshot");
    }

    #[test]
    fn duplicate_kernel_status_field_fails_closed() {
        let duplicated = format!("{EXACT_STATUS}CapEff:\t00000000002001c0\n");
        assert!(matches!(
            verify_status_snapshot(&duplicated),
            Err(super::LauncherPrivilegeError::DuplicateField("CapEff"))
        ));
    }

    #[test]
    fn every_identity_slot_and_capability_set_is_exact() {
        for field in ["Uid", "Gid"] {
            for slot in 0..4 {
                let mut values = [0_u32; 4];
                values[slot] = 1;
                let replacement = format!(
                    "{field}:\t{}\t{}\t{}\t{}",
                    values[0], values[1], values[2], values[3]
                );
                let changed =
                    EXACT_STATUS.replacen(&format!("{field}:\t0\t0\t0\t0"), &replacement, 1);
                assert!(matches!(
                    verify_status_snapshot(&changed),
                    Err(LauncherPrivilegeError::RootIdentityMismatch { .. })
                ));
            }
        }

        for (field, expected_set) in [
            ("CapEff", "effective"),
            ("CapPrm", "permitted"),
            ("CapBnd", "bounding"),
        ] {
            for actual in ["00000000000001c0", "00000000002001c1"] {
                let changed = EXACT_STATUS.replacen(
                    &format!("{field}:\t00000000002001c0"),
                    &format!("{field}:\t{actual}"),
                    1,
                );
                assert!(matches!(
                    verify_status_snapshot(&changed),
                    Err(LauncherPrivilegeError::CapabilityMismatch { set, .. })
                        if set == expected_set
                ));
            }
        }

        for (field, expected_set) in [("CapInh", "inheritable"), ("CapAmb", "ambient")] {
            let changed = EXACT_STATUS.replacen(
                &format!("{field}:\t0000000000000000"),
                &format!("{field}:\t0000000000000001"),
                1,
            );
            assert!(matches!(
                verify_status_snapshot(&changed),
                Err(LauncherPrivilegeError::CapabilityMismatch { set, .. })
                    if set == expected_set
            ));
        }

        let nnp = EXACT_STATUS.replacen("NoNewPrivs:\t0", "NoNewPrivs:\t1", 1);
        assert!(matches!(
            verify_status_snapshot(&nnp),
            Err(LauncherPrivilegeError::NoNewPrivilegesMismatch { actual: 1 })
        ));
    }

    #[test]
    fn missing_or_malformed_kernel_status_field_fails_closed() {
        let missing = EXACT_STATUS.replacen("CapAmb:\t0000000000000000\n", "", 1);
        assert!(matches!(
            verify_status_snapshot(&missing),
            Err(LauncherPrivilegeError::MissingField("CapAmb"))
        ));

        for malformed in [
            EXACT_STATUS.replacen("Uid:\t0\t0\t0\t0", "Uid:\t0\t0\t0", 1),
            EXACT_STATUS.replacen("CapPrm:\t00000000002001c0", "CapPrm:\tnot-a-capability", 1),
            EXACT_STATUS.replacen("NoNewPrivs:\t0", "NoNewPrivs:\t0\t0", 1),
        ] {
            assert!(matches!(
                verify_status_snapshot(&malformed),
                Err(LauncherPrivilegeError::MalformedField(_))
            ));
        }
    }

    #[test]
    fn compile_time_privilege_shape_matches_the_tracked_policy() {
        let policy: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../native/containment/native-shadow-execution-policy-v1.json"
        ))
        .expect("tracked execution policy JSON");
        let caps = json!(["CAP_SETGID", "CAP_SETUID", "CAP_SETPCAP", "CAP_SYS_ADMIN"]);

        assert_eq!(
            policy.pointer("/privilege/launcherUser"),
            Some(&json!("root"))
        );
        assert_eq!(
            policy.pointer("/privilege/launcherGroup"),
            Some(&json!("root"))
        );
        for set in ["effective", "permitted", "bounding"] {
            assert_eq!(
                policy.pointer(&format!("/privilege/launcherCapabilitySets/{set}")),
                Some(&caps)
            );
        }
        for set in ["inheritable", "ambient"] {
            assert_eq!(
                policy.pointer(&format!("/privilege/launcherCapabilitySets/{set}")),
                Some(&json!([]))
            );
        }
        assert_eq!(
            policy.pointer("/privilege/launcherNoNewPrivileges"),
            Some(&json!(false))
        );
        assert_eq!(
            policy.pointer("/privilege/startupSelfCheckBeforeSocketBind"),
            Some(&json!(true))
        );
        assert_eq!(
            policy.pointer("/privilege/failIfCapabilitySetDiffers"),
            Some(&json!(true))
        );
        assert_eq!(
            policy.pointer("/privilege/systemdUnit/CapabilityBoundingSet"),
            Some(&caps)
        );
        assert_eq!(
            policy.pointer("/privilege/systemdUnit/AmbientCapabilities"),
            Some(&json!([]))
        );
        assert_eq!(
            policy.pointer("/privilege/systemdUnit/NoNewPrivileges"),
            Some(&json!(false))
        );
        assert_eq!(FIXED_LAUNCHER_CAPABILITY_MASK, 0x0000_0000_0020_01c0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires the named root systemd service with the frozen capability set"]
    fn real_kernel_privilege_matches_frozen_policy() {
        let _proof = verify_fixed_launcher_privilege()
            .unwrap_or_else(|error| panic!("launcher privilege self-check failed: {error}"));
    }
}
