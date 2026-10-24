use anyhow::{Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    rlp,
    user_tx::keccak,
};
use boole_devnet_archive::input::InputRow;
use boole_devnet_audit::{rpc::RpcNode, Node};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
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
#[test]
fn l2_header_transactions_and_receipts_keep_one_authenticated_height_view() -> Result<()> {
    let rows = rows()?;
    let a = rpc_block(&rows[0], 0)?;
    let b = rpc_block(&rows[1], 0)?;
    let a_hash = a["hash"].clone();
    let reads = Arc::new(AtomicUsize::new(0));
    let seen = reads.clone();
    let transactions = rows[1].transactions.clone().unwrap();
    let server =
        Server::start(move |q| {
            reply(match q["method"].as_str().unwrap() {
                "eth_chainId" => chain(),
                "eth_getBlockByNumber" => {
                    if seen.fetch_add(1, Ordering::Relaxed) == 0 {
                        a.clone()
                    } else {
                        b.clone()
                    }
                }
                "eth_getRawTransactionByHash" => json!(transactions
                    .iter()
                    .find(|t| json!(hex_string(&keccak(&hex_bytes(t, "tx").unwrap())))
                        == q["params"][0])
                    .unwrap()),
                "eth_getBlockReceipts" => {
                    assert_eq!(q["params"][0], a_hash);
                    json!([])
                }
                m => panic!("unexpected {m}"),
            })
        })?;
    let mut node = RpcNode::new(&server.url)?;
    let header = node.header(0)?;
    assert_eq!(header.number, 0);
    assert!(
        node.transactions(0)?.is_empty(),
        "transactions came from another block at the same height"
    );
    assert!(node.receipt_statuses(0)?.is_empty());
    assert_eq!(reads.load(Ordering::Relaxed), 1);
    Ok(())
}
#[test]
fn l8_audited_rpc_rejects_response_bytes_above_the_cap() -> Result<()> {
    let server = Server::start(|_| {
        let mut v = reply(chain());
        v["extra"] = json!("x".repeat(16 * 1024 * 1024));
        v
    })?;
    let error = RpcNode::new(&server.url)
        .err()
        .expect("unbounded audited RPC response accepted");
    assert!(
        format!("{error:#}").contains("RPC response byte cap"),
        "{error:#}"
    );
    Ok(())
}
#[test]
fn l8_audited_rpc_rejects_response_row_counts_above_the_cap() -> Result<()> {
    let server = Server::start(|_| {
        let mut v = reply(chain());
        v["rows"] = json!(vec![Value::Null; 65537]);
        v
    })?;
    let error = RpcNode::new(&server.url)
        .err()
        .expect("unbounded audited RPC rows accepted");
    assert!(
        format!("{error:#}").contains("RPC response node cap"),
        "{error:#}"
    );
    Ok(())
}

