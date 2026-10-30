//! Guest-only W0 producer. Static tool stand-ins exercise executable slots and
//! four-worker scheduling; this is not a Comparator/nanoda proof qualification.
use anyhow::{bail, ensure, Context, Result};
use boole_gate_supervisor::real_w0::{self, Capabilities, Grant, Lifecycle, Receipt, Stop};
#[cfg(target_os = "linux")]
use boole_lean_runner::w0_linux;
use boole_lean_runner::w0_types::{Action, Config, Observation, Physical, ProbeOutput};
#[cfg(not(target_os = "linux"))]
mod w0_linux {
    use super::*;
    pub fn run(
        _c: &Config,
        _root: &Path,
        _action: Action,
        _wall: Duration,
        _kill: bool,
    ) -> Result<Observation> {
        bail!("W0 Linux backend unavailable on host")
    }
    pub fn helper(_args: &[String]) -> Result<()> {
        bail!("W0 Linux helper unavailable on host")
    }
    pub fn observer_death(
        _c: &Config,
        _root: &Path,
    ) -> Result<boole_lean_runner::w0_types::ObserverDeath> {
        bail!("W0 Linux observer unavailable on host")
    }
    pub fn live(_c: &Config) -> Result<()> {
        bail!("W0 Linux heartbeat unavailable on host")
    }
    pub fn preflight(_c: &Config) -> Result<()> {
        bail!("W0 Linux preflight unavailable on host")
    }
}
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    version: u8,
    backend: Config,
    grant: PathBuf,
    grant_origin: PathBuf,
    lock: PathBuf,
    source_sha256: String,
}
fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
fn read_private(path: &Path, cap: usize) -> Result<Vec<u8>> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let mut f = boole_lean_runner::artifact_handles::open_regular(path)?;
    let m = f.metadata()?;
    ensure!(
        m.uid() == unsafe_uid()?
            && m.nlink() == 1
            && m.permissions().mode() & 0o077 == 0
            && m.len() <= cap as u64,
        "W0 private file identity/mode/size"
    );
    use std::io::Read;
    let mut raw = vec![];
    (&mut f).take(cap as u64 + 1).read_to_end(&mut raw)?;
    let after = f.metadata()?;
    let named = fs::symlink_metadata(path)?;
    ensure!(
        raw.len() <= cap
            && after.dev() == m.dev()
            && after.ino() == m.ino()
            && after.len() == m.len()
            && after.mtime() == m.mtime()
            && after.mtime_nsec() == m.mtime_nsec()
            && after.ctime() == m.ctime()
            && after.ctime_nsec() == m.ctime_nsec()
            && !named.file_type().is_symlink()
            && named.dev() == m.dev()
            && named.ino() == m.ino(),
        "W0 private file changed"
    );
    Ok(raw)
}
fn unsafe_uid() -> Result<u32> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::metadata("/proc/self")?.uid())
}
fn valid(o: &Observation) -> bool {
    o.setup_ok
        && o.private_channel_valid
        && o.cleanup_complete
        && o.observer_cross_checked
        && o.capabilities.as_ref().is_some_and(|c| {
            c.pid_one
                && c.dumpable == 0
                && c.caps_zero
                && c.seccomp == 2
                && c.landlock_abi >= 4
                && c.landlock_enforced
                && c.apparmor == "boole-gate-bwrap (unconfined)"
                && c.namespaces.len() == 6
                && ["user", "net", "pid", "mnt", "ipc", "uts"]
                    .iter()
                    .all(|n| c.namespaces.get(*n).is_some_and(|v| !v.is_empty()))
        })
}
fn parsed(o: &Observation) -> Result<ProbeOutput> {
    let line = o
        .output
        .split(|b| *b == b'\n')
        .rev()
        .find(|v| !v.is_empty())
        .context("W0 bounded probe output absent")?;
    let p: ProbeOutput = serde_json::from_slice(line)?;
    ensure!(p.action == o.action.name(), "W0 probe action binding");
    Ok(p)
}
fn allowed(o: &Observation) -> bool {
    valid(o) && o.physical.is_none() && parsed(o).is_ok_and(|p| p.allowed)
}
fn denied(o: &Observation, network: bool) -> bool {
    valid(o)
        && o.control_pass
        && o.physical.is_none()
        && parsed(o).is_ok_and(|p| {
            !p.allowed
                && p.errno.is_some_and(|e| {
                    if network {
                        [1, 13].contains(&e)
                    } else {
                        match o.action {
                            Action::ReadSecret => e == 13,
                            Action::ScratchExec | Action::UnpinnedExec => [1, 13].contains(&e),
                            Action::WriteInput => [1, 13, 30].contains(&e),
                            Action::InheritedFd => e == 9,
                            _ => [2, 13, 30].contains(&e),
                        }
                    }
                })
        })
}
/// Explicit consumer control: no publication on a physical stop or failed cleanup.
fn publish_control(o: &Observation, path: &Path) -> Result<()> {
    ensure!(allowed(o), "W0 publication fence");
    use std::io::Write;
    let mut file = boole_lean_runner::artifact_handles::create_private(path)?;
    file.write_all(b"consumer-result")?;
    file.sync_all()?;
    Ok(())
}
struct Fixture<'a> {
    config: &'a Config,
    root: &'a Path,
    count: u64,
    control_pass: bool,
    tree_empty: bool,
}
impl Fixture<'_> {
    fn track(&mut self, o: Observation) -> Observation {
        self.control_pass |= o.action == Action::Noop && allowed(&o);
        self.tree_empty &= o.cleanup_complete;
        o
    }
    fn run(&mut self, action: Action) -> Result<Observation> {
        self.count += 1;
        let o = w0_linux::run(
            self.config,
            self.root,
            action,
            Duration::from_secs(if action == Action::Noop { 3 } else { 2 }),
            false,
        )?;
        Ok(self.track(o))
    }
    fn interrupted(&mut self, action: Action) -> Result<Observation> {
        self.count += 1;
        let o = w0_linux::run(
            self.config,
            self.root,
            action,
            Duration::from_secs(1),
            false,
        )?;
        Ok(self.track(o))
    }
}
fn caps(o: &Observation) -> Result<Capabilities> {
    let c = o.capabilities.as_ref().context("W0 kernel capabilities")?;
    Ok(Capabilities {
        namespaces: std::array::from_fn(|i| {
            let k = ["user", "net", "pid", "mnt", "ipc", "uts"][i];
            c.namespaces
                .get(k)
                .is_some_and(|v| Some(v) != o.parent_namespaces.get(k))
        }),
        landlock_abi: c.landlock_abi.try_into()?,
        seccomp: c.seccomp == 2,
        apparmor: c.apparmor == "boole-gate-bwrap (unconfined)",
        delegated: valid(o),
    })
}
fn observe_case(case: &str, f: &mut Fixture<'_>, _g: &Grant) -> Result<BTreeMap<String, bool>> {
    let control = f.run(Action::Noop)?;
    ensure!(allowed(&control), "W0 positive kernel control failed");
    let mut rows = BTreeMap::new();
    let mut put = |name: &str, value: bool| {
        rows.insert(name.to_owned(), value);
    };
    match case {
        "SBX-network" => {
            for (name, action) in [
                ("ipv4-denied", Action::NetworkV4),
                ("ipv6-denied", Action::NetworkV6),
                ("loopback-denied", Action::Loopback),
                ("dns-denied", Action::Dns),
            ] {
                put(name, denied(&f.run(action)?, true));
            }
        }
        "SBX-filesystem" => {
            for (name, action) in [
                ("secret-read-denied", Action::ReadSecret),
                ("outside-write-denied", Action::WriteOutside),
                ("input-write-denied", Action::WriteInput),
                ("inherited-fd-denied", Action::InheritedFd),
            ] {
                put(name, denied(&f.run(action)?, false));
            }
        }
        "SBX-exec" => {
            put(
                "scratch-exec-denied",
                denied(&f.run(Action::ScratchExec)?, false),
            );
            put(
                "unpinned-exec-denied",
                denied(&f.run(Action::UnpinnedExec)?, false),
            );
            put(
                "pinned-comparator-allowed",
                allowed(&f.run(Action::Comparator)?),
            );
            let o = f.run(Action::Workers)?;
            put(
                "four-nanoda-workers-allowed",
                allowed(&o) && parsed(&o)?.workers == 4,
            );
        }
        "SBX-capabilities" => {
            let c = caps(&control)?;
            c.validate()?;
            let mut b = c.clone();
            b.delegated = false;
            put("consumer-missing-delegation-refused", b.validate().is_err());
            b = c.clone();
            b.landlock_abi = 3;
            put("consumer-partial-landlock-refused", b.validate().is_err());
            b = c.clone();
            b.seccomp = false;
            put("consumer-missing-seccomp-refused", b.validate().is_err());
            b = c.clone();
            b.apparmor = false;
            put("consumer-missing-apparmor-refused", b.validate().is_err());
            let all = (0..6).all(|i| {
                let mut b = c.clone();
                b.namespaces[i] = false;
                b.validate().is_err()
            });
            put("consumer-missing-namespace-refused", all);
            let mut missing = f.config.clone();
            missing.cgroup = f.root.join("not-a-delegation");
            fs::create_dir(&missing.cgroup)?;
            put(
                "no-fallback",
                w0_linux::preflight(&missing).is_err()
                    && boole_gate_supervisor::real_linux::select("linux-aarch64").is_err(),
            );
        }
        "SBX-tasks" => {
            let o = f.run(Action::Tasks)?;
            put(
                "task-limit-enforced",
                valid(&o) && o.pids_events > 0 && o.physical == Some(Physical::Tasks),
            );
            let memory = f.run(Action::Memory)?;
            put(
                "memory-limit-enforced",
                valid(&memory) && memory.physical == Some(Physical::Memory),
            );
            put("stop-is-T", o.physical == Some(Physical::Tasks));
            put(
                "no-logical-reject",
                o.cleanup_complete && o.physical.is_some(),
            );
        }
        "LIFE-preparation" => {
            let mut all = true;
            for (name, a) in [
                ("slow-preparation-bounded", Action::SlowPrepare),
                ("blocked-read-killed", Action::BlockedRead),
                ("D4-read-bounded", Action::SlowD4),
            ] {
                let o = f.interrupted(a)?;
                let pass = valid(&o) && o.physical == Some(Physical::Wall) && o.elapsed_ms < 4000;
                all &= pass;
                put(name, pass);
            }
            // Consumer publication fence, not a production archive claim.
            let mut interrupted = control.clone();
            interrupted.physical = Some(Physical::Wall);
            let destination = f.root.join("published-artifact");
            let refused = publish_control(&interrupted, &destination).is_err();
            put(
                "consumer-no-partial-publication",
                all && refused && !destination.exists(),
            );
        }
        "LIFE-deadlines" => {
            let o = f.interrupted(Action::SlowPrepare)?;
            put(
                "absolute-job-deadline",
                valid(&o) && o.physical == Some(Physical::Wall) && o.elapsed_ms < 4000,
            );
            f.count += 1;
            let recheck = w0_linux::run(
                f.config,
                f.root,
                Action::Noop,
                Duration::from_secs(1),
                false,
            )?;
            let recheck = f.track(recheck);
            let bound = f.interrupted(Action::SlowPrepare)?;
            put(
                "independent-bounded-recheck",
                allowed(&recheck) && valid(&bound) && bound.physical == Some(Physical::Wall),
            );
            let mut state = Lifecycle {
                started: 0,
                wall: 1000,
                recheck: 1000,
                recheck_started: None,
                first: None,
            };
            let stopped = state.observe(1000, false, true);
            let mut retry = boole_gate_supervisor::real_retry::Retry::default();
            if stopped == Some(Stop::Transient) {
                retry.deferred(10, "T", false);
            }
            put(
                "counted-T",
                stopped == Some(Stop::Transient) && retry.attempts == 1 && retry.next_at == 70,
            );
            put(
                "consumer-no-cached-verdict",
                publish_control(&o, &f.root.join("cached-verdict")).is_err()
                    && publish_control(&bound, &f.root.join("cached-verdict")).is_err()
                    && !f.root.join("cached-verdict").exists(),
            );
        }
        "LIFE-tree" => {
            let o = f.run(Action::Tree)?;
            put(
                "setsid-contained",
                allowed(&o)
                    && parsed(&o)?.tree.is_some_and(|t| {
                        t.descendant_pid > 1
                            && t.session_id == t.session_leader
                            && t.session_leader > 1
                            && t.adopted_by == 1
                    }),
            );
            put("double-fork-reaped", o.reaped >= 2);
            put("cgroup-empty", o.cleanup_complete);
            put("pidfd-identity-bound", o.pidfd_used && o.cleanup_complete);
        }
        "LIFE-parent-kill" => {
            f.count += 1;
            let o = w0_linux::run(
                f.config,
                f.root,
                Action::HoldTree,
                Duration::from_secs(4),
                true,
            )?;
            let o = f.track(o);
            put("parent-sigkill", o.launcher_killed);
            put(
                "all-descendants-retired",
                o.automatic_tree_empty && o.pidfd_used,
            );
            let observer = w0_linux::observer_death(f.config, f.root)?;
            f.count += 1;
            f.tree_empty &= observer.cleanup_complete;
            put("observer-sigkill-drains", observer.qualifies());
            put(
                "cgroup-empty",
                o.cleanup_complete && observer.cleanup_complete,
            );
        }
        "LIFE-faults" => {
            let mut missing = f.config.clone();
            missing.probe = f.root.join("absent-tool");
            put(
                "spawn-is-D-node",
                w0_linux::run(
                    &missing,
                    f.root,
                    Action::Noop,
                    Duration::from_secs(1),
                    false,
                )
                .is_err(),
            );
            // A deliberate cleanup I/O-failure seam exercises the same publication
            // fence. Actual cgroup drain remains mandatory for this fixture receipt.
            let mut pending = control.clone();
            pending.cleanup_complete = false;
            put(
                "consumer-cleanup-holds-publication",
                publish_control(&pending, &f.root.join("blocked-result")).is_err()
                    && !f.root.join("blocked-result").exists(),
            );
            let o = f.run(Action::Capture)?;
            put(
                "capture-is-T",
                valid(&o) && o.physical == Some(Physical::Capture),
            );
            let mut forged = vec![];
            for line in control
                .private_channel
                .split(|b| *b == b'\n')
                .filter(|v| !v.is_empty())
            {
                let mut event: serde_json::Value = serde_json::from_slice(line)?;
                event["nonce"] = serde_json::Value::String("foreign-invocation".into());
                forged.extend(serde_json::to_vec(&event)?);
                forged.push(b'\n');
            }
            put(
                "forged-channel-refused",
                boole_lean_runner::w0_types::parse_channel(
                    &forged,
                    &control.nonce,
                    &control.parent_namespaces,
                    false,
                )
                .is_err(),
            );
            put(
                "no-terminal-verdict",
                o.physical.is_some() && o.cleanup_complete,
            );
        }
        _ => bail!("W0 unknown case"),
    }
    ensure!(rows.values().all(|v| *v), "W0 case failed: {rows:?}");
    Ok(rows)
}
pub fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|v| ["init-v1", "payload-v1", "tool-v1"].contains(&v.as_str()))
    {
        return w0_linux::helper(&args);
    }
    if args.first().is_some_and(|v| v == "observer-v1") {
        ensure!(args.len() == 3, "W0 observer arity");
        let c: Config = serde_json::from_slice(&read_private(Path::new(&args[1]), 16384)?)?;
        w0_linux::run(
            &c,
            Path::new(&args[2]),
            Action::HoldTree,
            Duration::from_secs(6),
            false,
        )?;
        return Ok(());
    }
    if args == ["identity-v1"] {
        println!("{}", real_w0::source_digest());
        return Ok(());
    }
    ensure!(
        args.len() == 5 && args[0] == "probe-v1",
        "W0 fixed probe invocation"
    );
    let case = &args[1];
    ensure!(real_w0::required(case).is_some(), "W0 closed case");
    let root = Path::new(&args[3]);
    boole_lean_runner::artifact_handles::private_directory(root)?;
    let bundle: Bundle = serde_json::from_slice(&read_private(Path::new(&args[4]), 16384)?)?;
    ensure!(
        bundle.version == 1 && bundle.source_sha256 == real_w0::source_digest(),
        "W0 source-bound binary"
    );
    let grant_bytes = read_private(&bundle.grant, 16384)?;
    ensure!(
        read_private(&bundle.grant_origin, 16384)? == grant_bytes,
        "W0 external grant changed"
    );
    let grant: Grant = serde_json::from_slice(&grant_bytes)?;
    grant.validate_guest(now()?)?;
    ensure!(grant.request_id == args[2], "W0 request binding");
    let lock = boole_lean_runner::artifact_handles::open_regular(&bundle.lock)?;
    lock.try_lock()
        .map_err(|_| anyhow::anyhow!("W0 guest window is already held"))?;
    let mut fixture = Fixture {
        config: &bundle.backend,
        root,
        count: 0,
        control_pass: false,
        tree_empty: true,
    };
    let assertions = observe_case(case, &mut fixture, &grant)?;
    ensure!(
        read_private(&bundle.grant_origin, 16384)? == grant_bytes
            && read_private(&bundle.grant, 16384)? == grant_bytes,
        "W0 grant changed"
    );
    grant.validate_guest(now()?)?;
    let receipt = Receipt {
        version: 1,
        case: case.into(),
        request_id: grant.request_id.clone(),
        generation: grant.generation,
        backend_invocations: fixture.count,
        control_pass: fixture.control_pass,
        tree_empty: fixture.count > 0 && fixture.tree_empty,
        quiet_window_held: w0_linux::live(&bundle.backend).is_ok(),
        assertions,
    };
    let raw = serde_json::to_vec(&receipt)?;
    real_w0::validate_receipt(&raw, case, &grant)?;
    println!("{}", String::from_utf8(raw)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn observed() -> Observation {
        Observation {
            action: Action::Noop,
            control_pass: true,
            physical: None,
            setup_ok: true,
            cleanup_complete: true,
            private_channel_valid: true,
            capabilities: Some(boole_lean_runner::w0_types::Capabilities {
                namespaces: ["user", "net", "pid", "mnt", "ipc", "uts"]
                    .into_iter()
                    .map(|n| (n.into(), "child".into()))
                    .collect(),
                landlock_abi: 4,
                landlock_enforced: true,
                seccomp: 2,
                apparmor: "boole-gate-bwrap (unconfined)".into(),
                dumpable: 0,
                caps_zero: true,
                pid_one: true,
            }),
            output: serde_json::to_vec(&ProbeOutput {
                action: "noop".into(),
                allowed: true,
                errno: None,
                workers: 0,
                tree: None,
                marker: None,
            })
            .unwrap(),
            reaped: 0,
            pids_events: 0,
            elapsed_ms: 1,
            pidfd_used: true,
            observer_cross_checked: true,
            nonce: "control".into(),
            parent_namespaces: BTreeMap::new(),
            private_channel: vec![],
            launcher_killed: false,
            automatic_tree_empty: false,
        }
    }
    #[test]
    fn host_consumer_requires_enforcement_and_independent_observer_evidence() {
        let mut o = observed();
        assert!(allowed(&o));
        o.capabilities.as_mut().unwrap().landlock_enforced = false;
        assert!(!allowed(&o));
        o.capabilities.as_mut().unwrap().landlock_enforced = true;
        o.observer_cross_checked = false;
        assert!(!allowed(&o));
    }
    #[test]
    fn consumer_publication_fence_has_a_real_positive_and_negative_control() {
        let root = boole_gate_supervisor::codec::Scratch::new(&std::env::temp_dir()).unwrap();
        let mut o = observed();
        publish_control(&o, &root.0.join("positive")).unwrap();
        assert_eq!(
            fs::read(root.0.join("positive")).unwrap(),
            b"consumer-result"
        );
        o.physical = Some(Physical::Wall);
        assert!(publish_control(&o, &root.0.join("interrupted")).is_err());
        o.physical = None;
        o.cleanup_complete = false;
        assert!(publish_control(&o, &root.0.join("unclean")).is_err());
        assert!(!root.0.join("interrupted").exists() && !root.0.join("unclean").exists());
    }
    #[test]
    fn host_consumer_never_accepts_a_claim_without_kernel_capabilities() {
        let mut o = observed();
        assert!(allowed(&o));
        o.capabilities = None;
        assert!(!allowed(&o));
    }
    #[test]
    fn host_consumer_rejects_cleanup_and_channel_failure_before_a_probe_claim() {
        let mut o = observed();
        o.cleanup_complete = false;
        assert!(!allowed(&o));
        o.cleanup_complete = true;
        o.private_channel_valid = false;
        assert!(!allowed(&o));
    }
    #[test]
    fn host_fixture_command_never_reaches_backend_on_missing_arguments() {
        assert!(main().is_err());
    }
}

#[cfg(test)]
mod visibility_control_tests {
    use super::*;
    #[test]
    fn absent_exec_is_not_proof_of_an_execute_denial() {
        let mut o = super::tests::observed();
        o.action = Action::UnpinnedExec;
        o.output = serde_json::to_vec(&ProbeOutput {
            action: o.action.name().into(),
            allowed: false,
            errno: Some(2),
            workers: 0,
            tree: None,
            marker: None,
        })
        .unwrap();
        assert!(!denied(&o, false));
    }
}
