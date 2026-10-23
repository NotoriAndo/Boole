//! D3/D16: finality needs only a number, including pre-London L1 genesis.
use boole_devnet_driver::{
    engine::Rpc,
    l1::{L1Source, RpcL1},
};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
};

fn source(include_finalized: bool) -> (RpcL1, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut replies = vec![("eth_chainId", json!("0x7a69"))];
        if include_finalized {
            replies.push(("eth_getBlockByNumber", json!({"number":"0x0"})));
        }
        replies.extend([
            (
                "eth_getBlockByNumber",
                json!({"number":"0x0","hash":format!("0x{}","ab".repeat(32))}),
            ),
            (
                "eth_getBlockByNumber",
                json!({"number":"0x0","hash":"0xab"}),
            ),
        ]);
        for (method, result) in replies {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(request["method"], method);
            if method == "eth_getBlockByNumber" {
                assert_eq!(
                    request["params"],
                    json!([
                        if result.get("hash").is_some() {
                            "0x0"
                        } else {
                            "finalized"
                        },
                        false
                    ])
                );
            }
            let body =
                serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"result":result})).unwrap();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).unwrap();
            stream.write_all(&body).unwrap();
        }
    });
    (RpcL1::new(&url).unwrap(), server)
}

#[test]
fn finalized_genesis_needs_only_number() {
    let (mut l1, server) = source(true);
    let actual = l1.finalized();
    let genesis = l1.genesis_hash();
    let invalid = l1.genesis_hash();
    server.join().unwrap();
    assert_eq!(actual.unwrap(), 0);
    assert_eq!(genesis.unwrap(), format!("0x{}", "ab".repeat(32)));
    assert!(invalid.is_err());
}

#[test]
fn genesis_hash_accepts_a_pre_london_header_and_refuses_an_invalid_hash() {
    let (mut l1, server) = source(false);
    let genesis = l1.genesis_hash();
    let invalid = l1.genesis_hash();
    server.join().unwrap();
    assert_eq!(genesis.unwrap(), format!("0x{}", "ab".repeat(32)));
    assert!(invalid.is_err());
}

#[test]
fn literal_loopback_urls_only() {
    for url in ["http://127.0.0.1:1", "http://[::1]:1"] {
        assert!(Rpc::new(url).is_ok());
    }
    for url in [
        "http://localhost:1",
        "http://192.0.2.1:1",
        "http://127.0.0.1.example:1",
        "http://user:pass@127.0.0.1:1",
        "http://127.0.0.1:1/?redirect=x",
        "ftp://127.0.0.1:1",
    ] {
        assert!(Rpc::new(url).is_err(), "{url}");
    }
}

#[test]
fn capture_receipts_are_pinned_to_the_header_hash_across_a_number_reorg() {
    use boole_derivation::{
        attributes::{hex_bytes, hex_string},
        rlp,
        user_tx::keccak,
    };
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../fixtures/devnet-m4/input-auth-v0.json"
    ))
    .unwrap();
    let bytes = hex_bytes(fixture["rows"][0]["header_rlp"].as_str().unwrap(), "header").unwrap();
    let decoded = rlp::decode(&bytes).unwrap();
    let nodes = decoded.children.unwrap();
    let mut fields: Vec<_> = nodes.iter().map(|n| n.bytes.to_vec()).collect();
    while fields.len() < 21 {
        fields.push(match fields.len() {
            15 => vec![1],
            16 | 19 | 20 => vec![0; 32],
            17 | 18 => vec![],
            _ => unreachable!(),
        });
    }
    fields[8] = vec![1];
    fields[11] = 1_790_000_012u64.to_be_bytes().to_vec();
    while fields[11].first() == Some(&0) {
        fields[11].remove(0);
    }
    let raw = rlp::list(&fields.iter().map(|f| rlp::bytes(f)).collect::<Vec<_>>());
    let mut header = json!({"hash":hex_string(&keccak(&raw)),"transactions":[]});
    for (i, key) in [
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
    ]
    .iter()
    .enumerate()
    .take(fields.len())
    {
        let mut value = hex_string(&fields[i]);
        if [7, 8, 9, 10, 11, 15, 17, 18].contains(&i) {
            let digits = value[2..].trim_start_matches('0');
            value = format!("0x{}", if digits.is_empty() { "0" } else { digits });
        }
        header[*key] = json!(value);
    }
    boole_devnet_archive::input::header_from_rpc(&header).unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let hash_pinned = Arc::new(AtomicBool::new(false));
    let observed = hash_pinned.clone();
    let worker = thread::spawn(move || {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut n = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                    n = v.trim().parse().unwrap();
                }
            }
            let mut bytes = vec![0; n];
            reader.read_exact(&mut bytes).unwrap();
            let req: Value = serde_json::from_slice(&bytes).unwrap();
            let result = match req["method"].as_str().unwrap() {
                "eth_chainId" => json!("0x7a69"),
                "eth_getBlockByNumber" => header.clone(),
                "eth_getBlockReceipts" => {
                    if req["params"][0] == header["hash"] {
                        observed.store(true, Ordering::SeqCst);
                        json!([])
                    } else {
                        json!([{"status":"0x1","type":"0x2","cumulativeGasUsed":"0x5208","logsBloom":hex_string(&[0;256]),"logs":[]}])
                    }
                }
                other => panic!("unexpected RPC {other}"),
            };
            let body = json!({"jsonrpc":"2.0","id":req["id"],"result":result}).to_string();
            write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    let mut source = RpcL1::new(&url).unwrap();
    let result = source.input_row(1);
    worker.join().unwrap();
    assert!(
        result.is_ok(),
        "number-selected receipts crossed a reorg: {result:?}"
    );
    assert!(hash_pinned.load(Ordering::SeqCst));
}
