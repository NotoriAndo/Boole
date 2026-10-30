#![cfg(feature = "w0-fixture")]
//! W0 fixture cases passed in one granted VM; repeats remain explicitly opt-in.
//! Ignored by default: an external grant and the pinned guest bundle are required.
use anyhow::{ensure, Context, Result};
use boole_gate_supervisor::codec::{digest, Scratch};
use boole_gate_supervisor::real_w0::{validate_receipt, Grant, RUNNER_SHA256 as W0_RUNNER_SHA256};
use std::io::Read;
use std::{
    fs,
    process::{Child, Command},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
// The script pin is reviewed in tracked code. Runtime bundle pins bind the
// separately compiled static fixture; neither grants a production profile.
fn read_held(path: &std::path::Path, cap: usize) -> Result<Vec<u8>> {
    let mut file = boole_lean_runner::artifact_handles::open_regular(path)?;
    let mut raw = vec![];
    (&mut file).take(cap as u64 + 1).read_to_end(&mut raw)?;
    ensure!(raw.len() <= cap, "W0 held read bound");
    Ok(raw)
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
        "W0 repeat: Linux required; no host fallback"
    );
    let grant_path = std::env::var_os("BOOLE_B2_W0_GRANT")
        .context("W0 repeat: explicit quiet-window grant required")?;
    let g: Grant = serde_json::from_slice(&read_held(std::path::Path::new(&grant_path), 16384)?)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    g.validate_guest(now)?;
    let expected = W0_RUNNER_SHA256.context("W0 repeat: reviewed tracked fixture pin absent")?;
    let runner = std::env::var_os("BOOLE_B2_W0_RUNNER")
        .context("W0 repeat: pinned runner location required")?;
    ensure!(
        digest(&read_held(
            std::path::Path::new(&runner),
            128 * 1024 * 1024
        )?) == expected,
        "W0 fixture pin"
    );
    let root = Scratch::new(&std::env::temp_dir())?;
    let evidence = std::path::PathBuf::from(
        std::env::var_os("BOOLE_B2_W0_RECEIPTS").context("durable evidence root required")?,
    )
    .join(case);
    let evidence_guard = boole_gate_supervisor::real_w0::EvidenceGuard::new(root, &evidence)?;
    let root = evidence_guard.source().to_path_buf();
    let result = (|| -> Result<()> {
        let stdout = root.join("receipt.json");
        let stderr = root.join("diagnostic");
        let mut child = OwnedProbe(
            Command::new(runner)
                .args(["probe-v1", case, &g.request_id])
                .arg(&root)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("TMPDIR", &root)
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
        let raw = read_held(&stdout, 65536)?;
        let r = validate_receipt(&raw, case, &g)?;
        ensure!(
            boole_gate_supervisor::real_w0::required(case) == Some(assertions),
            "W0 assertion contract drift"
        );
        ensure!(r.assertions.values().all(|v| *v), "W0 failed assertions");
        Ok(())
    })();
    evidence_guard.finish(result)
}
macro_rules! pending {
    ($name:ident,$case:literal,$($assertion:literal),+)=>{
        #[test]
        #[ignore="W0 repeat: reviewed Linux backend, pinned probe fixture and exclusive VM grant required"]
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
    "consumer-missing-delegation-refused",
    "consumer-partial-landlock-refused",
    "consumer-missing-seccomp-refused",
    "consumer-missing-apparmor-refused",
    "consumer-missing-namespace-refused",
    "no-fallback"
);
pending!(
    b2_sbx_tasks,
    "SBX-tasks",
    "task-limit-enforced",
    "memory-limit-enforced",
    "stop-is-T",
    "no-logical-reject"
);
pending!(
    b2_life_preparation,
    "LIFE-preparation",
    "slow-preparation-bounded",
    "blocked-read-killed",
    "D4-read-bounded",
    "consumer-no-partial-publication"
);
pending!(
    b2_life_deadlines,
    "LIFE-deadlines",
    "absolute-job-deadline",
    "independent-bounded-recheck",
    "counted-T",
    "consumer-no-cached-verdict"
);
pending!(
    b2_life_tree,
    "LIFE-tree",
    "setsid-contained",
    "double-fork-reaped",
    "cgroup-empty",
    "pidfd-identity-bound"
);
pending!(
    b2_life_parent_kill,
    "LIFE-parent-kill",
    "parent-sigkill",
    "all-descendants-retired",
    "observer-sigkill-drains",
    "cgroup-empty"
);
pending!(
    b2_life_faults,
    "LIFE-faults",
    "spawn-is-D-node",
    "consumer-cleanup-holds-publication",
    "capture-is-T",
    "forged-channel-refused",
    "no-terminal-verdict"
);

#[test]
fn w0_fixture_has_a_reviewed_tracked_runner_pin() {
    assert!(
        W0_RUNNER_SHA256.is_some(),
        "the ten W0 consumers have no pinned runner"
    );
}

#[test]
fn tracked_runner_bytes_and_rust_python_source_identities_agree() -> Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let runner = root.join("scripts/devnet/w0/runner.py");
    ensure!(
        Some(digest(&read_held(&runner, 65536)?)) == W0_RUNNER_SHA256,
        "tracked runner pin"
    );
    let result = Command::new("python3")
        .args(["-IB"])
        .arg(runner)
        .arg("source-id-v1")
        .arg(&root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()?;
    ensure!(result.status.success(), "Python source identity failed");
    ensure!(
        result.stdout
            == format!("{}\n", boole_gate_supervisor::real_w0::source_digest()).as_bytes(),
        "cross-language fixture source identity"
    );
    Ok(())
}
#[test]
#[cfg(not(target_os = "linux"))]
fn host_fixture_binary_refuses_every_execution_mode() -> Result<()> {
    for mode in ["probe-v1", "payload-v1", "init-v1", "tool-v1"] {
        let out = Command::new(env!("CARGO_BIN_EXE_boole-w0-probe"))
            .arg(mode)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()?;
        ensure!(
            out.status.code() == Some(2) && out.stdout.is_empty(),
            "non-Linux fixture fallback"
        );
    }
    Ok(())
}
