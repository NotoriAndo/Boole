//! M3 v1.3 execution qualification. Live tests require explicit pinned local clients.
#[allow(dead_code)]
mod support;
use anyhow::{bail, ensure, Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    m2::*,
    m3::*,
    Derived, L1Block,
};
use boole_devnet_driver::{
    engine::{Engine, Rpc, RpcEngine},
    l1::{L1Source, RpcL1},
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
use support::{m2::*, *};
const REVERT: &str = "0x42000000000000000000000000000000000000b4";
fn m3_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/devnet-m3")
        .join(name)
}
fn reset_genesis() -> Result<Value> {
    let mut g: Value = serde_json::from_slice(&fs::read(m3_fixture("l2-genesis-v1.3.json"))?)?;
    g["alloc"][hex_string(&address(&user_key()))] = json!({"balance":"0x56bc75e2d63100000"});
    ensure!(
        g["alloc"].get(REVERT).is_none(),
        "measurement payee already allocated"
    );
    g["alloc"][REVERT] = json!({"balance":"0x0","code":"0x60006000fd"});
    Ok(g)
}
fn write_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
fn record(variable: &str, rt: &Runtime, mut value: Value) -> Result<()> {
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()?;
    ensure!(commit.status.success(), "producer commit unavailable");
    value["provenance"]["producer_binary_sha256"] =
        json!(hex_string(&hash(&fs::read(std::env::current_exe()?)?)));
    value["provenance"]["producer_commit"] = json!(String::from_utf8(commit.stdout)?.trim());
    value["provenance"]["cargo_lock_sha256"] =
        json!(hex_string(&hash(include_bytes!("../../../Cargo.lock"))));
    value["provenance"]["producer_profile"] = json!(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    value["provenance"]["pins"] = fixture_json("pins.json")?;
    value["provenance"]["engine_profile"] = engine_profile();
    value["provenance"]["generator_sha256"] =
        json!(hex_string(&hash(include_bytes!("m3_devnet.rs"))));
    value["provenance"]["support_sha256"] =
        json!(hex_string(&hash(include_bytes!("support/mod.rs"))));
    value["provenance"]["wire_support_sha256"] =
        json!(hex_string(&hash(include_bytes!("support/m2.rs"))));
    value["provenance"]["reset_genesis_sha256"] = json!(hex_string(&hash(&fs::read(m3_fixture(
        "l2-genesis-v1.3.json"
    ))?)));
    value["cleanup"] = serde_json::from_slice::<Value>(&fs::read(rt.path.join("cleanup.json"))?)?;
    write_json(&rt.path.join("qualification.json"), &value)?;
    if let Some(path) = std::env::var_os(variable) {
        let path = PathBuf::from(path);
        ensure!(!path.exists(), "recording output already exists");
        write_json(&path, &value)?;
    }
    Ok(())
}
fn mine_source(l1: &Rpc, h: u64) -> Result<L1Block> {
    l1.call("evm_mine", json!([1_800_000_000 + 12 * h]))?;
    Ok(serde_json::from_value(l1.call(
        "eth_getBlockByNumber",
        json!([format!("0x{h:x}"), false]),
    )?)?)
}
fn execute(engine: &mut RpcEngine, attrs: Value) -> Result<Value> {
    let parent = engine.head()?;
    let fc=engine.call("engine_forkchoiceUpdatedV3",json!([{"headBlockHash":parent.hash,"safeBlockHash":parent.hash,"finalizedBlockHash":parent.hash},attrs]))?;
    ensure!(
        fc["payloadStatus"]["status"] == "VALID",
        "build status: {fc}"
    );
    let payload =
        engine.call("engine_getPayloadV5", json!([fc["payloadId"]]))?["executionPayload"].clone();
    let new = engine.call(
        "engine_newPayloadV4",
        json!([payload, [], attrs["parentBeaconBlockRoot"], []]),
    )?;
    ensure!(new["status"] == "VALID", "import: {new}");
    let selected=engine.call("engine_forkchoiceUpdatedV3",json!([{"headBlockHash":payload["blockHash"],"safeBlockHash":payload["blockHash"],"finalizedBlockHash":payload["blockHash"]},null]))?;
    ensure!(
        selected["payloadStatus"]["status"] == "VALID",
        "select: {selected}"
    );
    ensure!(engine.head()?.hash == payload["blockHash"], "selected head");
    Ok(payload)
}
fn observe_all(node: &Node, d: &Derived, root: Hash) -> Result<Value> {
    let block = node.block()?;
    let raw = node.engine()?.transactions(quantity(&block["number"])?)?;
    boole_devnet_driver::driver::check_payload_transactions(&json!({"transactions":raw}), d)?;
    let rpc = node.rpc()?;
    let mut receipts = vec![];
    for tx in block["transactions"].as_array().context("transactions")? {
        let r = rpc.call("eth_getTransactionReceipt", json!([tx]))?;
        ensure!(r["blockHash"] == block["hash"], "canonical receipt");
        receipts.push(r);
    }
    Ok(
        json!({"block_hash":block["hash"],"state_root":block["stateRoot"],"receipts_root":block["receiptsRoot"],"transactions":raw,"receipts":receipts,"m2_root":hex_string(&root),"attributes":d.attributes,"decisions":d.decisions}),
    )
}
#[test]
fn m3_m3_full_256_verdict_gas_reward_mint_failure_and_nonce() -> Result<()> {
    let Some(binary) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let mut rt = Runtime::new("m3-m3")?;
    let l1g = rt.path.join("l1-genesis.json");
    write_json(&l1g, &l1_genesis_value()?)?;
    let l1 = rt.anvil_with_genesis(&anvil, &l1g)?;
    let genesis = reset_genesis()?;
    let gp = rt.path.join("l2-genesis.json");
    write_json(&gp, &genesis)?;
    let mut c = config();
    c.m3 = Some(M3Config::devnet(c.slot_table.len()));
    c.v_per_deposit = 256;
    c.validate(30_000_000)
        .map_err(|s| anyhow::anyhow!("S{s}"))?;
    let forks = ForkTable::genesis(&c)?;
    let mut state = State::new(&c, 30_000_000)?;
    let nodes = [
        rt.node(&binary, &gp, "a", 0)?,
        rt.node(&binary, &gp, "b", 1)?,
        rt.node(&binary, &gp, "c", 2)?,
    ];
    let mut engines = nodes.iter().map(Node::engine).collect::<Result<Vec<_>>>()?;
    let solver = [6; 20];
    let payee = hex_string(&solver);
    let from = boole_derivation::config::DEPOSITOR;
    let amount = 1_000_000_000_000_000_000u128;
    let mut rows = vec![];
    for h in 1..=3 {
        let origin = mine_source(&l1, h)?;
        let parent = engines[0].head()?;
        let (mut d, next) = boole_derivation::attributes::derive_with_m3(
            &state,
            &c,
            &origin,
            &[],
            &parent,
            &[],
            &forks,
        )?;
        let before = nodes[0].state(from, &parent.hash)?;
        let extra = if h == 1 {
            let records = (0..256)
                .map(|i| IndexedVerdict {
                    queue_index: i,
                    record: VerdictRecord {
                        kind: RecordKind::Verdict,
                        commit_hash: hash(&i.to_be_bytes()),
                        outcome: Outcome::Accept,
                        reason: 0,
                        gate_epoch: 0,
                        fuel: 777,
                        vstmt_digest: hash(&[i as u8; 32]),
                    },
                })
                .collect::<Vec<_>>();
            let txs = verdict_deposits(&c, &origin, &records)?;
            ensure!(txs.len() == 1, "full 256-record deposit");
            txs
        } else {
            let solver = if h == 2 {
                solver
            } else {
                hex_bytes(REVERT, "revert")?
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("payee"))?
            };
            vec![reward_deposit(
                &c,
                &origin,
                &RewardPayment {
                    reward_id: h - 2,
                    solver,
                    amount,
                },
            )?]
        };
        d.attributes.transactions.extend(extra.clone());
        let mut observations = vec![];
        for (node, engine) in nodes.iter().zip(&mut engines) {
            execute(engine, serde_json::to_value(&d.attributes)?)?;
            observations.push(observe_all(node, &d, next.root)?);
        }
        ensure!(
            observations[0] == observations[1] && observations[0] == observations[2],
            "M3-M3 ABC mismatch at {h}"
        );
        let ob = &observations[0];
        let receipt = &ob["receipts"][2];
        let used = quantity(&receipt["gasUsed"])?;
        let hash = ob["block_hash"].as_str().context("hash")?;
        let after = nodes[0].state(from, hash)?;
        write_json(
            &rt.path.join(format!("probe-{h}.json")),
            &json!({"height":h,"abc":observations,"sender_before":before,"sender_after":after,"payee":nodes[0].state(&payee,hash)?,"revert_payee":nodes[0].state(REVERT,hash)?}),
        )?;
        ensure!(
            quantity(&after["nonce"])? == quantity(&before["nonce"])? + 3,
            "RW3: deposit sender nonce mismatch"
        );
        let mut measurement = json!({"height":h,"observed":ob,"sender_before":before,"sender_after":after,"gas_used":used});
        if h == 1 {
            let raw = hex_bytes(&extra[0], "deposit")?;
            let fields = boole_derivation::rlp::decode(&raw[1..])
                .map_err(anyhow::Error::msg)?
                .children
                .context("fields")?;
            let data = fields[7].bytes;
            let zeros = data.iter().filter(|b| **b == 0).count() as u64;
            let nonzero = data.len() as u64 - zeros;
            let intrinsic = 21000 + 4 * zeros + 16 * nonzero;
            let floor = 21000 + 10 * zeros + 40 * nonzero;
            ensure!(
                data.len() == 24 + 80 * 256 && used <= c.verdict_gas,
                "T49: full verdict exceeds gas"
            );
            ensure!(
                used == intrinsic || used == floor,
                "unclassified deposit gas {used}"
            );
            measurement["data_bytes"] = json!(data.len());
            measurement["intrinsic_gas"] = json!(intrinsic);
            measurement["eip7623_floor"] = json!(floor);
            measurement["floor_applies"] = json!(used == floor);
        } else if h == 2 {
            ensure!(receipt["status"] == "0x1", "RW3 successful EOA payment");
            ensure!(
                amount_value(&nodes[0].state(&payee, hash)?["balance"])? == amount,
                "RW3 mint/value not credited to solver"
            );
            ensure!(
                amount_value(&after["balance"])? == amount_value(&before["balance"])?,
                "RW3 mint residue on success"
            );
            ensure!(
                quantity(&nodes[0].state(&payee, hash)?["nonce"])? == 0,
                "solver nonce changed"
            );
        } else {
            ensure!(receipt["status"] == "0x0", "RW3 failed call status");
            ensure!(
                amount_value(&after["balance"])? == amount_value(&before["balance"])? + amount,
                "RW3 failed-call mint not retained at from"
            );
            ensure!(
                amount_value(&nodes[0].state(REVERT, hash)?["balance"])? == 0,
                "RW3 revert payee balance"
            );
        }
        rows.push(measurement);
        state = next;
    }
    rt.finish(true)?;
    record(
        "BOOLE_DEVNET_M3_MEASUREMENT",
        &rt,
        json!({"schema":"boole-devnet-m3-measurement-v1.3","scope":"execution probes; extra deposits intentionally injected outside the consensus fold","config_json":String::from_utf8(serde_json::to_vec(&c)?)?,"l1_genesis":l1_genesis_value()?,"l2_genesis":genesis,"acceptance":{"abc":true,"full_records":256,"rw3_matches":true},"heights":rows}),
    )?;
    Ok(())
}
fn amount_value(value: &Value) -> Result<u128> {
    support::amount(value)
}

