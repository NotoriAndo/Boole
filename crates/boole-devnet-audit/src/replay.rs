//! Replay writes only to a freshly spawned, auditor-owned private Engine.
use crate::{rpc::RpcNode, Inputs, Node};
use anyhow::{bail, ensure, Context, Result};
use boole_derivation::{m2::Config, m3::ForkTable, InboxLog, L1Block};
use boole_devnet_driver::{
    driver::{Driver, StepOutcome},
    engine::{Engine, Rpc, RpcEngine},
    l1::L1Source,
};
use boole_gate_supervisor::supervisor::Supervisor;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Write,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Debug)]
pub struct Failure {
    pub height: u64,
    pub class: &'static str,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "replay {} at {}", self.class, self.height)
    }
}
impl std::error::Error for Failure {}
fn at<T>(height: u64, class: &'static str, result: Result<T>) -> Result<T> {
    result.map_err(|error| {
        if error.is::<Failure>() {
            return error;
        }
        let (height, class) = match error.downcast_ref::<boole_devnet_archive::error::Error>() {
            Some(
                boole_devnet_archive::error::Error::Availability { height, .. }
                | boole_devnet_archive::error::Error::Integrity { height, .. },
            ) => (*height, "input"),
            _ => (height, class),
        };
        error.context(Failure { height, class })
    })
}
pub fn report_failure(receipt: &mut Value, error: &anyhow::Error, status: &str) {
    let (height, class) = if let Some(f) = error.downcast_ref::<Failure>() {
        (f.height, f.class)
    } else {
        match error.downcast_ref::<boole_devnet_archive::error::Error>() {
            Some(
                boole_devnet_archive::error::Error::Availability { height, .. }
                | boole_devnet_archive::error::Error::Integrity { height, .. },
            ) => (*height, "input"),
            _ => (completed(receipt), "replay-setup"),
        }
    };
    if let Some(stop) = error.downcast_ref::<crate::resource::ResourceStop>() {
        crate::mark_resource_stop(receipt, height, class, *stop);
        receipt["checks"]["execution"] = json!("resource-stop");
    } else {
        crate::mark_mismatch(receipt, height, class);
        receipt["checks"]["execution"] = json!(status);
    }
}
fn completed(receipt: &Value) -> u64 {
    receipt["replayed_blocks"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}
pub fn report_cleanup(receipt: &mut Value, stopped: bool, issues: usize) {
    receipt["private_engine_cleanup"] = json!({"stopped":stopped,"log_issues":issues});
    if !stopped || issues > 0 {
        let height = completed(receipt);
        crate::mark_mismatch(receipt, height, "engine-cleanup");
        receipt["checks"]["execution"] = json!("mismatch");
    }
}
pub struct PrivateEngine {
    child: Child,
    birth: String,
    pub http: String,
    auth: String,
    jwt: PathBuf,
    log: PathBuf,
    stopped: bool,
}
fn birth(pid: u32) -> Result<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "lstart="])
        .output()?;
    ensure!(output.status.success(), "own process birth unavailable");
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
fn record(pid: u32, birth: &str, state: &str) -> Result<()> {
    if let Some(path) = std::env::var_os("BOOLE_DEVNET_OWNED_PIDS") {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?;
        writeln!(
            file,
            "{}",
            json!({"name":"audit-replay-op-reth","pid":pid,"pid_birth":birth,"state":state})
        )?;
        file.sync_all()?;
    }
    Ok(())
}
impl PrivateEngine {
    pub fn start(binary: &Path, genesis: &Path, root: &Path) -> Result<Self> {
        let pins: Value =
            serde_json::from_str(include_str!("../../../fixtures/devnet-m1/pins.json"))?;
        let actual =
            boole_derivation::attributes::hex_string(&Sha256::digest(std::fs::read(binary)?));
        ensure!(
            actual.trim_start_matches("0x")
                == pins["op_reth"]["sha256"].as_str().context("op-reth pin")?,
            "private op-reth binary pin"
        );
        let mut ports = vec![];
        for _ in 0..3 {
            ports.push(TcpListener::bind("127.0.0.1:0")?);
        }
        let http_port = ports[0].local_addr()?.port();
        let auth_port = ports[1].local_addr()?.port();
        let p2p_port = ports[2].local_addr()?.port();
        let jwt = root.join("jwt.hex");
        let mut secret = [0; 32];
        use ring::rand::SecureRandom;
        ring::rand::SystemRandom::new()
            .fill(&mut secret)
            .map_err(|_| anyhow::anyhow!("private JWT randomness"))?;
        boole_gate_supervisor::codec::private_file(
            &jwt,
            &boole_derivation::attributes::hex_string(&secret).as_bytes()[2..],
        )?;
        let log = std::fs::File::create(root.join("private-op-reth.log"))?;
        drop(ports);
        let child = Command::new(binary)
            .arg("node")
            .args([
                "--http",
                "--ipcdisable",
                "--disable-discovery",
                "--disable-tx-gossip",
                "--engine.suppress-persistence-during-build",
            ])
            .arg("--chain")
            .arg(genesis)
            .arg("--datadir")
            .arg(root.join("engine"))
            .args([
                "--http.addr",
                "127.0.0.1",
                "--http.port",
                &http_port.to_string(),
                "--http.api",
                "eth,net,web3",
                "--authrpc.addr",
                "127.0.0.1",
                "--authrpc.port",
                &auth_port.to_string(),
            ])
            .arg("--authrpc.jwtsecret")
            .arg(&jwt)
            .args([
                "--addr",
                "127.0.0.1",
                "--port",
                &p2p_port.to_string(),
                "--max-outbound-peers",
                "0",
                "--max-inbound-peers",
                "0",
                "--engine.persistence-threshold",
                "0",
                "--engine.memory-block-buffer-target",
                "0",
            ])
            .arg("--log.file.directory")
            .arg(root.join("logs"))
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .spawn()?;
        let mut engine = Self {
            birth: String::new(),
            child,
            http: format!("http://127.0.0.1:{http_port}"),
            auth: format!("http://127.0.0.1:{auth_port}"),
            jwt,
            log: root.join("private-op-reth.log"),
            stopped: false,
        };
        engine.birth = birth(engine.child.id())?;
        record(engine.child.id(), &engine.birth, "started")?;
        let rpc = Rpc::new(&engine.http)?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(60) {
            ensure!(engine.child.try_wait()?.is_none(), "private Engine exited");
            if rpc.call("eth_chainId", json!([])).is_ok() {
                return Ok(engine);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        bail!("private Engine startup timeout")
    }
    pub fn engine(&self) -> Result<RpcEngine> {
        RpcEngine::new(&self.auth, &self.http, &self.jwt)
    }
    pub fn stop(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        let mut forced = false;
        if self.child.try_wait()?.is_none() {
            if self.birth.is_empty() {
                // An unreaped Child still owns its PID even if ps setup failed.
                let _ = self.child.kill();
                self.child.wait()?;
                self.stopped = true;
                bail!("private Engine birth instrumentation failed")
            }
            ensure!(
                birth(self.child.id())? == self.birth,
                "own Engine PID identity changed"
            );
            ensure!(
                Command::new("kill")
                    .args(["-s", "INT", &self.child.id().to_string()])
                    .status()?
                    .success(),
                "own Engine stop signal"
            );
            let start = Instant::now();
            while self.child.try_wait()?.is_none() && start.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(50));
            }
            if self.child.try_wait()?.is_none() {
                ensure!(
                    birth(self.child.id())? == self.birth,
                    "own Engine PID changed"
                );
                self.child.kill()?;
                self.child.wait()?;
                forced = true;
            }
        }
        self.stopped = true;
        record(self.child.id(), &self.birth, "stopped")?;
        self.preserve_log()?;
        ensure!(!forced, "private Engine required forced cleanup");
        Ok(())
    }
    pub fn log_issues(&self) -> Result<usize> {
        let (bytes, _) = boole_devnet_archive::journal::read_bound(&self.log, 4 * 1024 * 1024)?;
        Ok(std::str::from_utf8(&bytes)?
            .lines()
            .filter(|line| line.contains("WARN") || line.contains("ERROR"))
            .count())
    }
    fn preserve_log(&self) -> Result<()> {
        if let Some(directory) = std::env::var_os("BOOLE_DEVNET_AUDIT_LOG_DIR") {
            let directory = PathBuf::from(directory);
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&directory)?;
            let raw = boole_devnet_archive::journal::read_bound(&self.log, 4 * 1024 * 1024)?.0;
            let name = self
                .log
                .parent()
                .context("private log parent")?
                .file_name()
                .context("private log identity")?;
            let path = directory.join(format!("{}.log", name.to_string_lossy()));
            if path.try_exists()? {
                ensure!(
                    boole_devnet_archive::journal::read_bound(&path, 4 * 1024 * 1024)?.0 == raw,
                    "private log evidence changed"
                );
            } else {
                boole_devnet_archive::journal::publish_new(&path, &raw)?;
            }
        }
        Ok(())
    }
}
impl Drop for PrivateEngine {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
struct Source<'a> {
    inputs: &'a mut dyn Inputs,
    hashes: BTreeMap<String, u64>,
}
impl L1Source for Source<'_> {
    fn finalized(&mut self) -> Result<u64> {
        self.inputs.check()?;
        Ok(self.inputs.tip())
    }
    fn input_row(&mut self, h: u64) -> Result<boole_devnet_archive::input::InputRow> {
        self.inputs.row(h)
    }
    fn block(&mut self, h: u64) -> Result<L1Block> {
        Ok(self.inputs.row(h)?.verify()?.block)
    }
    fn block_by_hash(&mut self, hash: &str) -> Result<L1Block> {
        self.block(*self.hashes.get(hash).context("private replay origin")?)
    }
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>> {
        Ok(self
            .inputs
            .row(*self.hashes.get(hash).context("private replay logs")?)?
            .verify()?
            .logs)
    }
}
/// Return the first replay mismatch, or the exact number of reproduced blocks.
#[allow(clippy::too_many_arguments)]
pub fn replay(
    inputs: &mut dyn Inputs,
    audited: &mut dyn Node,
    c: Config,
    forks: Option<ForkTable>,
    supervisor: Option<Supervisor>,
    private: &PrivateEngine,
    receipt: &mut Value,
) -> Result<()> {
    receipt["replayed_blocks"] = json!("0");
    let mut engine = at(0, "replay-setup", private.engine())?;
    let genesis = at(0, "replay-setup", engine.header(0))?;
    at(
        0,
        "replay-setup",
        (|| -> Result<()> {
            ensure!(
                genesis.number == 0 && genesis.hash == audited.header(0)?.hash,
                "private Engine must be fresh with the audited genesis"
            );
            Ok(())
        })(),
    )?;
    let tip = inputs.tip();
    let mut hashes = BTreeMap::new();
    for h in 0..=tip {
        let row = at(h, "input", inputs.row(h))?;
        hashes.insert(at(h, "input", row.verify())?.block.hash, h);
    }
    let source = Source { inputs, hashes };
    let gas = genesis.gas_limit;
    let mut driver = at(
        0,
        "replay-setup",
        (|| -> Result<_> {
            Ok(if let Some(f) = forks {
                Driver::with_m3(
                    source,
                    engine,
                    c,
                    gas,
                    supervisor.context("replay supervisor")?,
                    f,
                )?
            } else {
                Driver::with_config(source, engine, c, gas)?
            })
        })(),
    )?;
    for h in 1..=tip {
        let mut phase = "fold";
        if let Err(error) = (|| -> Result<()> {
            ensure!(
                matches!(driver.step(h)?, StepOutcome::Derived(_)),
                "private replay waited for finality"
            );
            phase = "prefix";
            let actual = driver.engine.head()?;
            let expected = audited.header(h)?;
            ensure!(
                actual.hash == expected.hash && actual.number == h,
                "private replay L2 hash"
            );
            phase = "ledger";
            let mut private_node = RpcNode::new(&private.http)?;
            ensure!(
                private_node.receipt_statuses(h)? == audited.receipt_statuses(h)?
                    && private_node.depositor_balance(h)? == audited.depositor_balance(h)?,
                "private replay execution ledger"
            );
            phase = "input";
            driver.l1.inputs.check()?;
            Ok(())
        })() {
            let class = if driver.l1.inputs.check().is_err() {
                "input"
            } else if error
                .downcast_ref::<boole_devnet_driver::driver::Stall>()
                .is_some_and(|s| {
                    matches!(
                        s.kind,
                        boole_devnet_driver::driver::StallKind::VerdictPending { .. }
                            | boole_devnet_driver::driver::StallKind::Verdict { .. }
                    )
                })
            {
                "verdict"
            } else {
                phase
            };
            crate::mark_mismatch(receipt, h, class);
            receipt["checks"]["execution"] = json!("mismatch");
            return Ok(());
        }
        receipt["replayed_blocks"] = json!(h.to_string());
    }
    receipt["checks"]["execution"] = json!("ok");
    receipt["replayed_blocks"] = json!(tip.to_string());
    Ok(())
}
