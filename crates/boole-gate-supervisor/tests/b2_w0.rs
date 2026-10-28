//! Executable acceptance cases awaiting the reviewed Linux W0 fixture producer.
//! Ignored on purpose: the host slice has no controller backend or VM grant.
use anyhow::{ensure, Context, Result};
use boole_gate_supervisor::codec::{digest, read_bounded, Scratch};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    process::{Child, Command},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
// No reviewed W0 producer has been released. Only a reviewed tracked change may
// fill this slot; an operator environment variable cannot authorize a runner.
const W0_RUNNER_SHA256: Option<[u8; 32]> = None;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    state: String,
    owner: String,
    step: String,
    start_unix: u64,
    end_unix: u64,
    request_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u8,
    case: String,
    request_id: String,
    backend_invocations: u64,
    control_pass: bool,
    tree_empty: bool,
    quiet_window_held: bool,
    assertions: BTreeMap<String, bool>,
}
struct OwnedProbe(Child);
impl Drop for OwnedProbe {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn run(case: &str, assertions: &[&str]) -> Result<()> {
    ensure!(
        cfg!(target_os = "linux"),
        "PENDING-W0: Linux required; no host fallback"
    );
    let grant_path = std::env::var_os("BOOLE_B2_W0_GRANT")
        .context("PENDING-W0: explicit quiet-window grant required")?;
    let g: Grant =
        serde_json::from_slice(&read_bounded(std::path::Path::new(&grant_path), 16384)?)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    ensure!(
        g.state == "granted"
            && g.owner == "N"
            && g.step == "W0"
            && now >= g.start_unix
            && now.checked_add(130).is_some_and(|end| end <= g.end_unix),
        "W0 grant not active or insufficient cleanup window"
    );
    let expected = W0_RUNNER_SHA256.context("PENDING-W0: reviewed tracked fixture pin absent")?;
    let runner = std::env::var_os("BOOLE_B2_W0_RUNNER")
        .context("PENDING-W0: reviewed controller/probe fixture not implemented")?;
    ensure!(
        digest(&read_bounded(
            std::path::Path::new(&runner),
            128 * 1024 * 1024
        )?) == expected,
        "W0 fixture pin"
    );
    let root = Scratch::new(&std::env::temp_dir())?;
    let stdout = root.0.join("receipt.json");
    let stderr = root.0.join("diagnostic");
    let mut child = OwnedProbe(
        Command::new(runner)
            .args(["probe-v1", case, &g.request_id])
            .arg(&root.0)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TMPDIR", &root.0)
            .stdout(fs::File::create(&stdout)?)
            .stderr(fs::File::create(&stderr)?)
            .spawn()?,
    );
    let deadline = Instant::now() + Duration::from_secs(120);
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline
            || fs::metadata(&stdout)?.len() > 65536
            || fs::metadata(&stderr)?.len() > 65536
        {
            // The reviewed fixture owns an outer cgroup and must clean it when
            // its supervisor dies. LIFE-parent-kill independently proves that.
            child.0.kill()?;
            child.0.wait()?;
            anyhow::bail!("W0 fixture timeout/capture failure");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    ensure!(status.success(), "W0 fixture failed");
    let r: Receipt = serde_json::from_slice(&read_bounded(&stdout, 65536)?)?;
    ensure!(
        r.version == 1
            && r.case == case
            && r.request_id == g.request_id
            && r.backend_invocations > 0
            && r.control_pass
            && r.quiet_window_held
            && r.tree_empty,
        "W0 execution/cleanup evidence"
    );
    ensure!(
        r.assertions.len() == assertions.len()
            && assertions
                .iter()
                .all(|key| r.assertions.get(*key) == Some(&true)),
        "W0 acceptance assertions"
    );
    Ok(())
}
macro_rules! pending {
    ($name:ident,$case:literal,$($assertion:literal),+)=>{
        #[test]
        #[ignore="PENDING-W0: reviewed Linux backend, pinned probe fixture and exclusive VM grant required"]
        fn $name()->Result<()>{run($case,&[$($assertion),+])}
    };
}
pending!(
    b2_sbx_network,
    "SBX-network",
    "ipv4-denied",
    "ipv6-denied",
    "loopback-denied",
    "dns-denied"
);
pending!(
    b2_sbx_filesystem,
    "SBX-filesystem",
    "secret-read-denied",
    "outside-write-denied",
    "input-write-denied",
    "inherited-fd-denied"
);
pending!(
    b2_sbx_exec,
    "SBX-exec",
    "scratch-exec-denied",
    "unpinned-exec-denied",
    "pinned-comparator-allowed",
    "four-nanoda-workers-allowed"
);
pending!(
    b2_sbx_capabilities,
    "SBX-capabilities",
    "missing-delegation-refused",
    "partial-landlock-refused",
    "missing-seccomp-refused",
    "missing-apparmor-refused",
    "missing-namespace-refused",
    "no-fallback"
);
pending!(
    b2_sbx_tasks,
    "SBX-tasks",
    "task-limit-enforced",
    "stop-is-T",
    "no-logical-reject"
);
pending!(
    b2_life_preparation,
    "LIFE-preparation",
    "slow-preparation-bounded",
    "blocked-read-killed",
    "D4-read-bounded",
    "no-partial-publication"
);
pending!(
    b2_life_deadlines,
    "LIFE-deadlines",
    "absolute-job-deadline",
    "independent-bounded-recheck",
    "counted-T",
    "no-cached-verdict"
);
pending!(
    b2_life_tree,
    "LIFE-tree",
    "setsid-contained",
    "double-fork-reaped",
    "cgroup-empty",
    "pid-reuse-safe"
);
pending!(
    b2_life_parent_kill,
    "LIFE-parent-kill",
    "parent-sigkill",
    "all-descendants-retired",
    "cgroup-empty"
);
pending!(
    b2_life_faults,
    "LIFE-faults",
    "spawn-is-D-node",
    "cleanup-holds-publication",
    "capture-is-T",
    "forged-channel-refused",
    "no-terminal-verdict"
);
