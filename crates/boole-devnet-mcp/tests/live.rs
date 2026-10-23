//! Opt-in closed-local MCP stdio -> L1 -> driver -> pinned op-reth round.
#[path = "../../boole-devnet-driver/tests/support/mod.rs"]
#[allow(dead_code)]
mod support;
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    m2::*,
    L1Block,
};
use boole_devnet_driver::{
    driver::{Driver, StepOutcome},
    engine::{Rpc, RpcEngine},
    l1::{L1Source, RpcL1},
};
use serde_json::{json, Value};
use std::fs;
#[path = "support/mcp.rs"]
mod mcp;
use mcp::Server;
use support::{m2::*, *};

type LiveDriver = Driver<RpcL1, RpcEngine>;
fn mine(
    l1: &Rpc,
    driver: &mut LiveDriver,
    server: Option<&mut Server>,
    rows: &mut Vec<Value>,
) -> Result<Value> {
    let h = quantity(&l1.call("eth_blockNumber", json!([]))?)? + 1;
    l1.call("evm_mine", json!([1_800_000_000 + 12 * h]))?;
    let mut source = RpcL1::new_m2("http://127.0.0.1:28545")?;
    let f = source.finalized()?;
    while driver.m2_state().height() < f {
        let n = driver.m2_state().height() + 1;
        ensure!(
            matches!(driver.step(n)?, StepOutcome::Derived(_)),
            "driver finality wait"
        );
    }
    let expected = hex_string(&driver.m2_state().root);
    let status = if let Some(server) = server {
        let status = server.tool("boole.devnet_status", json!({}))?;
        ensure!(
            status["fold_height"] == f
                && status["m2_root"] == expected
                && if f == 0 {
                    status["l2_m2_root_match"].is_null()
                } else {
                    status["l2_m2_root_match"] == true
                },
            "MCP/driver root mismatch at latest {h}: {status}"
        );
        status
    } else {
        Value::Null
    };
    rows.push(json!({"latest_height":h,"finalized_height":f,"driver_m2_root":expected,"client_status":status}));
    Ok(rows.last().context("row")?.clone())
}
fn last_tx(view: &Value, stage: &str) -> bool {
    view["transactions"]
        .as_array()
        .is_some_and(|xs| xs.iter().any(|x| x["stage"] == stage))
}

