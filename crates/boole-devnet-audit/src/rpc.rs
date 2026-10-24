//! Read-only, bounded RPC access to one authenticated L2 view per height.
use crate::Node;
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    ParentHeader,
};
use serde::{
    de::{DeserializeSeed, MapAccess, SeqAccess, Visitor},
    Deserializer,
};
use serde_json::{json, Value};
use std::{cell::Cell, fmt, io::Read, net::IpAddr, time::Duration};
pub const RPC_RESPONSE_MAX: usize = 16 * 1024 * 1024;
pub const RPC_ROWS_MAX: usize = 65536;
pub const RPC_NODES_MAX: usize = 65536;
#[derive(Clone, Copy)]
struct BoundedValue<'a> {
    depth: usize,
    remaining: &'a Cell<usize>,
}
impl<'de> DeserializeSeed<'de> for BoundedValue<'_> {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Value, D::Error> {
        let remaining = self
            .remaining
            .get()
            .checked_sub(1)
            .ok_or_else(|| serde::de::Error::custom("RPC response node cap"))?;
        self.remaining.set(remaining);
        if self.depth >= 64 {
            return Err(serde::de::Error::custom("RPC response depth cap"));
        }
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for BoundedValue<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded JSON")
    }
    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<Value, E> {
        Ok(json!(v))
    }
    fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Value, E> {
        Ok(json!(v))
    }
    fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Value, E> {
        Ok(json!(v))
    }
    fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Value, E> {
        serde_json::Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("RPC number"))
    }
    fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Value, E> {
        Ok(Value::String(v.into()))
    }
    fn visit_string<E: serde::de::Error>(self, v: String) -> std::result::Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> std::result::Result<Value, A::Error> {
        let mut rows = vec![];
        while let Some(value) = a.next_element_seed(Self {
            depth: self.depth + 1,
            remaining: self.remaining,
        })? {
            rows.push(value);
        }
        Ok(Value::Array(rows))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> std::result::Result<Value, A::Error> {
        let mut map = serde_json::Map::new();
        while let Some(key) = a.next_key::<String>()? {
            if map.len() >= 256 {
                return Err(serde::de::Error::custom("RPC response field cap"));
            }
            if map.contains_key(&key) {
                return Err(serde::de::Error::custom("RPC duplicate field"));
            }
            let value = a.next_value_seed(Self {
                depth: self.depth + 1,
                remaining: self.remaining,
            })?;
            map.insert(key, value);
        }
        Ok(Value::Object(map))
    }
}
struct Rpc {
    url: reqwest::Url,
    client: reqwest::blocking::Client,
}
impl Rpc {
    fn new(url: &str) -> Result<Self> {
        let url = reqwest::Url::parse(url)?;
        let host = url.host_str().unwrap_or("").trim_matches(['[', ']']);
        let ip: IpAddr = host.parse().context("audited RPC literal loopback host")?;
        ensure!(
            ip.is_loopback()
                && ["http", "https"].contains(&url.scheme())
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "audited RPC requires loopback without credentials/query/fragment"
        );
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self { url, client })
    }
    fn call(&self, method: &str, params: Value) -> Result<Value> {
        let response = self
            .client
            .post(self.url.clone())
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()?
            .error_for_status()?;
        ensure!(
            response
                .content_length()
                .is_none_or(|n| n <= RPC_RESPONSE_MAX as u64),
            "RPC response byte cap"
        );
        let mut bytes = vec![];
        response
            .take(RPC_RESPONSE_MAX as u64 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= RPC_RESPONSE_MAX, "RPC response byte cap");
        let mut decoder = serde_json::Deserializer::from_slice(&bytes);
        let remaining = Cell::new(RPC_NODES_MAX);
        let mut value = BoundedValue {
            depth: 0,
            remaining: &remaining,
        }
        .deserialize(&mut decoder)?;
        decoder.end()?;
        ensure!(
            value["jsonrpc"] == "2.0" && value["id"] == 1 && value.get("error").is_none(),
            "audited RPC envelope"
        );
        value
            .as_object_mut()
            .and_then(|object| object.remove("result"))
            .context("audited RPC missing result")
    }
}
#[derive(Clone)]
struct Observation {
    height: u64,
    block: Value,
    header: Vec<u8>,
    txs: Vec<String>,
}
pub struct RpcNode {
    rpc: Rpc,
    observation: Option<Observation>,
}
impl RpcNode {
    pub fn new(url: &str) -> Result<Self> {
        let rpc = Rpc::new(url)?;
        ensure!(
            rpc.call("eth_chainId", json!([]))?
                == format!("0x{:x}", boole_derivation::config::L2_CHAIN_ID),
            "audited L2 chain id"
        );
        Ok(Self {
            rpc,
            observation: None,
        })
    }
    fn block(&mut self, tag: Value) -> Result<(Value, Vec<u8>, Vec<String>)> {
        if let Some(view) = &self.observation {
            if tag == format!("0x{:x}", view.height) {
                return Ok((view.block.clone(), view.header.clone(), view.txs.clone()));
            }
        }
        let block = self.rpc.call("eth_getBlockByNumber", json!([tag, false]))?;
        let header = boole_devnet_archive::input::header_from_rpc(&block)?;
        let hashes = block["transactions"]
            .as_array()
            .context("L2 transaction hashes")?;
        ensure!(hashes.len() <= RPC_ROWS_MAX, "RPC response row cap");
        let mut txs = vec![];
        let mut bytes = 0usize;
        for hash in hashes {
            let value = self
                .rpc
                .call("eth_getRawTransactionByHash", json!([hash]))?;
            let raw = value.as_str().context("L2 transaction bytes")?;
            bytes = bytes
                .checked_add(raw.len())
                .context("RPC transaction byte cap")?;
            ensure!(bytes <= RPC_RESPONSE_MAX, "RPC transaction byte cap");
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
        self.observation = Some(Observation {
            height: quantity(&block["number"])?,
            block: block.clone(),
            header: header.clone(),
            txs: txs.clone(),
        });
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
        let (block, _, _) = self.block(json!(format!("0x{h:x}")))?;
        let v = self.rpc.call(
            "eth_getBalance",
            json!([boole_derivation::config::DEPOSITOR, {"blockHash":block["hash"],"requireCanonical":true}]),
        )?;
        let bytes = boole_devnet_archive::input::quantity_bytes(&v)?;
        ensure!(bytes.len() <= 16, "depositor balance width");
        Ok(bytes.iter().fold(0, |n, b| (n << 8) | u128::from(*b)))
    }
}
