use crate::Node;
use anyhow::{Context, Result};
use boole_derivation::ParentHeader;
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    rlp,
    user_tx::keccak,
};
use boole_devnet_archive::input::InputRow;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn start(handler: impl Fn(Value) -> Value + Send + 'static) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}", listener.local_addr()?);
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread = thread::spawn(move || {
            while !s.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(_) => break,
                };
                let _ = (|| -> Result<()> {
                    stream.set_nonblocking(false)?;
                    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
                    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
                    let mut head = vec![];
                    let mut one = [0];
                    while !head.ends_with(b"\r\n\r\n") {
                        anyhow::ensure!(head.len() < 65536, "header cap");
                        stream.read_exact(&mut one)?;
                        head.push(one[0]);
                    }
                    let head = std::str::from_utf8(&head)?;
                    let len: usize = head
                        .lines()
                        .find_map(|l| {
                            l.split_once(':')
                                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                .map(|(_, v)| v.trim().parse())
                        })
                        .context("length")??;
                    anyhow::ensure!(len < 1024 * 1024, "request cap");
                    let mut body = vec![0; len];
                    stream.read_exact(&mut body)?;
                    let bytes = serde_json::to_vec(&handler(serde_json::from_slice(&body)?))?;
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        bytes.len()
                    )?;
                    stream.write_all(&bytes)?;
                    Ok(())
                })();
            }
        });
        Ok(Self {
            url,
            stop,
            thread: Some(thread),
        })
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
    }
}
fn reply(result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"result":result})
}
fn chain() -> Value {
    json!(format!("0x{:x}", boole_derivation::config::L2_CHAIN_ID))
}
fn rows() -> Result<Vec<InputRow>> {
    let v: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m4/input-auth-v0.json"
    ))?;
    Ok(serde_json::from_value(v["rows"].clone())?)
}
fn rpc_block(row: &InputRow, number: u64) -> Result<Value> {
    let raw = hex_bytes(&row.header_rlp, "header")?;
    let decoded = rlp::decode(&raw).map_err(anyhow::Error::msg)?;
    let fields = decoded.children.unwrap();
    let mut fields: Vec<_> = fields
        .iter()
        .map(|f| raw[f.span.clone()].to_vec())
        .collect();
    fields[8] = rlp::uint(number);
    // Native v1 parent metadata, matching the recorded M1 parent profile.
    fields[12] = rlp::bytes(&hex_bytes(
        "0x0100000032000000060000000000000000",
        "native extraData",
    )?);
    let raw = rlp::list(&fields);
    let decoded = rlp::decode(&raw).map_err(anyhow::Error::msg)?;
    let fields = decoded.children.unwrap();
    let names = [
        "parentHash",
        "sha3Uncles",
        "miner",
        "stateRoot",
        "transactionsRoot",
        "receiptsRoot",
        "logsBloom",
        "difficulty",
        "number",
        "gasLimit",
        "gasUsed",
        "timestamp",
        "extraData",
        "mixHash",
        "nonce",
        "baseFeePerGas",
        "withdrawalsRoot",
        "blobGasUsed",
        "excessBlobGas",
        "parentBeaconBlockRoot",
        "requestsHash",
    ];
    let mut out = json!({"hash":hex_string(&keccak(&raw)),"transactions":row.transactions.as_ref().unwrap().iter().map(|t|hex_string(&keccak(&hex_bytes(t,"tx").unwrap()))).collect::<Vec<_>>()});
    for (i, f) in fields.iter().enumerate() {
        let value = if [7, 8, 9, 10, 11, 15, 17, 18].contains(&i) {
            let n = f.bytes.iter().fold(0u128, |n, b| (n << 8) | u128::from(*b));
            format!("0x{n:x}")
        } else {
            hex_string(f.bytes)
        };
        out[names[i]] = json!(value);
    }
    Ok(out)
}

