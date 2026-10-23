//! I11 with all three verify replicas present before an unfinalized completion reorg.
#[allow(dead_code)]
mod support;
use anyhow::{bail, ensure, Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    m2::*,
    m3::*,
    InboxLog, L1Block, ParentHeader,
};
use boole_devnet_driver::{
    driver::{Driver, StallKind, StepOutcome},
    engine::{Engine, Rpc, RpcEngine},
    l1::{L1Source, RpcL1},
};
use boole_gate_supervisor::{
    codec::{digest, Codec},
    process::{Envelope, Worker},
    stub::{export, package, Descriptor},
    supervisor::{install_devnet_artifacts, Host, Mode, Supervisor},
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
use support::{m2::*, *};
struct Tap {
    inner: RpcEngine,
    payload: Value,
    imports: u64,
}
impl Engine for Tap {
    fn head(&mut self) -> Result<ParentHeader> {
        self.inner.head()
    }
    fn header(&mut self, h: u64) -> Result<ParentHeader> {
        self.inner.header(h)
    }
    fn transactions(&mut self, h: u64) -> Result<Vec<String>> {
        self.inner.transactions(h)
    }
    fn call(&mut self, m: &str, p: Value) -> Result<Value> {
        if m == "engine_newPayloadV4" {
            self.imports += 1;
        }
        let v = self.inner.call(m, p)?;
        if m == "engine_getPayloadV5" {
            self.payload = v["executionPayload"].clone();
        }
        Ok(v)
    }
}
type Live = Driver<RpcL1, Tap>;
fn start(node: &Node, rt: &Runtime, c: &Config, gate: &Worker, forks: &ForkTable) -> Result<Live> {
    let artifacts = rt.path.join("artifacts");
    if !artifacts.exists() {
        install_devnet_artifacts(c, &artifacts)?;
    }
    let cache = rt.path.join(format!("{}-cache", node.name));
    let supervisor = Supervisor::new(
        c,
        forks,
        Mode::Verify,
        vec![gate.clone()],
        Some(Codec::portable_stub()),
        &cache,
        &cache,
        &artifacts,
        Host::local()?,
        Envelope::default(),
    )?;
    let mut inner = node.engine()?;
    let gas = inner.head()?.gas_limit;
    let mut d = Driver::with_m3(
        RpcL1::new_m2("http://127.0.0.1:28545")?,
        Tap {
            inner,
            payload: Value::Null,
            imports: 0,
        },
        c.clone(),
        gas,
        supervisor,
        forks.clone(),
    )?;
    d.set_evidence(&rt.path.join(format!("{}-driver.jsonl", node.name)))?;
    d.set_journal(&rt.path.join(format!("{}-checkpoint.json", node.name)))?;
    Ok(d)
}
fn source(l1: &Rpc, h: u64) -> Result<Value> {
    let block: L1Block = serde_json::from_value(
        l1.call("eth_getBlockByNumber", json!([format!("0x{h:x}"), false]))?,
    )?;
    let logs=l1.call("eth_getLogs",json!([{"blockHash":block.hash,"address":boole_derivation::config::INBOX,"topics":[boole_derivation::config::posted_topic()]}]))?;
    Ok(json!({"height":h,"l1":block,"logs":logs}))
}
fn model(s: &State, c: &Config, row: &Value, forks: &ForkTable) -> Result<State> {
    let logs: Vec<InboxLog> = serde_json::from_value(row["logs"].clone())?;
    let mut inputs = vec![];
    for log in logs {
        let data = hex_bytes(&log.data, "item")?;
        if let Ok((kind, body)) = boole_derivation::inbox::item(&data) {
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
fn observed(d: &mut Live, node: &Node, h: u64) -> Result<Value> {
    let StepOutcome::Derived(out) = d.step(h)? else {
        bail!("unexpected finality wait")
    };
    let block = node.block()?;
    let txs = d.engine.transactions(h)?;
    boole_devnet_driver::driver::check_payload_transactions(&json!({"transactions":txs}), &out)?;
    Ok(
        json!({"block_hash":block["hash"],"state_root":block["stateRoot"],"receipts_root":block["receiptsRoot"],"transactions":txs,"attributes":out.attributes,"decisions":out.decisions,"m2_root":hex_string(&d.m2_state().root)}),
    )
}
#[test]
fn i11_all_three_wait_through_an_unfinalized_completion_reorg() -> Result<()> {
    let Some(binary) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let path = PathBuf::from(std::env::var_os("BOOLE_DEVNET_GATE").context("gate")?);
    let hash = digest(&fs::read(&path)?);
    let gate = Worker {
        descriptor: Descriptor::pinned(hash),
        binary: path,
        binary_sha256: hash,
    };
    let mut c = config();
    c.m3 = Some(M3Config::devnet(c.slot_table.len()));
    c.m3.as_mut().unwrap().genesis_gate_rule_digest = gate.descriptor.digest();
    let forks = ForkTable::genesis(&c)?;
    let mut rt = Runtime::new("i11-all-three")?;
    let l1_genesis = l1_genesis_value()?;
    let lp = rt.path.join("l1-genesis.json");
    fs::write(&lp, serde_json::to_vec_pretty(&l1_genesis)?)?;
    let l1 = rt.anvil_with_genesis(&anvil, &lp)?;
    let accounts = l1.call("eth_accounts", json!([]))?;
    let user = accounts[0].as_str().context("account")?;
    let reset =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/devnet-m3/l2-genesis-v1.3.json");
    let mut genesis: Value = serde_json::from_slice(&fs::read(&reset)?)?;
    genesis["alloc"][hex_string(&address(&user_key()))] = json!({"balance":"0x56bc75e2d63100000"});
    genesis["alloc"]["0x42000000000000000000000000000000000000b4"] =
        json!({"balance":"0x0","code":"0x60006000fd"});
    let gp = rt.path.join("l2-genesis.json");
    fs::write(&gp, serde_json::to_vec_pretty(&genesis)?)?;
    let nodes = [
        rt.node(&binary, &gp, "a", 0)?,
        rt.node(&binary, &gp, "b", 1)?,
        rt.node(&binary, &gp, "c", 2)?,
    ];
    let mut ds = nodes
        .iter()
        .map(|n| start(n, &rt, &c, &gate, &forks))
        .collect::<Result<Vec<_>>>()?;
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
    let mut state = State::new(&c, 30_000_000)?;
    let mut proof = None;
    let mut rows = vec![];
    for h in 1..=17 {
        let items = match h {
            1 => vec![(8, pool(&c, leaf, 1, 3, 1))],
            5 => vec![(
                9,
                problem_encoded(0, 1, 0, [9; 32], &[], &carried, (1, pkg.len() as u32)),
            )],
            6 => {
                let p = *state.problems.keys().next().context("problem")?;
                let raw = export(0, 777);
                let frame = boole_gate_supervisor::raw_frame::encode(&raw)?;
                let mut v = Proof::new(&c, p, &frame, [7; 32], [6; 20]);
                v.body[153..161].copy_from_slice(&(raw.len() as u64).to_be_bytes());
                v.body[161..193].copy_from_slice(&digest(&raw));
                v.commit = commit_hash(c.chain_id, p, cancel_hash(&v.secret), body_digest(&v.body));
                let item = v.commit_body();
                proof = Some(v);
                vec![(2, item)]
            }
            16 => vec![(3, proof.as_ref().context("proof")?.open())],
            17 => vec![(4, proof.as_ref().context("proof")?.part(0))],
            _ => vec![],
        };
        if !items.is_empty() {
            post(&l1, user, &items, items.len() > 1)?;
        }
        l1.call("evm_mine", json!([1_800_000_000 + 12 * h]))?;
        let row = source(&l1, h)?;
        if h <= 16 {
            state = model(&state, &c, &row, &forks)?;
        }
        rows.push(row);
    }
    let finalized = ds[0].l1.finalized()?;
    ensure!(finalized < 17, "completion must be unfinalized");
    for h in 1..=finalized {
        rows[(h - 1) as usize]["parent"] = serde_json::to_value(ds[0].engine.head()?)?;
        let mut observations = vec![];
        for (d, n) in ds.iter_mut().zip(&nodes) {
            observations.push(observed(d, n, h)?);
        }
        ensure!(
            observations[0] == observations[1] && observations[0] == observations[2],
            "early ABC mismatch"
        );
        rows[(h - 1) as usize]["result"] = observations[0].clone();
    }
    let mut stable = vec![];
    for d in &mut ds {
        stable.push((d.m2_state().root, d.engine.head()?.hash));
        ensure!(
            matches!(d.step(finalized + 1)?, StepOutcome::WaitingForFinality),
            "unfinalized completion adopted"
        );
    }
    ensure!(
        stable[0] == stable[1] && stable[0] == stable[2],
        "initial stable tips"
    );
    let orphan = rows[16].clone();
    let completed = model(&state, &c, &orphan, &forks)?;
    let original = proof.as_ref().context("proof")?;
    ensure!(
        completed.candidates[&(original.problem, original.commit)].status() == Status::Checking
            && completed.candidates[&(original.problem, original.commit)]
                .live()
                .and_then(|v| v.open.as_ref())
                .and_then(|o| o.h_r)
                == Some(17),
        "orphan must contain the actual accepted completion"
    );
    l1.call("anvil_reorg", json!([1, []]))?;
    let replacement = source(&l1, 17)?;
    ensure!(
        orphan["l1"]["hash"] != replacement["l1"]["hash"]
            && replacement["logs"].as_array().context("logs")?.is_empty(),
        "completion not removed"
    );
    rows[16] = replacement.clone();
    for (d, (root, tip)) in ds.iter_mut().zip(&stable) {
        ensure!(
            &d.m2_state().root == root && &d.engine.head()?.hash == tip,
            "unfinalized reorg changed a replica"
        );
        ensure!(
            matches!(d.step(finalized + 1)?, StepOutcome::WaitingForFinality),
            "replacement not finalized"
        );
    }
    for h in 18..=30 {
        if h == 18 {
            post(
                &l1,
                user,
                &[(4, proof.as_ref().context("proof")?.part(0))],
                false,
            )?;
        }
        l1.call("evm_mine", json!([1_800_000_000 + 12 * h]))?;
        rows.push(source(&l1, h)?);
    }
    l1.call("anvil_mine", json!(["0x10"]))?;
    ensure!(ds[0].l1.finalized()? >= 30, "finality");
    let mut mutant = Value::Null;
    for h in finalized + 1..=30 {
        rows[(h - 1) as usize]["parent"] = serde_json::to_value(ds[0].engine.head()?)?;
        let expected = observed(&mut ds[0], &nodes[0], h)?;
        if h == 26 {
            let mut bad = ds[0].engine.payload.clone();
            let mut raw = hex_bytes(
                bad["transactions"][2].as_str().context("verdict")?,
                "verdict",
            )?;
            *raw.last_mut().context("verdict bytes")? ^= 1;
            bad["transactions"][2] = json!(hex_string(&raw));
            let before = ds[2].engine.imports;
            let error = ds[2]
                .import_payload(h, bad)
                .expect_err("mutated verdict adopted");
            ensure!(
                error.kind == StallKind::Verdict { height: h }
                    && ds[2].engine.imports == before
                    && ds[2].engine.head()?.number == h - 1,
                "mutation reached Engine"
            );
            mutant = json!({"height":h,"engine_imports_unchanged":true});
            ds[2] = start(&nodes[2], &rt, &c, &gate, &forks)?;
        }
        for i in 1..3 {
            ensure!(
                expected == observed(&mut ds[i], &nodes[i], h)?,
                "post-reorg replica mismatch at {h}"
            );
        }
        rows[(h - 1) as usize]["result"] = expected;
        let p = proof.as_ref().context("proof")?;
        if h == 18 {
            ensure!(
                ds[0].m2_state().candidates[&(p.problem, p.commit)].status() == Status::Checking
                    && ds[0].m2_state().candidates[&(p.problem, p.commit)]
                        .live()
                        .and_then(|l| l.open.as_ref())
                        .and_then(|o| o.h_r)
                        == Some(18),
                "canonical completion position"
            );
        }
    }
    let p = proof.context("proof")?;
    ensure!(
        ds.iter()
            .all(|d| d.m2_state().candidates[&(p.problem, p.commit)].status() == Status::Accepted),
        "post-reorg verdict"
    );
    let root = ds[0].m2_state().root;
    rt.finish(true)?;
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()?;
    ensure!(commit.status.success(), "producer commit");
    let data = json!({"schema":"boole-devnet-m3-recording-v1.3","scenario":"reorg-all-three","config_json":String::from_utf8(serde_json::to_vec(&c)?)?,"forks":forks,"gate_descriptors":[gate.descriptor],"l1_genesis":l1_genesis,"l2_genesis":genesis,"provenance":{"producer_commit":String::from_utf8(commit.stdout)?.trim(),"producer_binary_sha256":hex_string(&digest(&fs::read(std::env::current_exe()?)?)),"generator_sha256":hex_string(&digest(include_bytes!("m3_reorg.rs"))),"support_sha256":hex_string(&digest(include_bytes!("support/mod.rs"))),"wire_support_sha256":hex_string(&digest(include_bytes!("support/m2.rs"))),"cargo_lock_sha256":hex_string(&digest(include_bytes!("../../../Cargo.lock"))),"pins":fixture_json("pins.json")?,"engine_profile":engine_profile(),"reset_genesis_sha256":hex_string(&digest(&fs::read(reset)?))},"acceptance":{"live_abc":true,"three_waiting":true,"mutated_verdict":mutant},"unfinalized_completion_reorg":{"removed":orphan,"replacement":replacement,"finalized_height":finalized,"stable_root":hex_string(&stable[0].0),"stable_block":stable[0].1},"final_state":{"root":hex_string(&root)},"heights":rows,"cleanup":serde_json::from_slice::<Value>(&fs::read(rt.path.join("cleanup.json"))?)?});
    fs::write(
        rt.path.join("qualification.json"),
        serde_json::to_vec_pretty(&data)?,
    )?;
    if let Some(path) = std::env::var_os("BOOLE_DEVNET_M3_REORG") {
        let path = PathBuf::from(path);
        ensure!(!path.exists(), "recording already exists");
        fs::write(path, serde_json::to_vec_pretty(&data)?)?;
    }
    Ok(())
}