#[test]
fn l6_replay_startup_failure_has_its_own_check_class() -> Result<()> {
    use boole_derivation::{
        attributes::derive_with_m2,
        m2::{Config, State},
        ParentHeader,
    };
    use boole_devnet_archive::{
        durability::PrivateTempDir,
        input::receipts_root,
        journal::{InputJournal, NetworkIdentity},
    };
    use std::{collections::BTreeMap, process::Command};
    let input = rows()?;
    let config = Config::devnet();
    let root = PrivateTempDir::new("audit-replay-class")?;
    let first = rpc_block(&input[0], 0)?;
    let mut parent: ParentHeader = serde_json::from_value(first.clone())?;
    let network = NetworkIdentity::new(&config, None, parent.hash.clone(), parent.gas_limit)?;
    let mut journal = InputJournal::open(
        &root.path().join("journal"),
        network,
        || Ok(input[0].clone()),
        false,
    )?;
    for r in &input[1..] {
        journal.append(r)?;
    }
    drop(journal);
    let mut state = State::new(&config, parent.gas_limit)?;
    let mut blocks = vec![first];
    let mut raw_txs = BTreeMap::new();
    let mut receipts_by_hash =
        BTreeMap::from([(blocks[0]["hash"].as_str().unwrap().to_owned(), json!([]))]);
    for (h, r) in input.iter().enumerate().skip(1) {
        let verified = r.verify()?;
        let (derived, next) =
            derive_with_m2(&state, &config, &verified.block, &verified.logs, &parent)?;
        let timestamp = u64::from_str_radix(&derived.attributes.timestamp[2..], 16)?;
        let gas_limit = u64::from_str_radix(&derived.attributes.gas_limit[2..], 16)?;
        let txs = derived.attributes.transactions;
        let mut receipts = vec![];
        for (i, tx) in txs.iter().enumerate() {
            let hash = hex_string(&keccak(&hex_bytes(tx, "system")?));
            raw_txs.insert(hash.clone(), tx.clone());
            receipts.push(json!({"type":"0x7e","status":"0x1","cumulativeGasUsed":format!("0x{:x}",i+1),"logsBloom":hex_string(&[0;256]),"logs":[],"depositNonce":format!("0x{i:x}"),"depositReceiptVersion":"0x1","transactionIndex":format!("0x{i:x}"),"transactionHash":hash}));
        }
        let raw_receipts = receipts
            .iter()
            .map(boole_devnet_audit::rpc::receipt_bytes)
            .collect::<Result<Vec<_>>>()?;
        let raw = hex_bytes(&input[1].header_rlp, "header")?;
        let d = rlp::decode(&raw).map_err(anyhow::Error::msg)?;
        let mut fields: Vec<_> = d
            .children
            .unwrap()
            .iter()
            .map(|f| raw[f.span.clone()].to_vec())
            .collect();
        fields[0] = rlp::bytes(&hex_bytes(&parent.hash, "parent")?);
        fields[4] = rlp::bytes(&receipts_root(
            &txs.iter()
                .map(|t| Ok(hex_bytes(t, "tx")?))
                .collect::<Result<Vec<_>>>()?,
        ));
        fields[5] = rlp::bytes(&receipts_root(&raw_receipts));
        fields[6] = rlp::bytes(&[0; 256]);
        fields[8] = rlp::uint(h as u64);
        fields[9] = rlp::uint(gas_limit);
        fields[10] = rlp::uint(txs.len() as u64);
        fields[11] = rlp::uint(timestamp);
        let raw = rlp::list(&fields);
        let block = rpc_block(
            &InputRow {
                schema_version: 1,
                block_hash: hex_string(&keccak(&raw)),
                header_rlp: hex_string(&raw),
                receipts: None,
                transactions: Some(txs.clone()),
            },
            h as u64,
        )?;
        for r in &mut receipts {
            r["blockHash"] = block["hash"].clone();
        }
        receipts_by_hash.insert(block["hash"].as_str().unwrap().to_owned(), json!(receipts));
        parent = serde_json::from_value(block.clone())?;
        parent.l1_info = txs.first().cloned();
        parent.m2_root_deposit = txs.get(1).cloned();
        state = next;
        blocks.push(block);
    }
    let server = Server::start(move |q| {
        reply(match q["method"].as_str().unwrap() {
            "eth_chainId" => chain(),
            "eth_getBlockByNumber" => {
                let tag = q["params"][0].as_str().unwrap();
                blocks[if tag == "latest" {
                    blocks.len() - 1
                } else {
                    usize::from_str_radix(&tag[2..], 16).unwrap()
                }]
                .clone()
            }
            "eth_getRawTransactionByHash" => json!(raw_txs[q["params"][0].as_str().unwrap()]),
            "eth_getBlockReceipts" => receipts_by_hash[q["params"][0].as_str().unwrap()].clone(),
            "eth_getBalance" => json!("0x0"),
            m => panic!("unexpected {m}"),
        })
    })?;
    assert_eq!(
        RpcNode::new(&server.url)?
            .head()
            .context("valid audited head fixture")?
            .number,
        2
    );
    let config_path = root.path().join("config.json");
    std::fs::write(&config_path, serde_json::to_vec(&config)?)?;
    let run = |mode: &str| -> Result<std::process::Output> {
        Ok(Command::new(env!("CARGO_BIN_EXE_boole-devnet-audit"))
            .arg("--journal")
            .arg(root.path().join("journal"))
            .args(["--node-rpc", &server.url, "--m2-config"])
            .arg(&config_path)
            .args([
                "--expected-l1-number",
                "2",
                "--expected-l1-hash",
                &input[2].block_hash,
                "--mode",
                mode,
                "--op-reth",
            ])
            .arg(root.path().join("missing-op-reth"))
            .arg("--l2-genesis")
            .arg(&config_path)
            .output()?)
    };
    let control = run("pure")?;
    assert!(
        control.status.success(),
        "{} {}",
        String::from_utf8_lossy(&control.stdout),
        String::from_utf8_lossy(&control.stderr)
    );
    let output = run("replay")?;
    assert!(!output.status.success());
    let receipt: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(
        receipt["first_mismatch"],
        json!({"height":"0","class":"replay-setup"})
    );
    assert_eq!(receipt["checks"]["replay-setup"], "mismatch");
    assert_eq!(receipt["checks"]["prefix"], "ok");
    assert_eq!(receipt["checks"]["execution"], "unavailable");
    Ok(())
}

