//! I16: one small v1.3 chain covers KERNEL-FIX and both quarantine dispositions.
use super::*;
use boole_devnet_archive::{
    archive::{ArchiveIdentity, ArchiveProfile, ArchiveStore, Selection},
    http::ArchiveServer,
    pins::ReadOnlyPins,
};
use std::collections::BTreeMap;

struct HistoryProxy {
    url: String,
    floor: Arc<std::sync::atomic::AtomicU64>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    requests: Arc<Mutex<Vec<Value>>>,
}
fn numeric(v: &Value) -> Option<u64> {
    let s = v.as_str()?;
    if s == "earliest" {
        Some(0)
    } else {
        s.strip_prefix("0x")
            .and_then(|s| u64::from_str_radix(s, 16).ok())
    }
}
fn block_height(
    rpc: &Rpc,
    tag: &Value,
    hashes: &BTreeMap<String, u64>,
    frozen: bool,
) -> Result<Option<u64>> {
    if let Some(hash) = tag
        .as_str()
        .filter(|s| s.len() == 66)
        .or_else(|| tag["blockHash"].as_str())
    {
        if let Some(h) = hashes.get(hash) {
            return Ok(Some(*h));
        }
        if frozen {
            return Ok(None);
        }
        let b = rpc.call("eth_getBlockByHash", json!([hash, false]))?;
        return Ok(numeric(&b["number"]));
    }
    Ok(numeric(tag).or_else(|| numeric(&tag["blockNumber"])))
}
fn request_height(
    rpc: &Rpc,
    method: &str,
    p: &Value,
    hashes: &BTreeMap<String, u64>,
    frozen: bool,
) -> Result<(Option<u64>, bool)> {
    if matches!(
        method,
        "eth_getBlockByNumber"
            | "eth_getBlockByHash"
            | "eth_getBlockReceipts"
            | "eth_getTransactionByBlockHashAndIndex"
            | "eth_getTransactionByBlockNumberAndIndex"
    ) {
        let tag = &p[0];
        let h = block_height(rpc, tag, hashes, frozen)?;
        let unknown =
            h.is_none() && (tag.as_str().is_some_and(|s| s.starts_with("0x")) || tag.is_object());
        return Ok((h, unknown));
    }
    if matches!(
        method,
        "eth_getTransactionReceipt" | "eth_getRawTransactionByHash" | "eth_getTransactionByHash"
    ) {
        if frozen {
            return Ok((None, true));
        }
        let tx = rpc.call("eth_getTransactionByHash", json!([p[0]]))?;
        let h = numeric(&tx["blockNumber"]);
        return Ok((h, h.is_none()));
    }
    if method == "eth_getLogs" {
        let f = &p[0];
        if !f["blockHash"].is_null() {
            let h = block_height(rpc, &f["blockHash"], hashes, frozen)?;
            return Ok((h, h.is_none()));
        }
        return Ok((
            [numeric(&f["fromBlock"]), numeric(&f["toBlock"])]
                .into_iter()
                .flatten()
                .min(),
            false,
        ));
    }
    if matches!(
        method,
        "eth_getCode"
            | "eth_getBalance"
            | "eth_getTransactionCount"
            | "eth_getStorageAt"
            | "eth_call"
            | "eth_getProof"
    ) {
        let tag = p.as_array().and_then(|p| p.last()).unwrap_or(&Value::Null);
        return Ok((block_height(rpc, tag, hashes, frozen)?, false));
    }
    Ok((None, false))
}
impl HistoryProxy {
    fn start(upstream: &str) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}", listener.local_addr()?);
        let floor = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(vec![]));
        let (f, s, log) = (floor.clone(), stop.clone(), requests.clone());
        let upstream = upstream.to_owned();
        let handle = thread::spawn(move || {
            let rpc = Rpc::new(&upstream).unwrap();
            let transport = reqwest::blocking::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap();
            let mut hashes = BTreeMap::new();
            // Only exact responses captured from the live L1 are replayed after
            // capture. The backend is stopped, and old-height keys are refused
            // before lookup. No historical source is accessible to a driver.
            let mut captured = BTreeMap::<String, Value>::new();
            while !s.load(Ordering::SeqCst) {
                let (mut stream, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(_) => break,
                };
                if s.load(Ordering::SeqCst) {
                    break;
                }
                let response=(||->Result<Value>{
                    stream.set_nonblocking(false)?;stream.set_read_timeout(Some(Duration::from_secs(10)))?;stream.set_write_timeout(Some(Duration::from_secs(10)))?;
                    let mut header=vec![];let mut one=[0;1];
                    while !header.ends_with(b"\r\n\r\n") {ensure!(header.len()<16384,"I16 header cap");stream.read_exact(&mut one)?;header.push(one[0]);}
                    let header=std::str::from_utf8(&header)?;
                    let length=header.lines().find_map(|l|l.split_once(':').filter(|(k,_)|k.eq_ignore_ascii_case("content-length")).and_then(|(_,v)|v.trim().parse::<usize>().ok())).context("I16 request length")?;
                    ensure!(length<=1024*1024,"I16 request cap");let mut body=vec![0;length];stream.read_exact(&mut body)?;
                    let request:Value=serde_json::from_slice(&body)?;let method=request["method"].as_str().context("I16 RPC method")?;
                    let floor=f.load(Ordering::SeqCst);
                    let (height,unknown)=request_height(&rpc,method,&request["params"],&hashes,floor>0)?;
                    let denied=height.is_some_and(|h|h<floor) || floor>0&&unknown;
                    log.lock().unwrap().push(json!({"method":method,"id":request["id"],"height":height,"floor":floor,"denied":denied}));
                    if denied {return Ok(json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32099,"message":"I16 retained history only"}}));}
                    let key=serde_json::to_string(&json!([method,request["params"]]))?;
                    let mut response:Value=if floor>0 {
                        captured.get(&key).context("uncaptured tip metadata; backend disconnected")?.clone()
                    } else {transport.post(&upstream).json(&request).send()?.json()?};
                    response["id"]=request["id"].clone();
                    if floor==0 { captured.insert(key,response.clone()); }
                    let v=&response["result"];
                    if let (Some(h),Some(hash))=(numeric(&v["number"]),v["hash"].as_str()) {hashes.insert(hash.to_owned(),h);}
                    Ok(response)
                })().unwrap_or_else(|e| {
                    log.lock().unwrap().push(json!({"proxy_error":format!("{e:#}")}));
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32098,"message":format!("I16 proxy error: {e:#}")}})
                });
                if let Ok(body) = serde_json::to_vec(&response) {
                    let _=write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                    let _ = stream.write_all(&body);
                }
            }
        });
        Ok(Self {
            url,
            floor,
            stop,
            thread: Some(handle),
            requests,
        })
    }
}
impl Drop for HistoryProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.url.trim_start_matches("http://"));
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}
fn cli(
    node: &Node,
    rt: &Runtime,
    proxy: &HistoryProxy,
    until: u64,
    archive: &str,
) -> Result<Command> {
    let mut c = Command::new(driver_binary());
    c.args([
        "--l1-rpc",
        &proxy.url,
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
    .arg(rt.path.join(format!("{}-state", node.name)))
    .arg("--m2-config")
    .arg(rt.path.join("config.json"))
    .arg("--m3-forks")
    .arg(rt.path.join("forks.json"))
    .arg("--m3-gates")
    .arg(rt.path.join("gates.json"))
    .arg("--gate-artifacts")
    .arg(rt.path.join("artifacts"))
    .arg("--archive")
    .arg(archive)
    .arg("--zstd")
    .arg(zstd_binary()?);
    Ok(c)
}
fn run_cli(
    node: &Node,
    rt: &Runtime,
    proxy: &HistoryProxy,
    h: u64,
    x: &str,
    label: &str,
) -> Result<()> {
    let log = rt.path.join(format!("{}-{label}-{h}.log", node.name));
    let mut process = Process::start(
        cli(node, rt, proxy, h, x)?,
        &rt.path,
        &format!("{}-{label}-{h}", node.name),
        &log,
    )?;
    ensure!(
        process.wait_for(Duration::from_secs(60))?.success(),
        "I16 driver failed: {}",
        fs::read_to_string(log)?
    );
    Ok(())
}
fn probe(url: &str, method: &str, params: Value, id: &str) -> Result<std::process::Output> {
    let mut c = Command::new("/usr/bin/curl");
    c.args([
        "--noproxy",
        "*",
        "--max-time",
        "3",
        "--silent",
        "--show-error",
        "-H",
        "Content-Type: application/json",
        "--data",
    ])
    .arg(serde_json::to_string(
        &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
    )?)
    .arg(url);
    Ok(c.output()?)
}
fn proof_for(c: &Config, p: Hash, tag: u8, salt: u8, lane: u8) -> Result<Proof> {
    let raw = export(tag, 777);
    let frame = boole_gate_supervisor::raw_frame::encode(&raw)?;
    let mut proof = Proof::new(c, p, &frame, [salt; 32], [6; 20]);
    proof.body[152] = lane;
    proof.body[153..161].copy_from_slice(&(raw.len() as u64).to_be_bytes());
    proof.body[161..193].copy_from_slice(&hash(&raw));
    proof.commit = commit_hash(
        c.chain_id,
        p,
        cancel_hash(&proof.secret),
        body_digest(&proof.body),
    );
    Ok(proof)
}
fn size(path: &Path) -> Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let mut logical = 0;
    let mut physical = 0;
    for e in fs::read_dir(path)? {
        let e = e?;
        let m = fs::symlink_metadata(e.path())?;
        if m.is_dir() {
            let (a, b) = size(&e.path())?;
            logical += a;
            physical += b;
        } else if m.is_file() {
            logical += m.len();
            physical += m.blocks() * 512;
        }
    }
    Ok((logical, physical))
}

#[test]
fn i16_pinned_forks_restart_with_no_old_l1_or_archive_and_resume_withheld_pin() -> Result<()> {
    let Some(op) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    ensure!(
        cfg!(target_os = "macos"),
        "I16 direct-backend denial requires the qualified Mac controller"
    );
    let gate = PathBuf::from(std::env::var_os("BOOLE_DEVNET_GATE").context("I16 gate")?);
    let pin = hash(&fs::read(&gate)?);
    let old = Descriptor::pinned(pin);
    let fixed = Descriptor {
        fixed: true,
        ..old.clone()
    };
    let mut c = config();
    c.m3 = Some(M3Config::devnet(c.slot_table.len()));
    c.m3.as_mut().unwrap().genesis_gate_rule_digest = old.digest();
    let mut table = ForkTable::genesis(&c)?;
    let mut rt = Runtime::new("m4c2-i16")?;
    install_devnet_artifacts(&c, &rt.path.join("artifacts"))?;
    install_devnet_artifacts(&c, &rt.path.join("audit-artifacts"))?;
    let originals = rt.path.join("original-release");
    fs::create_dir(&originals)?;
    let copied = originals.join("gate");
    fs::copy(&gate, &copied)?;
    let workers = vec![
        Worker {
            descriptor: old.clone(),
            binary: copied.clone(),
            binary_sha256: pin,
        },
        Worker {
            descriptor: fixed.clone(),
            binary: copied,
            binary_sha256: pin,
        },
    ];
    fs::write(rt.path.join("config.json"), serde_json::to_vec(&c)?)?;
    fs::write(rt.path.join("gates.json"), serde_json::to_vec(&workers)?)?;
    fs::write(rt.path.join("forks.json"), serde_json::to_vec(&table)?)?;
    let l1g = rt.path.join("l1-genesis.json");
    fs::write(&l1g, serde_json::to_vec(&l1_genesis_value()?)?)?;
    let l1 = rt.anvil_with_genesis(&anvil, &l1g)?;
    let accounts = l1.call("eth_accounts", json!([]))?;
    let user = accounts[0].as_str().context("account")?;
    let gp =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/devnet-m3/l2-genesis-v1.3.json");
    let nodes = [
        rt.node(&op, &gp, "a", 0)?,
        rt.node(&op, &gp, "b", 1)?,
        rt.node(&op, &gp, "c", 2)?,
    ];
    let proxy = HistoryProxy::start("http://127.0.0.1:28545")?;
    let (mut a, ag) = driver(&nodes[0], &rt, &c, &table, &workers, &proxy.url)?;
    let (mut b, bg) = driver(&nodes[1], &rt, &c, &table, &workers, &proxy.url)?;
    let (mut reference, rg) = driver(&nodes[2], &rt, &c, &table, &workers, &proxy.url)?;
    let _rg = rg;
    let source = reference.l1.journal.borrow();
    let x = Arc::new(Mutex::new(ArchiveStore::open(
        &rt.path.join("x"),
        ArchiveProfile {
            identity: ArchiveIdentity {
                network: source.network().clone(),
                l1_anchor_number: 0,
                l1_anchor_hash: source.anchor_hash().into(),
            },
            bind: "127.0.0.1:0".parse()?,
            prune_before: None,
        },
        false,
    )?));
    drop(source);
    let mut server = Some(ArchiveServer::start(x.clone(), "127.0.0.1:0".parse()?)?);
    let xurl = server.as_ref().unwrap().url();
    let pkg = package(10000);
    let carried = boole_gate_supervisor::raw_frame::encode(&pkg)?;
    let salts = [[9; 32], [10; 32]];
    let mut proofs: Vec<Proof> = vec![];
    let mut rows = vec![];
    let mut model = State::new(&c, 30_000_000)?;
    let mut immediate = RpcL1::new_m2(&proxy.url)?;
    let mut captured_tip = 0;
    for latest in 1..=72u64 {
        let mut items = vec![];
        match latest {
            1 => {
                for (slot, salt) in salts.iter().enumerate() {
                    let leaf = boole_derivation::m2::pool_leaf(
                        c.chain_id,
                        2,
                        &[],
                        1,
                        pkg.len() as u32,
                        &carried,
                        salt,
                    );
                    items.push((8, pool_at(&c, slot as u8, leaf, 1, 3, 1)));
                }
            }
            5 => {
                for (slot, salt) in salts.iter().enumerate() {
                    items.push((
                        9,
                        problem_encoded(
                            slot as u8,
                            1,
                            0,
                            *salt,
                            &[],
                            &carried,
                            (1, pkg.len() as u32),
                        ),
                    ));
                }
            }
            6..=9 => {
                let i = (latest - 6) as usize;
                let slot = usize::from(i >= 2);
                let p = model.rounds[&(slot as u8, 1)]
                    .problem
                    .context("I16 drawn problem")?;
                let proof = proof_for(
                    &c,
                    p,
                    [7, 10, 6, 6][i],
                    (17 + i) as u8,
                    c.slot_table[slot].lane,
                )?;
                items.push((2, proof.commit_body()));
                proofs.push(proof);
            }
            16 => items.push((3, proofs[0].open())),
            17 => {
                items.push((4, proofs[0].part(0)));
                items.push((3, proofs[1].open()));
                items.push((4, proofs[1].part(0)));
            }
            18 => items.push((3, proofs[2].open())),
            19 => items.push((3, proofs[3].open())),
            20 => {
                items.push((4, proofs[2].part(0)));
                items.push((4, proofs[3].part(0)));
            }
            _ => {}
        }
        if !items.is_empty() {
            post(&l1, user, &items, items.len() > 1)?;
        }
        l1.call("evm_mine", json!([1800000000 + 12 * latest]))?;
        captured_tip = reference.l1.finalized()?;
        ensure!(
            a.l1.finalized()? == captured_tip && b.l1.finalized()? == captured_tip,
            "I16 capture views"
        );
        if latest <= 5 {
            let row = immediate.input_row(latest)?.verify()?;
            let mut inputs = vec![];
            for log in &row.logs {
                let data = boole_derivation::attributes::hex_bytes(&log.data, "posted data")?;
                if let Ok((kind, body)) = boole_derivation::inbox::item(&data) {
                    inputs.push(Input {
                        transaction_index: log.transaction_index,
                        log_index: log.log_index,
                        kind,
                        body: body.to_vec(),
                    });
                }
            }
            model = model
                .step_with_verdicts(&c, &row.block, &inputs, &[], &table)?
                .state;
        }
    }
    ensure!(
        captured_tip >= 56 && proofs.len() == 4,
        "I16 complete finalized capture"
    );
    drop(a);
    drop(b);
    drop(ag);
    drop(bg);
    // Populate an honest, frozen metadata snapshot before disconnecting L1.
    let tip_rpc = Rpc::new(&proxy.url)?;
    for (method, params) in [
        ("eth_chainId", json!([])),
        ("eth_blockNumber", json!([])),
        (
            "eth_getCode",
            json!([boole_derivation::config::INBOX, "latest"]),
        ),
        ("eth_getBlockByNumber", json!(["finalized", false])),
        ("eth_getBlockByNumber", json!(["latest", false])),
    ] {
        tip_rpc.call(method, params)?;
    }
    let old_hash = tip_rpc.call("eth_getBlockByNumber", json!(["0x1", false]))?["hash"].clone();
    // X is a real finalized archive before the restart, then unavailable.
    let genesis = reference.engine.header(0)?;
    let source = reference.l1.journal.borrow();
    x.lock().unwrap().ingest(
        &source.row(0)?,
        captured_tip,
        &Selection {
            l2_number: 0,
            l2_hash: genesis.hash,
            root: hex_string(&reference.m2_state().root),
            system: vec![],
            inputs: "journal".into(),
            verdicts: "computed".into(),
        },
    )?;
    drop(source);
    for h in 1..=24 {
        let expected = step(&mut reference, &nodes[2], h)?;
        let source = reference.l1.journal.borrow();
        x.lock().unwrap().ingest(
            &source.row(h)?,
            captured_tip,
            &Selection {
                l2_number: h,
                l2_hash: expected["hash"].as_str().unwrap().into(),
                root: expected["m2_root"].as_str().unwrap().into(),
                system: expected["transactions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().into())
                    .collect(),
                inputs: "journal".into(),
                verdicts: "computed".into(),
            },
        )?;
        rows.push(expected);
    }
    for node in &nodes[..2] {
        run_cli(node, &rt, &proxy, 24, &xurl, "initial")?;
    }
    for expected in &rows {
        let h = expected["height"].as_u64().unwrap();
        let root =
            boole_derivation::attributes::hex_bytes(expected["m2_root"].as_str().unwrap(), "root")?
                .try_into()
                .unwrap();
        for node in &nodes[..2] {
            ensure!(
                observed(node, h, root, None)? == *expected,
                "I16 initial prefix at {h}"
            );
        }
    }
    drop(server.take());
    ensure!(
        reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()?
            .get(format!("{xurl}/boole/archive/v1/info"))
            .send()
            .is_err(),
        "X remained connected"
    );
    // Backend closure is the direct-access barrier, not an outer sandbox that
    // would interfere with the gate's unchanged worker sandbox. Only captured
    // current-tip metadata remains at the proxy; no below-tip row is served.
    rt.stop("anvil")?;
    proxy.floor.store(captured_tip, Ordering::SeqCst);
    let direct = probe(
        "http://127.0.0.1:28545",
        "eth_blockNumber",
        json!([]),
        "i16-direct-backend",
    )?;
    ensure!(
        !direct.status.success(),
        "direct backend bypass was reachable"
    );
    let allowed = probe(&proxy.url, "eth_blockNumber", json!([]), "i16-tip-positive")?;
    ensure!(
        allowed.status.success()
            && serde_json::from_slice::<Value>(&allowed.stdout)?["result"].is_string(),
        "captured tip positive control"
    );
    let mut denied = 0;
    for (method, params, id) in [
        (
            "eth_getBlockByNumber",
            json!(["0x1", false]),
            "i16-old-number",
        ),
        (
            "eth_getBlockByHash",
            json!([old_hash, false]),
            "i16-old-hash",
        ),
        (
            "eth_getLogs",
            json!([{"blockHash":old_hash}]),
            "i16-old-logs",
        ),
    ] {
        let response = probe(&proxy.url, method, params, id)?;
        ensure!(
            response.status.success()
                && serde_json::from_slice::<Value>(&response.stdout)?["error"]["code"] == -32099,
            "old L1 control not denied"
        );
        denied += 1;
    }
    let controls = json!({"direct_backend_refused":true,"backend_stopped":true,"captured_finalized_tip":captured_tip,"proxy_tip_allowed":true,"old_methods_refused":denied});
    fs::rename(
        rt.path.join("artifacts"),
        rt.path.join("preserved-original-artifacts"),
    )?;
    fs::rename(&originals, rt.path.join("preserved-original-release"))?;
    let mut list = vec![
        ForkItem {
            commit_hash: proofs[2].commit,
            disposition: Disposition::Void,
        },
        ForkItem {
            commit_hash: proofs[3].commit,
            disposition: Disposition::Recheck,
        },
    ];
    list.sort_by_key(|i| i.commit_hash);
    table.rows.push(ForkRow {
        epoch: 1,
        height: 28,
        gate_rule_digest: fixed.digest(),
        kind: ForkKind::Quarantine,
        list,
    });
    table.rows.push(ForkRow {
        epoch: 2,
        height: 40,
        gate_rule_digest: fixed.digest(),
        kind: ForkKind::KernelFix,
        list: vec![],
    });
    table
        .validate(&c)
        .map_err(|s| anyhow::anyhow!("I16 S{s}"))?;
    reference.set_forks(table.clone())?;
    fs::write(rt.path.join("forks.json"), serde_json::to_vec(&table)?)?;
    let node_root = rt.path.join("a-state");
    let retained = node_root
        .join("pins-v1/objects")
        .join(hex_string(&c.base_root));
    let backup = rt.path.join("withheld-base");
    let cp = fs::read(node_root.join("checkpoint.json"))?;
    let evidence = fs::read(node_root.join("evidence.jsonl"))?;
    fs::rename(&retained, &backup)?;
    let log = rt.path.join("withheld-stop.log");
    let mut child = Process::start(
        cli(&nodes[0], &rt, &proxy, 24, &xurl)?,
        &rt.path,
        "a-withheld-pin",
        &log,
    )?;
    let stopped = child.wait()?;
    let message = fs::read_to_string(&log)?;
    ensure!(
        !stopped.success() && message.contains("S20") && message.contains("pin-artifact"),
        "withheld pin not typed: {message}"
    );
    ensure!(
        fs::read(node_root.join("checkpoint.json"))? == cp
            && fs::read(node_root.join("evidence.jsonl"))? == evidence,
        "withheld pin changed dependent files"
    );
    ensure!(
        hash(&fs::read(&backup)?) == c.base_root,
        "restored object digest"
    );
    fs::rename(backup, &retained)?;
    for node in &nodes[..2] {
        run_cli(node, &rt, &proxy, 24, &xurl, "cold")?;
    }
    let stopped_pin = json!({"availability":true,"files_preserved":true,"restored_digest":hex_string(&c.base_root),"resumed":true});
    let mut snapshot_reasons = Value::Null;
    for h in 25..=captured_tip {
        rows.push(step(&mut reference, &nodes[2], h)?);
        if h == 25 {
            ensure!(
                reference.m2_state().candidates[&(proofs[1].problem, proofs[1].commit)].status()
                    == Status::Rejected
                    && reference.m2_state().m3.as_ref().unwrap().results
                        [&(proofs[1].problem, proofs[1].commit)]
                        .reason
                        == KERNEL_FAULT,
                "kernel-fault control"
            );
            snapshot_reasons = json!({"before_kernel_fix":format!("{:?}",reference.m2_state().candidates[&(proofs[0].problem,proofs[0].commit)].status()),"kernel_fault_reject":true});
        }
    }
    for node in &nodes[..2] {
        run_cli(node, &rt, &proxy, captured_tip, &xurl, "resume")?;
    }
    for expected in &rows {
        let h = expected["height"].as_u64().unwrap();
        let root =
            boole_derivation::attributes::hex_bytes(expected["m2_root"].as_str().unwrap(), "root")?
                .try_into()
                .unwrap();
        for node in &nodes[..2] {
            ensure!(
                observed(node, h, root, None)? == *expected,
                "I16 restarted prefix at {h}"
            );
        }
    }
    let state = reference.m2_state();
    ensure!(
        state.candidates[&(proofs[0].problem, proofs[0].commit)].status() == Status::Stale,
        "KERNEL-FIX did not recheck the accepted candidate"
    );
    ensure!(
        state.candidates[&(proofs[2].problem, proofs[2].commit)].status() == Status::Void,
        "QUARANTINE VOID was not applied"
    );
    ensure!(
        state.m3.as_ref().unwrap().results[&(proofs[3].problem, proofs[3].commit)].outcome
            == Outcome::Accept,
        "QUARANTINE RECHECK was not applied"
    );
    let mut audits = vec![];
    let mut audit_workers = workers.clone();
    for w in &mut audit_workers {
        w.binary = gate.clone();
    }
    fs::write(
        rt.path.join("audit-gates.json"),
        serde_json::to_vec(&audit_workers)?,
    )?;
    let tip = state.height();
    let source = reference.l1.journal.borrow();
    let l1_hash = source.get(tip)?.block.hash;
    for node in &nodes[..2] {
        let node_root = rt.path.join(format!("{}-state", node.name));
        let pins = ReadOnlyPins::open(&node_root, source.network(), source.anchor_hash())?
            .context("I16 readonly pins")?;
        ensure!(
            pins.intervals().contains(19),
            "interior OPEN/PART height not retained"
        );
        let required = boole_devnet_archive::pins::requirements(state, &c, &table, false)?;
        pins.verify(&*source, &required, tip)?;
        let mut command = Command::new(
            std::env::var_os("BOOLE_DEVNET_AUDIT_BINARY").context("I16 audit binary")?,
        );
        command
            .arg("--journal")
            .arg(&node_root)
            .arg("--node-rpc")
            .arg(&node.http)
            .arg("--m2-config")
            .arg(rt.path.join("config.json"))
            .arg("--m3-forks")
            .arg(rt.path.join("forks.json"))
            .arg("--m3-gates")
            .arg(rt.path.join("audit-gates.json"))
            .arg("--gate-artifacts")
            .arg(rt.path.join("audit-artifacts"))
            .arg("--zstd")
            .arg(zstd_binary()?)
            .arg("--expected-l1-number")
            .arg(tip.to_string())
            .arg("--expected-l1-hash")
            .arg(&l1_hash)
            .arg("--expected-l2-number")
            .arg(tip.to_string())
            .arg("--expected-l2-hash")
            .arg(node.block()?["hash"].as_str().unwrap());
        let output = command.output()?;
        ensure!(
            output.status.success(),
            "I16 audit failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: Value = serde_json::from_slice(&output.stdout)?;
        ensure!(
            receipt["k4"]["status"] == "ok"
                && receipt["k4"]["pin_retention"] == "ok"
                && receipt["k4"]["checked"] == receipt["k4"]["total"]
                && receipt["k4"]["total"].as_str().unwrap().parse::<u64>()? >= 4,
            "I16 nonempty K4 coverage: {receipt}"
        );
        ensure!(
            receipt["environment"]["status"] == "pending",
            "environment was falsely promoted"
        );
        audits.push(receipt);
    }
    drop(source);
    let requests = proxy.requests.lock().unwrap().clone();
    ensure!(
        !requests.iter().any(|r| r.get("proxy_error").is_some()),
        "I16 proxy failure: {requests:?}"
    );
    ensure!(
        requests
            .iter()
            .filter(|r| r["denied"] == true)
            .all(|r| r["id"].as_str().is_some_and(|s| s.starts_with("i16-old-"))),
        "driver attempted old L1 despite pins: {requests:?}"
    );
    drop(reference);
    drop(_rg);
    drop(proxy);
    drop(x);
    rt.finish(true)?;
    let (logical, physical) = size(&rt.path)?;
    let result = json!({"schema":"boole-devnet-m4c2-i16-v1.3","rows":rows,"forks":table,"controls":controls,"withheld_pin":stopped_pin,"states":snapshot_reasons,"audits":audits,"requests":requests,"store_bytes":{"logical":logical,"physical":physical},"cleanup":serde_json::from_slice::<Value>(&fs::read(rt.path.join("cleanup.json"))?)?,"acceptance":{"every_height_compared":rows.len(),"a_b_actual_cli_restarts":true,"x_disconnected":true,"old_original_paths_removed":true,"source":"own verified journal and L2 bindings","environment":"pending"},"provenance":{"head":String::from_utf8(Command::new("git").args(["rev-parse","HEAD"]).output()?.stdout)?.trim(),"producer_sha256":hex_string(&hash(include_bytes!("pin_live.rs"))),"parent_support_sha256":hex_string(&hash(include_bytes!("../m4_live.rs"))),"producer_binary_sha256":hex_string(&hash(&fs::read(std::env::current_exe()?)?)),"driver_binary_sha256":hex_string(&hash(&fs::read(driver_binary())?)),"gate_binary_sha256":hex_string(&pin),"audit_binary_sha256":hex_string(&hash(&fs::read(std::env::var_os("BOOLE_DEVNET_AUDIT_BINARY").unwrap())?))}});
    fs::write(
        rt.path.join("I16.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    if let Some(path) = std::env::var_os("BOOLE_DEVNET_I16_RECORDING") {
        ensure!(!Path::new(&path).exists(), "I16 output exists");
        fs::write(path, serde_json::to_vec_pretty(&result)?)?;
    }
    Ok(())
}
