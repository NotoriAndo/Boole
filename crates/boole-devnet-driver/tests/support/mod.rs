use anyhow::{anyhow, bail, Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    Derived,
};
use boole_devnet_driver::engine::{Rpc, RpcEngine};
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/devnet-m1")
        .join(name)
}
pub fn fixture_json(name: &str) -> Result<Value> {
    Ok(serde_json::from_str(&fs::read_to_string(fixture(name))?)?)
}
pub fn quantity(v: &Value) -> Result<u64> {
    Ok(u64::from_str_radix(
        v.as_str()
            .context("quantity not a string")?
            .strip_prefix("0x")
            .context("quantity prefix")?,
        16,
    )?)
}
pub fn amount(v: &Value) -> Result<u128> {
    Ok(u128::from_str_radix(
        v.as_str()
            .context("amount not a string")?
            .strip_prefix("0x")
            .context("amount prefix")?,
        16,
    )?)
}

pub fn pinned(variable: &str, pin: &str) -> Result<Option<PathBuf>> {
    let Some(path) = std::env::var_os(variable) else {
        eprintln!("skipping: {variable} absent");
        return Ok(None);
    };
    let path = PathBuf::from(path);
    let mut file = File::open(&path).with_context(|| format!("open {variable} executable"))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let actual = hex_string(&hash.finalize());
    let pins = fixture_json("pins.json")?;
    if actual.trim_start_matches("0x") != pins[pin]["sha256"].as_str().context("missing pin")? {
        bail!("{variable} binary digest mismatch: {actual}");
    }
    Ok(Some(path))
}

pub fn disk_guard() -> Result<()> {
    let output = Command::new("df").args(["-k"]).arg(fixture(".")).output()?;
    if !output.status.success() {
        bail!("disk check failed");
    }
    let text = String::from_utf8(output.stdout)?;
    let free: u64 = text
        .lines()
        .last()
        .context("df output")?
        .split_whitespace()
        .nth(3)
        .context("df available")?
        .parse()?;
    if free.checked_mul(1024).context("df overflow")? < 12 * 1024 * 1024 * 1024 {
        bail!("disk guard: less than 12 GiB free");
    }
    Ok(())
}
fn free_port(port: u16) -> Result<()> {
    drop(
        TcpListener::bind(("127.0.0.1", port))
            .with_context(|| format!("port {port} must be free"))?,
    );
    Ok(())
}

struct Owned {
    name: String,
    child: Child,
    log: PathBuf,
}
#[derive(Clone)]
pub struct Node {
    pub http: String,
    pub auth: String,
    pub name: String,
    pub jwt: PathBuf,
}
impl Node {
    pub fn rpc(&self) -> Result<Rpc> {
        Rpc::new(&self.http)
    }
    pub fn engine(&self) -> Result<RpcEngine> {
        RpcEngine::new(&self.auth, &self.http, &self.jwt)
    }
    pub fn block(&self) -> Result<Value> {
        self.rpc()?
            .call("eth_getBlockByNumber", json!(["latest", false]))
    }
    pub fn state(&self, address: &str, hash: &str) -> Result<Value> {
        let rpc = self.rpc()?;
        let tag = json!({"blockHash":hash,"requireCanonical":true});
        Ok(
            json!({"balance":rpc.call("eth_getBalance",json!([address,tag]))?,"nonce":rpc.call("eth_getTransactionCount",json!([address,tag]))?}),
        )
    }
}