#[test]
fn mcp_live_round_checking_cancel_offline_expiry_and_equal_roots() -> Result<()> {
    let Some(op) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let mut rt = Runtime::new("mcp1-round")?;
    let l1_genesis = rt.path.join("l1-genesis.json");
    fs::write(
        &l1_genesis,
        serde_json::to_vec_pretty(&l1_genesis_value()?)?,
    )?;
    let l1 = rt.anvil_with_genesis(&anvil, &l1_genesis)?;
    let accounts = l1.call("eth_accounts", json!([]))?;
    let user = accounts[0].as_str().context("account")?;
    let c = config();
    let l2_genesis = rt.path.join("l2-genesis.json");
    let mut l2 = genesis()?;
    l2["gasLimit"] = json!("0x2625a00"); // 40M, deliberately differs from the old fixed value.
    fs::write(&l2_genesis, serde_json::to_vec_pretty(&l2)?)?;
    let node = rt.node(&op, &l2_genesis, "miner-replica", 0)?;
    let mut driver = Driver::with_config(
        RpcL1::new_m2("http://127.0.0.1:28545")?,
        node.engine()?,
        c.clone(),
        boole_devnet_mcp::client::l2_gas_limit("http://127.0.0.1:29545")?,
    )?;
    driver.set_evidence(&rt.path.join("driver.jsonl"))?;
    let state = rt.path.join("miner-state");
    let mut server = Server::start(&rt.path.join("mcp-first"), &c, user, &state, &[])?;
    let package: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m2/package-codec-v1.1.json"
    ))?;
    let carried = hex_bytes(
        &format!("0x{}", package["carried_hex"].as_str().context("package")?),
        "package",
    )?;
    let required = vec![c.pkg_table[0]];
    let root =
        boole_derivation::m2::pool_leaf(c.chain_id, 1, &required, 1, 1_048_576, &carried, &[9; 32]);
    post(&l1, user, &[(8, pool_at(&c, 1, root, 1, 3, 1))], false)?;
    let (raw, _, _) = frame(&rt.path)?;
    let export = rt.path.join("export.raw");
    let mut rows = vec![];
    loop {
        let h = quantity(&l1.call("eth_blockNumber", json!([]))?)? + 1;
        if h == 5 {
            let mut body = problem_encoded(1, 1, 0, [9; 32], &[], &carried, (1, 1_048_576));
            body[10] = 1;
            body[11] = 1;
            body.splice(12..12, required[0]);
            post(&l1, user, &[(9, body)], false)?;
        }
        mine(&l1, &mut driver, Some(&mut server), &mut rows)?;
        if driver.m2_state().height() >= 5 {
            break;
        }
        ensure!(h < 24, "problem did not finalize");
    }
    let p = driver.m2_state().rounds[&(1, 1)]
        .problem
        .context("problem")?;
    let problem = server.tool("boole.devnet_problem", json!({"slot":1}))?;
    ensure!(
        problem["problem_id"] == hex_string(&p) && problem["decoded_len"] == 1_048_576,
        "MCP decoded problem"
    );
    ensure!(
        problem["package_utf8"]
            .as_str()
            .is_some_and(|s| s.as_bytes() == vec![b'a'; 1_048_576]),
        "decoded package bytes"
    );
    let arguments =
        json!({"problem_id":hex_string(&p),"payee":hex_string(&[6;20]),"export_file":export});
    // A configured deterministic reject must not create a COMMIT or submission.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let hook = rt.path.join("reject-hook");
        let bytes = b"#!/bin/sh\nexit 10\n";
        fs::write(&hook, bytes)?;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o700))?;
        let extra = vec![
            "--precheck".into(),
            hook.display().to_string(),
            "--precheck-sha256".into(),
            hex_string(&hash(bytes)),
        ];
        let reject_state = rt.path.join("reject-state");
        let mut rejected =
            Server::start(&rt.path.join("mcp-reject"), &c, user, &reject_state, &extra)?;
        let nonce = l1.call("eth_getTransactionCount", json!([user, "pending"]))?;
        let out = rejected.rpc(
            "tools/call",
            json!({"name":"boole.devnet_commit","arguments":arguments}),
        )?;
        ensure!(
            out["result"]["isError"] == true && out.to_string().contains("deterministic reject"),
            "pre-check refusal"
        );
        ensure!(
            nonce == l1.call("eth_getTransactionCount", json!([user, "pending"]))?,
            "reject posted a transaction"
        );
        ensure!(
            !fs::read_dir(&reject_state)?.any(
                |e| e.is_ok_and(|e| e.file_name().to_string_lossy().starts_with("submission-"))
            ),
            "reject persisted a submission"
        );
        rejected.stop()?;
    }
    let primary = server.tool("boole.devnet_commit", arguments.clone())?;
    let primary_id = primary["submission_id"]
        .as_str()
        .context("submission id")?
        .to_owned();
    ensure!(
        primary["local_check"] == "not_run",
        "local check disclosure"
    );
    server.posted(&primary_id, "commit")?;
    let mut primary_final = Value::Null;
    for _ in 0..45 {
        mine(&l1, &mut driver, Some(&mut server), &mut rows)?;
        let view = server.submission(&primary_id)?;
        if view["status"] == "CHECKING" {
            primary_final = view;
            break;
        }
    }
    ensure!(!primary_final.is_null(), "MCP did not complete the reveal");
    let primary_hash: Hash = boole_devnet_mcp::client::fixed(&primary_id)?;
    let candidate = &driver.m2_state().candidates[&(p, primary_hash)];
    ensure!(candidate.status() == Status::Checking, "driver CHECKING");
    let open = candidate
        .live()
        .and_then(|c| c.open.as_ref())
        .context("completed OPEN")?;
    ensure!(
        open.n == 14 && open.raw_len == raw.len() as u64,
        "14-part export layout"
    );
    let cancel = server.tool("boole.devnet_commit", arguments.clone())?;
    let cancel_id = cancel["submission_id"]
        .as_str()
        .context("cancel id")?
        .to_owned();
    server.posted(&cancel_id, "commit")?;
    server.tool("boole.devnet_cancel", json!({"submission_id":cancel_id}))?;
    // This independent miner goes offline immediately after its COMMIT, before
    // finality can automatically trigger OPEN. The online miner still cancels.
    let expiry_state = rt.path.join("expiry-state");
    let mut expiry_server = Server::start(
        &rt.path.join("mcp-expiry-first"),
        &c,
        user,
        &expiry_state,
        &[],
    )?;
    let expire = expiry_server.tool("boole.devnet_commit", arguments)?;
    let expire_id = expire["submission_id"]
        .as_str()
        .context("expire id")?
        .to_owned();
    ensure!(
        primary_id != cancel_id && cancel_id != expire_id,
        "fresh submission identities"
    );
    expiry_server.posted(&expire_id, "commit")?;
    ensure!(
        !last_tx(&expiry_server.submission(&expire_id)?, "open"),
        "offline candidate already opening"
    );
    expiry_server.stop()?;
    for _ in 0..40 {
        mine(&l1, &mut driver, Some(&mut server), &mut rows)?;
        if server.submission(&cancel_id)?["status"] == "CANCELLED" {
            break;
        }
    }
    ensure!(
        server.submission(&cancel_id)?["status"] == "CANCELLED",
        "cancel finality"
    );
    let expiry_hash: Hash = boole_devnet_mcp::client::fixed(&expire_id)?;
    let hc = driver.m2_state().candidates[&(p, expiry_hash)]
        .live()
        .context("expiry candidate")?
        .h_c;
    server.stop()?;
    while driver.m2_state().height() < hc + c.w + 1 {
        mine(&l1, &mut driver, None, &mut rows)?;
        ensure!(rows.len() < 200, "expiry finality timeout");
    }
    let mut restarted = Server::start(&rt.path.join("mcp-restarted"), &c, user, &state, &[])?;
    let status = restarted.tool("boole.devnet_status", json!({}))?;
    ensure!(
        status["m2_root"] == hex_string(&driver.m2_state().root),
        "restart root equality"
    );
    ensure!(
        restarted.submission(&primary_id)?["status"] == "CHECKING",
        "primary survives restart"
    );
    ensure!(
        restarted.submission(&cancel_id)?["status"] == "CANCELLED",
        "CANCEL finalized"
    );
    let mut expiry_restarted = Server::start(
        &rt.path.join("mcp-expiry-restarted"),
        &c,
        user,
        &expiry_state,
        &[],
    )?;
    let expiry_status = expiry_restarted.tool("boole.devnet_status", json!({}))?;
    ensure!(
        expiry_status["m2_root"] == status["m2_root"],
        "independent expiry miner root"
    );
    let expired = expiry_restarted.submission(&expire_id)?;
    ensure!(
        expired["status"] == "EXPIRED" && !last_tx(&expired, "open"),
        "offline expiry without late OPEN"
    );
    // The same owned records must retain final states after consensus tombstones
    // disappear, including when a fresh process refolds all heights at once.
    let before_prune_comparisons = rows
        .iter()
        .filter(|r| !r["client_status"].is_null())
        .count();
    ensure!(
        before_prune_comparisons == 52,
        "online comparison count before pruning: {before_prune_comparisons}"
    );
    let close_height = driver.m2_state().problems[&p].h_v + c.slot_table[1].solve_deadline + 1;
    let prune_height = close_height + c.w + 1;
    let before_prune_height = driver.m2_state().height();
    while driver.m2_state().height() <= prune_height {
        mine(&l1, &mut driver, Some(&mut restarted), &mut rows)?;
        ensure!(rows.len() < 300, "prune finality timeout");
    }
    ensure!(
        !driver.m2_state().candidates.contains_key(&(p, expiry_hash)),
        "expiry tombstone not pruned"
    );
    ensure!(
        restarted.submission(&cancel_id)?["status"] == "CANCELLED",
        "cancel after pruning"
    );
    ensure!(
        expiry_restarted.submission(&expire_id)?["status"] == "EXPIRED",
        "expiry after pruning"
    );
    restarted.stop()?;
    expiry_restarted.stop()?;
    let mut restarted = Server::start(&rt.path.join("mcp-pruned-restart"), &c, user, &state, &[])?;
    let mut expiry_restarted = Server::start(
        &rt.path.join("mcp-expiry-pruned-restart"),
        &c,
        user,
        &expiry_state,
        &[],
    )?;
    let after_prune = restarted.tool("boole.devnet_status", json!({}))?;
    let expiry_after_prune = expiry_restarted.tool("boole.devnet_status", json!({}))?;
    ensure!(
        after_prune["m2_root"] == hex_string(&driver.m2_state().root)
            && expiry_after_prune["m2_root"] == after_prune["m2_root"],
        "post-prune restart roots"
    );
    ensure!(
        restarted.submission(&cancel_id)?["status"] == "CANCELLED"
            && expiry_restarted.submission(&expire_id)?["status"] == "EXPIRED",
        "post-prune restart final states"
    );
    let comparisons = rows
        .iter()
        .filter(|r| !r["client_status"].is_null())
        .count();
    ensure!(
        comparisons == 52 + (prune_height + 1 - before_prune_height) as usize,
        "online comparison count after pruning"
    );
    let mut salts = vec![];
    let mut secrets = vec![];
    for id in [&primary_id, &cancel_id, &expire_id] {
        let path = if *id == expire_id {
            &expiry_state
        } else {
            &state
        }
        .join(format!("submission-{id}.json"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                fs::metadata(&path)?.permissions().mode() & 0o777 == 0o600,
                "0600 submission"
            );
        }
        let local: Value = serde_json::from_slice(&fs::read(path)?)?;
        let body: Vec<u8> = serde_json::from_value(local["prepared"]["body"].clone())?;
        let secret: Hash = serde_json::from_value(local["prepared"]["cancel_secret"].clone())?;
        ensure!(
            body[150] == 1 && body[151..183] == required[0] && body[184] == 1,
            "required package and lane-1 format"
        );
        salts.push(body[53..85].to_vec());
        secrets.push(secret);
        let progress = if *id == expire_id {
            expiry_restarted.submission(id)?
        } else {
            restarted.submission(id)?
        };
        for private in [&body[53..85], secret.as_slice()] {
            let needle = hex_string(private);
            let array = serde_json::to_string(private)?;
            ensure!(
                !progress.to_string().contains(&needle[2..])
                    && !progress.to_string().contains(&array),
                "private material in progress response"
            );
            for entry in fs::read_dir(&rt.path)? {
                let path = entry?.path();
                if !path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("mcp-"))
                {
                    continue;
                }
                for file in ["stdout.log", "stderr.log"] {
                    let text = fs::read_to_string(path.join(file))?;
                    ensure!(
                        !text.contains(&needle[2..]) && !text.contains(&array),
                        "private material in MCP stdout/stderr"
                    );
                }
            }
        }
    }
    ensure!(
        salts[0] != salts[1]
            && salts[1] != salts[2]
            && secrets[0] != secrets[1]
            && secrets[1] != secrets[2],
        "independent fresh salts/tokens"
    );
    // Every real OPEN obeyed finality/minimum delay and the conservative W3 cap.
    let mut posting = vec![];
    let mut posted_items = vec![];
    let mut l1_inputs = vec![];
    for h in 1..=quantity(&l1.call("eth_blockNumber", json!([]))?)? {
        let block: L1Block = serde_json::from_value(
            l1.call("eth_getBlockByNumber", json!([format!("0x{h:x}"), false]))?,
        )?;
        let logs = RpcL1::new_m2("http://127.0.0.1:28545")?.inbox_logs(&block.hash)?;
        l1_inputs.push(json!({"l1":block,"logs":logs}));
        for log in logs {
            let bytes = hex_bytes(&log.data, "log")?;
            if let Ok((kind, body)) = boole_derivation::inbox::item(&bytes) {
                posting.push(json!({"height":h,"kind":kind,"body_bytes":body.len()}));
                posted_items.push((h, kind, body.to_vec()));
            }
        }
    }
    let primary_commit = driver.m2_state().candidates[&(p, primary_hash)]
        .live()
        .context("primary")?
        .h_c;
    for (h, kind, body) in &posted_items {
        if *kind == 3 {
            ensure!(
                *h >= primary_commit + c.d
                    && *h <= primary_commit + c.w - 14u64.div_ceil(c.p_blk) - 2,
                "OPEN schedule"
            );
            ensure!(body[53 + 33..53 + 53] == [6; 20], "payee binding");
        }
        if *kind != 7 {
            ensure!(
                !body.windows(32).any(|w| w == secrets[1] || w == secrets[2]),
                "cancel secret outside CANCEL"
            );
        }
    }
    ensure!(
        posted_items.iter().filter(|(_, k, _)| *k == 4).count() == 14 && posted_items.len() == 21,
        "exact 14 PARTs and 21 posted items"
    );
    restarted.stop()?;
    expiry_restarted.stop()?;
    let provenance = json!({"generator":"crates/boole-devnet-mcp/tests/live.rs","generator_sha256":hex_string(&hash(include_bytes!("live.rs"))),"mcp_support_sha256":hex_string(&hash(include_bytes!("support/mcp.rs"))),"client_binary_sha256":hex_string(&hash(&fs::read(env!("CARGO_BIN_EXE_boole-devnet-mcp"))?)),"client_pins":fixture_json("pins.json")?,"engine_profile":engine_profile(),"l1_genesis_sha256":hex_string(&hash(&fs::read(&l1_genesis)?)),"l2_genesis_sha256":hex_string(&hash(&fs::read(&l2_genesis)?)),"l1_genesis_hash":RpcL1::new_m2("http://127.0.0.1:28545")?.genesis_hash()?});
    let result = json!({"schema":"boole-devnet-mcp-round-v0","config":c,"config_digest":hex_string(&c.digest()),"slot_table_digest":hex_string(&c.slot_table_digest()),"package_codec":1,"package_raw_len":1_048_576,"export_raw_len":raw.len(),"primary":primary_final,"cancel_status":"CANCELLED","expiry_status":"EXPIRED","restart_m2_root":after_prune["m2_root"],"pre_prune_restart_m2_root":status["m2_root"],"online_comparisons":comparisons,"restart_comparisons":4,"posted_items":posted_items.len(),"prune_height":prune_height,"l2_gas_limit":40_000_000,"required_packages":required,"export_format":1,"driver_m2_root":hex_string(&driver.m2_state().root),"rows":rows,"postings":posting,"l1_inputs":l1_inputs,"provenance":provenance,"strict_cleanup":"pending"});
    rt.finish(true)?;
    if let Some(path) = std::env::var_os("BOOLE_DEVNET_MCP_RECORDING") {
        let mut result = result;
        result["strict_cleanup"] = json!("PASS");
        fs::write(path, serde_json::to_vec_pretty(&result)?)?;
    }
    Ok(())
}
