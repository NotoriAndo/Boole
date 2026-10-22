use boole_devnet_driver::engine::{retry_rpc, Rpc};
use boole_devnet_mcp::client::{l2_gas_limit, Poster, RevertedReceipt, RpcPoster};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    thread,
};

fn serve(replies: Vec<(&'static str, u16, Value)>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        for (method, status, result) in replies {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse::<usize>().unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(request["method"], method);
            let body = if status == 200 {
                json!({"jsonrpc":"2.0","id":1,"result":result}).to_string()
            } else {
                "temporary outage".into()
            };
            write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    (url, worker)
}
#[test]
fn actual_http_transients_retry_but_invalid_rpc_results_do_not() {
    let (url, worker) = serve(vec![
        ("eth_blockNumber", 503, Value::Null),
        ("eth_blockNumber", 429, Value::Null),
        ("eth_blockNumber", 200, json!("0x2")),
    ]);
    let rpc = Rpc::new(&url).unwrap();
    assert_eq!(
        retry_rpc(|| rpc.call("eth_blockNumber", json!([]))).unwrap(),
        "0x2"
    );
    worker.join().unwrap();
    let (url, worker) = serve(vec![("eth_blockNumber", 400, Value::Null)]);
    let rpc = Rpc::new(&url).unwrap();
    let mut attempts = 0;
    assert!(retry_rpc(|| {
        attempts += 1;
        rpc.call("eth_blockNumber", json!([]))
    })
    .is_err());
    assert_eq!(attempts, 1, "format/application failures must not retry");
    worker.join().unwrap();
}
#[test]
fn receipts_distinguish_canonical_reverts_from_orphaned_reverts() {
    let hash = format!("0x{}", "ab".repeat(32));
    let (url, worker) = serve(vec![
        ("eth_chainId", 200, json!("0x7a69")),
        (
            "eth_accounts",
            200,
            json!([format!("0x{}", "01".repeat(20))]),
        ),
        (
            "eth_getTransactionReceipt",
            200,
            json!({"status":"0x0","blockNumber":"0xa","blockHash":hash}),
        ),
        ("eth_getBlockByNumber", 200, json!({"hash":hash})),
        (
            "eth_getTransactionReceipt",
            200,
            json!({"status":"0x0","blockNumber":"0xa","blockHash":hash}),
        ),
        (
            "eth_getBlockByNumber",
            200,
            json!({"hash":format!("0x{}","cd".repeat(32))}),
        ),
    ]);
    let mut poster = RpcPoster::new(&url, [1; 20]).unwrap();
    assert!(poster
        .included("test-transaction")
        .unwrap_err()
        .is::<RevertedReceipt>());
    assert_eq!(poster.included("test-transaction").unwrap(), None);
    worker.join().unwrap();
}
#[test]
fn l2_head_gas_limit_is_read_and_the_chain_identity_is_checked() {
    let (url, worker) = serve(vec![
        ("eth_chainId", 200, json!("0xb001e")),
        (
            "eth_getBlockByNumber",
            200,
            json!({"number":"0x0","hash":format!("0x{}","00".repeat(32)),"timestamp":"0x0","gasLimit":"0x2625a00","miner":format!("0x{}","00".repeat(20)),"extraData":"0x"}),
        ),
    ]);
    assert_eq!(l2_gas_limit(&url).unwrap(), 40_000_000);
    worker.join().unwrap();
    let (url, worker) = serve(vec![("eth_chainId", 200, json!("0x1"))]);
    assert!(l2_gas_limit(&url).is_err());
    worker.join().unwrap();
}