pub struct Runtime {
    pub path: PathBuf,
    jwt: PathBuf,
    owned: Vec<Owned>,
}
fn run_root(override_root: Option<PathBuf>, target_dir: Option<PathBuf>, pid: u32) -> PathBuf {
    override_root.unwrap_or_else(|| {
        target_dir
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .join("devnet-tests")
            .join(pid.to_string())
    })
}
impl Runtime {
    pub fn new(name: &str) -> Result<Self> {
        disk_guard()?;
        let root = run_root(
            std::env::var_os("BOOLE_DEVNET_RUN_DIR").map(PathBuf::from),
            std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from),
            std::process::id(),
        );
        let path = root.join(name);
        fs::create_dir_all(&root)?;
        fs::create_dir(&path)?;
        let jwt = path.join("jwt.hex");
        let mut secret = [0; 32];
        SystemRandom::new()
            .fill(&mut secret)
            .map_err(|_| anyhow!("generate test JWT"))?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options
            .open(&jwt)?
            .write_all(hex_string(&secret).trim_start_matches("0x").as_bytes())?;
        Ok(Self {
            path,
            jwt,
            owned: vec![],
        })
    }
    fn record_pid(&self, name: &str, pid: u32, state: &str) -> Result<()> {
        let line = serde_json::to_string(&json!({"name":name,"pid":pid,"state":state}))?;
        let mut paths = vec![self.path.join("owned-pids.jsonl")];
        if let Some(path) = std::env::var_os("BOOLE_DEVNET_OWNED_PIDS") {
            paths.push(path.into());
        }
        for path in paths {
            writeln!(
                OpenOptions::new().create(true).append(true).open(path)?,
                "{line}"
            )?;
        }
        Ok(())
    }
    fn start(&mut self, name: &str, binary: &Path, args: Vec<String>, url: &str) -> Result<()> {
        disk_guard()?;
        let log_path = self.path.join(format!("{name}-{}.log", self.owned.len()));
        let log = File::create(&log_path)?;
        let child = Command::new(binary)
            .args(args)
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .spawn()?;
        let pid = child.id();
        self.owned.push(Owned {
            name: name.into(),
            child,
            log: log_path,
        });
        self.record_pid(name, pid, "started")?;
        let rpc = Rpc::new(url)?;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(60) {
            if let Some(status) = self
                .owned
                .last_mut()
                .context("owned process")?
                .child
                .try_wait()?
            {
                bail!("{name} exited at startup: {status}");
            }
            if rpc.call("eth_chainId", json!([])).is_ok() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        bail!("{name} RPC startup timeout")
    }
    pub fn anvil(&mut self, binary: &Path) -> Result<Rpc> {
        self.anvil_with_genesis(binary, &fixture("l1-genesis.json"))
    }
    pub fn anvil_with_genesis(&mut self, binary: &Path, genesis: &Path) -> Result<Rpc> {
        free_port(28545)?;
        let args = vec![
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            "28545".into(),
            "--no-mining".into(),
            "--slots-in-an-epoch".into(),
            "4".into(),
            "--timestamp".into(),
            "1800000000".into(),
            "--init".into(),
            genesis.display().to_string(),
            "--silent".into(),
        ];
        self.start("anvil", binary, args, "http://127.0.0.1:28545")?;
        Rpc::new("http://127.0.0.1:28545")
    }
    pub fn node(&mut self, binary: &Path, genesis: &Path, name: &str, index: u16) -> Result<Node> {
        let http_port = 29545 + 10 * index;
        let auth_port = 29551 + 10 * index;
        let p2p_port = 29600 + index;
        for port in [http_port, auth_port, p2p_port] {
            free_port(port)?;
        }
        let http = format!("http://127.0.0.1:{http_port}");
        let auth = format!("http://127.0.0.1:{auth_port}");
        let pairs = [
            ("--chain", genesis.display().to_string()),
            ("--datadir", self.path.join(name).display().to_string()),
            ("--http.addr", "127.0.0.1".into()),
            ("--http.port", http_port.to_string()),
            ("--http.api", "eth,net,web3".into()),
            ("--authrpc.addr", "127.0.0.1".into()),
            ("--authrpc.port", auth_port.to_string()),
            ("--authrpc.jwtsecret", self.jwt.display().to_string()),
            ("--addr", "127.0.0.1".into()),
            ("--port", p2p_port.to_string()),
            ("--max-outbound-peers", "0".into()),
            ("--max-inbound-peers", "0".into()),
            (
                "--log.file.directory",
                self.path.join(format!("{name}-logs")).display().to_string(),
            ),
            ("--log.file.filter", "info,payload_builder=trace".into()),
            ("--engine.persistence-threshold", "0".into()),
        ];
        let mut args = vec![
            "node".into(),
            "--http".into(),
            "--ipcdisable".into(),
            "--disable-discovery".into(),
            "--disable-tx-gossip".into(),
        ];
        for (key, value) in pairs {
            args.push(key.into());
            args.push(value);
        }
        self.start(name, binary, args, &http)?;
        let response=reqwest::blocking::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(5)).build()?
            .post(&auth).json(&json!({"jsonrpc":"2.0","id":1,"method":"engine_exchangeCapabilities","params":[[]]})).send()?;
        if response.status() != 401 {
            bail!("Engine must refuse missing JWT");
        }
        let node = Node {
            http,
            auth,
            name: name.into(),
            jwt: self.jwt.clone(),
        };
        node.engine()?;
        Ok(node)
    }
    pub fn stop(&mut self, name: &str) -> Result<()> {
        let mut stopped = Vec::new();
        for owned in self.owned.iter_mut().filter(|x| x.name == name) {
            if owned.child.try_wait()?.is_some() {
                continue;
            }
            let pid = owned.child.id();
            // Only PIDs spawned and recorded by this Runtime can be signalled.
            for (signal, seconds) in [("INT", 30), ("TERM", 20)] {
                let status = Command::new("kill")
                    .args(["-s", signal, &pid.to_string()])
                    .status()?;
                if !status.success() {
                    bail!("signal {signal} failed for owned PID {pid}");
                }
                let start = Instant::now();
                while owned.child.try_wait()?.is_none()
                    && start.elapsed() < Duration::from_secs(seconds)
                {
                    thread::sleep(Duration::from_millis(50));
                }
                if owned.child.try_wait()?.is_some() {
                    break;
                }
            }
            if owned.child.try_wait()?.is_none() {
                bail!("owned PID {pid} did not stop after INT/TERM");
            }
            stopped.push(pid);
        }
        for pid in stopped {
            self.record_pid(name, pid, "stopped")?;
        }
        Ok(())
    }
    pub fn finish(&mut self, strict_logs: bool) -> Result<()> {
        let names: Vec<_> = self.owned.iter().rev().map(|x| x.name.clone()).collect();
        for name in names {
            self.stop(&name)?;
        }
        let mut issues = Vec::new();
        let mut jwt_probes = 0;
        for owned in &self.owned {
            if owned.child.id() == 0 {
                bail!("invalid owned PID");
            }
            for line in fs::read_to_string(&owned.log)?.lines() {
                if line.contains("ERROR") || line.contains("WARN") {
                    if line.contains("Invalid JWT: Authorization header is missing or invalid") {
                        jwt_probes += 1;
                    } else {
                        issues.push(line.to_owned());
                    }
                }
            }
        }
        fs::write(
            self.path.join("cleanup.json"),
            serde_json::to_vec_pretty(
                &json!({"stopped":self.owned.len(),"expected_jwt_probes":jwt_probes,"log_issues":issues}),
            )?,
        )?;
        if strict_logs && !issues.is_empty() {
            bail!("unexpected node log warnings/errors: {issues:?}");
        }
        disk_guard()
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        let names: Vec<_> = self.owned.iter().rev().map(|x| x.name.clone()).collect();
        for name in names {
            if let Err(e) = self.stop(&name) {
                eprintln!("owned process cleanup failed: {e:#}");
            }
        }
    }
}

