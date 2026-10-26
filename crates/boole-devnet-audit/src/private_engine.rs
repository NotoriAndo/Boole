//! Auditor-owned Engine faults are local observations, never chain evidence.
use anyhow::{bail, ensure, Context, Result};
use boole_derivation::ParentHeader;
use boole_devnet_driver::engine::{Engine, RpcEngine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub(crate) struct PrivateSide;
impl std::fmt::Display for PrivateSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("auditor private Engine")
    }
}
impl std::error::Error for PrivateSide {}
pub(crate) fn operation<T>(r: Result<T>) -> Result<T> {
    r.map_err(|e| e.context(PrivateSide))
}
pub(crate) fn unavailable(e: &anyhow::Error) -> bool {
    boole_devnet_driver::engine::rpc_unavailable(e)
        || e.is::<crate::resource::ResourceStop>()
        || e.downcast_ref::<boole_devnet_driver::driver::Stall>()
            .is_some_and(|s| {
                matches!(
                    s.kind,
                    boole_devnet_driver::driver::StallKind::Availability { .. }
                )
            })
        || e.chain()
            .any(|e| e.is::<reqwest::Error>() || e.is::<std::io::Error>())
}
#[derive(Clone, Debug)]
pub(crate) struct Fault {
    pub height: u64,
    pub origin: Option<u64>,
    pub phase: &'static str,
    pub operation: String,
    pub stop: crate::resource::ResourceStop,
}
impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("private Engine operation unavailable")
    }
}
impl std::error::Error for Fault {}
type OperationId = (String, [u8; 32]);
#[derive(Default)]
struct Step {
    height: u64,
    phase: &'static str,
    faults: BTreeMap<OperationId, (u64, Fault)>,
    next_fault: u64,
}
#[derive(Clone, Default)]
pub(crate) struct Tracker(Rc<RefCell<Step>>);
impl Tracker {
    pub fn begin(&self, height: u64, phase: &'static str) {
        *self.0.borrow_mut() = Step {
            height,
            phase,
            faults: BTreeMap::new(),
            next_fault: 0,
        };
    }
    pub fn phase(&self, phase: &'static str) {
        self.0.borrow_mut().phase = phase;
    }
    pub fn unresolved(&self) -> Option<Fault> {
        self.0
            .borrow()
            .faults
            .values()
            .min_by_key(|(order, _)| *order)
            .map(|(_, fault)| fault.clone())
    }
    fn observe<T>(
        &self,
        name: &str,
        params: &Value,
        origin: Option<u64>,
        result: Result<T>,
    ) -> Result<T> {
        let id = (
            name.to_owned(),
            Sha256::digest(serde_json::to_vec(params)?).into(),
        );
        let mut step = self.0.borrow_mut();
        match &result {
            Ok(_) => {
                step.faults.remove(&id);
            }
            Err(e) if unavailable(e) => {
                let fault = Fault {
                    height: step.height,
                    origin,
                    phase: step.phase,
                    operation: name.to_owned(),
                    stop: e
                        .downcast_ref::<crate::resource::ResourceStop>()
                        .copied()
                        .unwrap_or(crate::resource::ResourceStop {
                            resource: "private-engine-unavailable",
                            limit: 0,
                        }),
                };
                let order = step.next_fault;
                step.next_fault = step.next_fault.saturating_add(1);
                step.faults.entry(id).or_insert((order, fault));
            }
            _ => {}
        }
        operation(result)
    }
}
pub(crate) struct Tracked<E> {
    inner: E,
    tracker: Tracker,
}
impl<E> Tracked<E> {
    pub fn new(inner: E, tracker: Tracker) -> Self {
        Self { inner, tracker }
    }
    #[cfg(test)]
    pub(crate) fn inner_mut(&mut self) -> &mut E {
        &mut self.inner
    }
}
impl<E: Engine> Engine for Tracked<E> {
    fn head(&mut self) -> Result<ParentHeader> {
        self.tracker
            .observe("head", &Value::Null, None, self.inner.head())
    }
    fn header(&mut self, h: u64) -> Result<ParentHeader> {
        self.tracker
            .observe("header", &json!(h), Some(h), self.inner.header(h))
    }
    fn transactions(&mut self, h: u64) -> Result<Vec<String>> {
        self.tracker.observe(
            "transactions",
            &json!(h),
            Some(h),
            self.inner.transactions(h),
        )
    }
    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.tracker.observe(
            method,
            &params,
            None,
            self.inner.call(method, params.clone()),
        )
    }
}