#[test]
fn l8_exact_response_node_and_byte_caps_are_accepted() -> Result<()> {
    use boole_devnet_audit::rpc::{RPC_NODES_MAX, RPC_RESPONSE_MAX};
    let mut rows = reply(chain());
    rows["rows"] = json!(vec![Value::Null; RPC_NODES_MAX - 5]);
    let server = Server::start(move |_| rows.clone())?;
    RpcNode::new(&server.url)?;
    drop(server);
    let mut bytes = reply(chain());
    bytes["padding"] = json!("");
    let overhead = serde_json::to_vec(&bytes)?.len();
    bytes["padding"] = json!("x".repeat(RPC_RESPONSE_MAX - overhead));
    assert_eq!(serde_json::to_vec(&bytes)?.len(), RPC_RESPONSE_MAX);
    let server = Server::start(move |_| bytes.clone())?;
    RpcNode::new(&server.url)?;
    Ok(())
}

#[test]
fn f3_nested_arrays_share_one_response_node_budget() -> Result<()> {
    let mut response = reply(chain());
    response["extra"] = json!([vec![Value::Null; 40000], vec![Value::Null; 40000]]);
    assert!(serde_json::to_vec(&response)?.len() < boole_devnet_audit::rpc::RPC_RESPONSE_MAX);
    let server = Server::start(move |_| response.clone())?;
    let error = RpcNode::new(&server.url)
        .err()
        .context("nested response exceeding total node budget was accepted")?;
    assert!(format!("{error:#}").contains("node cap"), "{error:#}");
    Ok(())
}

#[test]
fn f6_balance_is_bound_to_the_authenticated_block_hash() -> Result<()> {
    let block = rpc_block(&rows()?[0], 0)?;
    let expected = block["hash"].clone();
    let reads = Arc::new(AtomicUsize::new(0));
    let observed = reads.clone();
    let server = Server::start(move |q| {
        reply(match q["method"].as_str().unwrap() {
            "eth_chainId" => chain(),
            "eth_getBlockByNumber" => {
                observed.fetch_add(1, Ordering::Relaxed);
                block.clone()
            }
            "eth_getBalance" => {
                if q["params"][1] == json!({"blockHash":expected,"requireCanonical":true}) {
                    json!("0x0")
                } else {
                    json!("0x1")
                }
            }
            m => panic!("unexpected {m}"),
        })
    })?;
    let mut node = RpcNode::new(&server.url)?;
    node.header(0)?;
    assert_eq!(
        node.depositor_balance(0)?,
        0,
        "balance came from a different block at the same number"
    );
    assert_eq!(
        reads.load(Ordering::Relaxed),
        1,
        "balance refetched a moving header"
    );
    Ok(())
}