pub fn post_calldata(raw: &str) -> Result<String> {
    let mut item = vec![1];
    item.extend(hex_bytes(raw, "fixture raw")?);
    let mut envelope = vec![0; 64];
    envelope[31] = 32;
    let size = u64::try_from(item.len())?.to_be_bytes();
    envelope[56..64].copy_from_slice(&size);
    envelope.extend(&item);
    envelope.extend(vec![0; (32 - item.len() % 32) % 32]);
    Ok(format!(
        "{}{}",
        boole_derivation::config::POST_SELECTOR,
        hex_string(&envelope).trim_start_matches("0x")
    ))
}

pub fn observe(node: &Node, derived: &Derived) -> Result<Value> {
    let block = node.block()?;
    let mut remaining: Vec<String> = block["transactions"]
        .as_array()
        .context("block txs")?
        .iter()
        .skip(1)
        .map(|x| x.as_str().unwrap_or("").to_owned())
        .collect();
    let rpc = node.rpc()?;
    let mut receipts = vec![];
    for decision in &derived.decisions {
        let Some(user) = &decision.user else {
            if decision.forward {
                bail!("non-user item forwarded");
            }
            receipts.push(Value::Null);
            continue;
        };
        let hash = user.tx_hash.clone();
        let position = remaining.iter().position(|x| *x == hash);
        let receipt = if let Some(position) = position {
            if !decision.forward {
                bail!("filtered transaction included");
            }
            remaining.remove(position);
            let receipt = rpc.call("eth_getTransactionReceipt", json!([hash]))?;
            if receipt["blockHash"] != block["hash"] {
                bail!("receipt is not at canonical observed block");
            }
            receipt
        } else {
            Value::Null
        };
        receipts.push(receipt);
    }
    if !remaining.is_empty() {
        bail!("unexpected user transactions in block");
    }
    Ok(
        json!({"blockHash":block["hash"],"stateRoot":block["stateRoot"],"receiptsRoot":block["receiptsRoot"],"receipts":receipts}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn run_folder_honors_cargo_target_dir() {
        assert_eq!(
            run_root(None, Some(PathBuf::from("build-out")), 7),
            PathBuf::from("build-out/devnet-tests/7")
        );
        assert_eq!(
            run_root(
                Some(PathBuf::from("evidence")),
                Some(PathBuf::from("build-out")),
                7
            ),
            PathBuf::from("evidence")
        );
    }
}

// Shared encoders are used by the M2 test executable.
#[allow(dead_code)]
pub mod m2;
