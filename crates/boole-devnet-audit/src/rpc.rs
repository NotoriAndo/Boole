//! Read-only RPC access to the audited L2. There is no Engine/JWT write handle.
use crate::Node;
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    ParentHeader,
};
use boole_devnet_driver::engine::Rpc;
use serde_json::{json, Value};
pub struct RpcNode {
    rpc: Rpc,
}
impl RpcNode {
    pub fn new(url: &str) -> Result<Self> {
        let rpc = Rpc::new(url)?;
        ensure!(
            rpc.call("eth_chainId", json!([]))?
                == format!("0x{:x}", boole_derivation::config::L2_CHAIN_ID),
            "audited L2 chain id"
        );
        Ok(Self { rpc })
    }
    fn block(&self, tag: Value) -> Result<(Value, Vec<u8>, Vec<String>)> {
        let block = self.rpc.call("eth_getBlockByNumber", json!([tag, false]))?;
        let header = boole_devnet_archive::input::header_from_rpc(&block)?;
        let mut txs = vec![];
        for hash in block["transactions"]
            .as_array()
            .context("L2 transaction hashes")?
        {
            let raw = self
                .rpc
                .call("eth_getRawTransactionByHash", json!([hash]))?;
            let raw = raw.as_str().context("L2 transaction bytes")?;
            ensure!(
                hex_string(&boole_derivation::user_tx::keccak(&hex_bytes(
                    raw,
                    "L2 transaction"
                )?)) == *hash,
                "L2 transaction hash"
            );
            txs.push(raw.to_owned());
        }
        boole_devnet_archive::input::verify_transactions(&header, &txs)?;
        Ok((block, header, txs))
    }
    fn parent(block: Value, txs: &[String]) -> Result<ParentHeader> {
        let mut header: ParentHeader = serde_json::from_value(block)?;
        header.l1_info = txs.first().cloned();
        header.m2_root_deposit = txs.get(1).cloned();
        Ok(header)
    }
}
pub fn receipt_bytes(value: &Value) -> Result<Vec<u8>> {
    if value["type"].is_null() || quantity(&value["type"])? != 0x7e {
        return boole_devnet_archive::input::receipt_from_rpc(value);
    }
    let mut base = value.clone();
    base["type"] = json!("0x0");
    let raw = boole_devnet_archive::input::receipt_from_rpc(&base)?;
    let children = boole_derivation::rlp::decode(&raw)
        .map_err(anyhow::Error::msg)?
        .children
        .context("OP receipt")?;
    let mut fields: Vec<_> = children
        .iter()
        .map(|f| raw[f.span.clone()].to_vec())
        .collect();
    if !value["depositNonce"].is_null() {
        fields.push(boole_derivation::rlp::bytes(
            &boole_devnet_archive::input::quantity_bytes(&value["depositNonce"])?,
        ));
    }
    if !value["depositReceiptVersion"].is_null() {
        ensure!(
            !value["depositNonce"].is_null() && value["depositReceiptVersion"] == "0x1",
            "OP receipt version"
        );
        fields.push(boole_derivation::rlp::bytes(&[1]));
    }
    Ok([vec![0x7e], boole_derivation::rlp::list(&fields)].concat())
}

fn quantity(value: &Value) -> Result<u64> {
    let b = boole_devnet_archive::input::quantity_bytes(value)?;
    ensure!(b.len() <= 8, "quantity width");
    Ok(b.iter().fold(0, |n, b| (n << 8) | u64::from(*b)))
}
impl Node for RpcNode {
    fn head(&mut self) -> Result<ParentHeader> {
        let (b, _, txs) = self.block(json!("latest"))?;
        Self::parent(b, &txs)
    }
    fn header(&mut self, h: u64) -> Result<ParentHeader> {
        let (b, _, txs) = self.block(json!(format!("0x{h:x}")))?;
        Self::parent(b, &txs)
    }
    fn transactions(&mut self, h: u64) -> Result<Vec<String>> {
        Ok(self.block(json!(format!("0x{h:x}")))?.2)
    }
    fn receipt_statuses(&mut self, h: u64) -> Result<Vec<bool>> {
        let (block, header, txs) = self.block(json!(format!("0x{h:x}")))?;
        let value = self
            .rpc
            .call("eth_getBlockReceipts", json!([block["hash"]]))?;
        let rows = value.as_array().context("L2 receipts")?;
        ensure!(rows.len() == txs.len(), "L2 receipt count");
        let raw = rows.iter().map(receipt_bytes).collect::<Result<Vec<_>>>()?;
        let fields = boole_derivation::rlp::decode(&header)
            .map_err(anyhow::Error::msg)?
            .children
            .context("L2 header")?;
        ensure!(
            boole_devnet_archive::input::receipts_root(&raw).as_slice() == fields[5].bytes,
            "L2 receipt trie root"
        );
        rows.iter()
            .enumerate()
            .map(|(i, r)| {
                ensure!(
                    quantity(&r["transactionIndex"])? == i as u64
                        && r["blockHash"] == block["hash"]
                        && r["transactionHash"] == block["transactions"][i],
                    "L2 receipt position"
                );
                ensure!(
                    r["status"] == "0x0" || r["status"] == "0x1",
                    "L2 receipt status"
                );
                Ok(r["status"] == "0x1")
            })
            .collect()
    }
    fn depositor_balance(&mut self, h: u64) -> Result<u128> {
        let v = self.rpc.call(
            "eth_getBalance",
            json!([boole_derivation::config::DEPOSITOR, format!("0x{h:x}")]),
        )?;
        let bytes = boole_devnet_archive::input::quantity_bytes(&v)?;
        ensure!(bytes.len() <= 16, "depositor balance width");
        Ok(bytes.iter().fold(0, |n, b| (n << 8) | u128::from(*b)))
    }
}
