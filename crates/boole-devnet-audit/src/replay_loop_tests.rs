use crate::Node;
use anyhow::{bail, Context, Result};
use boole_derivation::ParentHeader;
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    rlp,
    user_tx::keccak,
};
use boole_devnet_archive::input::InputRow;
use serde_json::{json, Value};
use std::process::Command;
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
        Self::with_status(200, handler)
    }
    fn with_status(status: u16, handler: impl Fn(Value) -> Value + Send + 'static) -> Result<Self> {
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
                        "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
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
    mode: String,
    fault_rpc: String,
    post_heads: usize,
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
            mode: String::new(),
            fault_rpc: String::new(),
            post_heads: 0,
            headers: BTreeMap::from([(0, head.clone())]),
            head,
            txs: BTreeMap::new(),
            attributes: Value::Null,
        }
    }
}
impl Engine for MemoryEngine {
    fn head(&mut self) -> Result<ParentHeader> {
        if self.head.number == 1 {
            self.post_heads += 1;
        }
        if self.mode == "engine-head-5xx" && self.post_heads >= 2 {
            boole_devnet_driver::engine::Rpc::new(&self.fault_rpc)?
                .call("eth_getBlockByNumber", json!(["latest", false]))?;
        }
        Ok(self.head.clone())
    }
    fn header(&mut self, h: u64) -> Result<ParentHeader> {
        if self.mode == "rebuild-header" && h > 0 {
            return Err(boole_devnet_driver::engine::TransientRpcError.into());
        }
        Ok(self.headers[&h].clone())
    }
    fn transactions(&mut self, h: u64) -> Result<Vec<String>> {
        Ok(self.txs[&h].clone())
    }
    fn call(&mut self, method: &str, p: Value) -> Result<Value> {
        if self.mode.starts_with("engine-step-5xx") && method == "engine_newPayloadV4" {
            return boole_devnet_driver::engine::Rpc::new(&self.fault_rpc)?.call(method, p);
        }
        if self.mode == "engine-invalid" && method == "engine_newPayloadV4" {
            return Ok(json!({"status":"INVALID","validationError":"fixture invalid"}));
        }
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
    fault_rpc: String,
}
impl Node for Audited {
    fn head(&mut self) -> Result<boole_derivation::ParentHeader> {
        self.header(1)
    }
    fn header(&mut self, h: u64) -> Result<boole_derivation::ParentHeader> {
        if h > 0 && self.mode == "audited-unavailable" {
            boole_devnet_driver::engine::Rpc::new(&self.fault_rpc)?
                .call("eth_getBlockByNumber", json!(["latest", false]))?;
        }
        if h > 0 && self.mode == "hash-difference" {
            let mut p = self.genesis.clone();
            p.number = h;
            p.hash = hex_string(&[99; 32]);
            return Ok(p);
        }
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
        if self.mode == "ledger-difference" {
            return Ok(vec![true]);
        }
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
    let fault_changed = input.changed.clone();
    let private_tamper = mode == "engine-step-5xx-tamper";
    let fault = Server::with_status(503, move |_| {
        if private_tamper {
            fault_changed.store(true, Ordering::Release);
        }
        json!({"error":"private fixture unavailable"})
    })?;
    engine.mode = mode.clone();
    engine.fault_rpc = fault.url.clone();
    let mut audited = Audited {
        genesis: engine.head.clone(),
        expected: engine.expected.clone(),
        mode: mode.clone(),
        fault_rpc: fault.url.clone(),
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
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let refused = format!("http://{}", listener.local_addr()?);
    drop(listener);
    let private_http = if mode == "private-rpc-refusal" {
        &refused
    } else {
        &server.url
    };
    replay_engine(
        &mut input,
        &mut audited,
        Config::devnet(),
        None,
        None,
        engine,
        private_http,
        &mut receipt,
    )?;
    drop(server);
    drop(fault);
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

fn fix5_loop_case(mode: &str, code: i32, class: &str) -> Result<Value> {
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
        "{mode}: {} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout)?;
    let value: Value = serde_json::from_str(
        text.lines()
            .find(|l| l.starts_with('{'))
            .context("child JSON")?,
    )?;
    if code == 2 {
        assert!(value["first_mismatch"].is_null());
        assert_eq!(value["resource_stop"]["height"], "1");
        assert_eq!(value["resource_stop"]["check"], class);
        assert_eq!(value["resource_stop"]["side"], "private-engine");
    } else {
        assert!(value["resource_stop"].is_null());
        assert_eq!(value["first_mismatch"], json!({"height":"1","class":class}));
    }
    Ok(value)
}
#[test]
fn fix5_private_step_5xx_through_driver_stall_is_local() -> Result<()> {
    fix5_loop_case("engine-step-5xx", 2, "fold")?;
    Ok(())
}
#[test]
fn fix5_private_head_5xx_is_local_at_prefix() -> Result<()> {
    fix5_loop_case("engine-head-5xx", 2, "prefix")?;
    Ok(())
}
#[test]
fn fix5_private_rpc_refusal_is_local_at_ledger() -> Result<()> {
    fix5_loop_case("private-rpc-refusal", 2, "ledger")?;
    Ok(())
}
#[test]
fn fix5_invalid_hash_and_ledger_controls_remain_mismatches() -> Result<()> {
    for (mode, class) in [
        ("engine-invalid", "fold"),
        ("hash-difference", "prefix"),
        ("ledger-difference", "ledger"),
        ("audited-unavailable", "prefix"),
        ("engine-step-5xx-tamper", "input"),
    ] {
        fix5_loop_case(mode, 1, class)?;
    }
    Ok(())
}

// Real Driver -> AnchoredHistory -> Supervisor rebuild erasure. No gate worker
// is needed: the owned Engine fails on the first rebuild read, before execution.
struct TapeSource(Vec<Value>);
impl L1Source for TapeSource {
    fn finalized(&mut self) -> Result<u64> {
        Ok(self.0.len() as u64)
    }
    fn block(&mut self, h: u64) -> Result<L1Block> {
        Ok(serde_json::from_value(
            self.0[h as usize - 1]["l1"].clone(),
        )?)
    }
    fn block_by_hash(&mut self, hash: &str) -> Result<L1Block> {
        Ok(serde_json::from_value(
            self.0
                .iter()
                .find(|r| r["l1"]["hash"] == hash)
                .context("tape origin")?["l1"]
                .clone(),
        )?)
    }
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>> {
        Ok(serde_json::from_value(
            self.0
                .iter()
                .find(|r| r["l1"]["hash"] == hash)
                .context("tape logs")?["logs"]
                .clone(),
        )?)
    }
}
#[test]
fn t81_private_real_m3_rebuild_erasure_and_input_precedence() -> Result<()> {
    use boole_devnet_archive::durability::PrivateTempDir;
    use boole_gate_supervisor::{
        codec::Codec,
        process::Envelope,
        supervisor::{Host, Mode},
    };
    let tape: Value =
        serde_json::from_str(include_str!("../../../fixtures/devnet-m3/round-v1.3.json"))?;
    let mut c: Config = serde_json::from_str(tape["config_json"].as_str().unwrap())?;
    let rows = tape["heights"].as_array().unwrap();
    let work = PrivateTempDir::new("private-rebuild")?;
    let configured_gate = std::env::var_os("BOOLE_DEVNET_GATE");
    let gate = configured_gate
        .clone()
        .map(std::path::PathBuf::from)
        .unwrap_or(
            std::env::current_exe()?
                .parent()
                .context("test binary directory")?
                .parent()
                .context("profile directory")?
                .join("boole-gate-supervisor"),
        );
    if !gate.try_exists()? {
        anyhow::ensure!(
            configured_gate.is_none(),
            "configured BOOLE_DEVNET_GATE is missing"
        );
        eprintln!("SKIP t81_private_real_m3_rebuild: build boole-gate-supervisor or set BOOLE_DEVNET_GATE to its binary");
        return Ok(());
    }
    let pin = boole_gate_supervisor::codec::digest(&std::fs::read(&gate)?);
    let descriptor = boole_gate_supervisor::stub::Descriptor::pinned(pin);
    c.m3.as_mut().unwrap().genesis_gate_rule_digest = descriptor.digest();
    let f = ForkTable::genesis(&c)?;
    let artifacts = work.path().join("artifacts");
    boole_gate_supervisor::supervisor::install_devnet_artifacts(&c, &artifacts)?;
    let sup = Supervisor::new(
        &c,
        &f,
        Mode::Verify,
        vec![boole_gate_supervisor::process::Worker {
            descriptor,
            binary: gate,
            binary_sha256: pin,
        }],
        Some(Codec::portable_stub()),
        &work.path().join("cache"),
        work.path(),
        &artifacts,
        Host::local()?,
        Envelope::default(),
    )?;
    let tracker = Tracker::default();
    let mut memory = MemoryEngine::new();
    memory.head = serde_json::from_value(rows[0]["parent"].clone())?;
    memory.headers.insert(0, memory.head.clone());
    memory.mode = "rebuild-header".into();
    let mut driver = Driver::with_m3(
        TapeSource(rows.clone()),
        Tracked::new(memory, tracker.clone()),
        c.clone(),
        30_000_000,
        sup,
        f.clone(),
    )?;
    for row in rows {
        let h = row["height"].as_u64().unwrap();
        driver.engine.inner_mut().expected = row["result"]["block_hash"].as_str().unwrap().into();
        tracker.begin(h, "fold");
        match driver.step(h) {
            Ok(_) => {}
            Err(error) => {
                assert!(
                    matches!(
                        error.kind,
                        boole_devnet_driver::driver::StallKind::VerdictPending { .. }
                    ),
                    "{error:?}"
                );
                let fault = tracker.unresolved().context("header failure erased")?;
                assert!(fault.origin.unwrap() < h);
                assert_eq!(fault.operation, "header");
                let mut receipt = crate::empty_receipt(&c, "journal");
                failed_step(
                    &mut receipt,
                    h,
                    "fold",
                    anyhow::Error::new(error.clone()),
                    &tracker,
                    Ok(()),
                );
                assert!(receipt["first_mismatch"].is_null(), "{receipt}");
                assert_eq!(receipt["resource_stop"]["check"], "verdict");
                assert_eq!(receipt["resource_stop"]["height"], h.to_string());
                assert_eq!(
                    receipt["resource_stop"]["origin_height"],
                    fault.origin.unwrap().to_string()
                );
                let mut tampered = crate::empty_receipt(&c, "journal");
                failed_step(
                    &mut tampered,
                    h,
                    "fold",
                    anyhow::Error::new(error),
                    &tracker,
                    Err(anyhow::anyhow!("owned input changed")),
                );
                assert_eq!(tampered["first_mismatch"]["class"], "input");
                assert!(tampered["resource_stop"].is_null());
                return Ok(());
            }
        }
    }
    bail!("no rebuild reached")
}

#[test]
fn t81_private_retry_clears_only_same_operation_and_step_resets() -> Result<()> {
    let tracker = Tracker::default();
    tracker.begin(26, "fold");
    let mut memory = MemoryEngine::new();
    memory.headers.insert(1, memory.head.clone());
    memory.headers.insert(2, memory.head.clone());
    memory.mode = "rebuild-header".into();
    let mut engine = Tracked::new(memory, tracker.clone());
    assert!(engine.header(1).is_err());
    engine.head()?;
    assert_eq!(tracker.unresolved().unwrap().origin, Some(1));
    assert!(engine.header(2).is_err());
    engine.inner_mut().mode.clear();
    engine.header(1)?;
    assert_eq!(tracker.unresolved().unwrap().origin, Some(2));
    engine.header(2)?;
    assert!(
        tracker.unresolved().is_none(),
        "recovered retry kept a fault"
    );
    engine.inner_mut().mode = "rebuild-header".into();
    assert!(engine.header(1).is_err());
    tracker.begin(27, "fold");
    assert!(tracker.unresolved().is_none(), "stale fault crossed a step");
    Ok(())
}

#[test]
fn t81_explicit_missing_rebuild_gate_dependency_is_a_failure() -> Result<()> {
    let work = boole_devnet_archive::durability::PrivateTempDir::new("missing-rebuild-gate")?;
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "replay::loop_tests::t81_private_real_m3_rebuild_erasure_and_input_precedence",
            "--nocapture",
        ])
        .env("BOOLE_DEVNET_GATE", work.path().join("absent-gate"))
        .output()?;
    assert!(
        !output.status.success(),
        "explicitly configured missing gate was silently skipped"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("configured BOOLE_DEVNET_GATE is missing"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
#[test]
fn t81_unset_and_unavailable_rebuild_gate_is_an_explicit_skip() -> Result<()> {
    let work = boole_devnet_archive::durability::PrivateTempDir::new("unset-rebuild-gate")?;
    let deps = work.path().join("profile/deps");
    std::fs::create_dir_all(&deps)?;
    let executable = deps.join("audit-test");
    std::fs::copy(std::env::current_exe()?, &executable)?;
    let output = Command::new(executable)
        .args([
            "--exact",
            "replay::loop_tests::t81_private_real_m3_rebuild_erasure_and_input_precedence",
            "--nocapture",
        ])
        .env_remove("BOOLE_DEVNET_GATE")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("SKIP t81_private_real_m3_rebuild"));
    Ok(())
}
