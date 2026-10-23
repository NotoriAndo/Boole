//! I13: actual MCP stdio, three independently verifying Engines, through vest.
#[path = "support/mcp.rs"]
mod mcp;
#[path = "../../boole-devnet-driver/tests/support/mod.rs"]
#[allow(dead_code)]
mod support;
use anyhow::{ensure, Context, Result};
use boole_derivation::{attributes::hex_string, m2::*, m3::*, Derived};
use boole_devnet_driver::{
    driver::{Driver, StepOutcome},
    engine::{Engine, RpcEngine},
    l1::{L1Source, RpcL1},
};
use boole_gate_supervisor::{
    codec::Codec,
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

type Live = Driver<RpcL1, RpcEngine>;
fn driver(
    node: &Node,
    rt: &Runtime,
    c: &Config,
    table: &ForkTable,
    workers: &[Worker],
) -> Result<Live> {
    let cache = rt.path.join(format!("{}-cache", node.name));
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
    let mut engine = node.engine()?;
    let gas = engine.head()?.gas_limit;
    let mut driver = Driver::with_m3(
        RpcL1::new_m2("http://127.0.0.1:28545")?,
        engine,
        c.clone(),
        gas,
        supervisor,
        table.clone(),
    )?;
    driver.set_evidence(&rt.path.join(format!("{}-driver.jsonl", node.name)))?;
    driver.set_journal(&rt.path.join(format!("{}-journal.json", node.name)))?;
    Ok(driver)
}
fn observed(node: &Node, d: &Derived, root: Hash, h: u64) -> Result<Value> {
    let block = node.block()?;
    ensure!(quantity(&block["number"])? == h, "Engine height");
    let raw = node.engine()?.transactions(h)?;
    boole_devnet_driver::driver::check_payload_transactions(&json!({"transactions":raw}), d)?;
    let origin = boole_derivation::system::l1_origin(&raw[0], h)?;
    ensure!(
        boole_derivation::system::read_m2_root(&raw[1], h, &origin)? == root,
        "embedded M2ROOT"
    );
    let rpc = node.rpc()?;
    let mut receipts = vec![];
    for tx in block["transactions"].as_array().context("transactions")? {
        let receipt = rpc.call("eth_getTransactionReceipt", json!([tx]))?;
        ensure!(receipt["blockHash"] == block["hash"], "canonical receipt");
        receipts.push(receipt);
    }
    Ok(
        json!({"block_hash":block["hash"],"state_root":block["stateRoot"],"receipts_root":block["receiptsRoot"],"transactions":raw,"receipts":receipts,"m2_root":hex_string(&root)}),
    )
}
fn step(driver: &mut Live, node: &Node, h: u64) -> Result<Value> {
    let StepOutcome::Derived(result) = driver.step(h)? else {
        anyhow::bail!("I13 driver waited at {h}")
    };
    observed(node, &result, driver.m2_state().root, h)
}
fn equal_status(server: &mut mcp::Server, driver: &Live, h: u64, finalized: u64) -> Result<Value> {
    let status = server.tool("boole.devnet_status", json!({}))?;
    ensure!(status["stalled"].is_null(), "client stalled: {status}");
    ensure!(
        status["fold_height"] == h && status["m2_root"] == hex_string(&driver.m2_state().root),
        "I13 client/driver root mismatch at {h}: {status}"
    );
    if h == finalized {
        ensure!(
            status["l2_m2_root_match"] == true,
            "L2 root match at caught-up height {h}"
        );
    } else {
        ensure!(
            status["l2_m2_root_match"] == true
                || (status["l2_m2_root_match"].is_null() && status["waiting_for_l2"] == true),
            "invalid root-match observation"
        );
    }
    ensure!(
        status["verdict_trust"] == "l2-node-with-own-verification",
        "own verification mode disclosure"
    );
    Ok(status)
}
fn write_json(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

#[test]
fn i13_miner_m3_live_abc_own_verification_restart_status_and_reward_vest() -> Result<()> {
    let Some(op) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let gate = PathBuf::from(
        std::env::var_os("BOOLE_DEVNET_GATE").context("pinned gate binary required")?,
    );
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
    let mut rt = Runtime::new("m3c-i13")?;
    let artifacts = rt.path.join("artifacts");
    install_devnet_artifacts(&c, &artifacts)?;
    let gate_file = rt.path.join("gates.json");
    fs::write(&gate_file, serde_json::to_vec(&workers)?)?;
    let forks_file = rt.path.join("forks.json");
    fs::write(&forks_file, serde_json::to_vec(&table)?)?;
    let l1g = l1_genesis_value()?;
    let l1p = rt.path.join("l1-genesis.json");
    write_json(&l1p, &l1g)?;
    let l1 = rt.anvil_with_genesis(&anvil, &l1p)?;
    let accounts = l1.call("eth_accounts", json!([]))?;
    let account = accounts[0].as_str().context("dev account")?;
    let base_genesis =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/devnet-m3/l2-genesis-v1.3.json");
    let genesis: Value = serde_json::from_slice(&fs::read(&base_genesis)?)?;
    ensure!(
        genesis["alloc"]["0x42000000000000000000000000000000000000b3"]["code"] == "0x00",
        "v1.3 STOP sink"
    );
    let gp = rt.path.join("l2-genesis.json");
    write_json(&gp, &genesis)?;
    let nodes = [
        rt.node(&op, &gp, "a", 0)?,
        rt.node(&op, &gp, "b", 1)?,
        rt.node(&op, &gp, "c", 2)?,
    ];
    let mut drivers = [
        driver(&nodes[0], &rt, &c, &table, &workers)?,
        driver(&nodes[1], &rt, &c, &table, &workers)?,
        driver(&nodes[2], &rt, &c, &table, &workers)?,
    ];
    let extra = vec![
        "--verify-own".into(),
        "--m3-gates".into(),
        gate_file.display().to_string(),
        "--gate-artifacts".into(),
        artifacts.display().to_string(),
        "--m3-forks".into(),
        forks_file.display().to_string(),
    ];
    let private = rt.path.join("miner-state");
    let mut server = mcp::Server::start(&rt.path.join("mcp-first"), &c, account, &private, &extra)?;
    let package = package(10_000);
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
    let mut primary: Option<(Hash, String)> = None;
    let mut rows = vec![];
    let mut restarted = false;
    let mut locked = Value::Null;
    let mut vested = Value::Null;
    let mut reward_view = Value::Null;
    let mut comparisons = 0usize;
    for latest in 1..=220u64 {
        if latest == 5 {
            post(
                &l1,
                account,
                &[(9, problem(0, 1, 0, salt, &[], &package))],
                false,
            )?;
        }
        l1.call("evm_mine", json!([1_800_000_000 + 12 * latest]))?;
        let finalized = RpcL1::new_m2("http://127.0.0.1:28545")?.finalized()?;
        while drivers[0].m2_state().height() < finalized {
            let h = drivers[0].m2_state().height() + 1;
            let a = step(&mut drivers[0], &nodes[0], h)?;
            let b = step(&mut drivers[1], &nodes[1], h)?;
            let cc = step(&mut drivers[2], &nodes[2], h)?;
            ensure!(
                a == b && a == cc,
                "I13 A/B/C payload/receipt/root mismatch at {h}"
            );
            let status = equal_status(&mut server, &drivers[0], h, finalized)?;
            comparisons += 1;
            let progress = if let Some((_, id)) = &primary {
                server.submission(id)?
            } else {
                Value::Null
            };
            rows.push(
                json!({"height":h,"observation":a,"client_status":status,"submission":progress}),
            );
        }
        if primary.is_none() && drivers[0].m2_state().height() >= 5 {
            let p = drivers[0].m2_state().rounds[&(0, 1)]
                .problem
                .context("drawn problem")?;
            let problem = server.tool("boole.devnet_problem", json!({"slot":0}))?;
            ensure!(problem["problem_id"] == hex_string(&p), "client problem");
            let view=server.tool("boole.devnet_commit",json!({"export_file":export_file,"payee":hex_string(&[6;20]),"problem_id":hex_string(&p)}))?;
            ensure!(
                view["schema_version"] == 2
                    && view["license_id"] == 1
                    && view["terms_digest"] == hex_string(&c.terms_table[&1])
                    && view["local_check"] == "not_run",
                "client terms and advisory disclosure"
            );
            let id = view["submission_id"]
                .as_str()
                .context("submission id")?
                .to_string();
            server.posted(&id, "commit")?;
            primary = Some((p, id));
        }
        if let Some((p, id)) = &primary {
            let hash = boole_devnet_mcp::client::fixed(id)?;
            if let Some(reward) = drivers[0]
                .m2_state()
                .m3
                .as_ref()
                .unwrap()
                .rewards
                .values()
                .find(|r| r.key == (*p, hash))
            {
                let progress = server.submission(id)?;
                ensure!(
                    progress["status"] == "ACCEPTED"
                        && progress["queue_index"] == 0
                        && progress["verdict"]["status"] == "ACCEPT"
                        && progress["verdict"]["fuel"] == 777,
                    "client verdict status"
                );
                let onchain = &drivers[0].m2_state().m3.as_ref().unwrap().results[&(*p, hash)];
                ensure!(
                    progress["verdict"]["vstmt_digest"] == hex_string(&onchain.vstmt_digest),
                    "client verdict statement"
                );
                ensure!(
                    progress["earlier_candidates"] == json!([]),
                    "settled barrier"
                );
                let amount = reward.amount;
                let vest_h = reward.vest_h;
                if reward.status == RewardStatus::Locked {
                    ensure!(progress["rewards"][0]["status"] == "LOCKED", "locked state");
                    locked = progress;
                    ensure!(
                        nodes[0]
                            .rpc()?
                            .call("eth_getBalance", json!([hex_string(&[6; 20]), "latest"]))?
                            == "0x0",
                        "premature mint"
                    );
                } else if reward.status == RewardStatus::Vested {
                    vested = progress;
                    reward_view = server
                        .tool("boole.devnet_rewards", json!({"payee":hex_string(&[6;20])}))?;
                    let entry = &reward_view["rewards"][0];
                    ensure!(
                        entry["status"] == "VESTED"
                            && entry["vested_at"] == vest_h
                            && entry["amount"] == amount.to_string()
                            && entry["balance_before"] == "0x0"
                            && entry["balance_change"] == format!("0x{amount:x}")
                            && entry["balance_after"] == entry["balance_change"],
                        "client vest balance observation: {reward_view}"
                    );
                    for node in &nodes {
                        ensure!(
                            node.rpc()?.call(
                                "eth_getBalance",
                                json!([hex_string(&[6; 20]), format!("0x{vest_h:x}")])
                            )? == format!("0x{amount:x}"),
                            "T56 A/B/C vested balance"
                        );
                    }
                    if drivers[0].m2_state().height() >= vest_h + 2 {
                        break;
                    }
                }
                if !restarted
                    && drivers[0].m2_state().height() >= 40
                    && reward.status == RewardStatus::Locked
                {
                    server.stop()?;
                    server = mcp::Server::start(
                        &rt.path.join("mcp-restarted"),
                        &c,
                        account,
                        &private,
                        &extra,
                    )?;
                    equal_status(
                        &mut server,
                        &drivers[0],
                        drivers[0].m2_state().height(),
                        finalized,
                    )?;
                    ensure!(
                        server.submission(id)?["rewards"][0]["status"] == "LOCKED",
                        "restart reward state"
                    );
                    restarted = true;
                }
            }
        }
        if latest % 10 == 0 {
            eprintln!("I13 L1 {latest}, finalized {finalized}");
        }
    }
    ensure!(
        restarted && !locked.is_null() && !vested.is_null() && !reward_view.is_null(),
        "I13 round/restart/vest incomplete"
    );
    ensure!(
        comparisons == drivers[0].m2_state().height() as usize && comparisons >= 64,
        "compare every produced height"
    );
    let (p, id) = primary.context("primary")?;
    let id_hash = boole_devnet_mcp::client::fixed(&id)?;
    let candidate = drivers[0].m2_state().candidates[&(p, id_hash)]
        .live()
        .context("candidate")?;
    let open = candidate.open.as_ref().context("open")?;
    ensure!(
        open.h_o >= candidate.h_c + c.d && open.h_r.is_some() && open.n == 1,
        "client reveal schedule"
    );
    let final_progress = server.submission(&id)?;
    ensure!(
        final_progress["transactions"]
            .as_array()
            .context("posted stages")?
            .len()
            == 3,
        "no M3 terminal reposts"
    );
    server.stop()?;
    let stored: Value =
        serde_json::from_slice(&fs::read(private.join(format!("submission-{id}.json")))?)?;
    ensure!(stored["schema_version"] == 2, "schema2 persisted");
    let body: Vec<u8> = serde_json::from_value(stored["prepared"]["body"].clone())?;
    let secret: Hash = serde_json::from_value(stored["prepared"]["cancel_secret"].clone())?;
    let public=json!({"rows":rows,"locked":locked,"vested":vested,"rewards":reward_view,"final_progress":final_progress}).to_string();
    for material in [body.as_slice(), &body[53..85], secret.as_slice()] {
        let hex = hex_string(material);
        let array = serde_json::to_string(material)?;
        ensure!(
            !public.contains(&hex[2..]) && !public.contains(&array),
            "private material in public evidence"
        );
        for name in ["mcp-first", "mcp-restarted"] {
            for file in ["stdout.log", "stderr.log"] {
                let text = fs::read_to_string(rt.path.join(name).join(file))?;
                ensure!(
                    !text.contains(&hex[2..]) && !text.contains(&array),
                    "private material in MCP output"
                );
            }
        }
    }
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()?;
    ensure!(commit.status.success(), "producer commit");
    let mut result = json!({"schema":"boole-devnet-m3c-i13-v1.3","config_json":serde_json::to_string(&c)?,"config_digest":hex_string(&c.digest()),"gate_descriptors":[descriptor],"forks":table,"acceptance":{"live_abc":true,"own_verification":true,"restart":true,"every_height_compared":comparisons,"no_private_material_in_output":true,"no_terminal_reposting":true},"rows":rows,"locked":locked,"vested":vested,"rewards":reward_view,"final_progress":final_progress,"provenance":{"producer_commit":String::from_utf8(commit.stdout)?.trim(),"producer_binary_sha256":hex_string(&hash(&fs::read(std::env::current_exe()?)?)),"client_binary_sha256":hex_string(&hash(&fs::read(mcp::client_binary())?)),"gate_binary_sha256":hex_string(&pin),"generator_sha256":hex_string(&hash(include_bytes!("m3_live.rs"))),"mcp_support_sha256":hex_string(&hash(include_bytes!("support/mcp.rs"))),"support_sha256":hex_string(&hash(include_bytes!("../../boole-devnet-driver/tests/support/mod.rs"))),"wire_support_sha256":hex_string(&hash(include_bytes!("../../boole-devnet-driver/tests/support/m2.rs"))),"cargo_lock_sha256":hex_string(&hash(include_bytes!("../../../Cargo.lock"))),"client_pins":fixture_json("pins.json")?,"engine_profile":engine_profile(),"l1_genesis_sha256":hex_string(&hash(&fs::read(&l1p)?)),"l2_genesis_sha256":hex_string(&hash(&fs::read(&gp)?)),"producer_profile":if cfg!(debug_assertions){"debug"}else{"release"}}});
    rt.finish(true)?;
    result["cleanup"] = serde_json::from_slice(&fs::read(rt.path.join("cleanup.json"))?)?;
    write_json(&rt.path.join("qualification.json"), &result)?;
    if let Some(path) = std::env::var_os("BOOLE_DEVNET_M3C_RECORDING") {
        let path = PathBuf::from(path);
        ensure!(!path.exists(), "fresh recording output required");
        write_json(&path, &result)?;
    }
    Ok(())
}
