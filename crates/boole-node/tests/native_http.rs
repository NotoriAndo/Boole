use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use boole_core::native_network::native_testnet;
use boole_node::NativeNode;
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
    let task = runtime.spawn(boole_node::serve_native_node(listener, node, shutdown));
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
    assert_eq!(info["resources"]["historyBytes"], 0);
    assert_eq!(info["resources"]["historyBlocks"], 0);
    assert_eq!(info["resources"]["pendingBytes"], 0);
    assert_eq!(info["resources"]["balanceEntries"], 0);
    assert_eq!(info["resources"]["nonceEntries"], 0);
    assert_eq!(info["resources"]["historyLimitBytes"], 256 * 1024 * 1024);
    assert_eq!(info["resources"]["pendingLimitTransfers"], 512);
    assert!(request("Origin: https://untrusted.example\r\n").starts_with("HTTP/1.1 403"));
    let mut oversized = TcpStream::connect(addr).unwrap();
    oversized
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    write!(oversized, "POST /native/transfers HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: 4097\r\nConnection: close\r\n\r\n{}", " ".repeat(4097)).unwrap();
    let mut rejected = String::new();
    oversized.read_to_string(&mut rejected).unwrap();
    assert!(rejected.starts_with("HTTP/1.1 413"), "{rejected}");
    std::fs::write(dir.join(boole_node::NATIVE_BLOCKS_FILE), b"{}\n").unwrap();
    assert!(
        request("").starts_with("HTTP/1.1 503"),
        "stale state cannot be reported ready"
    );
    stop.notify_one();
    runtime.block_on(task).unwrap().unwrap();
    assert!(boole_node::bind_native_loopback("0.0.0.0:0".parse().unwrap()).is_err());
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
        let task = runtime.spawn(boole_node::serve_native_node(
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
    let initial_resources = rpc(a, "GET", "/native/info", Value::Null)["resources"].clone();
    assert_eq!(initial_resources["historyBlocks"], 10);
    assert!(initial_resources["historyBytes"].as_u64().unwrap() > 0);
    assert_eq!(initial_resources["balanceEntries"], 1);
    assert_eq!(initial_resources["nonceEntries"], 0);
    assert_eq!(initial_resources["confirmedTransfers"], 0);
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
    let pending_resources = rpc(a, "GET", "/native/info", Value::Null)["resources"].clone();
    assert_eq!(
        pending_resources["historyBytes"],
        initial_resources["historyBytes"]
    );
    assert_eq!(pending_resources["pendingTransfers"], 1);
    assert!(pending_resources["pendingBytes"].as_u64().unwrap() > 0);
    assert_eq!(
        pending_resources["balanceEntries"], 1,
        "canonical entries, not reservations"
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
        let resources = rpc(node, "GET", "/native/info", Value::Null)["resources"].clone();
        assert_eq!(resources["historyBlocks"], 11);
        assert!(
            resources["historyBytes"].as_u64().unwrap()
                > initial_resources["historyBytes"].as_u64().unwrap()
        );
        assert_eq!(resources["pendingTransfers"], 0);
        assert_eq!(resources["pendingBytes"], 0);
        assert_eq!(resources["balanceEntries"], 3);
        assert_eq!(resources["nonceEntries"], 1);
        assert_eq!(resources["confirmedTransfers"], 1);
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

#[test]
fn native_rpc_and_secure_peers_share_the_same_durable_state_and_shutdown_boundary() {
    use boole_core::native_chain::NativeBlockTemplate;
    use boole_core::SigningKeyV2;
    use boole_node::NativePeerConfig;
    use boole_p2p::TlsIdentity;
    use serde_json::json;
    use std::time::{Duration, Instant};
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let identity = || TlsIdentity::from_pkcs8(&TlsIdentity::generate_pkcs8().unwrap()).unwrap();
    let keys = [identity(), identity()];
    let peer_listeners = [
        TcpListener::bind("127.0.0.1:0").unwrap(),
        TcpListener::bind("127.0.0.1:0").unwrap(),
    ];
    let peer_addresses = peer_listeners
        .each_ref()
        .map(|listener| listener.local_addr().unwrap());
    let mut servers = Vec::new();
    for (index, peer_listener) in peer_listeners.into_iter().enumerate() {
        let dir = std::env::temp_dir().join(format!(
            "boole-native-http-peers-{}",
            boole_testkit::rand_suffix()
        ));
        std::fs::create_dir(&dir).unwrap();
        let cleanup = TestDir(dir.clone());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let stop = Arc::new(tokio::sync::Notify::new());
        let config = NativePeerConfig {
            identity: keys[index].clone(),
            peers: vec![(peer_addresses[1 - index], keys[1 - index].peer_id())],
        };
        let task = runtime.spawn(boole_node::serve_native_node_with_peers(
            listener,
            NativeNode::open(&dir).unwrap(),
            peer_listener,
            config,
            stop.clone(),
        ));
        servers.push((addr, stop, task, cleanup));
    }
    let miner = SigningKeyV2::from_dev_id("native-rpc-peer-shared-miner");
    let template: NativeBlockTemplate = serde_json::from_value(rpc(
        servers[0].0,
        "POST",
        "/native/template",
        json!({"producerPk": miner.pk_hex(), "rewardPk": miner.pk_hex(), "timestampMs": 60000}),
    ))
    .unwrap();
    let block = template.mine(0, 2_000_000).unwrap().unwrap();
    let signature = miner
        .sign_for_network(
            &block.authorization_payload().unwrap(),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    let block = block.authorize(&signature).unwrap();
    assert_eq!(
        rpc(servers[0].0, "POST", "/native/blocks", json!(block))["accepted"],
        true
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let info = rpc(servers[1].0, "GET", "/native/info", Value::Null);
        if info["height"] == "1" {
            assert_eq!(info["headHash"], block.hash().unwrap().to_hex());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "RPC mutation never reached the peer RPC ledger"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let peers = rpc(servers[1].0, "GET", "/native/peers", Value::Null);
    assert_eq!(peers["enabled"], true);
    assert_eq!(peers["running"], true);
    assert_eq!(peers["localPeerId"], keys[1].peer_id().to_hex());
    assert_eq!(peers["peers"].as_array().unwrap().len(), 1);
    assert_eq!(peers["peers"][0]["peerId"], keys[0].peer_id().to_hex());
    assert_eq!(peers["limits"]["maxInboundWorkers"], 4);
    for (_, stop, task, cleanup) in servers {
        stop.notify_one();
        runtime.block_on(task).unwrap().unwrap();
        let recovered = NativeNode::open(&cleanup.0).unwrap();
        assert_eq!(recovered.chain().head_hash(), block.hash().unwrap());
    }
    for address in peer_addresses {
        assert!(TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err());
    }
}