use super::*;
use boole_gate_supervisor::rebuild::History;
#[derive(Clone)]
struct MemoryEngine {
    expected: String,
    head: ParentHeader,
    headers: BTreeMap<u64, ParentHeader>,
    txs: BTreeMap<u64, Vec<String>>,
    attributes: Value,
}
impl MemoryEngine {
    fn new() -> Self {
        let rows: Value = serde_json::from_str(include_str!(
            "../../../fixtures/devnet-m1/s1b-recorded-main.json"
        ))
        .unwrap();
        let mut head: ParentHeader = serde_json::from_value(rows[0]["parent"].clone()).unwrap();
        head.timestamp = 1_790_000_000;
        Self {
            expected: String::new(),
            headers: BTreeMap::from([(0, head.clone())]),
            head,
            txs: BTreeMap::new(),
            attributes: Value::Null,
        }
    }
}
impl Engine for MemoryEngine {
    fn head(&mut self) -> Result<ParentHeader> {
        Ok(self.head.clone())
    }
    fn header(&mut self, h: u64) -> Result<ParentHeader> {
        Ok(self.headers[&h].clone())
    }
    fn transactions(&mut self, h: u64) -> Result<Vec<String>> {
        Ok(self.txs[&h].clone())
    }
    fn call(&mut self, method: &str, p: Value) -> Result<Value> {
        Ok(match method {
            "engine_forkchoiceUpdatedV3" => {
                if p[1].is_null() {
                    let h = self.head.number + 1;
                    let txs: Vec<String> =
                        serde_json::from_value(self.attributes["transactions"].clone())?;
                    self.head.parent_hash = Some(self.head.hash.clone());
                    self.head.hash = p[0]["headBlockHash"].as_str().unwrap().into();
                    self.head.number = h;
                    self.head.timestamp = u64::from_str_radix(
                        self.attributes["timestamp"]
                            .as_str()
                            .unwrap()
                            .trim_start_matches("0x"),
                        16,
                    )?;
                    self.head.l1_info = Some(txs[0].clone());
                    self.head.m2_root_deposit = Some(txs[1].clone());
                    self.headers.insert(h, self.head.clone());
                    self.txs.insert(h, txs);
                } else {
                    self.attributes = p[1].clone();
                }
                json!({"payloadStatus":{"status":"VALID"},"payloadId":"0x1"})
            }
            "engine_getPayloadV5" => {
                json!({"executionPayload":{"transactions":self.attributes["transactions"],"blockNumber":format!("0x{:x}",self.head.number+1),"blockHash":self.expected,"parentHash":self.head.hash,"timestamp":self.attributes["timestamp"],"gasLimit":self.attributes["gasLimit"]}})
            }
            "engine_newPayloadV4" => {
                self.attributes = p[0].clone();
                json!({"status":"VALID"})
            }
            _ => bail!("unexpected engine method"),
        })
    }
}

