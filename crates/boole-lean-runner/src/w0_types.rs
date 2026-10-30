//! Portable W0 fixture schema; no OS operations or capability authorization.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u8,
    pub probe: PathBuf,
    pub comparator: PathBuf,
    pub nanoda: PathBuf,
    pub probe_sha256: String,
    pub bwrap: PathBuf,
    pub bwrap_sha256: String,
    pub cgroup: PathBuf,
    pub live_grant: PathBuf,
    pub grant_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    pub namespaces: BTreeMap<String, String>,
    pub landlock_abi: i32,
    pub landlock_enforced: bool,
    pub seccomp: u32,
    pub apparmor: String,
    pub dumpable: i32,
    pub caps_zero: bool,
    pub pid_one: bool,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Physical {
    Wall,
    Tasks,
    Memory,
    Capture,
}
#[derive(Clone, Debug, Serialize)]
pub struct Observation {
    pub action: Action,
    pub control_pass: bool,
    pub physical: Option<Physical>,
    pub setup_ok: bool,
    pub cleanup_complete: bool,
    pub private_channel_valid: bool,
    pub capabilities: Option<Capabilities>,
    pub output: Vec<u8>,
    pub reaped: u64,
    pub pids_events: u64,
    pub elapsed_ms: u64,
    pub pidfd_used: bool,
    pub observer_cross_checked: bool,
    pub nonce: String,
    pub parent_namespaces: BTreeMap<String, String>,
    pub private_channel: Vec<u8>,
    pub launcher_killed: bool,
    pub automatic_tree_empty: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Noop,
    NetworkV4,
    NetworkV6,
    Loopback,
    Dns,
    ReadSecret,
    WriteOutside,
    WriteInput,
    InheritedFd,
    ScratchExec,
    UnpinnedExec,
    Comparator,
    Workers,
    Tasks,
    Memory,
    SlowPrepare,
    BlockedRead,
    SlowD4,
    Tree,
    HoldTree,
    Capture,
}
impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Self::Noop => "noop",
            Self::NetworkV4 => "ipv4",
            Self::NetworkV6 => "ipv6",
            Self::Loopback => "loopback",
            Self::Dns => "dns",
            Self::ReadSecret => "read-secret",
            Self::WriteOutside => "write-outside",
            Self::WriteInput => "write-input",
            Self::InheritedFd => "inherited-fd",
            Self::ScratchExec => "scratch-exec",
            Self::UnpinnedExec => "unpinned-exec",
            Self::Comparator => "comparator",
            Self::Workers => "workers",
            Self::Tasks => "tasks",
            Self::Memory => "memory",
            Self::SlowPrepare => "slow-preparation",
            Self::BlockedRead => "blocked-read",
            Self::SlowD4 => "slow-d4",
            Self::Tree => "tree",
            Self::HoldTree => "hold-tree",
            Self::Capture => "capture",
        }
    }
    pub fn parse(value: &str) -> Result<Self> {
        [
            Self::Noop,
            Self::NetworkV4,
            Self::NetworkV6,
            Self::Loopback,
            Self::Dns,
            Self::ReadSecret,
            Self::WriteOutside,
            Self::WriteInput,
            Self::InheritedFd,
            Self::ScratchExec,
            Self::UnpinnedExec,
            Self::Comparator,
            Self::Workers,
            Self::Tasks,
            Self::Memory,
            Self::SlowPrepare,
            Self::BlockedRead,
            Self::SlowD4,
            Self::Tree,
            Self::HoldTree,
            Self::Capture,
        ]
        .into_iter()
        .find(|v| v.name() == value)
        .context("W0 closed probe action")
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeFacts {
    pub descendant_pid: u32,
    pub session_id: u32,
    pub session_leader: u32,
    pub adopted_by: u32,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeOutput {
    pub action: String,
    pub allowed: bool,
    pub errno: Option<i32>,
    pub workers: u32,
    pub marker: Option<String>,
    pub tree: Option<TreeFacts>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Prepared {
        nonce: String,
        capabilities: Capabilities,
    },
    Ready {
        nonce: String,
        capabilities: Capabilities,
    },
    Finished {
        nonce: String,
        code: i32,
        reaped: u64,
    },
}

#[derive(Debug)]
pub struct Channel {
    pub capabilities: Capabilities,
    pub code: Option<i32>,
    pub reaped: u64,
}
/// Private observer evidence is canonical, ordered and request-bound. A known
/// outer interruption permits a missing footer, never missing/foreign READY.
pub fn parse_channel(
    raw: &[u8],
    nonce: &str,
    parents: &BTreeMap<String, String>,
    interrupted: bool,
) -> Result<Channel> {
    use anyhow::ensure;
    ensure!(
        raw.len() <= 16384 && raw.ends_with(b"\n") && parents.len() == 6,
        "W0 private evidence size/framing"
    );
    let names = ["user", "net", "pid", "mnt", "ipc", "uts"];
    ensure!(
        names
            .iter()
            .all(|k| parents.get(*k).is_some_and(|s| !s.is_empty())),
        "W0 parent namespace identity"
    );
    let mut prepared = None;
    let mut ready = None;
    let mut code = None;
    let mut reaped = 0;
    for line in raw.split(|b| *b == b'\n').filter(|v| !v.is_empty()) {
        let event: Event = serde_json::from_slice(line)?;
        ensure!(
            serde_json::to_vec(&event)? == line,
            "W0 private canonical grammar"
        );
        match event {
            Event::Prepared {
                nonce: n,
                capabilities: c,
            } => {
                ensure!(
                    n == nonce
                        && prepared.is_none()
                        && ready.is_none()
                        && code.is_none()
                        && !c.landlock_enforced
                        && c.caps_zero
                        && c.seccomp == 2,
                    "W0 preparation/context/order"
                );
                prepared = Some(c.namespaces);
            }
            Event::Ready {
                nonce: n,
                capabilities: c,
            } => {
                ensure!(
                    n == nonce
                        && ready.is_none()
                        && code.is_none()
                        && c.pid_one
                        && c.dumpable == 0
                        && c.caps_zero
                        && c.seccomp == 2
                        && c.landlock_abi >= 4
                        && c.landlock_enforced
                        && prepared.as_ref() == Some(&c.namespaces)
                        && c.apparmor == "boole-gate-bwrap (unconfined)"
                        && c.namespaces.len() == 6
                        && names.iter().all(|k| c
                            .namespaces
                            .get(*k)
                            .is_some_and(|v| !v.is_empty() && parents.get(*k) != Some(v))),
                    "W0 private capability/context contradiction"
                );
                ready = Some(c);
            }
            Event::Finished {
                nonce: n,
                code: exit,
                reaped: r,
            } => {
                ensure!(
                    n == nonce && ready.is_some() && code.is_none(),
                    "W0 private footer/context/order"
                );
                code = Some(exit);
                reaped = r;
            }
        }
    }
    ensure!(
        interrupted || code.is_some(),
        "W0 footer missing without a trusted interruption"
    );
    Ok(Channel {
        capabilities: ready.context("W0 READY missing")?,
        code,
        reaped,
    })
}

/// Closed bubblewrap launch arguments shared with the preflight consumer.
pub const BWRAP_FLAGS: &[&str] = &[
    "--unshare-user",
    "--unshare-net",
    "--unshare-pid",
    "--unshare-ipc",
    "--unshare-uts",
    "--disable-userns",
    "--assert-userns-disabled",
    "--as-pid-1",
    "--die-with-parent",
    "--new-session",
    "--uid",
    "0",
    "--gid",
    "0",
    "--cap-drop",
    "ALL",
    "--cap-add",
    "CAP_SETPCAP",
    "--clearenv",
    "--proc",
    "/proc",
    "--dev",
    "/dev",
    "--tmpfs",
    "/tmp",
];

#[cfg(test)]
mod argv_tests {
    #[test]
    fn bubblewrap_launch_uses_supported_options() {
        let supported = [
            "--unshare-user",
            "--unshare-net",
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--disable-userns",
            "--assert-userns-disabled",
            "--as-pid-1",
            "--die-with-parent",
            "--new-session",
            "--uid",
            "--gid",
            "--cap-drop",
            "--cap-add",
            "--clearenv",
            "--proc",
            "--dev",
            "--tmpfs",
        ];
        for v in super::BWRAP_FLAGS.iter().filter(|v| v.starts_with("--")) {
            assert!(
                supported.contains(v),
                "unsupported bubblewrap argument: {v}"
            );
        }
    }
}

/// A host heartbeat reflects a locked authoritative ledger read. Expiry or any
/// revocation fails closed even when the guest grant copy has not changed.
pub const LIVE_FRAME_SIZE: usize = 4096;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveTick {
    pub version: u8,
    pub sequence: u64,
    pub grant: serde_json::Value,
    pub active: bool,
    pub tick_unix: u64,
    pub closing: bool,
    pub observed_monotonic_ns: u64,
}
pub fn validate_live(raw: &[u8], expected: &str, now: u64, mono: u64) -> Result<()> {
    validate_live_tick(raw, expected, now, mono).map(|_| ())
}
pub fn validate_live_tick(raw: &[u8], expected: &str, now: u64, mono: u64) -> Result<LiveTick> {
    use anyhow::ensure;
    use sha2::{Digest, Sha256};
    if raw.len() != LIVE_FRAME_SIZE || &raw[..8] != b"BW0SNAP3" {
        return Err(FrameTorn.into());
    }
    let end = LIVE_FRAME_SIZE - 32;
    if Sha256::digest(&raw[..end]).as_slice() != &raw[end..] {
        return Err(FrameTorn.into());
    }
    let size = u32::from_be_bytes(raw[16..20].try_into()?) as usize;
    ensure!(
        size <= end - 20 && raw[20 + size..end].iter().all(|b| *b == 0),
        "W0 heartbeat padding/length"
    );
    let value: serde_json::Value = serde_json::from_slice(&raw[20..20 + size])?;
    ensure!(
        serde_json::to_vec(&value)? == raw[20..20 + size],
        "W0 heartbeat canonical JSON"
    );
    let v: LiveTick = serde_json::from_value(value)?;
    ensure!(
        v.sequence == u64::from_be_bytes(raw[8..16].try_into()?) && v.sequence > 0,
        "W0 heartbeat sequence"
    );
    let pin: String = Sha256::digest(serde_json::to_vec(&v.grant)?)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    ensure!(
        v.grant.get("state").and_then(|v| v.as_str()) == Some("granted")
            && v.grant
                .get("start_unix")
                .and_then(|v| v.as_u64())
                .is_some_and(|v| v <= now.saturating_add(30))
            && v.grant
                .get("end_unix")
                .and_then(|v| v.as_u64())
                .is_some_and(|v| now.saturating_add(130) <= v.saturating_add(30))
            && v.version == 3
            && v.active
            && v.observed_monotonic_ns <= mono
            && mono.saturating_sub(v.observed_monotonic_ns) <= 3_000_000_000
            && pin == expected
            && v.tick_unix <= now.saturating_add(30)
            && now.saturating_sub(v.tick_unix) <= 33,
        "W0 heartbeat revoked/stale/foreign"
    );
    Ok(v)
}

#[derive(Debug, Serialize)]
pub struct ObserverDeath {
    pub sigkill: bool,
    pub members_before_kill: usize,
    pub automatic_empty: bool,
    pub cleanup_complete: bool,
}

impl ObserverDeath {
    pub fn qualifies(&self) -> bool {
        self.sigkill
            && self.members_before_kill >= 4
            && self.automatic_empty
            && self.cleanup_complete
    }
}

/// Allocation seam for the fixture's deliberately blocking sandbox input.
pub fn scratch_path(root: &std::path::Path, action: Action, nonce: &str) -> PathBuf {
    if action == Action::SlowD4 {
        std::path::Path::new("/dev/shm").join(format!("boole-w0-data-{nonce}"))
    } else {
        root.join(format!("data-{nonce}"))
    }
}

#[derive(Debug)]
pub struct FrameTorn;
impl std::fmt::Display for FrameTorn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "W0 snapshot torn/checksum/size")
    }
}
impl std::error::Error for FrameTorn {}

/// Read-only prerequisite seam shared by the Linux fixture and host controls.
pub fn userns_restriction(path: &std::path::Path) -> Result<Option<String>> {
    let value = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(value.trim() == "1", "W0 AppArmor userns policy");
    Ok(Some(value.trim().to_owned()))
}
