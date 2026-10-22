//! I4: distinguish transaction admission from actual mining at the EIP-7825 boundary.
#[allow(dead_code)]
mod support;
use anyhow::{Context, Result};
use boole_derivation::attributes::hex_string;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use support::*;
#[test]
fn i4_anvil_gas_limit_admission_and_mining() -> Result<()> {
    let Some(anvil) = pinned("BOOLE_DEVNET_ANVIL", "anvil")? else {
        return Ok(());
    };
    let genesis =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/devnet-m2/l1-genesis-v1.json");
    let mut observations = vec![];
    for gas in [16_777_216u64, 16_777_217] {
        let mut rt = Runtime::new(&format!("i4-gas-{gas}"))?;
        let rpc = rt.anvil_with_genesis(&anvil, &genesis)?;
        let accounts = rpc.call("eth_accounts", json!([]))?;
        let user = accounts[0].as_str().context("account")?;
        let mut observed = json!({"requested_gas":gas,"block_gas_limit":30_000_000});
        match rpc.call(
            "eth_sendTransaction",
            json!([{"from":user,"to":user,"gas":format!("0x{gas:x}")}]),
        ) {
            Ok(tx) => {
                observed["admitted"] = json!(true);
                observed["transaction_hash"] = tx.clone();
                let mined = rpc.call("evm_mine", json!([1_800_000_012]));
                observed["mine_rpc_succeeded"] = json!(mined.is_ok());
                if let Err(error) = mined {
                    observed["mine_error"] = json!(format!("{error:#}"));
                }
                let receipt = rpc.call("eth_getTransactionReceipt", json!([tx]))?;
                observed["mined"] = json!(!receipt.is_null());
                if !receipt.is_null() {
                    assert_eq!(receipt["status"], "0x1");
                    assert_eq!(quantity(&receipt["gasUsed"])?, 21000);
                }
                observed["receipt"] = receipt;
            }
            Err(error) => {
                observed["admitted"] = json!(false);
                observed["admission_error"] = json!(format!("{error:#}"));
            }
        }
        fs::write(
            rt.path.join("observation.json"),
            serde_json::to_vec_pretty(&observed)?,
        )?;
        rt.finish(true)?;
        observations.push(observed);
    }
    let value = json!({"schema":"boole-devnet-m2-l1-gas-limits-v1","provenance":{"anvil":fixture_json("pins.json")?["anvil"],"genesis_sha256":hex_string(&Sha256::digest(fs::read(&genesis)?))},"acceptance":{"pinned_binary":true,"clean_shutdown":true},"observations":observations});
    if let Some(path) = std::env::var_os("BOOLE_DEVNET_GAS_RECORDING") {
        fs::write(path, serde_json::to_vec_pretty(&value)?)?;
    }
    eprintln!(
        "I4 gas-limit observations: {}",
        serde_json::to_string(&value["observations"])?
    );
    Ok(())
}
