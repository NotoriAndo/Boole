use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use boole_core::native_network::native_testnet;
use boole_node::native_node::NativeNode;
use serde_json::Value;

struct TestDir(std::path::PathBuf);
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn native_http_reports_pinned_network_and_refuses_browser_cross_origin_and_public_bind() {
    let dir = std::env::temp_dir().join(format!(
        "boole-native-http-{}",
        boole_testkit::rand_suffix()
    ));
    std::fs::create_dir(&dir).unwrap();
    let _cleanup = TestDir(dir.clone());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let stop = Arc::new(tokio::sync::Notify::new());
    let shutdown = stop.clone();
    let node = NativeNode::open(&dir).unwrap();
    let task = runtime.spawn(boole_node::native_http::serve(listener, node, shutdown));
    let request = |origin: &str| {
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "GET /native/info HTTP/1.1\r\nHost: {addr}\r\n{origin}Connection: close\r\n\r\n"
        )
        .unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).unwrap();
        out
    };
    let response = request("");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let info: Value = serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(info["policy"]["totalSupply"], "100000000000000000");
    assert_eq!(
        info["genesisHash"],
        native_testnet().genesis_hash().to_hex()
    );
    assert_eq!(info["height"], "0");
    assert!(request("Origin: https://untrusted.example\r\n").starts_with("HTTP/1.1 403"));
    let mut oversized = TcpStream::connect(addr).unwrap();
    oversized
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    write!(oversized, "POST /native/transfers HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: 4097\r\nConnection: close\r\n\r\n{}", " ".repeat(4097)).unwrap();
    let mut rejected = String::new();
    oversized.read_to_string(&mut rejected).unwrap();
    assert!(rejected.starts_with("HTTP/1.1 413"), "{rejected}");
    std::fs::write(
        dir.join(boole_node::native_node::NATIVE_BLOCKS_FILE),
        b"{}\n",
    )
    .unwrap();
    assert!(
        request("").starts_with("HTTP/1.1 503"),
        "stale state cannot be reported ready"
    );
    stop.notify_one();
    runtime.block_on(task).unwrap().unwrap();
    assert!(boole_node::native_http::bind_loopback("0.0.0.0:0".parse().unwrap()).is_err());
}

fn rpc(addr: std::net::SocketAddr, method: &str, path: &str, body: Value) -> Value {
    let body = if method == "GET" {
        String::new()
    } else {
        body.to_string()
    };
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(15)))
        .unwrap();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{path}: {response}");
    serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap()).unwrap()
}

#[test]
fn two_independent_rpc_nodes_mine_transfer_and_rejoin_with_identical_accounting() {
    use boole_core::native_chain::{NativeBlockTemplate, NativeTransfer};
    use boole_core::SigningKeyV2;
    use serde_json::json;
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut servers = Vec::new();
    for _ in 0..2 {
        let dir = std::env::temp_dir().join(format!(
            "boole-native-http-{}",
            boole_testkit::rand_suffix()
        ));
        std::fs::create_dir(&dir).unwrap();
        let cleanup = TestDir(dir.clone());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(tokio::sync::Notify::new());
        let task = runtime.spawn(boole_node::native_http::serve(
            listener,
            NativeNode::open(&dir).unwrap(),
            stop.clone(),
        ));
        servers.push((addr, stop, task, cleanup));
    }
    let a = servers[0].0;
    let b = servers[1].0;
    let alice = SigningKeyV2::from_dev_id("native-http-alice");
    let bob = SigningKeyV2::from_dev_id("native-http-bob");
    let miner = SigningKeyV2::from_dev_id("native-http-miner");
    let network = native_testnet();
    let mut blocks = Vec::new();
    let mine = |height: u64, reward: &str| {
        let template: NativeBlockTemplate = serde_json::from_value(rpc(a, "POST", "/native/template", json!({"producerPk": miner.pk_hex(), "rewardPk": reward, "timestampMs": height * 60_000}))).unwrap();
        let block = template.mine(0, 2_000_000).unwrap().unwrap();
        let auth = miner
            .sign_for_network(
                &block.authorization_payload().unwrap(),
                Some(network.network_id()),
            )
            .unwrap();
        let block = block.authorize(&auth).unwrap();
        assert_eq!(
            rpc(
                a,
                "POST",
                "/native/blocks",
                serde_json::to_value(&block).unwrap()
            )["accepted"],
            true
        );
        block
    };
    for height in 1..=10 {
        blocks.push(mine(height, &alice.pk_hex()));
    }
    assert_eq!(
        rpc(b, "POST", "/native/chain", json!(blocks))["adopted"],
        true
    );
    let tx = NativeTransfer::try_from(
        &alice
            .sign_for_network(
                &json!({
                    "schema": "boole.transfer.v1", "from": alice.pk_hex(), "to": bob.pk_hex(),
                    "amount": "100000000", "fee": "1000", "nonce": "0", "validBefore": "100"
                }),
                Some(network.network_id()),
            )
            .unwrap(),
    )
    .unwrap();
    let txid = tx.id().to_hex();
    assert_eq!(
        rpc(a, "POST", "/native/transfers", json!(tx))["status"],
        "pending"
    );
    assert_eq!(
        rpc(
            a,
            "GET",
            &format!("/native/transactions/{txid}"),
            Value::Null
        )["status"],
        "pending"
    );
    blocks.push(mine(11, &miner.pk_hex()));
    assert_eq!(
        rpc(b, "POST", "/native/chain", json!(blocks))["adopted"],
        true
    );
    for node in [a, b] {
        assert_eq!(
            rpc(
                node,
                "GET",
                &format!("/native/accounts/{}", bob.pk_hex()),
                Value::Null
            )["balance"],
            "100000000"
        );
        assert_eq!(
            rpc(
                node,
                "GET",
                &format!("/native/transactions/{txid}"),
                Value::Null
            )["status"],
            "confirmed"
        );
    }
    assert_eq!(
        rpc(a, "GET", "/native/info", Value::Null),
        rpc(b, "GET", "/native/info", Value::Null)
    );
    for (_, stop, task, _) in servers {
        stop.notify_one();
        runtime.block_on(task).unwrap().unwrap();
    }
}
