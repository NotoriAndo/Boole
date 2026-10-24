//! Read-only, bounded RPC access to one authenticated L2 view per height.
use crate::{resource::ResourceStop, Node};
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
pub const RPC_CONNECT_TIME_MS: usize = 3_000;
pub const RPC_RESPONSE_TIME_MS: usize = 15_000;
pub const RPC_RESPONSE_MAX: usize = 16 * 1024 * 1024;
pub const RPC_ROWS_MAX: usize = 65536;
pub const RPC_NODES_MAX: usize = 65536;
pub const RPC_RECEIPT_NODES_MAX: usize = RPC_RESPONSE_MAX / 2 + 1;
pub const RPC_RECEIPT_MEMORY_MAX: usize = RPC_RESPONSE_MAX * 32;
pub const RPC_MEMORY_MAX: usize = RPC_RESPONSE_MAX * 8;
struct ValueBudget {
    nodes: Cell<usize>,
    memory: Cell<usize>,
    node_limit: usize,
    memory_limit: usize,
    stop: Cell<Option<ResourceStop>>,
}
impl ValueBudget {
    fn charge<E: serde::de::Error>(&self, bytes: usize) -> std::result::Result<(), E> {
        match self.memory.get().checked_sub(bytes) {
            Some(left) => {
                self.memory.set(left);
                Ok(())
            }
            None => {
                let stop = ResourceStop {
                    resource: "rpc-memory",
                    limit: self.memory_limit,
                };
                self.stop.set(Some(stop));
                Err(E::custom(stop))
            }
        }
    }
    fn exhausted<E: serde::de::Error>(&self, resource: &'static str, limit: usize) -> E {
        let stop = ResourceStop { resource, limit };
        self.stop.set(Some(stop));
        E::custom(stop)
    }
}
#[derive(Clone, Copy)]
struct BoundedValue<'a> {
    depth: usize,
    budget: &'a ValueBudget,
}
impl<'de> DeserializeSeed<'de> for BoundedValue<'_> {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> std::result::Result<Value, D::Error> {
        let remaining = self
            .budget
            .nodes
            .get()
            .checked_sub(1)
            .ok_or_else(|| self.budget.exhausted("rpc-nodes", self.budget.node_limit))?;
        self.budget.nodes.set(remaining);
        // Three Value slots cover vector growth's old and new allocations.
        self.budget
            .charge::<D::Error>(3 * std::mem::size_of::<Value>())?;
        if self.depth >= 64 {
            return Err(self.budget.exhausted("rpc-depth", 64));
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
        self.budget.charge::<E>(v.len().saturating_add(32))?;
        Ok(Value::String(v.into()))
    }
    fn visit_string<E: serde::de::Error>(self, v: String) -> std::result::Result<Value, E> {
        self.budget.charge::<E>(v.len().saturating_add(32))?;
        Ok(Value::String(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> std::result::Result<Value, A::Error> {
        self.budget.charge::<A::Error>(128)?;
        let mut rows = vec![];
        while let Some(value) = a.next_element_seed(Self {
            depth: self.depth + 1,
            budget: self.budget,
        })? {
            rows.push(value);
        }
        Ok(Value::Array(rows))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> std::result::Result<Value, A::Error> {
        // Conservatively cover a sparse BTree root and splits/keys per entry.
        self.budget.charge::<A::Error>(1024)?;
        let mut map = serde_json::Map::new();
        while let Some(key) = a.next_key::<String>()? {
            if map.len() >= 256 {
                return Err(self.budget.exhausted("rpc-fields", 256));
            }
            if map.contains_key(&key) {
                return Err(serde::de::Error::custom("RPC duplicate field"));
            }
            self.budget
                .charge::<A::Error>(key.len().saturating_add(288))?;
            let value = a.next_value_seed(Self {
                depth: self.depth + 1,
                budget: self.budget,
            })?;
            map.insert(key, value);
        }
        Ok(Value::Object(map))
    }
}
/// Preserve non-timeout transport errors; only an elapsed local deadline is
/// a resource stop. reqwest's body timeout is wrapped in an io::Error.
fn timeout_limit(error: &(dyn std::error::Error + 'static)) -> Option<usize> {
    if let Some(e) = error.downcast_ref::<reqwest::Error>() {
        if e.is_timeout() {
            return Some(if e.is_connect() {
                RPC_CONNECT_TIME_MS
            } else {
                RPC_RESPONSE_TIME_MS
            });
        }
    }
    if let Some(e) = error.downcast_ref::<std::io::Error>() {
        if e.kind() == std::io::ErrorKind::TimedOut {
            return Some(RPC_RESPONSE_TIME_MS);
        }
        if let Some(inner) = e.get_ref() {
            if let Some(limit) = timeout_limit(inner) {
                return Some(limit);
            }
        }
    }
    error.source().and_then(timeout_limit)
}
fn rpc_transport(error: impl Into<anyhow::Error>) -> anyhow::Error {
    let error = error.into();
    if let Some(limit) = timeout_limit(error.as_ref()) {
        ResourceStop {
            resource: "rpc-time",
            limit,
        }
        .into()
    } else {
        error
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
            .connect_timeout(Duration::from_millis(RPC_CONNECT_TIME_MS as u64))
            .timeout(Duration::from_millis(RPC_RESPONSE_TIME_MS as u64))
            .build()?;
        Ok(Self { url, client })
    }
    fn call(&self, method: &str, params: Value) -> Result<Value> {
        let response = self
            .client
            .post(self.url.clone())
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .map_err(rpc_transport)?
            .error_for_status()?;
        if response
            .content_length()
            .is_some_and(|n| n > RPC_RESPONSE_MAX as u64)
        {
            return Err(ResourceStop {
                resource: "rpc-bytes",
                limit: RPC_RESPONSE_MAX,
            }
            .into());
        }
        let mut bytes = vec![];
        response
            .take(RPC_RESPONSE_MAX as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(rpc_transport)?;
        if bytes.len() > RPC_RESPONSE_MAX {
            return Err(ResourceStop {
                resource: "rpc-bytes",
                limit: RPC_RESPONSE_MAX,
            }
            .into());
        }
        let mut decoder = serde_json::Deserializer::from_slice(&bytes);
        let (node_limit, memory_limit) = if method == "eth_getBlockReceipts" {
            (RPC_RECEIPT_NODES_MAX, RPC_RECEIPT_MEMORY_MAX)
        } else {
            (RPC_NODES_MAX, RPC_MEMORY_MAX)
        };
        let budget = ValueBudget {
            nodes: Cell::new(node_limit),
            memory: Cell::new(memory_limit),
            node_limit,
            memory_limit,
            stop: Cell::new(None),
        };
        let mut value = BoundedValue {
            depth: 0,
            budget: &budget,
        }
        .deserialize(&mut decoder)
        .map_err(|error| match budget.stop.get() {
            Some(stop) => anyhow::Error::new(stop),
            None => error.into(),
        })?;
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
        if hashes.len() > RPC_ROWS_MAX {
            return Err(ResourceStop {
                resource: "rpc-rows",
                limit: RPC_ROWS_MAX,
            }
            .into());
        }
        let mut txs = vec![];
        let mut bytes = 0usize;
        for hash in hashes {
            let value = self
                .rpc
                .call("eth_getRawTransactionByHash", json!([hash]))?;
            let raw = value.as_str().context("L2 transaction bytes")?;
            bytes = bytes
                .checked_add(raw.len())
                .filter(|n| *n <= RPC_RESPONSE_MAX)
                .ok_or(ResourceStop {
                    resource: "rpc-transaction-bytes",
                    limit: RPC_RESPONSE_MAX,
                })?;
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
    let raw = boole_devnet_archive::input::receipt_payload_from_rpc(value)?;
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

#[cfg(test)]
mod timeout_tests {
    use super::*;
    use std::{io::Write, net::TcpListener, thread};
    fn delayed(body: bool) -> Result<anyhow::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}", listener.local_addr()?);
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut header = vec![];
            let mut byte = [0];
            while !header.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).unwrap();
                header.push(byte[0]);
            }
            let text = String::from_utf8(header).unwrap();
            let len: usize = text
                .lines()
                .find_map(|l| {
                    l.to_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap();
            let mut bytes = vec![0; len];
            socket.read_exact(&mut bytes).unwrap();
            if body {
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n{{"
                )
                .unwrap();
                socket.flush().unwrap();
            }
            thread::sleep(Duration::from_secs(16));
        });
        let error = Rpc::new(&url)?
            .call("eth_chainId", json!([]))
            .expect_err("delayed response accepted");
        server.join().unwrap();
        Ok(error)
    }
    #[test]
    fn fix4_whole_response_header_timeout_is_typed() -> Result<()> {
        let e = delayed(false)?;
        assert!(
            e.is::<ResourceStop>(),
            "timeout became generic error: {e:#}"
        );
        assert_eq!(
            e.downcast_ref::<ResourceStop>().unwrap().resource,
            "rpc-time"
        );
        Ok(())
    }
    #[test]
    fn fix4_whole_response_body_timeout_is_typed() -> Result<()> {
        let e = delayed(true)?;
        assert!(
            e.is::<ResourceStop>(),
            "body timeout became generic error: {e:#}"
        );
        assert_eq!(
            e.downcast_ref::<ResourceStop>().unwrap().resource,
            "rpc-time"
        );
        Ok(())
    }
}

#[cfg(test)]
mod transport_controls {
    use super::*;
    use std::{io::Write, net::TcpListener, thread};
    #[test]
    fn fix4_refused_connection_is_not_a_resource_bound() -> Result<()> {
        let l = TcpListener::bind("127.0.0.1:0")?;
        let url = format!("http://{}", l.local_addr()?);
        drop(l);
        let e = Rpc::new(&url)?.call("eth_chainId", json!([])).unwrap_err();
        assert!(!e.is::<ResourceStop>());
        Ok(())
    }
    #[test]
    fn fix4_disconnect_and_malformed_http_keep_their_transport_class() -> Result<()> {
        for malformed in [false, true] {
            let l = TcpListener::bind("127.0.0.1:0")?;
            let url = format!("http://{}", l.local_addr()?);
            let server = thread::spawn(move || {
                let (mut s, _) = l.accept().unwrap();
                if malformed {
                    s.write_all(b"not HTTP\r\n\r\n").unwrap();
                }
            });
            let e = Rpc::new(&url)?.call("eth_chainId", json!([])).unwrap_err();
            server.join().unwrap();
            assert!(!e.is::<ResourceStop>(), "transport misclassified {e:#}");
        }
        Ok(())
    }
}