#[derive(Debug)]
struct LifecycleStop {
    resource: &'static str,
    limit: usize,
    code: Option<i32>,
    signal: Option<i32>,
}
impl std::fmt::Display for LifecycleStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.resource)
    }
}
impl std::error::Error for LifecycleStop {}
pub(crate) fn setup<T>(result: Result<T>) -> Result<T> {
    result.map_err(|_| {
        LifecycleStop {
            resource: "private-engine-setup",
            limit: 0,
            code: None,
            signal: None,
        }
        .into()
    })
}
#[derive(Debug)]
struct Cleanup {
    stopped: bool,
    log_issues: Option<usize>,
}
impl std::fmt::Display for Cleanup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("private startup cleanup")
    }
}
impl std::error::Error for Cleanup {}
pub(crate) fn report(receipt: &mut Value, error: &anyhow::Error) -> bool {
    if let Some(f) = error.downcast_ref::<Fault>() {
        crate::mark_resource_stop(receipt, f.height, f.phase, f.stop);
        receipt["resource_stop"]["side"] = json!("private-engine");
        receipt["resource_stop"]["operation"] = json!(f.operation);
        receipt["resource_stop"]["origin_height"] = json!(f.origin.map(|h| h.to_string()));
        receipt["checks"]["execution"] = json!("resource-stop");
        return true;
    }
    if let Some(s) = error.downcast_ref::<LifecycleStop>() {
        if let Some(c) = error.downcast_ref::<Cleanup>() {
            receipt["private_engine_cleanup"] =
                json!({"stopped":c.stopped,"log_issues":c.log_issues});
        }
        crate::mark_resource_stop(
            receipt,
            0,
            "replay-setup",
            crate::resource::ResourceStop {
                resource: s.resource,
                limit: s.limit,
            },
        );
        receipt["resource_stop"]["side"] = json!("private-engine");
        receipt["resource_stop"]["exit_code"] = json!(s.code);
        receipt["resource_stop"]["signal"] = json!(s.signal);
        receipt["checks"]["execution"] = json!("resource-stop");
        return true;
    }
    false
}
pub struct PrivateEngine {
    child: Child,
    birth: String,
    pub http: String,
    auth: String,
    jwt: PathBuf,
    log: PathBuf,
    state: Lifecycle,
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
#[derive(Clone, Copy, PartialEq)]
enum Lifecycle {
    Starting,
    Ready,
    Stopped,
}
const READINESS_MS: usize = 60_000;
impl PrivateEngine {
    pub fn start(binary: &Path, genesis: &Path, root: &Path) -> Result<Self> {
        let pins: Value =
            serde_json::from_str(include_str!("../../../fixtures/devnet-m1/pins.json"))?;
        let expected = pins["op_reth"]["sha256"].as_str().context("op-reth pin")?;
        Self::launch(
            binary,
            genesis,
            root,
            expected,
            Duration::from_millis(READINESS_MS as u64),
        )
    }
    fn launch(
        binary: &Path,
        genesis: &Path,
        root: &Path,
        expected: &str,
        timeout: Duration,
    ) -> Result<Self> {
        Self::launch_inner(binary, genesis, root, expected, timeout).map_err(|e| {
            if e.is::<LifecycleStop>() {
                e
            } else {
                e.context(LifecycleStop {
                    resource: "private-engine-startup-unavailable",
                    limit: 0,
                    code: None,
                    signal: None,
                })
            }
        })
    }
    fn launch_inner(
        binary: &Path,
        genesis: &Path,
        root: &Path,
        expected: &str,
        timeout: Duration,
    ) -> Result<Self> {
        // One absolute deadline includes preparation, spawn and every readiness read.
        let deadline = Instant::now() + timeout;
        setup((|| -> Result<()> {
            let metadata = std::fs::metadata(binary)?;
            ensure!(metadata.is_file(), "binary type");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                ensure!(
                    metadata.permissions().mode() & 0o111 != 0,
                    "binary executable"
                );
            }
            let actual =
                boole_derivation::attributes::hex_string(&Sha256::digest(std::fs::read(binary)?));
            ensure!(
                actual.trim_start_matches("0x") == expected.trim_start_matches("0x"),
                "binary pin"
            );
            let (raw, _) =
                boole_devnet_archive::journal::read_external_bound(genesis, 16 * 1024 * 1024)?;
            let config: Value = serde_json::from_slice(&raw)?;
            ensure!(
                config["config"]["chainId"].as_u64() == Some(boole_derivation::config::L2_CHAIN_ID),
                "genesis chain id"
            );
            ensure!(config["alloc"].is_object(), "genesis allocation");
            let gas = config["gasLimit"].as_str().context("genesis gas limit")?;
            ensure!(
                u64::from_str_radix(gas.strip_prefix("0x").context("genesis gas prefix")?, 16)? > 0,
                "genesis gas limit"
            );
            ensure!(
                root.is_dir() && !root.join("engine").try_exists()?,
                "fresh own data directory"
            );
            for path in [binary, genesis, root] {
                ensure!(!path.as_os_str().is_empty(), "empty argument");
                #[cfg(unix)]
                {
                    use std::os::unix::ffi::OsStrExt;
                    ensure!(!path.as_os_str().as_bytes().contains(&0), "NUL argument");
                }
            }
            Ok(())
        })())?;
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
        if Instant::now() >= deadline {
            return Err(LifecycleStop {
                resource: "private-engine-startup-time",
                limit: timeout.as_millis() as usize,
                code: None,
                signal: None,
            }
            .into());
        }
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
            .spawn()
            .map_err(|_| LifecycleStop {
                resource: "private-engine-startup-unavailable",
                limit: 0,
                code: None,
                signal: None,
            })?;
        let mut engine = Self {
            birth: String::new(),
            child,
            http: format!("http://127.0.0.1:{http_port}"),
            auth: format!("http://127.0.0.1:{auth_port}"),
            jwt,
            log: root.join("private-op-reth.log"),
            state: Lifecycle::Starting,
        };
        // Failure to observe an already exited child is still startup availability.
        engine.birth = birth(engine.child.id()).unwrap_or_default();
        record(engine.child.id(), &engine.birth, "started")?;
        let ready = engine.await_ready(deadline, timeout);
        if let Err(error) = ready {
            let stopped = engine.stop().is_ok();
            let log_issues = engine.log_issues().ok();
            return Err(error.context(Cleanup {
                stopped,
                log_issues,
            }));
        }
        engine.state = Lifecycle::Ready;
        Ok(engine)
    }
    fn await_ready(&mut self, deadline: Instant, timeout: Duration) -> Result<()> {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        loop {
            if let Some(status) = self.child.try_wait()? {
                #[cfg(unix)]
                let signal = {
                    use std::os::unix::process::ExitStatusExt;
                    status.signal()
                };
                #[cfg(not(unix))]
                let signal = None;
                return Err(LifecycleStop {
                    resource: "private-engine-startup-unavailable",
                    limit: 0,
                    code: status.code(),
                    signal,
                }
                .into());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(LifecycleStop {
                    resource: "private-engine-startup-time",
                    limit: timeout.as_millis() as usize,
                    code: None,
                    signal: None,
                }
                .into());
            }
            // The production Engine RPC has a longer per-call timeout. Readiness
            // uses the remaining absolute budget instead, with no retries inside it.
            let ready = (|| -> Result<Value> {
                let response = client
                    .post(&self.http)
                    .timeout(remaining.min(Duration::from_millis(300)))
                    .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}))
                    .send()?
                    .error_for_status()?;
                let mut bytes = Vec::new();
                response.take(4097).read_to_end(&mut bytes)?;
                ensure!(bytes.len() <= 4096, "private readiness response cap");
                Ok(serde_json::from_slice(&bytes)?)
            })();
            if Instant::now() < deadline
                && ready.is_ok_and(|v| {
                    v["id"] == 1
                        && v["jsonrpc"] == "2.0"
                        && v["result"] == format!("0x{:x}", boole_derivation::config::L2_CHAIN_ID)
                })
            {
                return Ok(());
            }
            std::thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(50)),
            );
        }
    }
    pub fn engine(&self) -> Result<RpcEngine> {
        RpcEngine::new(&self.auth, &self.http, &self.jwt)
    }
    pub fn stop(&mut self) -> Result<()> {
        if self.state == Lifecycle::Stopped {
            return Ok(());
        }
        let mut forced = false;
        if self.child.try_wait()?.is_none() {
            if self.birth.is_empty() {
                // An unreaped Child still owns its PID even if ps setup failed.
                let _ = self.child.kill();
                self.child.wait()?;
                self.state = Lifecycle::Stopped;
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
        self.state = Lifecycle::Stopped;
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

/// Sanitize missing CLI arguments as local private setup, before spawning.
pub fn replay_arguments<'a>(
    binary: Option<&'a Path>,
    genesis: Option<&'a Path>,
) -> Result<(&'a Path, &'a Path)> {
    setup((|| {
        Ok((
            binary.context("binary argument")?,
            genesis.context("genesis argument")?,
        ))
    })())
}

#[cfg(test)]
mod tests {
    use super::*;
    use boole_devnet_archive::durability::PrivateTempDir;
    #[test]
    fn t81_unresolved_preserves_first_occurrence_and_same_operation_retry() -> Result<()> {
        let tracker = Tracker::default();
        tracker.begin(33, "verdict");
        let unavailable =
            || anyhow::Error::new(std::io::Error::from(std::io::ErrorKind::ConnectionRefused));
        assert!(tracker
            .observe::<()>("transactions", &json!(24), Some(24), Err(unavailable()))
            .is_err());
        assert!(tracker
            .observe::<()>("head", &Value::Null, None, Err(unavailable()))
            .is_err());
        assert_eq!(
            tracker.unresolved().unwrap().origin,
            Some(24),
            "map-key order replaced the first operation"
        );
        assert!(tracker
            .observe::<()>("transactions", &json!(24), Some(24), Err(unavailable()))
            .is_err());
        assert_eq!(tracker.unresolved().unwrap().origin, Some(24));
        tracker.observe("transactions", &json!(24), Some(24), Ok(()))?;
        assert_eq!(tracker.unresolved().unwrap().operation, "head");
        tracker.begin(34, "fold");
        assert!(tracker.unresolved().is_none());
        Ok(())
    }
    #[cfg(unix)]
    fn launch_script(
        body: &str,
        timeout: Duration,
        bad_config: bool,
    ) -> Result<(anyhow::Error, Value)> {
        use std::os::unix::fs::PermissionsExt;
        let work = PrivateTempDir::new("audit-lifecycle")?;
        let binary = work.path().join("engine-fixture");
        std::fs::write(&binary, format!("#!/bin/sh\n{body}\n"))?;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))?;
        let hash =
            boole_derivation::attributes::hex_string(&Sha256::digest(std::fs::read(&binary)?));
        let genesis = work.path().join("genesis.json");
        std::fs::write(&genesis, if bad_config { json!({}) } else {json!({"config":{"chainId":boole_derivation::config::L2_CHAIN_ID},"gasLimit":"0x1c9c380","alloc":{}})}.to_string())?;
        std::fs::hard_link(&genesis, work.path().join("genesis-alias.json"))?;
        assert_eq!(
            std::os::unix::fs::MetadataExt::nlink(&std::fs::metadata(&genesis)?),
            2
        );
        let error = PrivateEngine::launch(&binary, &genesis, work.path(), &hash, timeout)
            .err()
            .context("fixture unexpectedly ready")?;
        let mut receipt = crate::empty_receipt(&boole_derivation::m2::Config::devnet(), "journal");
        crate::replay::report_failure(&mut receipt, &error, "unavailable");
        assert!(receipt["first_mismatch"].is_null(), "{receipt}");
        assert_eq!(receipt["resource_stop"]["height"], "0");
        assert_eq!(receipt["resource_stop"]["check"], "replay-setup");
        assert_eq!(receipt["resource_stop"]["side"], "private-engine");
        assert!(!receipt
            .to_string()
            .contains(&work.path().display().to_string()));
        Ok((error, receipt))
    }
    #[test]
    fn startup_receipt_child() -> Result<()> {
        let Ok(mode) = std::env::var("BOOLE_AUDIT_STARTUP_CHILD") else {
            return Ok(());
        };
        #[cfg(unix)]
        {
            let (_, r) = launch_script(
                if mode == "exit" {
                    "exit 7"
                } else {
                    "exec /bin/sleep 30"
                },
                Duration::from_millis(150),
                false,
            )?;
            println!();
            crate::emit_receipt(&r);
        }
        Ok(())
    }
    #[test]
    fn t81_startup_process_exit_codes_and_sanitized_output() -> Result<()> {
        for mode in ["exit", "timeout"] {
            let out = Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "private_engine::tests::startup_receipt_child",
                    "--nocapture",
                ])
                .env("BOOLE_AUDIT_STARTUP_CHILD", mode)
                .output()?;
            assert_eq!(
                out.status.code(),
                Some(2),
                "{}",
                String::from_utf8_lossy(&out.stdout)
            );
            let text = String::from_utf8(out.stdout)?;
            let r: Value = serde_json::from_str(
                text.lines()
                    .find(|s| s.starts_with('{'))
                    .context("startup JSON")?,
            )?;
            assert!(r["first_mismatch"].is_null());
            assert_eq!(r["resource_stop"]["side"], "private-engine");
            assert_eq!(r["private_engine_cleanup"]["stopped"], true);
            if let Some(output) = std::env::var_os("BOOLE_AUDIT_I17_OUTPUT") {
                std::fs::write(
                    PathBuf::from(output).join(format!("startup-{mode}.json")),
                    serde_json::to_vec_pretty(&r)?,
                )?;
            }
        }
        Ok(())
    }
    #[test]
    #[cfg(unix)]
    fn fix2_hard_linked_genesis_reaches_startup_without_a_setup_alias_stop() -> Result<()> {
        let (_, r) = launch_script("exit 7", Duration::from_secs(1), false)?;
        assert_eq!(
            r["resource_stop"]["resource"],
            "private-engine-startup-unavailable"
        );
        assert_eq!(r["resource_stop"]["exit_code"], 7);
        Ok(())
    }
    #[test]
    #[cfg(unix)]
    fn t81_startup_exit_before_ready() -> Result<()> {
        let (_, r) = launch_script("exit 7", Duration::from_secs(1), false)?;
        assert_eq!(
            r["resource_stop"]["resource"],
            "private-engine-startup-unavailable"
        );
        assert_eq!(r["resource_stop"]["exit_code"], 7);
        assert_eq!(r["private_engine_cleanup"]["stopped"], true);
        Ok(())
    }
    #[test]
    #[cfg(unix)]
    fn t81_startup_absolute_deadline_and_cleanup() -> Result<()> {
        let start = Instant::now();
        let (_, r) = launch_script("exec /bin/sleep 30", Duration::from_millis(150), false)?;
        assert_eq!(
            r["resource_stop"]["resource"],
            "private-engine-startup-time"
        );
        assert_eq!(r["resource_stop"]["limit"], "150");
        assert_eq!(r["private_engine_cleanup"]["stopped"], true);
        assert!(start.elapsed() < Duration::from_secs(2));
        Ok(())
    }
    #[test]
    #[cfg(unix)]
    fn t81_startup_oversized_readiness_is_not_ready_and_deadline_cleans_up() -> Result<()> {
        use std::io::{Read, Write};
        let work = PrivateTempDir::new("readiness-bound")?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let http = format!("http://{}", listener.local_addr()?);
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut head = vec![];
            let mut b = [0];
            while !head.ends_with(b"\r\n\r\n") && head.len() < 65536 {
                if stream.read_exact(&mut b).is_err() {
                    return;
                }
                head.push(b[0]);
            }
            let bytes=serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"result":format!("0x{:x}",boole_derivation::config::L2_CHAIN_ID),"padding":"x".repeat(1024*1024)})).unwrap();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            );
            let _ = stream.write_all(&bytes);
        });
        let child = Command::new("/bin/sleep").arg("10").spawn()?;
        let birth = birth(child.id())?;
        let log = work.path().join("engine.log");
        std::fs::write(&log, [])?;
        let mut engine = PrivateEngine {
            child,
            birth,
            http,
            auth: String::new(),
            jwt: work.path().join("jwt"),
            log,
            state: Lifecycle::Starting,
        };
        let timeout = Duration::from_millis(200);
        let start = Instant::now();
        let result = engine.await_ready(start + timeout, timeout);
        engine.stop()?;
        server.join().unwrap();
        let error = result.expect_err("oversized readiness response was accepted");
        assert_eq!(
            error
                .downcast_ref::<LifecycleStop>()
                .context("typed deadline")?
                .resource,
            "private-engine-startup-time"
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        Ok(())
    }
    #[test]
    #[cfg(unix)]
    fn t81_startup_bad_config_is_setup() -> Result<()> {
        let (_, r) = launch_script("exit 7", Duration::from_secs(1), true)?;
        assert_eq!(r["resource_stop"]["resource"], "private-engine-setup");
        assert!(
            r["private_engine_cleanup"].is_null(),
            "bad config spawned a child"
        );
        Ok(())
    }
}