struct Input {
    rows: Vec<InputRow>,
    changed: Arc<AtomicBool>,
}
impl History for Input {
    fn origin(&mut self, h: u64) -> Result<(L1Block, Vec<InboxLog>)> {
        let r = self.rows[h as usize].verify()?;
        Ok((r.block, r.logs))
    }
}
impl Inputs for Input {
    fn tip(&self) -> u64 {
        1
    }
    fn row(&mut self, h: u64) -> Result<InputRow> {
        Ok(self.rows[h as usize].clone())
    }
    fn check(&self) -> Result<()> {
        ensure!(!self.changed.load(Ordering::Acquire), "own input changed");
        Ok(())
    }
    fn label(&self) -> &'static str {
        "journal"
    }
}
struct Audited {
    genesis: boole_derivation::ParentHeader,
    expected: String,
    mode: String,
}
impl Node for Audited {
    fn head(&mut self) -> Result<boole_derivation::ParentHeader> {
        self.header(1)
    }
    fn header(&mut self, h: u64) -> Result<boole_derivation::ParentHeader> {
        if h > 0 && self.mode == "audited-header" {
            return Err(crate::resource::ResourceStop {
                resource: "rpc-nodes",
                limit: 65536,
            }
            .into());
        }
        let mut p = self.genesis.clone();
        if h > 0 {
            p.number = h;
            p.hash = self.expected.clone();
        }
        Ok(p)
    }
    fn transactions(&mut self, _: u64) -> Result<Vec<String>> {
        Ok(vec![])
    }
    fn receipt_statuses(&mut self, _: u64) -> Result<Vec<bool>> {
        if self.mode == "audited-receipts" {
            return Err(crate::resource::ResourceStop {
                resource: "rpc-nodes",
                limit: 65536,
            }
            .into());
        }
        Ok(vec![])
    }
    fn depositor_balance(&mut self, _: u64) -> Result<u128> {
        if self.mode == "audited-balance" {
            return Err(crate::resource::ResourceStop {
                resource: "rpc-nodes",
                limit: 65536,
            }
            .into());
        }
        Ok(0)
    }
}
#[test]
fn fixture_child() -> Result<()> {
    let Ok(mode) = std::env::var("BOOLE_AUDIT_FIX4_CHILD") else {
        return Ok(());
    };
    let changed = Arc::new(AtomicBool::new(false));
    let mark = changed.clone();
    let mut input = Input {
        rows: rows()?,
        changed,
    };
    // Empty authenticated RPC block suffices to reach the real receipts read;
    // this fixture qualifies error propagation, not EVM execution.
    let block = rpc_block(&empty_rpc_row()?, 1)?;
    let mut engine = MemoryEngine::new();
    engine.expected = block["hash"].as_str().unwrap().into();
    let mut audited = Audited {
        genesis: engine.head.clone(),
        expected: engine.expected.clone(),
        mode: mode.clone(),
    };
    let fail = mode == "resource" || mode == "tamper";
    let tamper = mode == "tamper";
    let new_limit = mode == "private-new";
    let balance_limit = mode == "private-balance";
    let server = Server::start(move |request| match request["method"].as_str().unwrap() {
        "eth_chainId" => {
            if new_limit {
                reply(json!("x".repeat(crate::rpc::RPC_RESPONSE_MAX)))
            } else {
                reply(chain())
            }
        }
        "eth_getBlockByNumber" => reply(block.clone()),
        "eth_getBlockReceipts" => {
            if tamper {
                mark.store(true, Ordering::Release);
            }
            if fail {
                reply(json!("x".repeat(crate::rpc::RPC_RESPONSE_MAX)))
            } else {
                reply(json!([]))
            }
        }
        "eth_getBalance" => {
            if balance_limit {
                reply(json!("x".repeat(crate::rpc::RPC_RESPONSE_MAX)))
            } else {
                reply(json!("0x0"))
            }
        }
        method => panic!("unexpected fixture RPC {method}"),
    })?;
    let mut receipt = crate::empty_receipt(&Config::devnet(), "journal");
    replay_engine(
        &mut input,
        &mut audited,
        Config::devnet(),
        None,
        None,
        engine,
        &server.url,
        &mut receipt,
    )?;
    drop(server);
    println!();
    crate::emit_receipt(&receipt);
    Ok(())
}
#[test]
fn fix4_real_replay_loop_http_resource_stop_and_cli_status() -> Result<()> {
    for (mode, code) in [
        ("resource", 2),
        ("tamper", 1),
        ("success", 0),
        ("private-new", 2),
        ("private-balance", 2),
        ("audited-header", 2),
        ("audited-receipts", 2),
        ("audited-balance", 2),
    ] {
        let output = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "replay::loop_tests::fixture_child",
                "--nocapture",
            ])
            .env("BOOLE_AUDIT_FIX4_CHILD", mode)
            .output()?;
        assert_eq!(
            output.status.code(),
            Some(code),
            "mode {mode}: {} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout)?;
        let value: Value = serde_json::from_str(
            text.lines()
                .find(|l| l.starts_with('{'))
                .context("CLI JSON")?,
        )?;
        match mode {
            "resource" => {
                assert!(value["first_mismatch"].is_null());
                assert_eq!(value["resource_stop"]["height"], "1");
                assert_eq!(value["resource_stop"]["check"], "ledger");
                assert_eq!(value["resource_stop"]["resource"], "rpc-bytes");
                assert_eq!(value["checks"]["execution"], "resource-stop");
            }
            "tamper" => {
                assert!(value["resource_stop"].is_null());
                assert_eq!(
                    value["first_mismatch"],
                    json!({"height":"1","class":"input"})
                );
            }
            "private-new" | "private-balance" | "audited-header" | "audited-receipts"
            | "audited-balance" => {
                assert!(value["first_mismatch"].is_null());
                assert_eq!(value["resource_stop"]["height"], "1");
                assert_eq!(
                    value["resource_stop"]["check"],
                    if mode == "audited-header" {
                        "prefix"
                    } else {
                        "ledger"
                    }
                );
            }
            _ => {
                assert!(crate::succeeded(&value));
                assert_eq!(value["replayed_blocks"], "1");
                assert_eq!(value["checks"]["execution"], "ok");
            }
        }
    }
    Ok(())
}

fn empty_rpc_row() -> Result<InputRow> {
    let mut row = rows()?[1].clone();
    let raw = hex_bytes(&row.header_rlp, "header")?;
    let decoded = rlp::decode(&raw).map_err(anyhow::Error::msg)?;
    let mut fields: Vec<_> = decoded
        .children
        .unwrap()
        .iter()
        .map(|f| raw[f.span.clone()].to_vec())
        .collect();
    fields[6] = rlp::bytes(&[0; 256]);
    fields[4] = rlp::bytes(&boole_devnet_archive::input::receipts_root(&[]));
    fields[5] = fields[4].clone();
    let raw = rlp::list(&fields);
    row.header_rlp = hex_string(&raw);
    row.block_hash = hex_string(&keccak(&raw));
    row.transactions = Some(vec![]);
    row.receipts = Some(vec![]);
    Ok(row)
}
