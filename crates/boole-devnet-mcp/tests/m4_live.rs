//! I15: actual SIGKILL boundaries over a complete v1.3 A/B/C round.
#![allow(clippy::too_many_arguments)]
//! Drivers and miner consume only their own authenticated journals below the live tip.
#[path = "../../boole-devnet-driver/tests/support/mod.rs"]
#[allow(dead_code)]
mod support;
use anyhow::{bail, ensure, Context, Result};
use boole_derivation::{attributes::hex_string, m2::*, m3::*, Derived};
use boole_devnet_archive::{journal::NetworkIdentity, state_dir::StateDirGuard};
use boole_devnet_driver::{
    driver::{Driver, StepOutcome},
    engine::{Engine, Rpc, RpcEngine},
    l1::{JournalSource, L1Source, RpcL1},
};
use boole_devnet_mcp::{
    client::{Client, ClientOptions, RpcL2, RpcPoster},
    own::{OwnVerification, PinnedOwnGate},
};
use boole_gate_supervisor::{
    codec::Codec,
    process::{Envelope, Worker},
    stub::{export, package, Descriptor},
    supervisor::{install_devnet_artifacts, Host, Mode, Supervisor},
};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use support::{m2::*, *};
type Live = Driver<JournalSource<RpcL1>, RpcEngine>;
type Miner = Client<JournalSource<RpcL1>, RpcPoster>;