use boole_devnet_driver::driver::{Driver, StallKind, StepOutcome};
use boole_gate_supervisor::{
    codec::{digest, Codec},
    process::{Envelope, Worker},
    stub::{export, package, Descriptor},
    supervisor::{install_devnet_artifacts, Host, Mode, Supervisor},
};
struct Capture {
    inner: RpcEngine,
    payload: Value,
    imports: u64,
}
impl Engine for Capture {
    fn head(&mut self) -> Result<boole_derivation::ParentHeader> {
        self.inner.head()
    }
    fn header(&mut self, h: u64) -> Result<boole_derivation::ParentHeader> {
        self.inner.header(h)
    }
    fn transactions(&mut self, h: u64) -> Result<Vec<String>> {
        self.inner.transactions(h)
    }
    fn call(&mut self, m: &str, p: Value) -> Result<Value> {
        if m == "engine_newPayloadV4" {
            self.imports += 1;
        }
        let out = self.inner.call(m, p)?;
        if m == "engine_getPayloadV5" {
            self.payload = out["executionPayload"].clone();
        }
        Ok(out)
    }
}
type Live = Driver<RpcL1, Capture>;
fn gates() -> Result<Vec<Worker>> {
    let binary =
        PathBuf::from(std::env::var_os("BOOLE_DEVNET_GATE").context("BOOLE_DEVNET_GATE missing")?);
    let hash = digest(&fs::read(&binary)?);
    let mut out = vec![];
    for fixed in [false, true] {
        let mut descriptor = Descriptor::pinned(hash);
        descriptor.fixed = fixed;
        out.push(Worker {
            descriptor,
            binary: binary.clone(),
            binary_sha256: hash,
        });
    }
    Ok(out)
}
fn live_driver(
    node: &Node,
    rt: &Runtime,
    c: &Config,
    table: &ForkTable,
    workers: &[Worker],
) -> Result<Live> {
    let artifacts = rt.path.join("artifacts");
    if !artifacts.exists() {
        install_devnet_artifacts(c, &artifacts)?;
    }
    let cache = rt.path.join(format!("{}-cache", node.name));
    let supervisor = Supervisor::new(
        c,
        table,
        Mode::Verify,
        workers.to_vec(),
        Some(Codec::portable_stub()),
        &cache,
        &cache,
        &artifacts,
        Host::local()?,
        Envelope::default(),
    )?;
    let mut inner = node.engine()?;
    let gas = inner.head()?.gas_limit;
    let mut out = Driver::with_m3(
        RpcL1::new_m2("http://127.0.0.1:28545")?,
        Capture {
            inner,
            payload: Value::Null,
            imports: 0,
        },
        c.clone(),
        gas,
        supervisor,
        table.clone(),
    )?;
    out.set_evidence(&rt.path.join(format!("{}-driver.jsonl", node.name)))?;
    out.set_journal(&rt.path.join(format!("{}-checkpoint.json", node.name)))?;
    Ok(out)
}
fn step(d: &mut Live, node: &Node, h: u64) -> Result<Value> {
    let StepOutcome::Derived(result) = d.step(h)? else {
        bail!("unexpected wait at {h}")
    };
    observe_all(node, &result, d.m2_state().root)
}
fn source(l1: &Rpc, h: u64) -> Result<Value> {
    let header: L1Block = serde_json::from_value(
        l1.call("eth_getBlockByNumber", json!([format!("0x{h:x}"), false]))?,
    )?;
    let logs=l1.call("eth_getLogs",json!([{"blockHash":header.hash,"address":boole_derivation::config::INBOX,"topics":[boole_derivation::config::posted_topic()]}]))?;
    Ok(json!({"height":h,"l1":header,"logs":logs}))
}
fn model_step(s: &State, c: &Config, row: &Value, forks: &ForkTable) -> Result<State> {
    let logs: Vec<boole_derivation::InboxLog> = serde_json::from_value(row["logs"].clone())?;
    let mut inputs = vec![];
    for log in logs {
        let bytes = hex_bytes(&log.data, "inbox")?;
        if let Ok((kind, body)) = boole_derivation::inbox::item(&bytes) {
            if kind >= 2 {
                inputs.push(Input {
                    kind,
                    body: body.to_vec(),
                    transaction_index: log.transaction_index,
                    log_index: log.log_index,
                });
            }
        }
    }
    Ok(s.step_with_verdicts(
        c,
        &serde_json::from_value(row["l1"].clone())?,
        &inputs,
        &[],
        forks,
    )?
    .state)
}
fn proof(c: &Config, p: Hash, tag: u8, salt: u8) -> Result<Proof> {
    let raw = export(tag, 777);
    let frame = boole_gate_supervisor::raw_frame::encode(&raw)?;
    let mut proof = Proof::new(c, p, &frame, [salt; 32], [6; 20]);
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
fn mutate_verdict(payload: &Value) -> Result<Value> {
    let mut bad = payload.clone();
    let raw = bad["transactions"][2].as_str().context("verdict raw")?;
    let mut bytes = hex_bytes(raw, "verdict")?;
    *bytes.last_mut().context("verdict bytes")? ^= 1;
    bad["transactions"][2] = json!(hex_string(&bytes));
    Ok(bad)
}
fn round(kind: &str) -> Result<()> {
    let Some(binary) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let workers = gates()?;
    let mut c = config();
    c.m3 = Some(M3Config::devnet(c.slot_table.len()));
    c.m3.as_mut().unwrap().genesis_gate_rule_digest = workers[0].descriptor.digest();
    let mut forks = ForkTable::genesis(&c)?;
    let mut rt = Runtime::new(kind)?;
    let l1g = l1_genesis_value()?;
    let l1p = rt.path.join("l1-genesis.json");
    write_json(&l1p, &l1g)?;
    let l1 = rt.anvil_with_genesis(&anvil, &l1p)?;
    let accounts = l1.call("eth_accounts", json!([]))?;
    let user = accounts[0].as_str().context("account")?;
    let genesis = reset_genesis()?;
    let gp = rt.path.join("l2-genesis.json");
    write_json(&gp, &genesis)?;
    let a = rt.node(&binary, &gp, "a", 0)?;
    let mut b = rt.node(&binary, &gp, "b", 1)?;
    let mut da = live_driver(&a, &rt, &c, &forks, &workers)?;
    let mut db = live_driver(&b, &rt, &c, &forks, &workers)?;
    let mut model = State::new(&c, 30_000_000)?;
    let pkg = package(10000);
    let carried = boole_gate_supervisor::raw_frame::encode(&pkg)?;
    let leaf = boole_derivation::m2::pool_leaf(
        c.chain_id,
        2,
        &[],
        1,
        pkg.len() as u32,
        &carried,
        &[9; 32],
    );
    let mut first = None;
    let mut second = None;
    let quarantine = kind.starts_with("quarantine");
    let mut rows = vec![];
    let mut initial_finalized = 0;
    let mut reorg = Value::Null;
    for h in 1..=104 {
        if h % 10 == 0 {
            eprintln!("{kind}: L1 build {h}");
        }
        let mut items = vec![];
        match h {
            1 => items.push((8, pool(&c, leaf, 1, 3, 1))),
            5 => items.push((
                9,
                problem_encoded(0, 1, 0, [9; 32], &[], &carried, (1, pkg.len() as u32)),
            )),
            6 => {
                let p = *model.problems.keys().next().context("problem")?;
                let tag = if quarantine {
                    6
                } else if kind == "kernel-fix" {
                    7
                } else {
                    0
                };
                let v = proof(&c, p, tag, 7)?;
                items.push((2, v.commit_body()));
                first = Some(v);
            }
            7 if quarantine => {
                let v = proof(&c, first.as_ref().unwrap().problem, 0, 8)?;
                items.push((2, v.commit_body()));
                second = Some(v);
            }
            16 => items.push((3, first.as_ref().unwrap().open())),
            17 if quarantine => items.push((3, second.as_ref().unwrap().open())),
            17 => items.push((4, first.as_ref().unwrap().part(0))),
            18 if quarantine => items.push((4, second.as_ref().unwrap().part(0))),
            18 if kind == "round" => items.push((4, first.as_ref().unwrap().part(0))),
            20 if quarantine => items.push((4, first.as_ref().unwrap().part(0))),
            27 if kind == "round" => items.push((8, pool(&c, leaf, 1, 30, 2))),
            _ => {}
        }
        // A signed L2 user shares the verdict and maturity blocks in I9.
        if kind == "round" && [26, 90].contains(&h) {
            items.push((
                1,
                transfer(u64::from(h == 90), 21000, &address(&user_key()), &[]),
            ));
        }
        let tx = if items.is_empty() {
            None
        } else {
            Some(post(&l1, user, &items, items.len() > 1)?)
        };
        mine_source(&l1, h)?;
        if let Some(tx) = tx {
            let receipt = l1.call("eth_getTransactionReceipt", json!([tx]))?;
            ensure!(receipt["status"] == "0x1", "L1 post failed at {h}");
            ensure!(
                receipt["logs"].as_array().context("logs")?.len() == items.len(),
                "missing L1 log"
            );
        }
        let row = source(&l1, h)?;
        if h <= 16 {
            model = model_step(&model, &c, &row, &forks)?;
        }
        rows.push(row);
        if h == 17 && kind == "round" {
            initial_finalized = da.l1.finalized()?;
            ensure!(initial_finalized < 17, "completion already finalized");
            for j in 1..=initial_finalized {
                rows[(j - 1) as usize]["parent"] = serde_json::to_value(da.engine.head()?)?;
                let av = step(&mut da, &a, j)?;
                ensure!(av == step(&mut db, &b, j)?, "early AB mismatch");
                rows[(j - 1) as usize]["result"] = av;
            }
            let stable = (da.m2_state().root, da.engine.head()?.hash);
            for d in [&mut da, &mut db] {
                ensure!(
                    matches!(
                        d.step(initial_finalized + 1)?,
                        StepOutcome::WaitingForFinality
                    ),
                    "unfinalized input adopted"
                );
            }
            let old = rows[16].clone();
            l1.call("anvil_reorg", json!([1, []]))?;
            let replacement = source(&l1, 17)?;
            ensure!(
                old["l1"]["hash"] != replacement["l1"]["hash"]
                    && replacement["logs"].as_array().context("logs")?.is_empty(),
                "completion reorg failed"
            );
            ensure!(
                stable == (da.m2_state().root, da.engine.head()?.hash),
                "reorg changed finalized L2"
            );
            reorg = json!({"removed":old,"replacement":replacement,"finalized_height":initial_finalized,"stable_root":hex_string(&stable.0),"stable_block":stable.1});
            rows[16] = replacement;
        }
    }
    l1.call("anvil_mine", json!(["0x10"]))?;
    ensure!(da.l1.finalized()? >= 104, "insufficient finality");
    let first = first.context("first")?;
    let key = (first.problem, first.commit);
    let mut cc = None;
    let mut dc: Option<Live> = None;
    let mut mutant = Value::Null;
    let mut pending = Value::Null;
    let verdict_h = if quarantine {
        28
    } else if kind == "round" {
        26
    } else {
        25
    };
    for h in initial_finalized + 1..=100 {
        if h % 10 == 0 {
            eprintln!("{kind}: ABC verify {h}");
        }
        if kind == "kernel-fix" && h == 26 {
            forks.rows.push(ForkRow {
                epoch: 1,
                height: h,
                gate_rule_digest: workers[1].descriptor.digest(),
                kind: ForkKind::KernelFix,
                list: vec![],
            });
            for d in [&mut da, &mut db] {
                d.set_forks(forks.clone())?;
            }
            if let Some(d) = dc.as_mut() {
                d.set_forks(forks.clone())?;
            }
        }
        if quarantine && h == 28 {
            let before = da.m2_state().root;
            for d in [&mut da, &mut db] {
                let stall = d.step(h).expect_err("all-DEFER built a block");
                ensure!(
                    matches!(
                        stall.kind,
                        StallKind::VerdictPending {
                            height: 28,
                            queue_index: 1
                        }
                    ),
                    "wrong pending: {stall}"
                );
                ensure!(d.m2_state().root == before, "DEFER changed state");
            }
            if let Some(d) = dc.as_mut() {
                let stall = d.step(h).expect_err("C DEFER built");
                ensure!(
                    matches!(stall.kind, StallKind::VerdictPending { .. }),
                    "C pending"
                );
            }
            pending = json!({"height":h,"queue_index":1,"root":hex_string(&before)});
            forks.rows.push(ForkRow {
                epoch: 1,
                height: h,
                gate_rule_digest: workers[1].descriptor.digest(),
                kind: ForkKind::Quarantine,
                list: vec![ForkItem {
                    commit_hash: first.commit,
                    disposition: if kind == "quarantine-void" {
                        Disposition::Void
                    } else {
                        Disposition::Recheck
                    },
                }],
            });
            for d in [&mut da, &mut db] {
                d.set_forks(forks.clone())?;
            }
            if let Some(d) = dc.as_mut() {
                d.set_forks(forks.clone())?;
            }
        }
        rows[(h - 1) as usize]["parent"] = serde_json::to_value(da.engine.head()?)?;
        let av = step(&mut da, &a, h)?;
        ensure!(av == step(&mut db, &b, h)?, "AB mismatch {kind} at {h}");
        if let (Some(node), Some(d)) = (&cc, &mut dc) {
            if h == verdict_h {
                let before = d.engine.imports;
                let bad = mutate_verdict(&da.engine.payload)?;
                let error = d
                    .import_payload(h, bad)
                    .expect_err("mutated verdict imported");
                ensure!(
                    error.kind == StallKind::Verdict { height: h }
                        && d.engine.imports == before
                        && d.engine.head()?.number == h - 1,
                    "mutation not refused before Engine"
                );
                mutant =
                    json!({"height":h,"kind":"Verdict","engine_imports_unchanged":true,"head":h-1});
                *d = live_driver(node, &rt, &c, &forks, &workers)?;
            }
            ensure!(av == step(d, node, h)?, "AC mismatch {kind} at {h}");
        }
        rows[(h - 1) as usize]["result"] = av;
        if h == 18 {
            ensure!(
                da.m2_state().candidates[&key].status()
                    == if quarantine {
                        Status::Open
                    } else {
                        Status::Checking
                    },
                "restart queue state"
            );
            rt.stop("b")?;
            b = rt.node(&binary, &gp, "b", 1)?;
            db = live_driver(&b, &rt, &c, &forks, &workers)?;
        }
        if h == 22 {
            let node = rt.node(&binary, &gp, "c", 2)?;
            let mut d = live_driver(&node, &rt, &c, &forks, &workers)?;
            for j in 1..=h {
                ensure!(
                    step(&mut d, &node, j)? == rows[(j - 1) as usize]["result"],
                    "fresh late C differs {j}"
                );
            }
            cc = Some(node);
            dc = Some(d);
        }
        let balance = amount_value(
            &a.state(
                &hex_string(&[6; 20]),
                a.block()?["hash"].as_str().context("hash")?,
            )?["balance"],
        )?;
        if kind == "round" {
            if h == 26 || h == 90 {
                ensure!(
                    rows[(h - 1) as usize]["result"]["transactions"]
                        .as_array()
                        .context("transactions")?
                        .len()
                        == 4,
                    "I9 user missing from verdict/reward block"
                );
            }
            if h == 26 {
                ensure!(
                    da.m2_state().slots[0].scheduled == Some(30),
                    "next draw not scheduled from settlement"
                );
            }
            if h == 31 {
                ensure!(da.m2_state().slots[0].round == 2, "next round draw missing");
            }
            if h < 90 {
                ensure!(balance == 0, "T56 premature balance at {h}");
            } else {
                ensure!(
                    balance == c.m3.as_ref().unwrap().reward[0],
                    "T56 balance at vest {h}"
                );
            }
            if h == 18 {
                ensure!(
                    da.m2_state().candidates[&key]
                        .live()
                        .and_then(|v| v.open.as_ref())
                        .and_then(|o| o.h_r)
                        == Some(18),
                    "reorg completion height"
                );
            }
        }
    }
    let status = da.m2_state().candidates[&key].status();
    ensure!(
        status
            == match kind {
                "kernel-fix" => Status::Stale,
                "quarantine-void" => Status::Void,
                _ => Status::Accepted,
            },
        "final status {kind}: {status:?}"
    );
    let m3 = da.m2_state().m3.as_ref().context("M3 state")?;
    if quarantine {
        ensure!(
            m3.queue[1].key == key && m3.queue[1].avail == 20,
            "quarantine original index/avail"
        );
    }
    if kind == "kernel-fix" {
        let state = da.m2_state();
        let m = state.m3.as_ref().context("M3")?;
        ensure!(
            m.rewards
                .values()
                .all(|r| r.status == RewardStatus::Cancelled),
            "STALE locked reward not cancelled"
        );
        ensure!(
            m.admissions.values().all(|a| a.retired),
            "STALE admission not retired"
        );
        ensure!(
            state.problems.values().any(|p| p.id != first.problem
                && p.epoch == 1
                && p.reveal_pos == state.problems[&first.problem].reveal_pos),
            "original problem not reposed"
        );
        ensure!(
            amount_value(
                &a.state(
                    &hex_string(&[6; 20]),
                    a.block()?["hash"].as_str().context("hash")?
                )?["balance"]
            )? == 0,
            "cancelled reward minted"
        );
    }
    if quarantine {
        let winner = da.m2_state().problems[&first.problem].winner;
        ensure!(
            winner
                == Some(if kind == "quarantine-void" {
                    second.as_ref().context("second")?.commit
                } else {
                    first.commit
                }),
            "quarantine did not release original barrier"
        );
    }
    let root = da.m2_state().root;
    ensure!(
        db.m2_state().root == root && dc.as_ref().context("C")?.m2_state().root == root,
        "final roots"
    );
    let final_state = json!({"status":format!("{status:?}"),"root":hex_string(&root),"rewards":format!("{:?}",m3.rewards),"admissions":format!("{:?}",m3.admissions),"rounds":format!("{:?}",da.m2_state().slots)});
    rt.finish(true)?;
    let variable = match kind {
        "round" => "BOOLE_DEVNET_M3_ROUND",
        "kernel-fix" => "BOOLE_DEVNET_M3_KERNEL_FIX",
        "quarantine-void" => "BOOLE_DEVNET_M3_QUARANTINE_VOID",
        _ => "BOOLE_DEVNET_M3_QUARANTINE_RECHECK",
    };
    record(
        variable,
        &rt,
        json!({"schema":"boole-devnet-m3-recording-v1.3","scenario":kind,"config_json":String::from_utf8(serde_json::to_vec(&c)?)?,"forks":forks,"gate_descriptors":workers.iter().map(|w|&w.descriptor).collect::<Vec<_>>(),"l1_genesis":l1g,"l2_genesis":genesis,"acceptance":{"live_abc":true,"mid_queue_restart":18,"fresh_late_start":22,"mutated_verdict":mutant,"all_defer":pending},"unfinalized_completion_reorg":reorg,"final_state":final_state,"heights":rows.into_iter().take(100).collect::<Vec<_>>()}),
    )?;
    Ok(())
}
#[test]
fn i9_i10_i11_t56_live_round_restart_late_start_completion_reorg_and_vest() -> Result<()> {
    round("round")
}
#[test]
fn i12_live_kernel_fix_stale_recheck_and_repose() -> Result<()> {
    round("kernel-fix")
}
#[test]
fn i12_live_quarantine_void_releases_original_barrier() -> Result<()> {
    round("quarantine-void")
}
#[test]
fn i12_live_quarantine_recheck_preserves_original_queue_index() -> Result<()> {
    round("quarantine-recheck")
}
