//! A small process-level replay RED/GREEN; I17 separately qualifies a full M3 round.
#[allow(dead_code)]
#[path = "../../boole-devnet-driver/tests/support/mod.rs"]
mod support;
use anyhow::{ensure, Context, Result};
use boole_devnet_archive::journal::NetworkIdentity;
use boole_devnet_driver::{
    driver::Driver,
    engine::Engine,
    l1::{JournalSource, L1Source, RpcL1},
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
#[test]
fn audit_replay_process_reproduces_a_fresh_private_engine() -> Result<()> {
    let Some(op) = support::pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let Some(anvil) = support::pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let mut runtime = support::Runtime::new("audit-replay-process")?;
    let l1gen = runtime.path.join("l1-genesis.json");
    fs::write(
        &l1gen,
        serde_json::to_vec(&support::m2::l1_genesis_value()?)?,
    )?;
    let rpc = runtime.anvil_with_genesis(&anvil, &l1gen)?;
    let genesis =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/devnet-m3/l2-genesis-v1.3.json");
    let node = runtime.node(&op, &genesis, "a", 0)?;
    let c = boole_derivation::m2::Config::devnet();
    let mut engine = node.engine()?;
    let gas = engine.head()?.gas_limit;
    let network = NetworkIdentity::new(&c, None, engine.header(0)?.hash, gas)?;
    let state = runtime.path.join("state");
    let source = JournalSource::open(
        RpcL1::new_m2("http://127.0.0.1:28545")?,
        &state,
        network,
        false,
    )?;
    let mut driver = Driver::with_config(source, engine, c.clone(), gas)?;
    for h in 1..=10 {
        rpc.call("evm_mine", json!([1_800_000_000 + 12 * h]))?;
        // Observe each unfinalized tip before finality expires its history.
        driver.l1.finalized()?;
    }
    let tip = driver.l1.finalized()?;
    ensure!(tip >= 2, "probe finality");
    for h in 1..=tip {
        driver.step(h)?;
    }
    let config = runtime.path.join("config.json");
    fs::write(&config, serde_json::to_vec(&c)?)?;
    let expected = driver.l1.journal.borrow().row(tip)?.block_hash;
    let binary = std::env::var_os("BOOLE_DEVNET_AUDIT_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_boole-devnet-audit")));
    let source_bytes = fs::read(state.join("input.ndjson"))?;
    let output = Command::new(binary)
        .env(
            "BOOLE_DEVNET_AUDIT_LOG_DIR",
            runtime.path.join("audit-logs"),
        )
        .arg("--journal")
        .arg(&state)
        .arg("--node-rpc")
        .arg(&node.http)
        .arg("--m2-config")
        .arg(config)
        .arg("--expected-l1-number")
        .arg(tip.to_string())
        .arg("--expected-l1-hash")
        .arg(expected)
        .args(["--mode", "replay"])
        .arg("--op-reth")
        .arg(op)
        .arg("--l2-genesis")
        .arg(genesis)
        .output()?;
    fs::write(runtime.path.join("audit-stdout.json"), &output.stdout)?;
    fs::write(runtime.path.join("audit-stderr.log"), &output.stderr)?;
    let receipt: Value = serde_json::from_slice(&output.stdout).context("audit process JSON")?;
    ensure!(
        output.status.success() && receipt["first_mismatch"].is_null(),
        "process replay unavailable: {receipt}"
    );
    let tip_decimal = tip.to_string();
    ensure!(
        receipt["checks"]["execution"] == "ok"
            && receipt["replayed_blocks"].as_str() == Some(tip_decimal.as_str()),
        "private execution was not checked"
    );
    ensure!(
        fs::read(state.join("input.ndjson"))? == source_bytes,
        "auditor wrote the input source"
    );
    runtime.finish(true)?;
    Ok(())
}
