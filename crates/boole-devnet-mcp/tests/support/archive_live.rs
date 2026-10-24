//! I14/I18: actual fresh verify join, with untrusted archive transport.
use super::*;
use boole_devnet_archive::{
    archive::{ArchiveIdentity, ArchiveProfile, ArchiveStore},
    client::{ArchiveClient, Fetch, HttpFetch},
    http::ArchiveServer,
};
use boole_devnet_driver::driver::archive::{ArchiveSink, CapturePaths};
use boole_devnet_mcp::client::Poster;
struct MutationProxy {
    url: String,
    mode: Arc<Mutex<String>>,
    seen: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl MutationProxy {
    fn start(upstream: String) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}/boole/archive/v1/", listener.local_addr()?);
        let mode = Arc::new(Mutex::new("mutate".to_owned()));
        let seen = Arc::new(Mutex::new(vec![]));
        let stop = Arc::new(AtomicBool::new(false));
        let (m, s, f) = (mode.clone(), seen.clone(), stop.clone());
        let handle = thread::spawn(move || {
            while !f.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(_) => break,
                };
                let result = (|| -> Result<()> {
                    stream.set_nonblocking(false)?;
                    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                    let mut head = vec![];
                    let mut one = [0; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        ensure!(head.len() < 65536, "mutation proxy request cap");
                        stream.read_exact(&mut one)?;
                        head.push(one[0]);
                    }
                    let route = std::str::from_utf8(&head)?
                        .lines()
                        .next()
                        .context("request")?
                        .split_whitespace()
                        .nth(1)
                        .context("path")?
                        .strip_prefix("/boole/archive/v1/")
                        .context("prefix")?;
                    let mut body = match HttpFetch.get(&upstream, route, 135_266_304) {
                        Ok(bytes) => bytes,
                        Err(e) if e.is::<boole_devnet_archive::client::NotServed>() => {
                            s.lock()
                                .unwrap()
                                .push(json!({"route":route,"mutation":"not-served"}));
                            write!(stream,"HTTP/1.1 404 Not Found\r\nContent-Length: 10\r\nConnection: close\r\n\r\nnot-served")?;
                            return Ok(());
                        }
                        Err(e) => return Err(e),
                    };
                    let mode = m.lock().unwrap().clone();
                    let mutation = if let Some(target) = mode.strip_prefix("audit:") {
                        if route == target {
                            let n = body.len();
                            body[n - 1] ^= 1;
                            "audit-input-byte"
                        } else {
                            "none"
                        }
                    } else if mode == "clean" {
                        "none"
                    } else if mode == "oversize" {
                        "content-length-over-cap"
                    } else if mode == "short" {
                        "truncated-body"
                    } else if route == "info" {
                        let mut v: Value = serde_json::from_slice(&body)?;
                        v["identity"]["network"]["config_digest"] = json!(hex_string(&[255; 32]));
                        body = serde_json::to_vec(&v)?;
                        "stale-identity"
                    } else if route.starts_with("index/") {
                        let mut v: Value = serde_json::from_slice(&body)?;
                        if let Some(row) = v.as_array_mut().and_then(|v| v.first_mut()) {
                            row["root"] = json!(hex_string(&[255; 32]));
                        }
                        body = serde_json::to_vec(&v)?;
                        "index-root-tripwire"
                    } else {
                        let n = body.len();
                        body[n - 1] ^= 1;
                        "content-byte"
                    };
                    s.lock()
                        .unwrap()
                        .push(json!({"route":route,"mutation":mutation}));
                    let length = if mode == "oversize" {
                        135_266_305
                    } else if mode == "short" {
                        body.len() + 1
                    } else {
                        body.len()
                    };
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
                    )?;
                    stream.write_all(&body)?;
                    Ok(())
                })();
                // The client deliberately closes oversized replies without reading them.
                if let Err(e) = result {
                    if !matches!(
                        e.downcast_ref::<std::io::Error>().map(std::io::Error::kind),
                        Some(std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset)
                    ) {
                        s.lock().unwrap().push(json!({"proxy_error":e.to_string()}));
                    }
                }
            }
        });
        Ok(Self {
            url,
            mode,
            seen,
            stop,
            thread: Some(handle),
        })
    }
}
impl Drop for MutationProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}
fn fresh_miner(
    c: &Config,
    table: &ForkTable,
    root: &Path,
    proxy: &str,
    node: &Node,
    user: [u8; 20],
    archives: Vec<String>,
) -> Result<Miner> {
    let mut engine = node.engine()?;
    let gas = engine.head()?.gas_limit;
    let network = NetworkIdentity::new(c, Some(table), engine.header(0)?.hash, gas)?;
    let source = JournalSource::open_archive(
        RpcL1::new_m2(proxy)?,
        root,
        network,
        false,
        ArchiveClient::http(archives)?,
    )?;
    Client::new(
        source,
        RpcPoster::new(proxy, user)?,
        c.clone(),
        user,
        root.into(),
        ClientOptions {
            l2: Some(Box::new(RpcL2::new(&node.http)?)),
            l2_gas_limit: gas,
            codec: Some(boole_devnet_mcp::codec::Codec::new(zstd_binary()?)?),
            hook: None,
            forks: Some(table.clone()),
            own: None,
        },
    )
}
fn auditor_binary() -> Result<PathBuf> {
    std::env::var_os("BOOLE_DEVNET_AUDIT_BINARY")
        .map(PathBuf::from)
        .context("M4b2 live requires the normal CI auditor binary")
}
#[allow(clippy::too_many_arguments)]
fn audit_process(
    rt: &Runtime,
    node: &Node,
    own_l1: &str,
    source: Option<&str>,
    expected: (u64, &str),
    mode: &str,
    case: &str,
    op: &Path,
    genesis: &Path,
) -> Result<Value> {
    let mut command = Command::new(auditor_binary()?);
    command
        .arg("--node-rpc")
        .arg(&node.http)
        .arg("--m2-config")
        .arg(rt.path.join("config.json"))
        .arg("--m3-forks")
        .arg(rt.path.join("forks.json"))
        .arg("--m3-gates")
        .arg(rt.path.join("gates.json"))
        .arg("--gate-artifacts")
        .arg(rt.path.join("artifacts"))
        .arg("--zstd")
        .arg(zstd_binary()?)
        .arg("--expected-l1-number")
        .arg(expected.0.to_string())
        .arg("--expected-l1-hash")
        .arg(expected.1)
        .arg("--mode")
        .arg(mode)
        .arg("--op-reth")
        .arg(op)
        .arg("--l2-genesis")
        .arg(genesis)
        .env(
            "BOOLE_DEVNET_AUDIT_LOG_DIR",
            rt.path.join("audit-engine-logs"),
        );
    if let Some(archive) = source {
        command
            .arg("--archive")
            .arg(archive)
            .arg("--l1-rpc")
            .arg(own_l1);
    } else {
        command.arg("--journal").arg(rt.path.join("a-state"));
    }
    let path = rt.path.join(format!("audit-{case}.json"));
    let mut process = Process::start(command, &rt.path, &format!("audit-{case}"), &path)?;
    let status = process.wait_for(Duration::from_secs(180))?;
    let receipt: Value = serde_json::from_slice(&fs::read(&path)?)?;
    ensure!(
        receipt["schema"] == "boole.devnet.audit.v1"
            && receipt["unsigned"] == true
            && receipt["mode"] == mode,
        "auditor process schema/mode"
    );
    Ok(json!({"case":case,"exit_code":status.code(),"receipt":receipt}))
}
#[test]
fn i14_i18_full_round_archive_fresh_driver_and_miner_with_mutating_proxy() -> Result<()> {
    let Some(op) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
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
    let mut rt = Runtime::new("m4b2-i14-i17-i18")?;
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
    let nodes = [
        rt.node(&op, &gp, "a", 0)?,
        rt.node(&op, &gp, "b", 1)?,
        rt.node(&op, &gp, "x", 2)?,
    ];
    let proxy = TipProxy::start("http://127.0.0.1:28545")?;
    let (a, ag) = driver(&nodes[0], &rt, &c, &table, &workers, &proxy.url)?;
    let (b, bg) = driver(&nodes[1], &rt, &c, &table, &workers, &proxy.url)?;
    let (x, xg) = driver(&nodes[2], &rt, &c, &table, &workers, &proxy.url)?;
    let _guards = [ag, bg, xg];
    let mut drivers = [a, b, x];
    for d in &mut drivers {
        d.resume()?;
    }
    let network = NetworkIdentity::new(
        &c,
        Some(&table),
        drivers[2].engine.header(0)?.hash,
        drivers[2].engine.head()?.gas_limit,
    )?;
    let archive = Arc::new(Mutex::new(ArchiveStore::open(
        &rt.path.join("archive"),
        ArchiveProfile {
            identity: ArchiveIdentity {
                network: network.clone(),
                l1_anchor_number: 0,
                l1_anchor_hash: drivers[2].l1.input_row(0)?.block_hash,
            },
            bind: "127.0.0.1:0".parse()?,
            prune_before: None,
        },
        false,
    )?));
    let server = ArchiveServer::start(archive.clone(), "127.0.0.1:0".parse()?)?;
    let mut sink = ArchiveSink::new(
        &mut drivers[2],
        archive.clone(),
        Some(CapturePaths {
            cache: rt.path.join("x-state/gate-cache"),
            artifacts: artifacts.clone(),
            scratch: rt.path.join("x-state/gate-cache"),
        }),
    )?;
    sink.synchronize(&mut drivers[2])?;
    let bad = MutationProxy::start(server.url())?;
    let private = rt.path.join("miner-state");
    let mut client = miner(
        &c, &table, &workers, &artifacts, &private, &proxy.url, &nodes[0], user,
    )?;
    let pkg = package(10000);
    let salt = [9; 32];
    let leaf =
        boole_derivation::m2::pool_leaf(c.chain_id, 2, &[], 0, pkg.len() as u32, &pkg, &salt);
    post(&l1, account, &[(8, pool(&c, leaf, 1, 3, 1))], false)?;
    let export_file = rt.path.join("export.raw");
    fs::write(&export_file, export(0, 777))?;
    let mut id = None::<String>;
    let mut problem_id = None;
    let mut rows = vec![];
    let mut locked = Value::Null;
    let mut vested = Value::Null;
    let mut checked_objects = vec![];
    let mut l1_fields = vec![];
    let mut reverted_post = None::<String>;
    let mut reverted_receipt = Value::Null;
    for latest in 1..=220u64 {
        if latest == 5 {
            post(
                &l1,
                account,
                &[(9, problem(0, 1, 0, salt, &[], &pkg))],
                false,
            )?;
        }
        if latest == 7 {
            let tx=l1.call("eth_sendTransaction",json!([{"from":account,"to":boole_derivation::config::INBOX,"data":"0xdeadbeef","gas":"0xf4240"}]))?;
            reverted_post = Some(tx.as_str().context("reverted post hash")?.to_owned());
        }
        l1.call("evm_mine", json!([1800000000 + 12 * latest]))?;
        if latest == 7 {
            reverted_receipt = l1.call(
                "eth_getTransactionReceipt",
                json!([reverted_post.as_ref().context("reverted post")?]),
            )?;
            ensure!(
                reverted_receipt["blockNumber"] == "0x7" && reverted_receipt["status"] == "0x0",
                "actual freshly mined reverted post"
            );
        }

        let finalized = RpcL1::new_m2("http://127.0.0.1:28545")?.finalized()?;
        for d in &mut drivers {
            ensure!(
                d.l1.finalized()? == finalized,
                "source finalized disagreement"
            );
        }
        while drivers[0].m2_state().height() < finalized {
            let h = drivers[0].m2_state().height() + 1;
            let mut states = vec![];
            for (d, n) in drivers.iter_mut().zip(&nodes) {
                states.push(step(d, n, h)?);
            }
            ensure!(
                states[0] == states[1] && states[0] == states[2],
                "A/B/X mismatch at {h}"
            );
            sink.synchronize(&mut drivers[2])?;
            let local = drivers[2].l1.block(h)?;
            let rpc = RpcL1::new_m2("http://127.0.0.1:28545")?.block(h)?;
            ensure!(
                serde_json::to_value(&local)? == serde_json::to_value(rpc)?,
                "seven header field parity"
            );
            let mut parity = RpcL1::new_m2("http://127.0.0.1:28545")?;
            let own_logs = drivers[2].l1.inbox_logs(&local.hash)?;
            ensure!(
                serde_json::to_value(own_logs)?
                    == serde_json::to_value(parity.inbox_logs(&local.hash)?)?,
                "receipt-derived logs/global logIndex differ from eth_getLogs"
            );
            l1_fields.push(serde_json::to_value(local)?);
            rows.push(states.remove(0));
        }
        client.sync()?;
        if finalized > 0 {
            client.tick()?;
        }
        ensure!(
            client.status()["stalled"].is_null() && client.status()["fold_height"] == finalized,
            "miner fold"
        );
        ensure!(
            client.m2_state().root == drivers[0].m2_state().root,
            "miner root"
        );
        if id.is_none() && finalized >= 5 {
            let p = drivers[0].m2_state().rounds[&(0, 1)]
                .problem
                .context("problem")?;
            problem_id = Some(p);
            let view=client.call("boole.devnet_commit",&json!({"export_file":export_file,"payee":hex_string(&[6;20]),"problem_id":hex_string(&p)}))?;
            id = Some(
                view["submission_id"]
                    .as_str()
                    .context("submission id")?
                    .into(),
            );
        }
        if let Some(id) = &id {
            let key = (problem_id.unwrap(), boole_devnet_mcp::client::fixed(id)?);
            if checked_objects.is_empty()
                && drivers[2]
                    .m2_state()
                    .candidates
                    .get(&key)
                    .and_then(|c| c.live())
                    .and_then(|l| l.open.as_ref())
                    .is_some_and(|o| o.h_r.is_some())
            {
                let mut fetch = ArchiveClient::http(vec![bad.url.clone(), server.url()])?;
                let state = drivers[2].m2_state();
                ensure!(
                    fetch.export(state, key)? == fs::read(&export_file)?,
                    "archive export"
                );
                checked_objects.push("exp");
                fetch.transport(state, key)?;
                checked_objects.push("tr");
                fetch.package(
                    state,
                    &c,
                    key.0,
                    hash(&pkg),
                    &boole_devnet_archive::derived::PackageProof {
                        kind: 2,
                        codec: 0,
                        raw_len: pkg.len() as u32,
                        salt: [9; 32],
                        path: vec![],
                    },
                )?;
                checked_objects.push("pkg");
                fetch.content("env", c.base_root, finalized)?;
                checked_objects.push("env");
                fetch.content("gate", pin, finalized)?;
                checked_objects.push("gate");
                ensure!(
                    fetch.failures[0] >= 5 && fetch.failures[1] == 0,
                    "mutated derived object fallback"
                );
            }
            if let Some(reward) = drivers[0]
                .m2_state()
                .m3
                .as_ref()
                .unwrap()
                .rewards
                .values()
                .find(|r| r.key == key)
            {
                let view = client.call("boole.devnet_submission", &json!({"submission_id":id}))?;
                if reward.status == RewardStatus::Locked {
                    locked = view.clone();
                }
                if reward.status == RewardStatus::Vested {
                    vested = client.call(
                        "boole.devnet_rewards",
                        &json!({"payee":hex_string(&[6;20])}),
                    )?;
                    ensure!(
                        view["status"] == "ACCEPTED" && view["rewards"][0]["status"] == "VESTED",
                        "round through vest"
                    );
                    if finalized >= reward.vest_h + 2 {
                        break;
                    }
                }
            }
        }
        if latest % 10 == 0 {
            eprintln!("I14/I18 latest {latest} finalized {finalized}");
        }
    }
    ensure!(
        !locked.is_null() && !vested.is_null() && rows.len() >= 64 && checked_objects.len() == 5,
        "full round/archive classes incomplete"
    );
    let reverted_post = reverted_post.context("actual reverted inbox post")?;
    ensure!(
        reverted_receipt["status"] == "0x0"
            && reverted_receipt["logs"]
                .as_array()
                .is_some_and(Vec::is_empty),
        "reverted post emitted a log or succeeded"
    );
    let reverted_recorded = drivers[0]
        .l1
        .journal
        .borrow()
        .transaction_receipt(&reverted_post)?
        .context("own reverted receipt")?;
    ensure!(
        reverted_recorded == (7, false),
        "reverted post was not alone at height7"
    );
    let reverted_input = drivers[0].l1.input_row(reverted_recorded.0)?;
    let reverted_header =
        boole_derivation::attributes::hex_bytes(&reverted_input.header_rlp, "reverted header")?;
    ensure!(
        !boole_devnet_archive::input::header_needs_receipts(&reverted_header)?,
        "reverted post must be bloom-negative"
    );
    let reverted_index = archive.lock().unwrap().index(reverted_recorded.0, 1)?;
    ensure!(
        reverted_index[0].l1_number == reverted_recorded.0
            && reverted_index[0].inbox_touching
            && !reverted_index[0].bloom_positive,
        "negative inbox block not indexed as touching"
    );
    let through = drivers[0].m2_state().height();
    let anchor = drivers[0].l1.input_row(through)?.block_hash;
    // Optional driver diagnostics may legitimately skip a probe under load.
    // Require an actual bounded HTTP index mutation witness independently of
    // that scheduling, before accepting this qualification recording.
    let explicit_tripwire_alerts = observe_index_conflict(
        ArchiveClient::http(vec![bad.url.clone()])?,
        through,
        &anchor,
        rows.last().unwrap()["hash"]
            .as_str()
            .context("own L2 hash")?,
        rows.last().unwrap()["m2_root"]
            .as_str()
            .context("own root")?,
    )?;
    let mut fetch = ArchiveClient::http(vec![bad.url.clone(), server.url()])?;
    let index_rows = archive.lock().unwrap().index(0, 1024)?;
    for row in index_rows {
        if row.l1_number == 0 {
            continue;
        }
        let system: Vec<String> = serde_json::from_slice(
            &archive
                .lock()
                .unwrap()
                .system(row.l1_number)?
                .context("system")?,
        )?;
        // No archive bytes are trusted here: compare to A's independently computed prefix.
        let actual = &rows[row.l1_number as usize - 1]["transactions"];
        let expected: Vec<String> = serde_json::from_value(actual.clone())?;
        ensure!(
            system
                == expected
                    .into_iter()
                    .take_while(|t| t.starts_with("0x7e"))
                    .collect::<Vec<_>>(),
            "X system differs from A"
        );
        fetch.system(row.l1_number, &system)?;
    }
    let verdict = drivers[0]
        .m2_state()
        .m3
        .as_ref()
        .unwrap()
        .results
        .values()
        .next()
        .context("applied verdict")?;
    fetch.content("vstmt", verdict.vstmt_digest, through)?;
    checked_objects.push("vstmt");
    let submission = client.call(
        "boole.devnet_submission",
        &json!({"submission_id":id.as_ref().context("submission")?}),
    )?;
    let known_tx = submission["transactions"]
        .as_array()
        .context("posting transactions")?
        .iter()
        .find(|r| r["stage"] == "commit")
        .context("recorded COMMIT")?["transaction_hash"]
        .as_str()
        .context("known post hash")?
        .to_owned();
    let recorded = drivers[0]
        .l1
        .journal
        .borrow()
        .transaction_receipt(&known_tx)?
        .context("own recorded receipt")?;
    ensure!(recorded.1, "live COMMIT should have succeeded");
    let mut original = drivers[0].l1.input_row(recorded.0)?;
    let expected_receipts = original.verify()?.canonical_receipts;
    original.transactions = None;
    let proof = fetch.transaction_row(&original)?;
    ensure!(
        proof.verify()?.canonical_receipts == expected_receipts,
        "l1/txs canonical statuses"
    );
    checked_objects.push("l1/txs");
    let legacy = rt.path.join("legacy-proof-miner");
    let mut journal = boole_devnet_archive::journal::InputJournal::open(
        &legacy,
        network.clone(),
        || {
            let mut r = drivers[0].l1.input_row(0)?;
            r.transactions = None;
            Ok(r)
        },
        false,
    )?;
    for h in 1..=through {
        let mut r = drivers[0].l1.input_row(h)?;
        r.transactions = None;
        if !boole_devnet_archive::input::header_needs_receipts(
            &boole_derivation::attributes::hex_bytes(&r.header_rlp, "legacy header")?,
        )? {
            r.receipts = None;
        }
        journal.append(&r)?;
    }
    ensure!(
        journal.transaction_receipt_since(&known_tx, 0).is_err(),
        "legacy proof fixture did not pause"
    );
    drop(journal);
    let legacy_bytes = fs::read(legacy.join("input.ndjson"))?;
    let mut legacy_source = JournalSource::open_archive(
        RpcL1::new_m2(&proxy.url)?,
        &legacy,
        network.clone(),
        false,
        ArchiveClient::http(vec![bad.url.clone()])?,
    )?;
    let reverted_pause = legacy_source
        .restore_posting_proof(&reverted_post, 0)
        .expect_err("mutated proof did not pause reverted-post reconciliation");
    ensure!(
        matches!(
            reverted_pause.downcast_ref::<boole_devnet_archive::error::Error>(),
            Some(boole_devnet_archive::error::Error::Availability {
                class: "archive-tx-proof",
                ..
            })
        ),
        "reverted proof pause lost availability class"
    );
    let mut proof_failures = vec![];
    for mode in ["mutate", "oversize", "short"] {
        *bad.mode.lock().unwrap() = mode.into();
        let error = legacy_source
            .restore_posting_proof(&known_tx, 0)
            .expect_err("bad transaction proof adopted");
        ensure!(
            matches!(
                error.downcast_ref::<boole_devnet_archive::error::Error>(),
                Some(boole_devnet_archive::error::Error::Availability {
                    class: "archive-tx-proof",
                    ..
                })
            ),
            "bad proof lost resumable class"
        );
        proof_failures.push(json!({"mode":mode,"availability":true}));
    }
    *bad.mode.lock().unwrap() = "clean".into();
    legacy_source.restore_posting_proof(&reverted_post, 0)?;
    legacy_source.restore_posting_proof(&known_tx, 0)?;
    let mut poster = RpcPoster::new(&proxy.url, user)?;
    poster.bind_journal(legacy_source.journal.clone());
    ensure!(
        poster.included_for_posting_since(&known_tx, 0)? == Some(recorded.0),
        "miner did not reconcile the exact archived receipt"
    );
    let reverted = poster
        .included_for_posting_since(&reverted_post, 0)
        .expect_err("archived status0 did not produce exact REVERTED");
    ensure!(
        reverted.is::<boole_devnet_mcp::client::RevertedReceipt>(),
        "wrong reverted posting class"
    );
    let dropped = hex_string(&[77; 32]);
    legacy_source.restore_posting_proof(&dropped, 0)?;
    ensure!(
        poster.included_for_posting_since(&dropped, 0)?.is_none(),
        "dropped post became REVERTED or success"
    );
    drop(poster);
    drop(legacy_source);
    *bad.mode.lock().unwrap() = "short".into();
    let requests_before = bad.seen.lock().unwrap().len();
    let mut legacy_source = JournalSource::open_archive(
        RpcL1::new_m2(&proxy.url)?,
        &legacy,
        network.clone(),
        false,
        ArchiveClient::http(vec![bad.url.clone()])?,
    )?;
    legacy_source.restore_posting_proof(&known_tx, 0)?;
    ensure!(
        legacy_source
            .journal
            .borrow()
            .transaction_receipt_since(&known_tx, 0)?
            == Some(recorded),
        "own transaction cache restart"
    );
    ensure!(
        bad.seen.lock().unwrap().len() == requests_before
            && fs::read(legacy.join("input.ndjson"))? == legacy_bytes,
        "proof cache restart fetched history or rewrote original journal"
    );
    ensure!(
        legacy_source
            .journal
            .borrow()
            .transaction_receipt_since(&reverted_post, 0)?
            == Some(reverted_recorded),
        "restart lost negative-inbox proof cache"
    );
    let reverted_post_reconciliation = json!({"transaction_hash":reverted_post,"height":reverted_recorded.0,"block_hash":reverted_input.block_hash,"header_rlp":reverted_input.header_rlp,"canonical_receipt_status":false,"bloom_positive":false,"inbox_touching":true,"initial_pause":true,"status":"REVERTED","dropped_not_reverted":true,"restart_cache":true,"original_journal_unchanged":fs::read(legacy.join("input.ndjson"))?==legacy_bytes});
    let tx_reconciliation = json!({"known_hash":known_tx,"height":recorded.0,"receipt_status":recorded.1,"poster_included":recorded.0,"bad_only":proof_failures,"restart_cache":true,"original_journal_unchanged":true});
    *bad.mode.lock().unwrap() = "mutate".into();

    let node_c = rt.node(&op, &gp, "c", 3)?;
    let mut cmd = c_command(&node_c, &rt, &proxy.url, through, false, false);
    cmd.arg("--archive")
        .arg(&bad.url)
        .arg("--archive")
        .arg(server.url());
    let log = rt.path.join("fresh-c.log");
    let mut process = Process::start(cmd, &rt.path, "fresh-c", &log)?;
    ensure!(
        process.wait()?.success(),
        "fresh C: {}",
        fs::read_to_string(log)?
    );
    for row in &rows {
        let h = row["height"].as_u64().unwrap();
        ensure!(
            observed(
                &node_c,
                h,
                boole_devnet_mcp::client::fixed(row["m2_root"].as_str().unwrap())?,
                None
            )? == *row,
            "A/C at {h}"
        );
    }
    let evidence = fs::read_to_string(rt.path.join("c-state/evidence.jsonl"))?;
    let evidence: Vec<Value> = evidence
        .lines()
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?;
    ensure!(
        evidence.len() == rows.len()
            && evidence
                .iter()
                .all(|r| r["inputs"] == "archive-verified" && r["verification"] == "computed"),
        "J5 driver labels"
    );
    let cp = authority(&rt.path.join("c-state"))?;
    let mut cmd = c_command(&node_c, &rt, &proxy.url, through, false, false);
    cmd.arg("--archive").arg(server.url());
    let mut process = Process::start(cmd, &rt.path, "restart-c", &rt.path.join("restart-c.log"))?;
    ensure!(
        process.wait()?.success() && authority(&rt.path.join("c-state"))? == cp,
        "own journal restart"
    );
    let fresh_root = rt.path.join("fresh-miner-state");
    let mut fresh = fresh_miner(
        &c,
        &table,
        &fresh_root,
        &proxy.url,
        &node_c,
        user,
        vec![bad.url.clone(), server.url()],
    )?;
    fresh.sync()?;
    let fresh_status = fresh.status();
    ensure!(
        fresh_status["fold_height"] == through
            && fresh_status["inputs"] == "archive-verified"
            && fresh_status["l2_m2_root_match"] == true
            && fresh.m2_state().root == drivers[0].m2_state().root,
        "fresh miner join: {fresh_status}"
    );
    let verified_headers = (1..=through)
        .map(|h| {
            fresh
                .source
                .block(h)
                .and_then(|b| Ok(serde_json::to_value(b)?))
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        verified_headers == l1_fields,
        "archive seven-field parity with own endpoint"
    );
    drop(fresh);
    let mut fresh = fresh_miner(
        &c,
        &table,
        &fresh_root,
        &proxy.url,
        &node_c,
        user,
        vec![server.url()],
    )?;
    fresh.sync()?;
    ensure!(
        fresh.status()["inputs"] == "journal"
            && fresh.m2_state().root == drivers[0].m2_state().root,
        "miner journal restart"
    );
    let blocked = rt.path.join("blocked-join");
    let mut failures = vec![];
    for mode in ["mutate", "oversize", "short"] {
        *bad.mode.lock().unwrap() = mode.into();
        let result = JournalSource::open_archive(
            RpcL1::new_m2(&proxy.url)?,
            &blocked,
            network.clone(),
            false,
            ArchiveClient::http(vec![bad.url.clone()])?,
        );
        let error = result.err().context("bad-only archive adopted")?;
        ensure!(
            matches!(
                error.downcast_ref::<boole_devnet_archive::error::Error>(),
                Some(boole_devnet_archive::error::Error::Availability { .. })
            ),
            "archive input became REJECT/integrity: {error:#}"
        );
        failures.push(json!({"mode":mode,"availability":true}));
    }
    *bad.mode.lock().unwrap() = "clean".into();
    let source = JournalSource::open_archive(
        RpcL1::new_m2(&proxy.url)?,
        &blocked,
        network,
        false,
        ArchiveClient::http(vec![bad.url.clone()])?,
    )?;
    ensure!(
        source.journal.borrow().tip() == through,
        "bad-only join not resumable"
    );
    let source_before = authority(&rt.path.join("a-state"))?;
    let mut audits = vec![];
    let through_decimal = through.to_string();
    for mode in ["pure", "replay"] {
        for (name, source) in [("journal", None), ("archive", Some(server.url()))] {
            let result = audit_process(
                &rt,
                &nodes[0],
                &proxy.url,
                source.as_deref(),
                (through, &anchor),
                mode,
                &format!("{name}-{mode}"),
                &op,
                &gp,
            )?;
            let receipt = &result["receipt"];
            ensure!(
                result["exit_code"] == 0 && receipt["first_mismatch"].is_null(),
                "I17 {name}/{mode}: {receipt}"
            );
            ensure!(
                receipt["l1_head"]["number"].as_str() == Some(through_decimal.as_str())
                    && receipt["l1_head"]["hash"] == anchor
                    && receipt["l2_head"]["hash"] == rows.last().unwrap()["hash"]
                    && receipt["root"] == rows.last().unwrap()["m2_root"],
                "I17 independent heads/root"
            );
            ensure!(
                receipt["rewards"]["minted"] == vested["rewards"][0]["amount"]
                    && receipt["admissions"]["active"] == "1"
                    && receipt["labels"]["verdicts"] == "computed"
                    && receipt["labels"]["environment"] == "pending"
                    && receipt["k4"]["checked"] == "1"
                    && receipt["k4"]["status"] == "pending",
                "I17 ledger/admission/K4 labels"
            );
            if mode == "replay" {
                ensure!(
                    receipt["checks"]["execution"] == "ok"
                        && receipt["replayed_blocks"].as_str() == Some(through_decimal.as_str())
                        && receipt["private_engine_cleanup"]["stopped"] == true
                        && receipt["private_engine_cleanup"]["log_issues"] == 0,
                    "I17 private replay cleanup"
                );
            }
            audits.push(result);
        }
    }
    let changed_hash = drivers[0].l1.input_row(1)?.block_hash;
    *bad.mode.lock().unwrap() = format!("audit:l1/header/{}", &changed_hash[2..]);
    for mode in ["pure", "replay"] {
        let result = audit_process(
            &rt,
            &nodes[0],
            &proxy.url,
            Some(&bad.url),
            (through, &anchor),
            mode,
            &format!("mutated-{mode}"),
            &op,
            &gp,
        )?;
        ensure!(
            result["exit_code"] != 0
                && result["receipt"]["first_mismatch"] == json!({"height":"1","class":"input"}),
            "I17 mutation: {result}"
        );
        audits.push(result);
    }
    *bad.mode.lock().unwrap() = "clean".into();
    let mut newer = through;
    for _ in 0..12 {
        let latest = quantity(&l1.call("eth_blockNumber", json!([]))?)?;
        l1.call("evm_mine", json!([1_800_000_000 + 12 * (latest + 1)]))?;
        newer = RpcL1::new_m2("http://127.0.0.1:28545")?.finalized()?;
        if newer > through {
            break;
        }
    }
    ensure!(newer > through, "stale audit head fixture");
    let newer_hash = RpcL1::new_m2("http://127.0.0.1:28545")?.block(newer)?.hash;
    for mode in ["pure", "replay"] {
        let result = audit_process(
            &rt,
            &nodes[0],
            &proxy.url,
            None,
            (newer, &newer_hash),
            mode,
            &format!("stale-{mode}"),
            &op,
            &gp,
        )?;
        ensure!(
            result["exit_code"] != 0
                && result["receipt"]["first_mismatch"]
                    == json!({"height":newer.to_string(),"class":"input"}),
            "I17 stale prefix: {result}"
        );
        audits.push(result);
    }
    ensure!(
        authority(&rt.path.join("a-state"))? == source_before,
        "auditor wrote an audited source"
    );
    ensure!(
        proxy.denied.lock().unwrap().is_empty(),
        "historical own endpoint reads/proxy failure"
    );
    ensure!(
        bad.seen
            .lock()
            .unwrap()
            .iter()
            .all(|v| v.get("proxy_error").is_none()),
        "mutation proxy fault"
    );
    let mut result = json!({"schema":"boole-devnet-m4b2-i14-i17-i18-v1.3","audits":audits,"explicit_tripwire_alerts":explicit_tripwire_alerts,"tx_reconciliation":tx_reconciliation,"reverted_post_reconciliation":reverted_post_reconciliation,"rows":rows,"anchor":{"height":through,"hash":anchor,"source":"own-l1-endpoint"},"locked":locked,"vested":vested,"checked_objects":checked_objects,"fresh_miner_status":fresh_status,"bad_only_resumable":failures,"mutation_responses":bad.seen.lock().unwrap().clone(),"acceptance":{"every_height_compared":through,"seven_header_fields_equal":true,"receipt_logs_equal_rpc":true,"driver_execution":"actual CLI binary","fresh_miner_execution":"production client library","fresh_miner_restart":true,"driver_restart":true,"full_round_through_vest":true,"inputs":"archive-verified","restart_inputs":"journal","verdicts":"computed","tip_only_denied_requests":proxy.denied.lock().unwrap().clone(),"unauthenticated_engine_401_probes":4},"provenance":{"producer_commit":String::from_utf8(Command::new("git").args(["rev-parse","HEAD"]).output()?.stdout)?.trim(),"producer_sha256":hex_string(&hash(include_bytes!("archive_live.rs"))),"support_sha256":hex_string(&hash(include_bytes!("../m4_live.rs"))),"producer_binary_sha256":hex_string(&hash(&fs::read(std::env::current_exe()?)?)),"driver_binary_sha256":hex_string(&hash(&fs::read(driver_binary())?)),"client_binary_sha256":hex_string(&hash(&fs::read(client_binary())?)),"gate_binary_sha256":hex_string(&pin),"audit_binary_sha256":hex_string(&hash(&fs::read(auditor_binary()?)?)),"cargo_lock_sha256":hex_string(&hash(include_bytes!("../../../../Cargo.lock"))),"engine_profile":engine_profile(),"l1_genesis_sha256":hex_string(&hash(&fs::read(l1p)?)),"l2_genesis_sha256":hex_string(&hash(&fs::read(gp)?))}});
    rt.finish(true)?;
    result["cleanup"] = serde_json::from_slice(&fs::read(rt.path.join("cleanup.json"))?)?;
    let recording =
        PathBuf::from(std::env::var_os("BOOLE_DEVNET_M4_RECORDING").context("fresh recording")?);
    ensure!(!recording.exists(), "recording exists");
    fs::write(recording, serde_json::to_vec_pretty(&result)?)?;
    Ok(())
}

fn observe_index_conflict(
    mut diagnostic: ArchiveClient,
    height: u64,
    anchor: &str,
    l2_hash: &str,
    root: &str,
) -> Result<Vec<String>> {
    // Keep cached coverage between calls: info can spend this call's shared
    // 50 ms budget, leaving the index for the next call. Permit one bounded
    // 30-second source cooldown without enlarging any production I/O budget.
    let deadline = Instant::now() + Duration::from_secs(35);
    while Instant::now() < deadline {
        let alerts = diagnostic.tripwire(height, anchor, l2_hash, root);
        if alerts.len() == 1 {
            return Ok(alerts);
        }
        thread::sleep(Duration::from_millis(20));
    }
    bail!("explicit index inconsistency diagnostic was not observed")
}

#[test]
fn index_probe_keeps_coverage_when_info_uses_the_first_budget() -> Result<()> {
    struct SplitBudget;
    impl Fetch for SplitBudget {
        fn get(&mut self, _: &str, _: &str, _: usize) -> Result<Vec<u8>> {
            bail!("diagnostic used required fetch")
        }
        fn observe(
            &mut self,
            _: &str,
            route: &str,
            _: usize,
            deadline: Instant,
        ) -> Result<Vec<u8>> {
            if route == "info" {
                thread::sleep(Duration::from_millis(25));
                return Ok(br#"{"covered_through":1}"#.to_vec());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining < Duration::from_millis(40) {
                thread::sleep(remaining);
                bail!("index needs a fresh observation budget");
            }
            Ok(serde_json::to_vec(&json!([{
                "l1_number":1,"l2_number":1,"l1_hash":hex_string(&[1;32]),
                "parent_hash":hex_string(&[0;32]),"bloom_positive":false,"inbox_touching":false,
                "blob_ids":[],"l2_hash":hex_string(&[2;32]),"root":hex_string(&[255;32]),
                "inputs":"journal","verdicts":"computed","system_digest":hex_string(&[4;32])
            }]))?)
        }
    }
    let alerts = observe_index_conflict(
        ArchiveClient::new(
            vec!["http://127.0.0.1:1/boole/archive/v1/".into()],
            Box::new(SplitBudget),
        )?,
        1,
        &hex_string(&[1; 32]),
        &hex_string(&[2; 32]),
        &hex_string(&[3; 32]),
    )?;
    ensure!(
        alerts.len() == 1 && alerts[0].contains("verified inconsistency; local fold wins"),
        "missing conflict alert"
    );
    Ok(())
}
