//! Pure parser and verdict for the checker child's post-drop kernel snapshot.
//!
//! The v1 launcher performs these checks directly against libc and `/proc`, but
//! the status parser sits inside the Linux-only process setup function and had
//! no table-driven failure matrix.  Launcher v2 calls this pure function after
//! the same irreversible syscalls.  Tests can therefore inject every bad
//! kernel answer without pretending to perform a privilege transition on the
//! developer Mac.

use thiserror::Error;

const CAPABILITY_FIELDS: [&str; 5] = ["CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DroppedPrivilegeSnapshot {
    pub(crate) uids: [u32; 4],
    pub(crate) gids: [u32; 4],
    pub(crate) supplementary_groups: Vec<u32>,
    pub(crate) capabilities: [u64; 5],
    pub(crate) no_new_privileges: u32,
}

#[derive(Debug, Error, Eq, PartialEq)]
pub(crate) enum DroppedPrivilegeError {
    #[error("checker status is missing required field {0}")]
    MissingField(&'static str),
    #[error("checker status repeats required field {0}")]
    DuplicateField(&'static str),
    #[error("checker status field {0} is malformed")]
    MalformedField(&'static str),
    #[error("checker UID slots differ from exact unprivileged identity {expected}: {actual:?}")]
    UidMismatch { expected: u32, actual: [u32; 4] },
    #[error("checker GID slots differ from exact unprivileged identity {expected}: {actual:?}")]
    GidMismatch { expected: u32, actual: [u32; 4] },
    #[error("checker retained supplementary groups: {0:?}")]
    SupplementaryGroups(Vec<u32>),
    #[error("checker capability set {field} is not exact empty: {actual:#018x}")]
    CapabilityNotEmpty { field: &'static str, actual: u64 },
    #[error("checker NoNewPrivs is not exact 1: {actual}")]
    NoNewPrivilegesMismatch { actual: u32 },
}

/// Parse the fields whose exact values prove that the checker stayed dropped.
pub(crate) fn parse_dropped_status_snapshot(
    status: &str,
) -> Result<DroppedPrivilegeSnapshot, DroppedPrivilegeError> {
    let mut uids = None;
    let mut gids = None;
    let mut groups = None;
    let mut capabilities: [Option<u64>; 5] = [None; 5];
    let mut no_new_privileges = None;

    for line in status.lines() {
        let Some((field, value)) = line.split_once(':') else {
            continue;
        };
        match field {
            "Uid" => set_once(&mut uids, "Uid", parse_exact_four_ids("Uid", value)?)?,
            "Gid" => set_once(&mut gids, "Gid", parse_exact_four_ids("Gid", value)?)?,
            "Groups" => set_once(&mut groups, "Groups", parse_groups(value)?)?,
            "NoNewPrivs" => set_once(
                &mut no_new_privileges,
                "NoNewPrivs",
                parse_decimal("NoNewPrivs", value)?,
            )?,
            _ => {
                if let Some(index) = CAPABILITY_FIELDS.iter().position(|name| *name == field) {
                    set_once(
                        &mut capabilities[index],
                        CAPABILITY_FIELDS[index],
                        parse_capability(CAPABILITY_FIELDS[index], value)?,
                    )?;
                }
            }
        }
    }

    Ok(DroppedPrivilegeSnapshot {
        uids: required(uids, "Uid")?,
        gids: required(gids, "Gid")?,
        supplementary_groups: required(groups, "Groups")?,
        capabilities: [
            required(capabilities[0], "CapInh")?,
            required(capabilities[1], "CapPrm")?,
            required(capabilities[2], "CapEff")?,
            required(capabilities[3], "CapBnd")?,
            required(capabilities[4], "CapAmb")?,
        ],
        no_new_privileges: required(no_new_privileges, "NoNewPrivs")?,
    })
}

/// Refuse every post-drop state except the exact frozen unprivileged shape.
pub(crate) fn verify_dropped_status_snapshot(
    status: &str,
    expected_uid: u32,
    expected_gid: u32,
) -> Result<DroppedPrivilegeSnapshot, DroppedPrivilegeError> {
    let snapshot = parse_dropped_status_snapshot(status)?;
    if snapshot.uids != [expected_uid; 4] {
        return Err(DroppedPrivilegeError::UidMismatch {
            expected: expected_uid,
            actual: snapshot.uids,
        });
    }
    if snapshot.gids != [expected_gid; 4] {
        return Err(DroppedPrivilegeError::GidMismatch {
            expected: expected_gid,
            actual: snapshot.gids,
        });
    }
    if !snapshot.supplementary_groups.is_empty() {
        return Err(DroppedPrivilegeError::SupplementaryGroups(
            snapshot.supplementary_groups,
        ));
    }
    for (field, actual) in CAPABILITY_FIELDS.into_iter().zip(snapshot.capabilities) {
        if actual != 0 {
            return Err(DroppedPrivilegeError::CapabilityNotEmpty { field, actual });
        }
    }
    if snapshot.no_new_privileges != 1 {
        return Err(DroppedPrivilegeError::NoNewPrivilegesMismatch {
            actual: snapshot.no_new_privileges,
        });
    }
    Ok(snapshot)
}

fn set_once<T>(
    slot: &mut Option<T>,
    field: &'static str,
    value: T,
) -> Result<(), DroppedPrivilegeError> {
    if slot.replace(value).is_some() {
        return Err(DroppedPrivilegeError::DuplicateField(field));
    }
    Ok(())
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, DroppedPrivilegeError> {
    value.ok_or(DroppedPrivilegeError::MissingField(field))
}

fn parse_exact_four_ids(
    field: &'static str,
    value: &str,
) -> Result<[u32; 4], DroppedPrivilegeError> {
    let values = value
        .split_whitespace()
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| DroppedPrivilegeError::MalformedField(field))?;
    values
        .try_into()
        .map_err(|_| DroppedPrivilegeError::MalformedField(field))
}

fn parse_groups(value: &str) -> Result<Vec<u32>, DroppedPrivilegeError> {
    value
        .split_whitespace()
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| DroppedPrivilegeError::MalformedField("Groups"))
}

fn parse_capability(field: &'static str, value: &str) -> Result<u64, DroppedPrivilegeError> {
    let value = value.trim();
    if value.len() != 16 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(DroppedPrivilegeError::MalformedField(field));
    }
    u64::from_str_radix(value, 16).map_err(|_| DroppedPrivilegeError::MalformedField(field))
}

fn parse_decimal(field: &'static str, value: &str) -> Result<u32, DroppedPrivilegeError> {
    value
        .trim()
        .parse()
        .map_err(|_| DroppedPrivilegeError::MalformedField(field))
}

#[cfg(test)]
mod tests {
    use super::{verify_dropped_status_snapshot, DroppedPrivilegeError, CAPABILITY_FIELDS};

    const UID: u32 = 61184;
    const GID: u32 = 61184;

    fn healthy() -> String {
        [
            format!("Uid:\t{UID}\t{UID}\t{UID}\t{UID}"),
            format!("Gid:\t{GID}\t{GID}\t{GID}\t{GID}"),
            "Groups:\t".to_owned(),
            "CapInh:\t0000000000000000".to_owned(),
            "CapPrm:\t0000000000000000".to_owned(),
            "CapEff:\t0000000000000000".to_owned(),
            "CapBnd:\t0000000000000000".to_owned(),
            "CapAmb:\t0000000000000000".to_owned(),
            "NoNewPrivs:\t1".to_owned(),
        ]
        .join("\n")
    }

    #[test]
    fn exact_dropped_shape_passes() {
        verify_dropped_status_snapshot(&healthy(), UID, GID).expect("exact shape");
    }

    #[test]
    fn any_uid_slot_retaining_root_is_refused() {
        for index in 0..4 {
            let mut values = [UID; 4];
            values[index] = 0;
            let changed = healthy().replacen(
                &format!("Uid:\t{UID}\t{UID}\t{UID}\t{UID}"),
                &format!(
                    "Uid:\t{}\t{}\t{}\t{}",
                    values[0], values[1], values[2], values[3]
                ),
                1,
            );
            assert!(matches!(
                verify_dropped_status_snapshot(&changed, UID, GID),
                Err(DroppedPrivilegeError::UidMismatch { .. })
            ));
        }
    }

    #[test]
    fn any_gid_slot_retaining_root_is_refused() {
        for index in 0..4 {
            let mut values = [GID; 4];
            values[index] = 0;
            let changed = healthy().replacen(
                &format!("Gid:\t{GID}\t{GID}\t{GID}\t{GID}"),
                &format!(
                    "Gid:\t{}\t{}\t{}\t{}",
                    values[0], values[1], values[2], values[3]
                ),
                1,
            );
            assert!(matches!(
                verify_dropped_status_snapshot(&changed, UID, GID),
                Err(DroppedPrivilegeError::GidMismatch { .. })
            ));
        }
    }

    #[test]
    fn supplementary_group_is_refused() {
        let changed = healthy().replace("Groups:\t", "Groups:\t42");
        assert!(matches!(
            verify_dropped_status_snapshot(&changed, UID, GID),
            Err(DroppedPrivilegeError::SupplementaryGroups(_))
        ));
    }

    #[test]
    fn each_nonempty_capability_set_is_refused() {
        for field in CAPABILITY_FIELDS {
            let changed = healthy().replace(
                &format!("{field}:\t0000000000000000"),
                &format!("{field}:\t0000000000000001"),
            );
            assert!(matches!(
                verify_dropped_status_snapshot(&changed, UID, GID),
                Err(DroppedPrivilegeError::CapabilityNotEmpty { .. })
            ));
        }
    }

    #[test]
    fn no_new_privileges_zero_is_refused() {
        let changed = healthy().replace("NoNewPrivs:\t1", "NoNewPrivs:\t0");
        assert_eq!(
            verify_dropped_status_snapshot(&changed, UID, GID),
            Err(DroppedPrivilegeError::NoNewPrivilegesMismatch { actual: 0 })
        );
    }

    #[test]
    fn missing_duplicate_and_malformed_fields_are_refused() {
        let missing = healthy().replace("CapEff:\t0000000000000000\n", "");
        assert_eq!(
            verify_dropped_status_snapshot(&missing, UID, GID),
            Err(DroppedPrivilegeError::MissingField("CapEff"))
        );
        let duplicate = format!("{}\nCapEff:\t0000000000000000", healthy());
        assert_eq!(
            verify_dropped_status_snapshot(&duplicate, UID, GID),
            Err(DroppedPrivilegeError::DuplicateField("CapEff"))
        );
        let malformed = healthy().replace("NoNewPrivs:\t1", "NoNewPrivs:\tyes");
        assert_eq!(
            verify_dropped_status_snapshot(&malformed, UID, GID),
            Err(DroppedPrivilegeError::MalformedField("NoNewPrivs"))
        );
    }
}
