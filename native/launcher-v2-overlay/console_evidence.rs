//! Guest-side producer for the sealed one-line console evidence protocol.
//!
//! Every value here is an observation.  The host compares it with values fixed
//! before a machine exists; no guest record can assert that a condition passed.

use std::fmt::Write as _;
use std::io::{self, Write};

use thiserror::Error;

pub const PREFIX: &str = "BOOLE-GUEST-EVIDENCE-1";
pub const RECORD_IDS: [&str; 4] = [
    "launcher-executable",
    "launcher-prerequisites",
    "supervisor-privilege",
    "readiness",
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Prerequisite {
    pub name: String,
    pub resolved: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SupervisorPrivilege {
    pub uids: [u32; 4],
    pub gids: [u32; 4],
    pub capabilities_inheritable: u64,
    pub capabilities_permitted: u64,
    pub capabilities_effective: u64,
    pub capabilities_bounding: u64,
    pub capabilities_ambient: u64,
    pub no_new_privileges: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Record {
    LauncherExecutable {
        path: String,
        sha256: String,
    },
    LauncherPrerequisites(Vec<Prerequisite>),
    SupervisorPrivilege(SupervisorPrivilege),
    Readiness {
        ready: bool,
        failed_units: Vec<String>,
    },
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ObserveError {
    #[error("thread status is missing field {0}")]
    MissingField(&'static str),
    #[error("thread status repeats field {0}")]
    DuplicateField(&'static str),
    #[error("thread status field {0} is malformed")]
    MalformedField(&'static str),
}

impl Record {
    #[must_use]
    pub fn id(&self) -> &'static str {
        match self {
            Self::LauncherExecutable { .. } => "launcher-executable",
            Self::LauncherPrerequisites(_) => "launcher-prerequisites",
            Self::SupervisorPrivilege(_) => "supervisor-privilege",
            Self::Readiness { .. } => "readiness",
        }
    }

    #[must_use]
    pub fn render(&self) -> String {
        let mut line = String::with_capacity(256);
        line.push_str(PREFIX);
        line.push(' ');
        line.push_str(self.id());
        line.push(' ');
        match self {
            Self::LauncherExecutable { path, sha256 } => {
                line.push_str("{\"path\":");
                push_string(&mut line, path);
                line.push_str(",\"sha256\":");
                push_string(&mut line, sha256);
                line.push('}');
            }
            Self::LauncherPrerequisites(rows) => {
                line.push_str("{\"prerequisites\":[");
                for (index, row) in rows.iter().enumerate() {
                    if index > 0 {
                        line.push(',');
                    }
                    line.push_str("{\"name\":");
                    push_string(&mut line, &row.name);
                    line.push_str(",\"resolved\":");
                    line.push_str(if row.resolved { "true" } else { "false" });
                    line.push('}');
                }
                line.push_str("]}");
            }
            Self::SupervisorPrivilege(observed) => {
                let _ = write!(
                    line,
                    "{{\"capabilitiesAmbient\":\"{:016x}\",\
                     \"capabilitiesBounding\":\"{:016x}\",\
                     \"capabilitiesEffective\":\"{:016x}\",\
                     \"capabilitiesInheritable\":\"{:016x}\",\
                     \"capabilitiesPermitted\":\"{:016x}\",\
                     \"gids\":[{},{},{},{}],\
                     \"noNewPrivileges\":{},\
                     \"uids\":[{},{},{},{}]}}",
                    observed.capabilities_ambient,
                    observed.capabilities_bounding,
                    observed.capabilities_effective,
                    observed.capabilities_inheritable,
                    observed.capabilities_permitted,
                    observed.gids[0],
                    observed.gids[1],
                    observed.gids[2],
                    observed.gids[3],
                    observed.no_new_privileges,
                    observed.uids[0],
                    observed.uids[1],
                    observed.uids[2],
                    observed.uids[3],
                );
            }
            Self::Readiness {
                ready,
                failed_units,
            } => {
                line.push_str("{\"failedUnits\":[");
                for (index, unit) in failed_units.iter().enumerate() {
                    if index > 0 {
                        line.push(',');
                    }
                    push_string(&mut line, unit);
                }
                line.push_str("],\"ready\":");
                line.push_str(if *ready { "true" } else { "false" });
                line.push('}');
            }
        }
        line
    }
}

fn push_string(target: &mut String, value: &str) {
    target.push('"');
    for character in value.chars() {
        match character {
            '"' => target.push_str("\\\""),
            '\\' => target.push_str("\\\\"),
            '\n' => target.push_str("\\n"),
            '\r' => target.push_str("\\r"),
            '\t' => target.push_str("\\t"),
            '\u{08}' => target.push_str("\\b"),
            '\u{0c}' => target.push_str("\\f"),
            control if control < ' ' || control == '\u{7f}' => {
                let _ = write!(target, "\\u{:04x}", control as u32);
            }
            other => target.push(other),
        }
    }
    target.push('"');
}

pub fn observe_supervisor_privilege(status: &str) -> Result<SupervisorPrivilege, ObserveError> {
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
            "Uid" => set_once(&mut uids, "Uid", parse_ids("Uid", value)?)?,
            "Gid" => set_once(&mut gids, "Gid", parse_ids("Gid", value)?)?,
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
    Ok(SupervisorPrivilege {
        uids: required(uids, "Uid")?,
        gids: required(gids, "Gid")?,
        capabilities_inheritable: required(cap_inheritable, "CapInh")?,
        capabilities_permitted: required(cap_permitted, "CapPrm")?,
        capabilities_effective: required(cap_effective, "CapEff")?,
        capabilities_bounding: required(cap_bounding, "CapBnd")?,
        capabilities_ambient: required(cap_ambient, "CapAmb")?,
        no_new_privileges: required(no_new_privileges, "NoNewPrivs")?,
    })
}

pub fn emit(sink: &mut impl Write, records: &[Record]) -> io::Result<()> {
    for record in records {
        writeln!(sink, "{}", record.render())?;
    }
    sink.flush()
}

fn set_once<T>(slot: &mut Option<T>, field: &'static str, value: T) -> Result<(), ObserveError> {
    if slot.replace(value).is_some() {
        return Err(ObserveError::DuplicateField(field));
    }
    Ok(())
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ObserveError> {
    value.ok_or(ObserveError::MissingField(field))
}

fn parse_ids(field: &'static str, value: &str) -> Result<[u32; 4], ObserveError> {
    let values = value
        .split_whitespace()
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ObserveError::MalformedField(field))?;
    values
        .try_into()
        .map_err(|_| ObserveError::MalformedField(field))
}

fn parse_capability(field: &'static str, value: &str) -> Result<u64, ObserveError> {
    let value = value.trim();
    if value.len() != 16 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ObserveError::MalformedField(field));
    }
    u64::from_str_radix(value, 16).map_err(|_| ObserveError::MalformedField(field))
}

fn parse_decimal(field: &'static str, value: &str) -> Result<u32, ObserveError> {
    value
        .trim()
        .parse()
        .map_err(|_| ObserveError::MalformedField(field))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        emit, observe_supervisor_privilege, ObserveError, Prerequisite, Record,
        SupervisorPrivilege, PREFIX,
    };

    fn privilege() -> SupervisorPrivilege {
        SupervisorPrivilege {
            uids: [0; 4],
            gids: [0; 4],
            capabilities_inheritable: 0,
            capabilities_permitted: 0x2001c0,
            capabilities_effective: 0x2001c0,
            capabilities_bounding: 0x2001c0,
            capabilities_ambient: 0,
            no_new_privileges: 0,
        }
    }

    fn status() -> String {
        [
            "Uid:\t0\t0\t0\t0",
            "Gid:\t0\t0\t0\t0",
            "CapInh:\t0000000000000000",
            "CapPrm:\t00000000002001c0",
            "CapEff:\t00000000002001c0",
            "CapBnd:\t00000000002001c0",
            "CapAmb:\t0000000000000000",
            "NoNewPrivs:\t0",
        ]
        .join("\n")
    }

    #[test]
    fn every_record_is_one_line_json_with_the_fixed_prefix() {
        let records = [
            Record::LauncherExecutable {
                path: "/usr/libexec/boole/launcher\nnot-a-second-line".to_owned(),
                sha256: "a".repeat(64),
            },
            Record::LauncherPrerequisites(vec![Prerequisite {
                name: "runtime-rootfs-replay".to_owned(),
                resolved: true,
            }]),
            Record::SupervisorPrivilege(privilege()),
            Record::Readiness {
                ready: true,
                failed_units: Vec::new(),
            },
        ];
        for record in records {
            let rendered = record.render();
            assert!(rendered.starts_with(PREFIX));
            assert_eq!(rendered.lines().count(), 1);
            let (_, payload) = rendered.rsplit_once(' ').expect("record payload");
            serde_json::from_str::<serde_json::Value>(payload).expect("valid JSON");
        }
    }

    #[test]
    fn prerequisite_key_is_resolved_and_never_present() {
        let rendered = Record::LauncherPrerequisites(vec![Prerequisite {
            name: "toolchain".to_owned(),
            resolved: true,
        }])
        .render();
        assert!(rendered.contains("\"resolved\":true"));
        assert!(!rendered.contains("\"present\""));
    }

    #[test]
    fn full_supervisor_snapshot_is_reported_without_a_verdict() {
        let rendered = Record::SupervisorPrivilege(privilege()).render();
        for key in [
            "uids",
            "gids",
            "capabilitiesInheritable",
            "capabilitiesPermitted",
            "capabilitiesEffective",
            "capabilitiesBounding",
            "capabilitiesAmbient",
            "noNewPrivileges",
        ] {
            assert!(rendered.contains(key), "missing {key}");
        }
        for forbidden in ["passed", "verdict", "conditionMet"] {
            assert!(!rendered.contains(forbidden));
        }
    }

    #[test]
    fn status_observation_is_exact_and_duplicate_fields_fail() {
        assert_eq!(observe_supervisor_privilege(&status()), Ok(privilege()));
        let duplicate = format!("{}\nUid:\t0\t0\t0\t0", status());
        assert_eq!(
            observe_supervisor_privilege(&duplicate),
            Err(ObserveError::DuplicateField("Uid"))
        );
    }

    #[test]
    fn emit_flushes_complete_lines() {
        let mut sink = Vec::new();
        emit(
            &mut sink,
            &[Record::Readiness {
                ready: true,
                failed_units: Vec::new(),
            }],
        )
        .expect("vector write");
        let text = String::from_utf8(sink).expect("UTF-8");
        assert!(text.ends_with('\n'));
        assert_eq!(text.lines().count(), 1);
    }

    #[test]
    fn producer_lines_equal_the_shared_host_fixture() {
        let prerequisites = [
            "fixed-launcher-prelock-prerequisites",
            "fixed-launcher-lifetime-lock",
            "fresh-launcher-instance",
            "fixed-manager-cgroup",
            "fixed-startup-orphan-recovery",
            "fixed-startup-toolchain-compatibility",
            "runtime-rootfs-replay",
            "closed-local-replay-startup",
            "failed-unit-query",
        ]
        .into_iter()
        .map(|name| Prerequisite {
            name: name.to_owned(),
            resolved: true,
        })
        .collect();
        let records = [
            Record::LauncherExecutable {
                path: "/usr/libexec/boole/boole-native-shadow-launcher".to_owned(),
                sha256: "0".repeat(64),
            },
            Record::LauncherPrerequisites(prerequisites),
            Record::SupervisorPrivilege(privilege()),
            Record::Readiness {
                ready: true,
                failed_units: Vec::new(),
            },
        ];
        let rendered = records
            .iter()
            .map(Record::render)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
            "../../native/containment/\
             native-shadow-launcher-v2-console-evidence-example.txt",
        );
        assert_eq!(
            rendered,
            std::fs::read_to_string(fixture).expect("shared host fixture")
        );
    }
}
