//! Native encoding qualification; no gate or consensus state is adopted here.
#[allow(dead_code)]
mod support;
use anyhow::{Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    user_tx::keccak,
};
use boole_devnet_archive::journal::{InputJournal, NetworkIdentity};
use boole_devnet_driver::l1::{L1Source, RpcL1};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

#[test]
fn native_headers_receipts_and_transactions_match_pinned_anvil() -> Result<()> {
    let Some(binary) = support::pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let mut runtime = support::Runtime::new("m4-input-auth")?;
    let mut genesis = support::m2::l1_genesis_value()?;
    let account = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
    let emitter = hex_string(&[0xee; 20]);
    let reverter = hex_string(&[0xdd; 20]);
    genesis["alloc"][account] = json!({"balance":"0x3635c9adc5dea00000"});
    genesis["alloc"][&emitter] = json!({"balance":"0x0","code":"0x60006000a000"});
    genesis["alloc"][&reverter] = json!({"balance":"0x0","code":"0x60006000fd"});
    let path = runtime.path.join("l1-genesis.json");
    fs::write(&path, serde_json::to_vec_pretty(&genesis)?)?;
    let rpc = runtime.anvil_with_genesis(&binary, &path)?;
    let mut source = RpcL1::new_m2("http://127.0.0.1:28545")?;
    let first = source.input_row(0)?;
    assert_eq!(first.verify()?.block.number, 0);
    let network = NetworkIdentity::new(
        &boole_derivation::m2::Config::devnet(),
        None,
        hex_string(&[1; 32]),
        30_000_000,
    )?;
    let mut journal = InputJournal::open(
        &runtime.path.join("inputs"),
        network.clone(),
        || Ok(first.clone()),
        false,
    )?;
    let mut txs = vec![];
    txs.push(rpc.call(
        "eth_sendTransaction",
        json!([{"from":account,"to":emitter,"gas":"0xf4240"}]),
    )?);
    txs.push(support::m2::post(
        &rpc,
        account,
        &[(2, b"public-header-proof-probe".to_vec())],
        false,
    )?);
    txs.push(rpc.call(
        "eth_sendTransaction",
        json!([{"from":account,"to":reverter,"gas":"0xf4240"}]),
    )?);
    rpc.call("evm_mine", json!([]))?;
    let second = source.input_row(1)?;
    let checked = second.verify()?;
    assert_eq!(checked.logs.len(), 1);
    assert_eq!(checked.logs[0].transaction_index, 1);
    assert_eq!(checked.logs[0].log_index, 1);
    assert_eq!(checked.canonical_receipts.len(), 3);
    for (i, (_, success)) in checked.canonical_receipts.iter().enumerate() {
        assert_eq!(*success, i != 2);
    }
    journal.append(&second)?;
    for (i, hash) in txs.iter().enumerate() {
        assert_eq!(
            journal.transaction_receipt(hash.as_str().context("tx hash")?)?,
            Some((1, i != 2))
        );
    }
    rpc.call("evm_mine", json!([]))?;
    let third = source.input_row(2)?;
    journal.append(&third)?;
    let rows = vec![first, second, third];
    for row in &rows {
        let h = row.block()?.number;
        let raw = rpc.call("eth_getBlockByNumber", json!([format!("0x{h:x}"), false]))?;
        assert_eq!(row.block_hash, raw["hash"]);
        assert_eq!(
            hex_string(&keccak(&hex_bytes(&row.header_rlp, "header")?)),
            row.block_hash
        );
    }
    let mut mutated = rows[1].clone();
    mutated.receipts.as_mut().context("receipts")?.remove(0);
    assert!(mutated.verify().is_err());
    let mut mutated = rows[1].clone();
    mutated
        .transactions
        .as_mut()
        .context("transactions")?
        .swap(0, 1);
    assert!(mutated.verify().is_err());
    drop(journal);
    let reopened = InputJournal::open(
        &runtime.path.join("inputs"),
        network,
        || unreachable!(),
        false,
    )?;
    assert_eq!(reopened.tip(), 2);
    for h in 0..=2 {
        reopened.get(h)?;
    }
    let finalized = source.finalized()?;
    runtime.finish(true)?;
    if let Some(output) = std::env::var_os("BOOLE_DEVNET_M4_INPUT_RECORDING") {
        let output = PathBuf::from(output);
        assert!(!output.exists(), "fresh recording path only");
        let producer = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()?;
        assert!(producer.status.success());
        let recording = json!({"schema":"boole-devnet-m4-input-auth-v0","scope":"native encoding probe; not finalized ingestion or an L2 round","rows":rows,"genesis":genesis,"finalized_at_probe_end":finalized,"acceptance":{"header_keccak":true,"receipt_roots":true,"transaction_roots":true,"global_log_index":true,"reverted_receipt":true,"restart_reverified":true,"strict_cleanup":true},"cleanup":serde_json::from_slice::<Value>(&fs::read(runtime.path.join("cleanup.json"))?)?,"provenance":{"producer_commit":String::from_utf8(producer.stdout)?.trim(),"producer_profile":"debug","generator_sha256":hex_string(&Sha256::digest(include_bytes!("input_auth_live.rs"))),"support_sha256":hex_string(&Sha256::digest(include_bytes!("support/mod.rs"))),"wire_support_sha256":hex_string(&Sha256::digest(include_bytes!("support/m2.rs"))),"cargo_lock_sha256":hex_string(&Sha256::digest(include_bytes!("../../../Cargo.lock"))),"producer_binary_sha256":hex_string(&Sha256::digest(fs::read(std::env::current_exe()?)?)),"anvil_sha256":hex_string(&Sha256::digest(fs::read(binary)?))}});
        fs::write(output, serde_json::to_vec_pretty(&recording)?)?;
    }
    Ok(())
}
