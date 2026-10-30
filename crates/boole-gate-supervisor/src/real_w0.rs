//! W0 fixture contract only. None of these receipts authorizes production dispatch.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Grant,
    Capability,
    Policy,
    Evidence,
    Cleanup,
}
impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "W0 fixture refused: {self:?}")
    }
}
impl std::error::Error for Refusal {}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub state: String,
    pub owner: String,
    pub step: String,
    pub start_unix: u64,
    pub end_unix: u64,
    pub request_id: String,
    pub generation: u64,
}
impl Grant {
    pub fn validate(&self, now: u64) -> Result<(), Refusal> {
        self.validate_skew(now, 0)
    }
    pub fn validate_guest(&self, now: u64) -> Result<(), Refusal> {
        self.validate_skew(now, 30)
    }
    fn validate_skew(&self, now: u64, skew: u64) -> Result<(), Refusal> {
        if self.state != "granted"
            || self.owner != "N"
            || self.step != "W0"
            || self.generation == 0
            || self.request_id.is_empty()
            || self.request_id.len() > 96
            || !self
                .request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            || self
                .end_unix
                .checked_sub(self.start_unix)
                .is_none_or(|n| n > 2700)
            || now.saturating_add(skew) < self.start_unix
            || now
                .checked_add(130)
                .is_none_or(|v| v > self.end_unix.saturating_add(skew))
        {
            return Err(Refusal::Grant);
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct Capabilities {
    pub namespaces: [bool; 6],
    pub landlock_abi: u32,
    pub seccomp: bool,
    pub apparmor: bool,
    pub delegated: bool,
}
impl Capabilities {
    pub fn validate(&self) -> Result<(), Refusal> {
        if !self.namespaces.iter().all(|v| *v)
            || self.landlock_abi < 4
            || !self.seccomp
            || !self.apparmor
            || !self.delegated
        {
            return Err(Refusal::Capability);
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct Policy {
    pub executables: Vec<String>,
    pub readonly: Vec<String>,
    pub scratch: String,
}
impl Policy {
    pub fn validate(&self) -> Result<(), Refusal> {
        use std::path::{Component, Path};
        fn absolute(p: &str) -> bool {
            Path::new(p).is_absolute()
                && p != "/"
                && !p.ends_with('/')
                && Path::new(p)
                    .components()
                    .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
        }
        if !absolute(&self.scratch)
            || self.executables.len() < 3
            || self.executables.len() > 8
            || self.readonly.is_empty()
            || self.readonly.len() > 16
        {
            return Err(Refusal::Policy);
        }
        let mut unique = std::collections::BTreeSet::new();
        for e in &self.executables {
            if !absolute(e)
                || !unique.insert(e)
                || ["/usr", "/bin", "/sbin", "/lib", "/lib64", "/usr/lib"].contains(&e.as_str())
                || Path::new(e).starts_with(&self.scratch)
                || self.readonly.iter().any(|r| Path::new(e) == Path::new(r))
            {
                return Err(Refusal::Policy);
            }
        }
        for r in &self.readonly {
            if !absolute(r)
                || ["/proc", "/sys", "/dev"].contains(&r.as_str())
                || Path::new(&self.scratch).starts_with(r)
                || Path::new(r).starts_with(&self.scratch)
            {
                return Err(Refusal::Policy);
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Transient,
    Node,
}
#[derive(Debug)]
pub struct Lifecycle {
    pub started: u64,
    pub wall: u64,
    pub recheck: u64,
    pub recheck_started: Option<u64>,
    pub first: Option<Stop>,
}
impl Lifecycle {
    pub fn observe(&mut self, now: u64, physical: bool, cleanup: bool) -> Option<Stop> {
        if self.first.is_none() {
            let (begin, bound) = self
                .recheck_started
                .map(|v| (v, self.recheck.min(self.wall).min(30000)))
                .unwrap_or((self.started, self.wall));
            self.first = if physical || now.saturating_sub(begin) >= bound {
                Some(Stop::Transient)
            } else if !cleanup {
                Some(Stop::Node)
            } else {
                None
            };
        }
        self.first
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: u8,
    pub case: String,
    pub request_id: String,
    pub generation: u64,
    pub backend_invocations: u64,
    pub control_pass: bool,
    pub tree_empty: bool,
    pub quiet_window_held: bool,
    pub assertions: BTreeMap<String, bool>,
}
pub fn required(case: &str) -> Option<&'static [&'static str]> {
    match case {
        "SBX-network" => Some(&[
            "ipv4-denied",
            "ipv6-denied",
            "loopback-denied",
            "dns-denied",
        ]),
        "SBX-filesystem" => Some(&[
            "secret-read-denied",
            "outside-write-denied",
            "input-write-denied",
            "inherited-fd-denied",
        ]),
        "SBX-exec" => Some(&[
            "scratch-exec-denied",
            "unpinned-exec-denied",
            "pinned-comparator-allowed",
            "four-nanoda-workers-allowed",
        ]),
        "SBX-capabilities" => Some(&[
            "consumer-missing-delegation-refused",
            "consumer-partial-landlock-refused",
            "consumer-missing-seccomp-refused",
            "consumer-missing-apparmor-refused",
            "consumer-missing-namespace-refused",
            "no-fallback",
        ]),
        "SBX-tasks" => Some(&[
            "task-limit-enforced",
            "memory-limit-enforced",
            "stop-is-T",
            "no-logical-reject",
        ]),
        "LIFE-preparation" => Some(&[
            "slow-preparation-bounded",
            "blocked-read-killed",
            "D4-read-bounded",
            "consumer-no-partial-publication",
        ]),
        "LIFE-deadlines" => Some(&[
            "absolute-job-deadline",
            "independent-bounded-recheck",
            "counted-T",
            "consumer-no-cached-verdict",
        ]),
        "LIFE-tree" => Some(&[
            "setsid-contained",
            "double-fork-reaped",
            "cgroup-empty",
            "pidfd-identity-bound",
        ]),
        "LIFE-parent-kill" => Some(&[
            "parent-sigkill",
            "all-descendants-retired",
            "observer-sigkill-drains",
            "cgroup-empty",
        ]),
        "LIFE-faults" => Some(&[
            "spawn-is-D-node",
            "consumer-cleanup-holds-publication",
            "capture-is-T",
            "forged-channel-refused",
            "no-terminal-verdict",
        ]),
        _ => None,
    }
}
pub fn validate_receipt(raw: &[u8], case: &str, grant: &Grant) -> Result<Receipt, Refusal> {
    if raw.len() > 65536 {
        return Err(Refusal::Evidence);
    }
    let r: Receipt = serde_json::from_slice(raw).map_err(|_| Refusal::Evidence)?;
    let keys = required(case).ok_or(Refusal::Evidence)?;
    let canonical = serde_json::to_vec(&r).map_err(|_| Refusal::Evidence)?;
    if raw.strip_suffix(b"\n").unwrap_or(raw) != canonical {
        return Err(Refusal::Evidence);
    }
    if r.version != 1
        || r.case != case
        || r.request_id != grant.request_id
        || r.generation != grant.generation
        || r.backend_invocations == 0
        || !r.control_pass
        || !r.tree_empty
        || !r.quiet_window_held
        || r.assertions.len() != keys.len()
        || !keys.iter().all(|k| r.assertions.get(*k) == Some(&true))
    {
        return Err(Refusal::Evidence);
    }
    Ok(r)
}

/// Source/build-input identity is not a production executable attestation.
pub fn source_digest() -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"BOOLE-M3/W0-SOURCE/v1");
    for (name, raw) in [
        (
            "crates/boole-lean-runner/src/lib.rs",
            include_bytes!("../../boole-lean-runner/src/lib.rs").as_slice(),
        ),
        (
            "crates/boole-lean-runner/src/w0_types.rs",
            include_bytes!("../../boole-lean-runner/src/w0_types.rs").as_slice(),
        ),
        (
            "crates/boole-lean-runner/src/w0_linux.rs",
            include_bytes!("../../boole-lean-runner/src/w0_linux.rs").as_slice(),
        ),
        (
            "crates/boole-lean-runner/src/w0_linux_payload.rs",
            include_bytes!("../../boole-lean-runner/src/w0_linux_payload.rs").as_slice(),
        ),
        (
            "crates/boole-gate-supervisor/src/real_w0.rs",
            include_bytes!("real_w0.rs").as_slice(),
        ),
        (
            "crates/boole-gate-supervisor/src/w0_fixture.rs",
            include_bytes!("w0_fixture.rs").as_slice(),
        ),
        (
            "crates/boole-gate-supervisor/src/bin/boole-w0-probe.rs",
            include_bytes!("bin/boole-w0-probe.rs").as_slice(),
        ),
        (
            "scripts/devnet/w0/runner.py",
            include_bytes!("../../../scripts/devnet/w0/runner.py").as_slice(),
        ),
        (
            "scripts/devnet/w0/guest.sh",
            include_bytes!("../../../scripts/devnet/w0/guest.sh").as_slice(),
        ),
        (
            "scripts/devnet/w0/window.py",
            include_bytes!("../../../scripts/devnet/w0/window.py").as_slice(),
        ),
        (
            "scripts/zk_registry/data/acceptance_tools_v2.json",
            include_bytes!("../../../scripts/zk_registry/data/acceptance_tools_v2.json").as_slice(),
        ),
        (
            "scripts/devnet/w0/authority.py",
            include_bytes!("../../../scripts/devnet/w0/authority.py").as_slice(),
        ),
        (
            "scripts/devnet/w0/preflight.py",
            include_bytes!("../../../scripts/devnet/w0/preflight.py").as_slice(),
        ),
        (
            "crates/boole-gate-supervisor/Cargo.toml",
            include_bytes!("../../../crates/boole-gate-supervisor/Cargo.toml").as_slice(),
        ),
        (
            "crates/boole-lean-runner/Cargo.toml",
            include_bytes!("../../../crates/boole-lean-runner/Cargo.toml").as_slice(),
        ),
        (
            "Cargo.lock",
            include_bytes!("../../../Cargo.lock").as_slice(),
        ),
        (
            "rust-toolchain.toml",
            include_bytes!("../../../rust-toolchain.toml").as_slice(),
        ),
        (
            "scripts/devnet/w0/custody.py",
            include_bytes!("../../../scripts/devnet/w0/custody.py").as_slice(),
        ),
        (
            "scripts/devnet/w0/budget.py",
            include_bytes!("../../../scripts/devnet/w0/budget.py").as_slice(),
        ),
        (
            "scripts/devnet/w0/kernel_check.py",
            include_bytes!("../../../scripts/devnet/w0/kernel_check.py").as_slice(),
        ),
        (
            "scripts/devnet/w0/heartbeat.py",
            include_bytes!("../../../scripts/devnet/w0/heartbeat.py").as_slice(),
        ),
        (
            "scripts/devnet/w0/inventory.py",
            include_bytes!("../../../scripts/devnet/w0/inventory.py").as_slice(),
        ),
        (
            "scripts/devnet/w0/inputs.py",
            include_bytes!("../../../scripts/devnet/w0/inputs.py").as_slice(),
        ),
        (
            "scripts/devnet/w0/toolchain-v1.json",
            include_bytes!("../../../scripts/devnet/w0/toolchain-v1.json").as_slice(),
        ),
    ] {
        h.update((name.len() as u32).to_be_bytes());
        h.update(name.as_bytes());
        h.update((raw.len() as u64).to_be_bytes());
        h.update(raw);
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub const RUNNER_SHA256: Option<[u8; 32]> = Some([
    245, 235, 15, 251, 138, 67, 72, 49, 163, 31, 213, 7, 175, 182, 61, 230, 52, 72, 110, 102, 179,
    136, 4, 124, 175, 93, 44, 160, 213, 43, 226, 156,
]);

/// Reserve evidence before execution; retain the owned scratch if copying fails.
pub struct EvidenceGuard {
    source: Option<crate::codec::Scratch>,
    destination: std::path::PathBuf,
    finished: bool,
}
impl EvidenceGuard {
    pub fn new(
        source: crate::codec::Scratch,
        destination: &std::path::Path,
    ) -> anyhow::Result<Self> {
        reserve_evidence(destination)?;
        Ok(Self {
            source: Some(source),
            destination: destination.into(),
            finished: false,
        })
    }
    pub fn source(&self) -> &std::path::Path {
        &self.source.as_ref().expect("owned scratch").0
    }
    fn retain(&mut self) -> anyhow::Result<()> {
        let result = copy_evidence(self.source(), &self.destination);
        if result.is_err() {
            // Deliberate preservation: an incomplete durable copy must not delete
            // the original evidence, including oversized or nonregular entries.
            if let Some(source) = self.source.take() {
                std::mem::forget(source);
            }
        }
        self.finished = true;
        result
    }
    pub fn finish(mut self, outcome: anyhow::Result<()>) -> anyhow::Result<()> {
        combine_evidence(outcome, self.retain())
    }
}
impl Drop for EvidenceGuard {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.retain();
        }
    }
}
fn combine_evidence(outcome: anyhow::Result<()>, retain: anyhow::Result<()>) -> anyhow::Result<()> {
    match (outcome, retain) {
        (Err(e), Err(copy)) => Err(e.context(format!("evidence retention failed: {copy}"))),
        (Err(e), Ok(())) | (Ok(()), Err(e)) => Err(e),
        (Ok(()), Ok(())) => Ok(()),
    }
}
fn reserve_evidence(destination: &std::path::Path) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::os::unix::fs::PermissionsExt;
    let parent = destination.parent().context("evidence parent")?;
    boole_lean_runner::artifact_handles::private_directory(parent)?;
    std::fs::create_dir(destination)?;
    std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o700))?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}
pub fn finish_evidence(
    source: &std::path::Path,
    destination: &std::path::Path,
    outcome: anyhow::Result<()>,
) -> anyhow::Result<()> {
    reserve_evidence(destination)?;
    combine_evidence(outcome, copy_evidence(source, destination))
}
fn copy_evidence(source: &std::path::Path, destination: &std::path::Path) -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        io::{Read, Write},
        os::unix::fs::PermissionsExt,
    };
    boole_lean_runner::artifact_handles::private_directory(source)?;
    boole_lean_runner::artifact_handles::private_directory(destination)?;
    let write = |name: &str, raw: &[u8]| -> anyhow::Result<()> {
        let mut f = boole_lean_runner::artifact_handles::create_private(&destination.join(name))?;
        f.write_all(raw)?;
        f.set_permissions(fs::Permissions::from_mode(0o400))?;
        f.sync_all()?;
        Ok(())
    };
    let mut hashes = BTreeMap::new();
    let mut failures = false;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().context("evidence filename")?;
        if !(matches!(name, "receipt.json" | "diagnostic")
            || name.ends_with(".private.ndjson")
            || name.ends_with(".stderr")
            || name.ends_with(".stdout")
            || name.ends_with(".observation.json"))
        {
            continue;
        }
        let mut record = serde_json::json!({});
        let copied = (|| -> anyhow::Result<()> {
            let mut f = boole_lean_runner::artifact_handles::open_regular(&entry.path())?;
            let before = f.metadata()?;
            let mut raw = vec![];
            (&mut f).take(1048577).read_to_end(&mut raw)?;
            let truncated = raw.len() > 1048576 || before.len() > 1048576;
            raw.truncate(1048576);
            write(name, &raw)?;
            let digest: String = Sha256::digest(&raw)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            record = serde_json::json!({"sha256":digest,"bytes":raw.len(),"original_size":before.len(),"truncated":truncated});
            ensure!(!truncated, "evidence truncated at 1 MiB");
            Ok(())
        })();
        if let Err(error) = copied {
            failures = true;
            record["error"] = error.to_string().into();
        }
        hashes.insert(name.to_string(), record);
    }
    write("hashes.json", &serde_json::to_vec(&hashes)?)?;
    fs::File::open(destination)?.sync_all()?;
    fs::File::open(destination.parent().context("evidence parent")?)?.sync_all()?;
    ensure!(
        !failures,
        "evidence retention incomplete; original scratch preserved"
    );
    Ok(())
}