struct TipProxy {
    url: String,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    denied: Arc<Mutex<Vec<Value>>>,
}
impl TipProxy {
    fn start(upstream: &str) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let denied = Arc::new(Mutex::new(vec![]));
        let s = stop.clone();
        let d = denied.clone();
        let upstream = upstream.to_owned();
        let handle = thread::spawn(move || {
            let rpc = Rpc::new(&upstream).unwrap();
            let mut transactions = std::collections::BTreeMap::<String, u64>::new();
            let mut blocks = std::collections::BTreeMap::<String, u64>::new();
            while !s.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(_) => break,
                };
                let result = (|| -> Result<()> {
                    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
                    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
                    let mut bytes = vec![];
                    let mut one = [0u8; 1];
                    while !bytes.ends_with(b"\r\n\r\n") {
                        ensure!(bytes.len() < 16384, "HTTP header cap");
                        stream.read_exact(&mut one)?;
                        bytes.push(one[0]);
                    }
                    let header = std::str::from_utf8(&bytes)?;
                    let len = header
                        .lines()
                        .find_map(|l| {
                            l.split_once(':')
                                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                        })
                        .context("request length")?;
                    ensure!(len <= 1024 * 1024, "proxy request cap");
                    let mut body = vec![0; len];
                    stream.read_exact(&mut body)?;
                    let request: Value = serde_json::from_slice(&body)?;
                    let method = request["method"].as_str().context("method")?;
                    let params = request["params"].clone();
                    let finalized =
                        rpc.call("eth_getBlockByNumber", json!(["finalized", false]))?;
                    let tip = quantity(&finalized["number"])?;
                    let n = match method {
                        "eth_getBlockByNumber" | "eth_getBlockReceipts" => params[0]
                            .as_str()
                            .filter(|v| v.starts_with("0x"))
                            .and_then(|v| u64::from_str_radix(&v[2..], 16).ok()),
                        "eth_getBlockByHash" => {
                            params[0].as_str().and_then(|h| blocks.get(h).copied())
                        }
                        "eth_getRawTransactionByHash" | "eth_getTransactionReceipt" => params[0]
                            .as_str()
                            .and_then(|h| transactions.get(h).copied()),
                        _ => None,
                    };
                    let response = if n.is_some_and(|n| n < tip) {
                        d.lock()
                            .unwrap()
                            .push(json!({"method":method,"height":n,"tip":tip}));
                        json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32099,"message":"TIP_ONLY historical read refused"}})
                    } else {
                        let value = rpc.call(method, params)?;
                        if method == "eth_getBlockByNumber" || method == "eth_getBlockByHash" {
                            if let (Ok(n), Some(h)) =
                                (quantity(&value["number"]), value["hash"].as_str())
                            {
                                blocks.insert(h.into(), n);
                                if let Some(txs) = value["transactions"].as_array() {
                                    for tx in txs {
                                        if let Some(h) = tx.as_str().or_else(|| tx["hash"].as_str())
                                        {
                                            transactions.insert(h.into(), n);
                                        }
                                    }
                                }
                            }
                        }
                        json!({"jsonrpc":"2.0","id":request["id"],"result":value})
                    };
                    let body = serde_json::to_vec(&response)?;
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len())?;
                    stream.write_all(&body)?;
                    Ok(())
                })();
                if let Err(e) = result {
                    d.lock().unwrap().push(json!({"proxy_error":e.to_string()}));
                }
            }
        });
        Ok(Self {
            url: format!("http://{addr}"),
            stop,
            thread: Some(handle),
            denied,
        })
    }
}
impl Drop for TipProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.url.trim_start_matches("http://"));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn driver(
    node: &Node,
    rt: &Runtime,
    c: &Config,
    table: &ForkTable,
    workers: &[Worker],
    proxy: &str,
) -> Result<(Live, StateDirGuard)> {
    let root = rt.path.join(format!("{}-state", node.name));
    let cache = root.join("gate-cache");
    let mut engine = node.engine()?;
    let gas = engine.head()?.gas_limit;
    let network = NetworkIdentity::new(c, Some(table), engine.header(0)?.hash, gas)?;
    let source = JournalSource::open(RpcL1::new_m2(proxy)?, &root, network, false)?;
    let guard = boole_devnet_archive::state_dir::acquire(&cache)?;
    let supervisor = Supervisor::new(
        c,
        table,
        Mode::Verify,
        workers.to_vec(),
        Some(Codec::new(zstd_binary()?)?),
        &cache,
        &cache,
        &rt.path.join("artifacts"),
        Host::local()?,
        Envelope::default(),
    )?;
    let mut d = Driver::with_m3(source, engine, c.clone(), gas, supervisor, table.clone())?;
    d.set_evidence(&root.join("evidence.jsonl"))?;
    d.set_journal(&root.join("checkpoint.json"))?;
    Ok((d, guard))
}
fn miner(
    c: &Config,
    table: &ForkTable,
    workers: &[Worker],
    artifacts: &Path,
    root: &Path,
    proxy: &str,
    node: &Node,
    user: [u8; 20],
) -> Result<Miner> {
    let mut engine = node.engine()?;
    let gas = engine.head()?.gas_limit;
    let network = NetworkIdentity::new(c, Some(table), engine.header(0)?.hash, gas)?;
    let source = JournalSource::open(RpcL1::new_m2(proxy)?, root, network, false)?;
    let pins: Vec<_> = workers.iter().map(|w| w.binary_sha256).collect();
    source.journal.borrow().cleanup(&pins, None)?;
    Client::new(
        source,
        RpcPoster::new(proxy, user)?,
        c.clone(),
        user,
        root.into(),
        ClientOptions {
            l2: Some(Box::new(RpcL2::new(&node.http)?)),
            l2_gas_limit: gas,
            codec: Some(Codec::new(zstd_binary()?)?),
            hook: None,
            forks: Some(table.clone()),
            own: Some(OwnVerification {
                gates: workers
                    .iter()
                    .cloned()
                    .map(|worker| {
                        Box::new(PinnedOwnGate {
                            worker,
                            envelope: Envelope::default(),
                        }) as Box<dyn boole_devnet_mcp::own::OwnGate>
                    })
                    .collect(),
                artifacts: artifacts.into(),
            }),
        },
    )
}
fn observed(node: &Node, h: u64, root: Hash, derived: Option<&Derived>) -> Result<Value> {
    let mut engine = node.engine()?;
    let header = engine.header(h)?;
    let txs = engine.transactions(h)?;
    if let Some(d) = derived {
        boole_devnet_driver::driver::check_payload_transactions(&json!({"transactions":txs}), d)?;
    }
    let origin = boole_derivation::system::l1_origin(&txs[0], h)?;
    ensure!(
        boole_derivation::system::read_m2_root(&txs[1], h, &origin)? == root,
        "embedded M2ROOT"
    );
    Ok(json!({"height":h,"hash":header.hash,"m2_root":hex_string(&root),"transactions":txs}))
}
fn step(driver: &mut Live, node: &Node, h: u64) -> Result<Value> {
    let StepOutcome::Derived(d) = driver.step(h)? else {
        bail!("unexpected finality wait")
    };
    observed(node, h, driver.m2_state().root, Some(&d))
}
fn driver_binary() -> PathBuf {
    std::env::var_os("BOOLE_DEVNET_DRIVER_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/boole-devnet-driver")
        })
}
fn client_binary() -> PathBuf {
    std::env::var_os("BOOLE_DEVNET_MCP_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_boole-devnet-mcp")))
}
fn owned(root: &Path, pid: u32, state: &str, role: &str) -> Result<()> {
    let v = json!({"pid":pid,"state":state,"name":role});
    let mut paths = vec![root.join("m4-owned.jsonl")];
    if let Some(p) = std::env::var_os("BOOLE_DEVNET_OWNED_PIDS") {
        paths.push(p.into());
    }
    for p in paths {
        writeln!(OpenOptions::new().create(true).append(true).open(p)?, "{v}")?;
    }
    Ok(())
}
struct Process {
    child: Child,
    root: PathBuf,
    role: String,
}
impl Process {
    fn start(mut command: Command, root: &Path, role: &str, log: &Path) -> Result<Self> {
        let f = File::create(log)?;
        let child = command.stdout(f.try_clone()?).stderr(f).spawn()?;
        owned(root, child.id(), "started", role)?;
        Ok(Self {
            child,
            root: root.into(),
            role: role.into(),
        })
    }
    fn wait(&mut self) -> Result<std::process::ExitStatus> {
        let end = Instant::now() + Duration::from_secs(40);
        loop {
            if let Some(s) = self.child.try_wait()? {
                owned(&self.root, self.child.id(), "stopped", &self.role)?;
                return Ok(s);
            }
            ensure!(Instant::now() < end, "{} child timeout", self.role);
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn kill(&mut self) -> Result<()> {
        self.child.kill()?;
        let _ = self.child.wait()?;
        owned(&self.root, self.child.id(), "stopped", &self.role)
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = owned(&self.root, self.child.id(), "stopped", &self.role);
    }
}
fn c_command(
    node: &Node,
    rt: &Runtime,
    proxy: &str,
    until: u64,
    repair: bool,
    recover: bool,
) -> Command {
    let mut cmd = Command::new(driver_binary());
    cmd.args([
        "--l1-rpc",
        proxy,
        "--node-rpc",
        &node.http,
        "--engine-rpc",
        &node.auth,
        "--jwt-secret",
    ])
    .arg(&node.jwt)
    .arg("--until")
    .arg(until.to_string())
    .arg("--state-dir")
    .arg(rt.path.join("c-state"))
    .arg("--m2-config")
    .arg(rt.path.join("config.json"))
    .arg("--m3-gates")
    .arg(rt.path.join("gates.json"))
    .arg("--m3-forks")
    .arg(rt.path.join("forks.json"))
    .arg("--gate-artifacts")
    .arg(rt.path.join("artifacts"))
    .arg("--zstd")
    .arg(zstd_binary().unwrap());
    if repair {
        cmd.arg("--repair-journals");
    }
    if recover {
        cmd.arg("--recover-engine-tail");
    }
    cmd.stdin(Stdio::null());
    cmd
}
fn authority(root: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let mut out = vec![];
    for name in [
        "input.manifest.json",
        "input.ndjson",
        "checkpoint.json",
        "evidence.jsonl",
    ] {
        if root.join(name).exists() {
            out.push((name.into(), fs::read(root.join(name))?));
        }
    }
    Ok(out)
}
fn c_run(
    node: &Node,
    rt: &Runtime,
    proxy: &str,
    until: u64,
    case: &str,
    killed: &mut bool,
    events: &mut Vec<Value>,
    serial: u64,
) -> Result<()> {
    let marker = rt.path.join("kill-marker.json");
    let log = rt.path.join(format!("c-{serial}.log"));
    let mut cmd = c_command(node, rt, proxy, until, false, false);
    let active = !*killed && !matches!(case, "B11" | "B12");
    if active {
        let point = case.split('-').next().unwrap();
        let role = if case == "B9" {
            "staged-input"
        } else if case == "B10-input" {
            "input"
        } else if case == "B10" {
            "evidence"
        } else {
            "driver"
        };
        cmd.env("BOOLE_M4_KILL_POINT", point)
            .env("BOOLE_M4_KILL_HEIGHT", if case == "B8" { "*" } else { "2" })
            .env("BOOLE_M4_KILL_ROLE", role)
            .env("BOOLE_M4_KILL_MARKER", &marker);
    }
    let mut p = Process::start(cmd, &rt.path, "driver-c", &log)?;
    let end = Instant::now() + Duration::from_secs(40);
    loop {
        if active && marker.exists() {
            let v: Value = serde_json::from_slice(&fs::read(&marker)?)?;
            ensure!(v["pid"] == p.child.id(), "kill target identity");
            p.kill()?;
            events.push(v);
            *killed = true;
            let saved = authority(&rt.path.join("c-state"))?;
            let mut restart = Process::start(
                c_command(node, rt, proxy, until, false, false),
                &rt.path,
                "driver-c-restart",
                &rt.path.join(format!("c-{serial}-restart.log")),
            )?;
            let status = restart.wait()?;
            if case.starts_with("B10") {
                ensure!(!status.success(), "torn tail silently repaired");
                ensure!(
                    authority(&rt.path.join("c-state"))? == saved,
                    "read-only torn-tail refusal changed authority files"
                );
                let text = fs::read_to_string(rt.path.join(format!("c-{serial}-restart.log")))?;
                ensure!(
                    text.contains("integrity") && text.contains("torn"),
                    "untyped torn-tail stop: {text}"
                );
                let mut repair = Process::start(
                    c_command(node, rt, proxy, until, true, false),
                    &rt.path,
                    "driver-c-repair",
                    &rt.path.join(format!("c-{serial}-repair.log")),
                )?;
                ensure!(repair.wait()?.success(), "explicit repair failed");
            } else {
                ensure!(
                    status.success(),
                    "restart failed: {}",
                    fs::read_to_string(rt.path.join(format!("c-{serial}-restart.log")))?
                );
            }
            return Ok(());
        }
        if let Some(status) = p.child.try_wait()? {
            owned(&rt.path, p.child.id(), "stopped", "driver-c")?;
            ensure!(
                status.success(),
                "driver C failed: {}",
                fs::read_to_string(&log)?
            );
            return Ok(());
        }
        ensure!(Instant::now() < end, "driver C boundary/run timeout");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn i15_kill_boundary_full_round_abc_tip_only_and_miner_restart() -> Result<()> {
    let Some(op) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let case = std::env::var("BOOLE_DEVNET_M4_CASE")
        .context("one explicit I15 boundary per leased run")?;
    ensure!(
        [
            "B1",
            "B2",
            "B3",
            "B4",
            "B5",
            "B6",
            "B7",
            "B8",
            "B9",
            "B10",
            "B10-input",
            "B11",
            "B12"
        ]
        .contains(&case.as_str()),
        "unknown boundary"
    );
    let gate = PathBuf::from(std::env::var_os("BOOLE_DEVNET_GATE").context("pinned gate")?);
    let pin = hash(&fs::read(&gate)?);
    let descriptor = Descriptor::pinned(pin);
    let workers = vec![Worker {
        descriptor: descriptor.clone(),
        binary: gate,
        binary_sha256: pin,
    }];
    let mut c = config();
    c.m3 = Some(M3Config::devnet(c.slot_table.len()));
    c.m3.as_mut().unwrap().genesis_gate_rule_digest = descriptor.digest();
    let table = ForkTable::genesis(&c)?;
    let mut rt = Runtime::new(&format!("m4a-i15-{case}"))?;
    let artifacts = rt.path.join("artifacts");
    install_devnet_artifacts(&c, &artifacts)?;
    fs::write(rt.path.join("config.json"), serde_json::to_vec(&c)?)?;
    fs::write(rt.path.join("gates.json"), serde_json::to_vec(&workers)?)?;
    fs::write(rt.path.join("forks.json"), serde_json::to_vec(&table)?)?;
    let l1p = rt.path.join("l1-genesis.json");
    fs::write(&l1p, serde_json::to_vec(&l1_genesis_value()?)?)?;
    let l1 = rt.anvil_with_genesis(&anvil, &l1p)?;
    let accounts = l1.call("eth_accounts", json!([]))?;
    let account = accounts[0].as_str().context("dev account")?;
    let user = boole_devnet_mcp::client::fixed(account)?;
    let gp =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/devnet-m3/l2-genesis-v1.3.json");
    let mut nodes = [
        rt.node(&op, &gp, "a", 0)?,
        rt.node(&op, &gp, "b", 1)?,
        rt.node(&op, &gp, "c", 2)?,
    ];
    let proxy = TipProxy::start("http://127.0.0.1:28545")?;
    let (a, ag) = driver(&nodes[0], &rt, &c, &table, &workers, &proxy.url)?;
    let (b, bg) = driver(&nodes[1], &rt, &c, &table, &workers, &proxy.url)?;
    let _guards = [ag, bg];
    let mut drivers = [a, b];
    let private = rt.path.join("miner-state");
    let mut client = Some(miner(
        &c, &table, &workers, &artifacts, &private, &proxy.url, &nodes[0], user,
    )?);
    let mut killed = false;
    let mut events = vec![];
    c_run(
        &nodes[2],
        &rt,
        &proxy.url,
        0,
        &case,
        &mut killed,
        &mut events,
        0,
    )?;
    let package = package(10000);
    let salt = [9; 32];
    let leaf = boole_derivation::m2::pool_leaf(
        c.chain_id,
        2,
        &[],
        0,
        package.len() as u32,
        &package,
        &salt,
    );
    post(&l1, account, &[(8, pool(&c, leaf, 1, 3, 1))], false)?;
    let export_file = rt.path.join("export.raw");
    fs::write(&export_file, export(0, 777))?;
    let mut id = None::<String>;
    let mut problem_id = None;
    let mut rows = vec![];
    let mut locked = Value::Null;
    let mut vested = Value::Null;
    let mut restarted = false;
    let mut comparisons = 0;
    for latest in 1..=220u64 {
        if latest == 5 {
            post(
                &l1,
                account,
                &[(9, problem(0, 1, 0, salt, &[], &package))],
                false,
            )?;
        }
        l1.call("evm_mine", json!([1800000000 + 12 * latest]))?;
        let finalized = RpcL1::new_m2("http://127.0.0.1:28545")?.finalized()?;
        // All calls finish before the next mine. This gives TIP_ONLY an exact,
        // independently enforced current tip rather than a moving request race.
        for driver in &mut drivers {
            ensure!(
                driver.l1.finalized()? == finalized,
                "finalized source disagreement"
            );
        }
        let mut batch = vec![];
        while drivers[0].m2_state().height() < finalized {
            let h = drivers[0].m2_state().height() + 1;
            let a = step(&mut drivers[0], &nodes[0], h)?;
            let b = step(&mut drivers[1], &nodes[1], h)?;
            ensure!(a == b, "A/B mismatch at {h}");
            batch.push((h, drivers[0].m2_state().root, a));
        }
        c_run(
            &nodes[2],
            &rt,
            &proxy.url,
            finalized,
            &case,
            &mut killed,
            &mut events,
            latest,
        )?;
        for (h, root, a) in batch {
            let cc = observed(&nodes[2], h, root, None)?;
            ensure!(a == cc, "A/C mismatch at {h}");
            comparisons += 1;
            rows.push(a);
        }
        client.as_mut().unwrap().sync()?;
        if finalized > 0 {
            client.as_mut().unwrap().tick()?;
        }
        let status = client
            .as_mut()
            .unwrap()
            .call("boole.devnet_status", &json!({}))?;
        ensure!(
            status["stalled"].is_null() && status["fold_height"] == finalized,
            "miner finalized fold: {status}"
        );
        ensure!(
            status["m2_root"] == hex_string(&drivers[0].m2_state().root),
            "miner root differs"
        );
        if finalized > 0 {
            ensure!(status["l2_m2_root_match"] == true, "miner L2 match");
        }
        if id.is_none() && finalized >= 5 {
            let p = drivers[0].m2_state().rounds[&(0, 1)]
                .problem
                .context("drawn problem")?;
            problem_id = Some(p);
            let args = json!({"export_file":export_file,"payee":hex_string(&[6;20]),"problem_id":hex_string(&p)});
            if case == "B11" {
                drop(client.take());
                let marker = rt.path.join("client-kill-marker.json");
                let mut command = Command::new(client_binary());
                command
                    .args([
                        "--devnet",
                        "--l1-rpc",
                        &proxy.url,
                        "--node-rpc",
                        &nodes[0].http,
                        "--account",
                        account,
                        "--state-dir",
                    ])
                    .arg(&private)
                    .arg("--m2-config")
                    .arg(rt.path.join("config.json"))
                    .arg("--m3-forks")
                    .arg(rt.path.join("forks.json"))
                    .arg("--verify-own")
                    .arg("--m3-gates")
                    .arg(rt.path.join("gates.json"))
                    .arg("--gate-artifacts")
                    .arg(&artifacts)
                    .arg("--zstd")
                    .arg(zstd_binary()?)
                    .env("BOOLE_M4_KILL_POINT", "B11")
                    .env("BOOLE_M4_KILL_HEIGHT", "0")
                    .env("BOOLE_M4_KILL_ROLE", "client")
                    .env("BOOLE_M4_KILL_MARKER", &marker)
                    .stdin(Stdio::piped());
                let mut process = Process::start(
                    command,
                    &rt.path,
                    "miner-b11",
                    &rt.path.join("miner-b11.log"),
                )?;
                let request = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"boole.devnet_commit","arguments":args}});
                writeln!(
                    process.child.stdin.as_mut().context("miner input")?,
                    "{request}"
                )?;
                process.child.stdin.as_mut().unwrap().flush()?;
                let end = Instant::now() + Duration::from_secs(20);
                while !marker.exists() {
                    if let Some(exit) = process.child.try_wait()? {
                        bail!(
                            "B11 miner exited {exit}: {}",
                            fs::read_to_string(rt.path.join("miner-b11.log"))?
                        );
                    }
                    ensure!(Instant::now() < end, "B11 marker timeout");
                    thread::sleep(Duration::from_millis(10));
                }
                let hit: Value = serde_json::from_slice(&fs::read(marker)?)?;
                ensure!(hit["pid"] == process.child.id(), "B11 PID");
                process.kill()?;
                events.push(hit);
                killed = true;
                let names: Vec<_> = fs::read_dir(&private)?
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .strip_prefix("submission-")
                            .and_then(|n| n.strip_suffix(".json"))
                            .map(str::to_owned)
                    })
                    .collect();
                ensure!(names.len() == 1, "one durable submission after B11");
                id = Some(names[0].clone());
                client = Some(miner(
                    &c, &table, &workers, &artifacts, &private, &proxy.url, &nodes[0], user,
                )?);
                client.as_mut().unwrap().tick()?;
            } else {
                let view = client
                    .as_mut()
                    .unwrap()
                    .call("boole.devnet_commit", &args)?;
                id = Some(
                    view["submission_id"]
                        .as_str()
                        .context("submission id")?
                        .into(),
                );
            }
        }
        if let Some(id) = &id {
            let view = client
                .as_mut()
                .unwrap()
                .call("boole.devnet_submission", &json!({"submission_id":id}))?;
            let key = (problem_id.unwrap(), boole_devnet_mcp::client::fixed(id)?);
            if let Some(reward) = drivers[0]
                .m2_state()
                .m3
                .as_ref()
                .unwrap()
                .rewards
                .values()
                .find(|r| r.key == key)
            {
                if reward.status == RewardStatus::Locked {
                    locked = view.clone();
                }
                if reward.status == RewardStatus::Vested {
                    vested = client.as_mut().unwrap().call(
                        "boole.devnet_rewards",
                        &json!({"payee":hex_string(&[6;20])}),
                    )?;
                    ensure!(
                        view["status"] == "ACCEPTED" && view["rewards"][0]["status"] == "VESTED",
                        "miner terminal state"
                    );
                    for node in &nodes {
                        ensure!(
                            node.rpc()?
                                .call("eth_getBalance", json!([hex_string(&[6; 20]), "latest"]))?
                                == format!("0x{:x}", reward.amount),
                            "A/B/C vested balance"
                        );
                    }
                    if finalized >= reward.vest_h + 2 && killed && restarted {
                        break;
                    }
                }
            }
            if !restarted && finalized >= 40 && !locked.is_null() {
                drop(client.take());
                client = Some(miner(
                    &c, &table, &workers, &artifacts, &private, &proxy.url, &nodes[0], user,
                )?);
                client.as_mut().unwrap().sync()?;
                restarted = true;
                if case == "B12" {
                    let before = authority(&rt.path.join("c-state"))?;
                    let text = fs::read_to_string(rt.path.join("owned-pids.jsonl"))?;
                    let pid = text
                        .lines()
                        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                        .find(|v| v["name"] == "c" && v["state"] == "started")
                        .and_then(|v| v["pid"].as_u64())
                        .context("owned C Engine PID")?;
                    ensure!(
                        Command::new("kill")
                            .args(["-KILL", &pid.to_string()])
                            .status()?
                            .success(),
                        "kill owned C Engine"
                    );
                    rt.stop("c")?;
                    owned(&rt.path, pid as u32, "stopped", "c")?;
                    let end = Instant::now() + Duration::from_secs(5);
                    while TcpStream::connect(nodes[2].http.trim_start_matches("http://")).is_ok() {
                        ensure!(Instant::now() < end, "C Engine port after kill");
                        thread::sleep(Duration::from_millis(20));
                    }
                    nodes[2] = rt.node(&op, &gp, "c-rebuilt", 2)?;
                    let log = rt.path.join("c-tail-default.log");
                    let mut default = Process::start(
                        c_command(&nodes[2], &rt, &proxy.url, finalized, false, false),
                        &rt.path,
                        "driver-c-tail-default",
                        &log,
                    )?;
                    ensure!(!default.wait()?.success(), "automatic engine-tail recovery");
                    ensure!(
                        authority(&rt.path.join("c-state"))? == before,
                        "default tail refusal changed authority files"
                    );
                    ensure!(
                        fs::read_to_string(log)?.contains("engine-tail"),
                        "typed tail refusal"
                    );
                    let mut recover = Process::start(
                        c_command(&nodes[2], &rt, &proxy.url, finalized, false, true),
                        &rt.path,
                        "driver-c-tail-explicit",
                        &rt.path.join("c-tail-explicit.log"),
                    )?;
                    ensure!(recover.wait()?.success(), "explicit tail recovery");
                    for row in &rows {
                        let h = row["height"].as_u64().unwrap();
                        let root =
                            boole_devnet_mcp::client::fixed(row["m2_root"].as_str().unwrap())?;
                        ensure!(
                            observed(&nodes[2], h, root, None)? == *row,
                            "rebuilt tail differs at {h}"
                        );
                    }
                    events.push(json!({"point":"B12","height":finalized,"pid":pid,"signal":"SIGKILL","default_refused_preserving_all_authority":true,"explicit_rebuild_from_journal":true}));
                    killed = true;
                }
            }
        }
        if latest % 10 == 0 {
            eprintln!("I15 {case} head {latest} finalized {finalized}");
        }
    }
    ensure!(
        killed && restarted && !locked.is_null() && !vested.is_null(),
        "I15 boundary/round/restart/vest incomplete"
    );
    ensure!(
        comparisons == drivers[0].m2_state().height() as usize && comparisons >= 64,
        "every derived height compared"
    );
    let id = id.context("id")?;
    let final_view = client
        .as_mut()
        .unwrap()
        .call("boole.devnet_submission", &json!({"submission_id":id}))?;
    ensure!(
        final_view["transactions"]
            .as_array()
            .context("stages")?
            .len()
            == 3,
        "no terminal reposting"
    );
    drop(client.take());
    let prepared: Value =
        serde_json::from_slice(&fs::read(private.join(format!("submission-{id}.json")))?)?;
    let body: Vec<u8> = serde_json::from_value(prepared["prepared"]["body"].clone())?;
    let secret: Hash = serde_json::from_value(prepared["prepared"]["cancel_secret"].clone())?;
    let mut result = json!({"schema":"boole-devnet-m4a-i15-v1.3","case":case,"config_json":serde_json::to_string(&c)?,"forks":table,"gate_descriptors":[descriptor],"events":events,"rows":rows,"locked":locked,"vested":vested,"final_submission":final_view,"acceptance":{"every_height_compared":comparisons,"actual_sigkill":true,"full_round_through_vest":true,"miner_journal_restart":true,"tip_only_denied_requests":proxy.denied.lock().unwrap().clone(),"client_execution":"library; B11 uses the actual stdio client binary","no_terminal_reposting":true},"provenance":{"producer_commit":String::from_utf8(Command::new("git").args(["rev-parse","HEAD"]).output()?.stdout)?.trim(),"producer_sha256":hex_string(&hash(include_bytes!("m4_live.rs"))),"producer_binary_sha256":hex_string(&hash(&fs::read(std::env::current_exe()?)?)),"driver_binary_sha256":hex_string(&hash(&fs::read(driver_binary())?)),"client_binary_sha256":hex_string(&hash(&fs::read(client_binary())?)),"gate_binary_sha256":hex_string(&pin),"cargo_lock_sha256":hex_string(&hash(include_bytes!("../../../Cargo.lock"))),"support_sha256":hex_string(&hash(include_bytes!("../../boole-devnet-driver/tests/support/mod.rs"))),"wire_support_sha256":hex_string(&hash(include_bytes!("../../boole-devnet-driver/tests/support/m2.rs"))),"engine_profile":engine_profile(),"client_pins":fixture_json("pins.json")?,"l1_genesis_sha256":hex_string(&hash(&fs::read(l1p)?)),"l2_genesis_sha256":hex_string(&hash(&fs::read(gp)?)),"producer_profile":if cfg!(debug_assertions){"debug"}else{"release"}}});
    ensure!(
        proxy.denied.lock().unwrap().is_empty(),
        "TIP_ONLY proxy saw historical reads or protocol failures"
    );
    for material in [body.as_slice(), &body[53..85], secret.as_slice()] {
        let h = hex_string(material);
        let a = serde_json::to_string(material)?;
        let public = result.to_string();
        ensure!(
            !public.contains(&h[2..]) && !public.contains(&a),
            "private material in I15 recording"
        );
        if rt.path.join("miner-b11.log").exists() {
            let log = fs::read_to_string(rt.path.join("miner-b11.log"))?;
            ensure!(
                !log.contains(&h[2..]) && !log.contains(&a),
                "private material in stdio output"
            );
        }
    }
    rt.finish(true)?;
    result["cleanup"] = serde_json::from_slice(&fs::read(rt.path.join("cleanup.json"))?)?;
    let path = PathBuf::from(
        std::env::var_os("BOOLE_DEVNET_M4_RECORDING").context("fresh I15 recording path")?,
    );
    ensure!(!path.exists(), "fresh recording required");
    fs::write(path, serde_json::to_vec_pretty(&result)?)?;
    Ok(())
}
