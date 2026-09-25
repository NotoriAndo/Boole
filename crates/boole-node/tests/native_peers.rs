use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use boole_core::native_chain::{NativeChain, NativeTransfer};
use boole_core::native_network::native_testnet;
use boole_core::signed_envelope::SigningKeyV2;
use boole_node::{NativeNode, NativePeerConfig, NativePeerService};
use boole_p2p::{TlsIdentity, TlsTransport, Transport};

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boole-native-peers-test-{}",
            boole_testkit::rand_suffix()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn node(&self) -> Arc<Mutex<NativeNode>> {
        Arc::new(Mutex::new(NativeNode::open(&self.0).unwrap()))
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn identity() -> TlsIdentity {
    TlsIdentity::from_pkcs8(&TlsIdentity::generate_pkcs8().unwrap()).unwrap()
}

fn mine(node: &Arc<Mutex<NativeNode>>, key: &SigningKeyV2, reward: &str, count: u64) {
    let mut node = node.lock().unwrap();
    for _ in 0..count {
        let height = node.chain().ledger().height() + 1;
        let block = node
            .template(&key.pk_hex(), reward, height * 60_000)
            .unwrap()
            .mine(0, 2_000_000)
            .unwrap()
            .expect("bounded disposable PoW");
        let signature = key
            .sign_for_network(
                &block.authorization_payload().unwrap(),
                Some(native_testnet().network_id()),
            )
            .unwrap();
        node.submit_block(block.authorize(&signature).unwrap())
            .unwrap();
    }
}

fn await_condition(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "native peers did not converge");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn start_pair(
    node_a: &Arc<Mutex<NativeNode>>,
    node_b: &Arc<Mutex<NativeNode>>,
) -> (NativePeerService, NativePeerService) {
    let listener_a = TcpListener::bind("127.0.0.1:0").unwrap();
    let listener_b = TcpListener::bind("127.0.0.1:0").unwrap();
    let address_a = listener_a.local_addr().unwrap();
    let address_b = listener_b.local_addr().unwrap();
    let key_a = identity();
    let key_b = identity();
    let config_a = NativePeerConfig {
        identity: key_a.clone(),
        peers: vec![(address_b, key_b.peer_id())],
    };
    let config_b = NativePeerConfig {
        identity: key_b,
        peers: vec![(address_a, key_a.peer_id())],
    };
    let service_a = NativePeerService::start(listener_a, node_a.clone(), config_a).unwrap();
    let service_b = NativePeerService::start(listener_b, node_b.clone(), config_b).unwrap();
    (service_a, service_b)
}

#[test]
fn stop_cancels_authenticated_outbound_wait_for_local_state_lock() {
    check_authenticated_outbound_state_lock_wait(false);
}

#[test]
fn authenticated_outbound_state_lock_wait_obeys_the_existing_round_deadline() {
    check_authenticated_outbound_state_lock_wait(true);
}

fn check_authenticated_outbound_state_lock_wait(wait_for_expiration: bool) {
    let dir = TestDir::new();
    let node = dir.node();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let local_address = listener.local_addr().unwrap();
    let remote_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let remote_address = remote_listener.local_addr().unwrap();
    let local_key = identity();
    let remote_key = identity();
    let remote_id = remote_key.peer_id();
    let transport =
        TlsTransport::new(remote_key, vec![(local_address, local_key.peer_id())]).unwrap();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
    let remote = std::thread::spawn(move || {
        let (socket, _) = remote_listener.accept().unwrap();
        release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let mut connection = transport
            .accept_stream_until(socket, Instant::now() + Duration::from_secs(2))
            .unwrap();
        // The authenticated client cannot create its hello while the real
        // ledger mutex is held. Keep TLS open after observing that wait.
        assert!(transport
            .recv_json_counted_until::<serde_json::Value>(
                &mut connection,
                4096,
                Instant::now() + Duration::from_millis(100),
            )
            .is_err());
        waiting_tx.send(()).unwrap();
        let _ = transport.recv_json_counted_until::<serde_json::Value>(
            &mut connection,
            4096,
            Instant::now() + Duration::from_secs(15),
        );
    });
    let mut service = NativePeerService::start(
        listener,
        node.clone(),
        NativePeerConfig {
            identity: local_key,
            peers: vec![(remote_address, remote_id)],
        },
    )
    .unwrap();
    let monitor = service.monitor();
    let guard = node.lock().unwrap();
    release_tx.send(()).unwrap();
    waiting_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(monitor.snapshot().peers[0].failed_rounds, 0);
    let expired_while_locked = if wait_for_expiration {
        let deadline = Instant::now() + Duration::from_secs(11);
        while service.status()[0].failed_rounds == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let status = &service.status()[0];
        status.failed_rounds > 0 && status.last_failure_stage == Some("local_state")
    } else {
        true
    };
    let (stopped_tx, stopped_rx) = std::sync::mpsc::channel();
    let stopper = std::thread::spawn(move || {
        service.stop();
        stopped_tx.send(()).unwrap();
    });
    let stopped_while_locked = stopped_rx.recv_timeout(Duration::from_secs(1)).is_ok();
    // Always release and join before asserting, including the pre-fix failure.
    drop(guard);
    stopper.join().unwrap();
    remote.join().unwrap();
    assert!(
        expired_while_locked,
        "peer round deadline did not bound the ledger lock wait"
    );
    assert!(
        stopped_while_locked,
        "peer shutdown waited for an unrelated ledger lock holder"
    );
    assert_eq!(monitor.snapshot().active_outbound_rounds, 0);
    assert_eq!(node.lock().unwrap().chain().ledger().height(), 0);
}

#[test]
fn stop_cancels_authenticated_inbound_wait_for_local_state_lock() {
    let dir = TestDir::new();
    let node = dir.node();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let local_address = listener.local_addr().unwrap();
    let remote_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let remote_address = remote_listener.local_addr().unwrap();
    let local_key = identity();
    let remote_key = identity();
    let remote_id = remote_key.peer_id();
    let transport =
        TlsTransport::new(remote_key, vec![(local_address, local_key.peer_id())]).unwrap();
    let mut service = NativePeerService::start(
        listener,
        node.clone(),
        NativePeerConfig {
            identity: local_key,
            peers: vec![(remote_address, remote_id)],
        },
    )
    .unwrap();
    let monitor = service.monitor();
    let mut connection = transport.connect(&local_address).unwrap();
    let guard = node.lock().unwrap();
    transport
        .send_json_counted_until(
            &mut connection,
            &wire_hello(),
            4096,
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
    assert!(transport
        .recv_json_counted_until::<serde_json::Value>(
            &mut connection,
            4096,
            Instant::now() + Duration::from_millis(100),
        )
        .is_err());
    assert_eq!(monitor.snapshot().failed_inbound_rounds, 0);
    let (stopped_tx, stopped_rx) = std::sync::mpsc::channel();
    let stopper = std::thread::spawn(move || {
        service.stop();
        stopped_tx.send(()).unwrap();
    });
    let stopped_while_locked = stopped_rx.recv_timeout(Duration::from_secs(1)).is_ok();
    drop(guard);
    stopper.join().unwrap();
    assert!(
        stopped_while_locked,
        "inbound shutdown waited for the ledger lock"
    );
    let after = monitor.snapshot();
    assert_eq!(after.active_inbound_workers, 0);
    assert_eq!(after.active_outbound_rounds, 0);
    assert_eq!(node.lock().unwrap().chain().ledger().height(), 0);
}

#[test]
fn stop_cancels_queued_block_fork_and_pending_mutations_without_publishing_them() {
    for kind in ["extension", "fork", "pending"] {
        let dir = TestDir::new();
        let remote_dir = TestDir::new();
        let node = dir.node();
        let remote_node = remote_dir.node();
        let owner = SigningKeyV2::from_dev_id("native-queued-mutation-owner");
        mine(&node, &owner, &owner.pk_hex(), 10);
        for block in node.lock().unwrap().chain().blocks() {
            remote_node
                .lock()
                .unwrap()
                .submit_block(block.clone())
                .unwrap();
        }
        if kind == "fork" {
            let local_miner = SigningKeyV2::from_dev_id("native-queued-fork-local");
            mine(&node, &local_miner, &owner.pk_hex(), 1);
        }
        if kind != "pending" {
            mine(
                &remote_node,
                &owner,
                &owner.pk_hex(),
                if kind == "fork" { 2 } else { 1 },
            );
        }
        let transfer = NativeTransfer::try_from(
            &owner
                .sign_for_network(
                    &serde_json::json!({
                        "schema": "boole.transfer.v1", "from": owner.pk_hex(), "to": owner.pk_hex(),
                        "amount": "1", "fee": "1000", "nonce": "0", "validBefore": "100"
                    }),
                    Some(native_testnet().network_id()),
                )
                .unwrap(),
        )
        .unwrap();
        let chain = remote_node.lock().unwrap().chain().clone();
        let expected = node.lock().unwrap().chain().clone();
        let local = TcpListener::bind("127.0.0.1:0").unwrap();
        let remote = TcpListener::bind("127.0.0.1:0").unwrap();
        let remote_address = remote.local_addr().unwrap();
        let local_key = identity();
        let remote_key = identity();
        let remote_id = remote_key.peer_id();
        let transport = TlsTransport::new(
            remote_key,
            vec![(local.local_addr().unwrap(), local_key.peer_id())],
        )
        .unwrap();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
        let fixture = std::thread::spawn(move || {
            let (socket, _) = remote.accept().unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut connection = transport.accept_stream_until(socket, deadline).unwrap();
            let (hello, _): (serde_json::Value, _) = transport
                .recv_json_counted_until(&mut connection, 4096, deadline)
                .unwrap();
            assert_eq!(hello["type"], "hello");
            let snapshot = serde_json::json!({"height": chain.ledger().height(), "hash": chain.head_hash().to_hex()});
            let mut greeting = wire_hello();
            greeting["head"] = snapshot.clone();
            transport
                .send_json_counted_until(&mut connection, &greeting, 4096, deadline)
                .unwrap();
            loop {
                let (request, _): (serde_json::Value, _) = transport
                    .recv_json_counted_until(&mut connection, 4096, deadline)
                    .unwrap();
                let response = match request["type"].as_str().unwrap() {
                    "getHash" => {
                        let height = request["height"].as_u64().unwrap();
                        let hash = if height == 0 {
                            native_testnet().genesis_hash()
                        } else {
                            chain.blocks()[height as usize - 1].hash().unwrap()
                        };
                        serde_json::json!({"type": "hash", "snapshot": snapshot, "height": height, "hash": hash.to_hex()})
                    }
                    "getBlocks" => {
                        let from = request["from"].as_u64().unwrap() as usize;
                        serde_json::json!({"type": "blocks", "snapshot": snapshot, "from": from, "blocks": &chain.blocks()[from - 1..]})
                    }
                    "getPending" => {
                        serde_json::json!({"type": "pending", "snapshot": snapshot, "offset": 0, "total": 1, "transfers": [transfer]})
                    }
                    other => panic!("unexpected queued mutation request: {other}"),
                };
                let final_data = request["type"] != "getHash";
                if final_data {
                    ready_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                }
                transport
                    .send_json_counted_until(&mut connection, &response, 1024 * 1024, deadline)
                    .unwrap();
                if final_data {
                    assert!(transport
                        .recv_json_counted_until::<serde_json::Value>(
                            &mut connection,
                            4096,
                            Instant::now() + Duration::from_millis(100)
                        )
                        .is_err());
                    waiting_tx.send(()).unwrap();
                    let _ = transport.recv_json_counted_until::<serde_json::Value>(
                        &mut connection,
                        4096,
                        deadline,
                    );
                    break;
                }
            }
        });
        let mut service = NativePeerService::start(
            local,
            node.clone(),
            NativePeerConfig {
                identity: local_key,
                peers: vec![(remote_address, remote_id)],
            },
        )
        .unwrap();
        let monitor = service.monitor();
        ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let guard = node.lock().unwrap();
        release_tx.send(()).unwrap();
        waiting_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(monitor.snapshot().peers[0].failed_rounds, 0);
        let (stopped_tx, stopped_rx) = std::sync::mpsc::channel();
        let stopper = std::thread::spawn(move || {
            service.stop();
            stopped_tx.send(()).unwrap();
        });
        let stopped_while_locked = stopped_rx.recv_timeout(Duration::from_secs(1)).is_ok();
        drop(guard);
        stopper.join().unwrap();
        fixture.join().unwrap();
        assert!(
            stopped_while_locked,
            "queued {kind} held the shutdown mutation barrier"
        );
        let status = monitor.snapshot();
        assert_eq!(status.active_outbound_rounds, 0);
        assert_eq!(
            status.peers[0].last_failure_stage,
            Some(match kind {
                "extension" => "block_apply",
                "fork" => "fork_apply",
                _ => "pending_admission",
            })
        );
        assert_eq!(node.lock().unwrap().chain(), &expected);
        assert!(node.lock().unwrap().pending().is_empty());
        drop(node);
        let reopened = NativeNode::open(&dir.0).unwrap();
        assert_eq!(reopened.chain(), &expected);
        assert!(reopened.pending().is_empty());
    }
}

#[test]
fn outbound_connection_failure_exposes_a_bounded_diagnostic_stage() {
    let dir = TestDir::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let unused = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = unused.local_addr().unwrap();
    drop(unused);
    let mut service = NativePeerService::start(
        listener,
        dir.node(),
        NativePeerConfig {
            identity: identity(),
            peers: vec![(address, identity().peer_id())],
        },
    )
    .unwrap();
    await_condition(|| service.status()[0].failed_rounds > 0);
    let snapshot = serde_json::to_value(service.monitor().snapshot()).unwrap();
    assert_eq!(snapshot["peers"][0]["state"], "retrying");
    assert_eq!(snapshot["peers"][0]["lastFailureStage"], "connect");
    service.stop();
    assert_eq!(service.monitor().snapshot().active_outbound_rounds, 0);
}

#[test]
fn peer_failure_stages_distinguish_tls_hello_and_signature_and_clear_after_recovery() {
    let dir = TestDir::new();
    let node = dir.node();
    let local = TcpListener::bind("127.0.0.1:0").unwrap();
    let local_address = local.local_addr().unwrap();
    let remote = TcpListener::bind("127.0.0.1:0").unwrap();
    remote.set_nonblocking(true).unwrap();
    let remote_address = remote.local_addr().unwrap();
    let local_key = identity();
    let remote_key = identity();
    let expected_remote = remote_key.peer_id();
    let authenticated =
        TlsTransport::new(remote_key, vec![(local_address, local_key.peer_id())]).unwrap();
    let wrong_key =
        TlsTransport::new(identity(), vec![(local_address, local_key.peer_id())]).unwrap();
    let owner = SigningKeyV2::from_dev_id("native-diagnostic-invalid-pending");
    let transfer = NativeTransfer::try_from(
        &owner
            .sign_for_network(
                &serde_json::json!({
                    "schema": "boole.transfer.v1", "from": owner.pk_hex(), "to": owner.pk_hex(),
                    "amount": "1", "fee": "1000", "nonce": "0", "validBefore": "100"
                }),
                Some(native_testnet().network_id()),
            )
            .unwrap(),
    )
    .unwrap();
    let mut invalid_transfer = serde_json::to_value(transfer).unwrap();
    invalid_transfer["signature"] = "00".repeat(64).into();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let fixture = std::thread::spawn(move || {
        let accept = || {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match remote.accept() {
                    Ok((socket, _)) => return socket,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("diagnostic peer accept: {error}"),
                }
            }
        };
        // A pinned identity mismatch must stay a TLS-phase failure, not a
        // claimed invalid block or a leaked remote diagnostic string.
        let rejected =
            wrong_key.accept_stream_until(accept(), Instant::now() + Duration::from_secs(2));
        assert!(rejected.is_err());
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        for (bad_hello, bad_pending) in [(true, false), (false, true), (false, false)] {
            let socket = accept();
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut connection = authenticated.accept_stream_until(socket, deadline).unwrap();
            let (request, _): (serde_json::Value, _) = authenticated
                .recv_json_counted_until(&mut connection, 4096, deadline)
                .unwrap();
            assert_eq!(request["type"], "hello");
            let mut greeting = wire_hello();
            if bad_hello {
                greeting["networkId"] = "do-not-echo-untrusted-native-peer-text".into();
            }
            authenticated
                .send_json_counted_until(&mut connection, &greeting, 4096, deadline)
                .unwrap();
            if bad_hello {
                assert!(authenticated
                    .recv_json_counted_until::<serde_json::Value>(&mut connection, 4096, deadline)
                    .is_err());
            } else {
                let (request, _): (serde_json::Value, _) = authenticated
                    .recv_json_counted_until(&mut connection, 4096, deadline)
                    .unwrap();
                assert_eq!(request["type"], "getPending");
                let transfers = if bad_pending {
                    vec![invalid_transfer.clone()]
                } else {
                    vec![]
                };
                authenticated.send_json_counted_until(&mut connection,
                    &serde_json::json!({"type": "pending", "snapshot": greeting["head"], "offset": 0, "total": transfers.len(), "transfers": transfers}),
                    4096, deadline).unwrap();
                if bad_pending {
                    assert!(authenticated
                        .recv_json_counted_until::<serde_json::Value>(
                            &mut connection,
                            4096,
                            deadline
                        )
                        .is_err());
                } else {
                    let (done, _): (serde_json::Value, _) = authenticated
                        .recv_json_counted_until(&mut connection, 4096, deadline)
                        .unwrap();
                    assert_eq!(done["type"], "done");
                }
            }
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    });
    let mut service = NativePeerService::start(
        local,
        node.clone(),
        NativePeerConfig {
            identity: local_key,
            peers: vec![(remote_address, expected_remote)],
        },
    )
    .unwrap();
    await_condition(|| service.status()[0].failed_rounds == 1);
    let failed = serde_json::to_value(service.monitor().snapshot()).unwrap();
    assert_eq!(failed["peers"][0]["lastFailureStage"], "tls_handshake");
    release_tx.send(()).unwrap();
    await_condition(|| service.status()[0].failed_rounds == 2);
    let failed = serde_json::to_value(service.monitor().snapshot()).unwrap();
    assert_eq!(failed["peers"][0]["lastFailureStage"], "hello");
    assert!(!failed
        .to_string()
        .contains("do-not-echo-untrusted-native-peer-text"));
    release_tx.send(()).unwrap();
    await_condition(|| service.status()[0].failed_rounds == 3);
    let failed = serde_json::to_value(service.monitor().snapshot()).unwrap();
    assert_eq!(failed["peers"][0]["lastFailureStage"], "pending_signature");
    assert!(node.lock().unwrap().pending().is_empty());
    release_tx.send(()).unwrap();
    await_condition(|| service.status()[0].successful_rounds == 1);
    let recovered = serde_json::to_value(service.monitor().snapshot()).unwrap();
    assert_eq!(recovered["peers"][0]["state"], "snapshot_match");
    assert!(recovered["peers"][0]["lastFailureStage"].is_null());
    assert_eq!(recovered["peers"][0]["consecutiveFailures"], 0);
    service.stop();
    release_tx.send(()).unwrap();
    fixture.join().unwrap();
    assert_eq!(node.lock().unwrap().chain().ledger().height(), 0);
}

#[test]
fn approved_encrypted_peers_incrementally_catch_up_and_replay_identically() {
    let dir_a = TestDir::new();
    let dir_b = TestDir::new();
    let node_a = dir_a.node();
    let node_b = dir_b.node();
    let miner = SigningKeyV2::from_dev_id("native-secure-peer-miner");
    // More than one bounded 256-block synchronization round.
    mine(&node_a, &miner, &miner.pk_hex(), 280);
    let expected = node_a.lock().unwrap().chain().clone();
    let (mut service_a, mut service_b) = start_pair(&node_a, &node_b);
    await_condition(|| node_b.lock().unwrap().chain() == &expected);
    service_b.stop();
    service_a.stop();
    drop(service_b);
    drop(node_b);
    let recovered = NativeNode::open(&dir_b.0).unwrap();
    assert_eq!(recovered.chain(), &expected);
}

#[test]
fn signed_transfer_gossip_reaches_miner_and_confirms_once_on_both_ledgers() {
    let dir_a = TestDir::new();
    let dir_b = TestDir::new();
    let node_a = dir_a.node();
    let node_b = dir_b.node();
    let miner = SigningKeyV2::from_dev_id("native-peer-gossip-miner");
    let alice = SigningKeyV2::from_dev_id("native-peer-gossip-alice");
    let bob = SigningKeyV2::from_dev_id("native-peer-gossip-bob").pk_hex();
    mine(&node_a, &miner, &alice.pk_hex(), 10);
    let initial = node_a.lock().unwrap().chain().clone();
    let (mut service_a, mut service_b) = start_pair(&node_a, &node_b);
    await_condition(|| node_b.lock().unwrap().chain() == &initial);
    let transfer = NativeTransfer::try_from(
        &alice
            .sign_for_network(
                &serde_json::json!({
                    "schema": "boole.transfer.v1", "from": alice.pk_hex(), "to": bob,
                    "amount": "100000000", "fee": "1000", "nonce": "0", "validBefore": "100"
                }),
                Some(native_testnet().network_id()),
            )
            .unwrap(),
    )
    .unwrap();
    node_b
        .lock()
        .unwrap()
        .submit_transfer(transfer.clone())
        .unwrap();
    await_condition(|| node_a.lock().unwrap().pending() == std::slice::from_ref(&transfer));
    mine(&node_a, &miner, &miner.pk_hex(), 1);
    await_condition(|| node_b.lock().unwrap().confirmed_height(&transfer.id()) == Some(11));
    service_a.stop();
    service_b.stop();
    for node in [&node_a, &node_b] {
        let mut node = node.lock().unwrap();
        assert_eq!(node.chain().ledger().balance(&bob), 100_000_000);
        assert_eq!(node.chain().ledger().next_nonce(&alice.pk_hex()), 1);
        assert!(node.pending().is_empty());
        assert!(!node.submit_transfer(transfer.clone()).unwrap());
    }
    assert_eq!(
        node_a.lock().unwrap().chain(),
        node_b.lock().unwrap().chain()
    );
}

#[test]
fn partitioned_peers_rejoin_the_verified_heavier_fork_and_recover_orphaned_transfer() {
    let dir_a = TestDir::new();
    let dir_b = TestDir::new();
    let node_a = dir_a.node();
    let node_b = dir_b.node();
    let miner = SigningKeyV2::from_dev_id("native-partition-miner");
    let alice = SigningKeyV2::from_dev_id("native-partition-alice");
    let bob = SigningKeyV2::from_dev_id("native-partition-bob").pk_hex();
    mine(&node_a, &miner, &alice.pk_hex(), 10);
    let initial = node_a.lock().unwrap().chain().clone();
    let (mut service_a, mut service_b) = start_pair(&node_a, &node_b);
    await_condition(|| node_b.lock().unwrap().chain() == &initial);
    service_a.stop();
    service_b.stop();
    let transfer = NativeTransfer::try_from(
        &alice
            .sign_for_network(
                &serde_json::json!({
                    "schema": "boole.transfer.v1", "from": alice.pk_hex(), "to": bob,
                    "amount": "100000000", "fee": "1000", "nonce": "0", "validBefore": "100"
                }),
                Some(native_testnet().network_id()),
            )
            .unwrap(),
    )
    .unwrap();
    node_a
        .lock()
        .unwrap()
        .submit_transfer(transfer.clone())
        .unwrap();
    mine(&node_a, &miner, &miner.pk_hex(), 1);
    mine(&node_b, &miner, &alice.pk_hex(), 2);
    let winner = node_b.lock().unwrap().chain().clone();
    assert!(winner.outranks(node_a.lock().unwrap().chain()));
    let (mut rejoined_a, mut rejoined_b) = start_pair(&node_a, &node_b);
    await_condition(|| node_a.lock().unwrap().chain() == &winner);
    await_condition(|| node_b.lock().unwrap().pending() == std::slice::from_ref(&transfer));
    assert_eq!(node_a.lock().unwrap().chain().ledger().balance(&bob), 0);
    assert_eq!(
        node_a.lock().unwrap().confirmed_height(&transfer.id()),
        None
    );
    mine(&node_b, &miner, &miner.pk_hex(), 1);
    await_condition(|| node_a.lock().unwrap().confirmed_height(&transfer.id()) == Some(13));
    rejoined_a.stop();
    rejoined_b.stop();
    assert_eq!(
        node_a.lock().unwrap().chain(),
        node_b.lock().unwrap().chain()
    );
    assert_eq!(
        node_a.lock().unwrap().chain().ledger().balance(&bob),
        100_000_000
    );
}

#[test]
fn returning_peer_recovers_orphaned_transfer_while_the_other_side_is_in_connect_backoff() {
    let dir_a = TestDir::new();
    let dir_b = TestDir::new();
    let node_a = dir_a.node();
    let node_b = dir_b.node();
    let miner = SigningKeyV2::from_dev_id("native-backoff-miner");
    let alice = SigningKeyV2::from_dev_id("native-backoff-alice");
    let recipient = SigningKeyV2::from_dev_id("native-backoff-recipient").pk_hex();
    mine(&node_a, &miner, &alice.pk_hex(), 10);
    for block in node_a.lock().unwrap().chain().blocks() {
        node_b.lock().unwrap().submit_block(block.clone()).unwrap();
    }
    let transfer = NativeTransfer::try_from(
        &alice
            .sign_for_network(
                &serde_json::json!({
                    "schema": "boole.transfer.v1", "from": alice.pk_hex(), "to": recipient,
                    "amount": "100000000", "fee": "1000", "nonce": "0", "validBefore": "100"
                }),
                Some(native_testnet().network_id()),
            )
            .unwrap(),
    )
    .unwrap();
    node_b
        .lock()
        .unwrap()
        .submit_transfer(transfer.clone())
        .unwrap();
    mine(&node_b, &miner, &miner.pk_hex(), 1);
    mine(&node_a, &miner, &alice.pk_hex(), 2);
    let winner = node_a.lock().unwrap().chain().clone();
    let listener_a = TcpListener::bind("127.0.0.1:0").unwrap();
    let listener_b = TcpListener::bind("127.0.0.1:0").unwrap();
    let address_a = listener_a.local_addr().unwrap();
    let address_b = listener_b.local_addr().unwrap();
    let key_a = identity();
    let key_b = identity();
    drop(listener_b);
    let mut service_a = NativePeerService::start(
        listener_a,
        node_a.clone(),
        NativePeerConfig {
            identity: key_a.clone(),
            peers: vec![(address_b, key_b.peer_id())],
        },
    )
    .unwrap();
    let started = Instant::now();
    while service_a.status()[0].retry_delay_ms != 30_000 {
        assert!(started.elapsed() < Duration::from_secs(40));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(service_a.status()[0].last_failure_stage, Some("connect"));
    let mut service_b = NativePeerService::start(
        TcpListener::bind(address_b).unwrap(),
        node_b.clone(),
        NativePeerConfig {
            identity: key_b,
            peers: vec![(address_a, key_a.peer_id())],
        },
    )
    .unwrap();
    await_condition(|| node_b.lock().unwrap().chain() == &winner);
    assert_eq!(
        node_b.lock().unwrap().pending(),
        std::slice::from_ref(&transfer)
    );
    let rejoined = Instant::now();
    while node_a.lock().unwrap().pending() != std::slice::from_ref(&transfer) {
        assert!(
            rejoined.elapsed() < Duration::from_secs(10),
            "orphan exists at returning peer but did not propagate: a={:?}, b={:?}",
            service_a.status(),
            service_b.status()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    mine(&node_a, &miner, &miner.pk_hex(), 1);
    await_condition(|| node_b.lock().unwrap().confirmed_height(&transfer.id()) == Some(13));
    service_a.stop();
    service_b.stop();
    assert_eq!(
        node_a.lock().unwrap().chain(),
        node_b.lock().unwrap().chain()
    );
    assert_eq!(
        node_a.lock().unwrap().chain().ledger().balance(&recipient),
        100_000_000
    );
}

fn wire_hello() -> serde_json::Value {
    serde_json::json!({ "type": "hello", "protocolVersion": 1,
        "networkId": native_testnet().network_id(), "genesisHash": native_testnet().genesis_hash().to_hex(),
        "head": {"height": 0, "hash": native_testnet().genesis_hash().to_hex()} })
}

#[test]
fn returning_peer_hint_requires_a_complete_round_and_cannot_refill_the_connect_probe() {
    let dir = TestDir::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let offline = TcpListener::bind("127.0.0.1:0").unwrap();
    let offline_address = offline.local_addr().unwrap();
    drop(offline);
    let local_key = identity();
    let remote_key = identity();
    let client =
        TlsTransport::new(remote_key.clone(), vec![(address, local_key.peer_id())]).unwrap();
    let mut service = NativePeerService::start(
        listener,
        dir.node(),
        NativePeerConfig {
            identity: local_key,
            peers: vec![(offline_address, remote_key.peer_id())],
        },
    )
    .unwrap();
    await_condition(|| service.status()[0].retry_delay_ms == 8_000);
    let failures = service.status()[0].failed_rounds;
    // Authentication alone is not a completed protocol round.
    drop(client.connect(&address).unwrap());
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(service.status()[0].failed_rounds, failures);
    let mut connection = client.connect(&address).unwrap();
    let mut invalid = wire_hello();
    invalid["networkId"] = serde_json::json!("wrong-network");
    client
        .send_json_counted_until(
            &mut connection,
            &invalid,
            4096,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    assert!(client
        .recv_json_counted_until::<serde_json::Value>(
            &mut connection,
            4096,
            Instant::now() + Duration::from_secs(2),
        )
        .is_err());
    drop(connection);
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(service.status()[0].failed_rounds, failures);
    let complete_round = || {
        let mut connection = client.connect(&address).unwrap();
        client
            .send_json_counted_until(
                &mut connection,
                &wire_hello(),
                4096,
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap();
        let hello: serde_json::Value = client
            .recv_json_counted_until(
                &mut connection,
                4096,
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap()
            .0;
        assert_eq!(hello["type"], "hello");
        client
            .send_json_counted_until(
                &mut connection,
                &serde_json::json!({"type": "done"}),
                4096,
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap();
    };
    complete_round();
    await_condition(|| service.status()[0].failed_rounds == failures + 1);
    assert_eq!(service.status()[0].retry_delay_ms, 16_000);
    assert_eq!(service.status()[0].consecutive_failures, 6);
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(600));
        complete_round();
    }
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(service.status()[0].failed_rounds, failures + 1);
    assert_eq!(service.monitor().snapshot().completed_inbound_rounds, 4);
    let started = Instant::now();
    service.stop();
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn completed_inbound_round_does_not_bypass_tls_failure_backoff() {
    let dir = TestDir::new();
    let local = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = local.local_addr().unwrap();
    let remote = TcpListener::bind("127.0.0.1:0").unwrap();
    remote.set_nonblocking(true).unwrap();
    let remote_address = remote.local_addr().unwrap();
    let local_key = identity();
    let expected_remote = identity();
    let client = TlsTransport::new(
        expected_remote.clone(),
        vec![(address, local_key.peer_id())],
    )
    .unwrap();
    let wrong_server = TlsTransport::new(identity(), vec![(address, local_key.peer_id())]).unwrap();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        while matches!(
            stop_rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ) {
            match remote.accept() {
                Ok((socket, _)) => {
                    let _ = wrong_server
                        .accept_stream_until(socket, Instant::now() + Duration::from_secs(2));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("TLS rejection fixture: {error}"),
            }
        }
    });
    let mut service = NativePeerService::start(
        local,
        dir.node(),
        NativePeerConfig {
            identity: local_key,
            peers: vec![(remote_address, expected_remote.peer_id())],
        },
    )
    .unwrap();
    await_condition(|| service.status()[0].retry_delay_ms == 8_000);
    assert_eq!(
        service.status()[0].last_failure_stage,
        Some("tls_handshake")
    );
    let failures = service.status()[0].failed_rounds;
    let mut connection = client.connect(&address).unwrap();
    client
        .send_json_counted_until(
            &mut connection,
            &wire_hello(),
            4096,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    let hello: serde_json::Value = client
        .recv_json_counted_until(
            &mut connection,
            4096,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap()
        .0;
    assert_eq!(hello["type"], "hello");
    client
        .send_json_counted_until(
            &mut connection,
            &serde_json::json!({"type": "done"}),
            4096,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    drop(connection);
    await_condition(|| service.monitor().snapshot().completed_inbound_rounds == 1);
    std::thread::sleep(Duration::from_millis(700));
    let observed_failures = service.status()[0].failed_rounds;
    service.stop();
    stop_tx.send(()).unwrap();
    server.join().unwrap();
    assert_eq!(
        observed_failures, failures,
        "inbound hint bypassed a TLS failure delay"
    );
}

#[test]
fn authenticated_wrong_network_schema_and_excessive_range_are_rejected_before_mutation() {
    let dir = TestDir::new();
    let node = dir.node();
    let miner = SigningKeyV2::from_dev_id("native-rejection-range-miner");
    mine(&node, &miner, &miner.pk_hex(), 1);
    let node_key = identity();
    let client_key = identity();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let client =
        TlsTransport::new(client_key.clone(), vec![(address, node_key.peer_id())]).unwrap();
    let mut service = NativePeerService::start(
        listener,
        node.clone(),
        NativePeerConfig {
            identity: node_key,
            peers: vec![("127.0.0.1:1".parse().unwrap(), client_key.peer_id())],
        },
    )
    .unwrap();
    for (field, value) in [
        ("networkId", serde_json::json!("foreign-native-network")),
        ("protocolVersion", serde_json::json!(0)),
        ("genesisHash", serde_json::json!("00".repeat(32))),
        ("unknownField", serde_json::json!(true)),
        (
            "head",
            serde_json::json!({"height": 100_001, "hash": "01".repeat(32)}),
        ),
    ] {
        let mut connection = client.connect(&address).unwrap();
        let mut message = wire_hello();
        message[field] = value;
        client
            .send_json_counted_until(
                &mut connection,
                &message,
                4096,
                Instant::now() + Duration::from_secs(2),
            )
            .unwrap();
        assert!(client
            .recv_json_counted_until::<serde_json::Value>(
                &mut connection,
                4096,
                Instant::now() + Duration::from_secs(2)
            )
            .is_err());
        // Each rejection below must reach its own protocol guard, not merely
        // the key's half-second reconnect throttle.
        std::thread::sleep(Duration::from_millis(510));
    }
    let mut connection = client.connect(&address).unwrap();
    client
        .send_json_counted_until(
            &mut connection,
            &wire_hello(),
            4096,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    let (hello, _): (serde_json::Value, _) = client
        .recv_json_counted_until(
            &mut connection,
            4096,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
    client.send_json_counted_until(&mut connection, &serde_json::json!({"type": "getBlocks", "snapshot": hello["head"], "from": 1, "limit": 17}), 4096, Instant::now() + Duration::from_secs(2)).unwrap();
    assert!(client
        .recv_json_counted_until::<serde_json::Value>(
            &mut connection,
            4096,
            Instant::now() + Duration::from_secs(2)
        )
        .is_err());
    service.stop();
    assert_eq!(node.lock().unwrap().chain().ledger().height(), 1);
}

#[test]
fn authenticated_read_requests_cannot_renew_round_request_or_byte_budgets() {
    let dir = TestDir::new();
    let node = dir.node();
    let miner = SigningKeyV2::from_dev_id("native-peer-round-budget-miner");
    mine(&node, &miner, &miner.pk_hex(), 10);
    let recipient = SigningKeyV2::from_dev_id("native-peer-round-budget-recipient").pk_hex();
    let transfers: Vec<_> = (0..512)
        .map(|nonce| {
            NativeTransfer::try_from(&miner.sign_for_network(
        &serde_json::json!({"schema": "boole.transfer.v1", "from": miner.pk_hex(), "to": recipient,
            "amount": "1", "fee": "1000", "nonce": nonce.to_string(), "validBefore": "100"}),
        Some(native_testnet().network_id())).unwrap()).unwrap()
        })
        .collect();
    {
        let mut node = node.lock().unwrap();
        let block = node
            .chain()
            .template(&miner.pk_hex(), &miner.pk_hex(), 660_000, &transfers)
            .unwrap()
            .mine(0, 2_000_000)
            .unwrap()
            .unwrap();
        let signature = miner
            .sign_for_network(
                &block.authorization_payload().unwrap(),
                Some(native_testnet().network_id()),
            )
            .unwrap();
        node.submit_block(block.authorize(&signature).unwrap())
            .unwrap();
    }
    let expected = node.lock().unwrap().chain().head_hash();
    let node_key = identity();
    let client_key = identity();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let client =
        TlsTransport::new(client_key.clone(), vec![(address, node_key.peer_id())]).unwrap();
    let mut service = NativePeerService::start(
        listener,
        node.clone(),
        NativePeerConfig {
            identity: node_key,
            peers: vec![("127.0.0.1:1".parse().unwrap(), client_key.peer_id())],
        },
    )
    .unwrap();
    for large in [false, true] {
        let mut connection = client.connect(&address).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut bytes = client
            .send_json_counted_until(&mut connection, &wire_hello(), 4096, deadline)
            .unwrap();
        let (hello, count): (serde_json::Value, _) = client
            .recv_json_counted_until(&mut connection, 4096, deadline)
            .unwrap();
        bytes += count;
        let request = if large {
            serde_json::json!({"type": "getBlocks", "snapshot": hello["head"], "from": 11, "limit": 1})
        } else {
            serde_json::json!({"type": "getHash", "snapshot": hello["head"], "height": 1})
        };
        let mut responses = 0;
        for _ in 0..65 {
            if client
                .send_json_counted_until(&mut connection, &request, 4096, deadline)
                .is_err()
            {
                break;
            }
            match client.recv_json_counted_until::<serde_json::Value>(
                &mut connection,
                1024 * 1024,
                deadline,
            ) {
                Ok((_, count)) => {
                    bytes += request.to_string().len() + 1 + count;
                    responses += 1;
                }
                Err(_) => break,
            }
        }
        if large {
            assert!(
                responses > 1 && responses < 64,
                "byte bound must win before request count: {responses}"
            );
            assert!(bytes <= 8 * 1024 * 1024);
            assert!(
                bytes > 7 * 1024 * 1024,
                "test must actually consume the round byte budget"
            );
        } else {
            assert_eq!(responses, 64);
        }
        std::thread::sleep(Duration::from_millis(510));
    }
    service.stop();
    assert_eq!(node.lock().unwrap().chain().head_hash(), expected);
}

#[test]
fn invalid_owner_signature_from_an_approved_peer_never_becomes_a_block() {
    let fixture_dir = TestDir::new();
    let fixture_node = fixture_dir.node();
    let miner = SigningKeyV2::from_dev_id("native-bad-peer-miner");
    mine(&fixture_node, &miner, &miner.pk_hex(), 1);
    let mut block = fixture_node.lock().unwrap().chain().blocks()[0].clone();
    block.producer_signature = "00".repeat(64);
    let dir = TestDir::new();
    let node = dir.node();
    let node_key = identity();
    let peer_key = identity();
    let mut malicious_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let malicious_addr = malicious_listener.local_addr().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let server = TlsTransport::new(
        peer_key.clone(),
        vec![(listener.local_addr().unwrap(), node_key.peer_id())],
    )
    .unwrap();
    let malicious = std::thread::spawn(move || {
        let (mut connection, _) = server.accept(&mut malicious_listener).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let _: (serde_json::Value, _) = server
            .recv_json_counted_until(&mut connection, 4096, deadline)
            .unwrap();
        let mut hello = wire_hello();
        hello["head"] = serde_json::json!({"height": 1, "hash": block.hash().unwrap().to_hex()});
        server
            .send_json_counted_until(&mut connection, &hello, 4096, deadline)
            .unwrap();
        let (request, _): (serde_json::Value, _) = server
            .recv_json_counted_until(&mut connection, 4096, deadline)
            .unwrap();
        assert_eq!(request["type"], "getBlocks");
        server.send_json_counted_until(&mut connection, &serde_json::json!({"type": "blocks", "snapshot": hello["head"], "from": 1, "blocks": [block]}), 16384, deadline).unwrap();
    });
    let mut service = NativePeerService::start(
        listener,
        node.clone(),
        NativePeerConfig {
            identity: node_key,
            peers: vec![(malicious_addr, peer_key.peer_id())],
        },
    )
    .unwrap();
    malicious.join().unwrap();
    await_condition(|| service.status()[0].failed_rounds > 0);
    assert_eq!(service.status()[0].last_failure_stage, Some("block_apply"));
    service.stop();
    assert_eq!(node.lock().unwrap().chain().ledger().height(), 0);
}

#[test]
fn peers_refuse_forks_before_the_local_undo_window_without_downloading_a_suffix() {
    let dir = TestDir::new();
    let node = dir.node();
    let miner = SigningKeyV2::from_dev_id("native-deep-local-fork");
    mine(&node, &miner, &miner.pk_hex(), 257);
    let expected = node.lock().unwrap().chain().clone();
    let node_key = identity();
    let peer_key = identity();
    let mut remote_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let remote_address = remote_listener.local_addr().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let server = TlsTransport::new(
        peer_key.clone(),
        vec![(listener.local_addr().unwrap(), node_key.peer_id())],
    )
    .unwrap();
    let remote = std::thread::spawn(move || {
        let (mut connection, _) = server.accept(&mut remote_listener).unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let _: (serde_json::Value, _) = server
            .recv_json_counted_until(&mut connection, 4096, deadline)
            .unwrap();
        let mut hello = wire_hello();
        hello["head"] = serde_json::json!({"height": 1, "hash": "aa".repeat(32)});
        server
            .send_json_counted_until(&mut connection, &hello, 4096, deadline)
            .unwrap();
        let (request, _): (serde_json::Value, _) = server
            .recv_json_counted_until(&mut connection, 4096, deadline)
            .unwrap();
        assert_eq!(request["type"], "getHash");
        server.send_json_counted_until(&mut connection, &serde_json::json!({
            "type": "hash", "snapshot": hello["head"], "height": 1, "hash": hello["head"]["hash"]
        }), 4096, deadline).unwrap();
        let (next, _): (serde_json::Value, _) = server
            .recv_json_counted_until(&mut connection, 4096, deadline)
            .unwrap();
        next
    });
    let mut service = NativePeerService::start(
        listener,
        node.clone(),
        NativePeerConfig {
            identity: node_key,
            peers: vec![(remote_address, peer_key.peer_id())],
        },
    )
    .unwrap();
    let next = remote.join().unwrap();
    await_condition(|| {
        service.status()[0].successful_rounds + service.status()[0].failed_rounds > 0
    });
    service.stop();
    assert_eq!(
        next["type"], "done",
        "cannot request unbounded-prefix replay work"
    );
    assert_eq!(service.status()[0].state, "bounded_reorg_requires_recovery");
    assert_eq!(node.lock().unwrap().chain(), &expected);
}

#[test]
fn stop_closes_stalled_tls_handshakes_and_public_peers_are_refused() {
    let dir = TestDir::new();
    let node = dir.node();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let mut service = NativePeerService::start(
        listener,
        node.clone(),
        NativePeerConfig {
            identity: identity(),
            peers: vec![],
        },
    )
    .unwrap();
    let mut stalled = Vec::new();
    for _ in 0..4 {
        stalled.push(TcpStream::connect(address).unwrap());
    }
    std::thread::sleep(Duration::from_millis(50));
    let started = Instant::now();
    service.stop();
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "stop waited for the 2-second TLS handshake timeout"
    );
    drop(stalled);
    assert!(NativePeerService::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        node,
        NativePeerConfig {
            identity: identity(),
            peers: vec![("192.0.2.1:9999".parse().unwrap(), identity().peer_id())]
        }
    )
    .is_err());
}

#[test]
fn inbound_worker_limit_is_observable_and_shutdown_releases_all_leases() {
    let dir = TestDir::new();
    let node = dir.node();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let mut service = NativePeerService::start(
        listener,
        node.clone(),
        NativePeerConfig {
            identity: identity(),
            peers: vec![],
        },
    )
    .unwrap();
    let monitor = service.monitor();
    let stalled: Vec<_> = (0..4)
        .map(|_| TcpStream::connect(address).unwrap())
        .collect();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let snapshot = serde_json::to_value(monitor.snapshot()).unwrap();
        if snapshot["activeInboundWorkers"] == 4 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "active inbound worker count unavailable"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let _refused = TcpStream::connect(address).unwrap();
    await_condition(|| {
        serde_json::to_value(monitor.snapshot()).unwrap()["rejectedConnections"]
            .as_u64()
            .is_some_and(|count| count > 0)
    });
    service.stop();
    let snapshot = serde_json::to_value(monitor.snapshot()).unwrap();
    assert_eq!(snapshot["running"], false);
    assert_eq!(snapshot["activeInboundWorkers"], 0);
    assert_eq!(snapshot["peakInboundWorkers"], 4);
    assert_eq!(snapshot["acceptedConnections"], 4);
    assert!(snapshot["authenticationFailures"].as_u64().unwrap() <= 4);
    assert_eq!(snapshot["failedInboundRounds"], 4);
    drop(stalled);
    drop(service);
    drop(node);
    // Keeping only the monitor must not retain the durable state lock.
    assert!(NativeNode::open(&dir.0).is_ok());
}

#[test]
fn consecutive_outbound_failures_back_off_without_growing_peer_state() {
    let dir = TestDir::new();
    let unavailable = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = unavailable.local_addr().unwrap();
    drop(unavailable);
    let mut service = NativePeerService::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        dir.node(),
        NativePeerConfig {
            identity: identity(),
            peers: vec![(address, identity().peer_id())],
        },
    )
    .unwrap();
    await_condition(|| service.status()[0].failed_rounds == 2);
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(
        service.status()[0].failed_rounds,
        2,
        "second failure must back off for at least one second"
    );
    assert_eq!(service.status().len(), 1);
    assert_eq!(service.status()[0].state, "retrying");
    service.stop();
}

fn serve_fork_round(
    listener: &TcpListener,
    transport: &TlsTransport,
    chain: &NativeChain,
    corrupt_signature: bool,
) -> usize {
    serve_fork_round_after_hello(listener, transport, chain, corrupt_signature, || false)
}

fn serve_fork_round_after_hello(
    listener: &TcpListener,
    transport: &TlsTransport,
    chain: &NativeChain,
    corrupt_signature: bool,
    after_hello: impl FnOnce() -> bool,
) -> usize {
    let deadline = Instant::now() + Duration::from_secs(5);
    listener.set_nonblocking(true).unwrap();
    let socket = loop {
        match listener.accept() {
            Ok((socket, _)) => break socket,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "peer did not start next round");
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("accept: {error}"),
        }
    };
    let mut connection = transport.accept_stream_until(socket, deadline).unwrap();
    let (hello, _): (serde_json::Value, _) = transport
        .recv_json_counted_until(&mut connection, 4096, deadline)
        .unwrap();
    assert_eq!(hello["type"], "hello");
    let expect_disconnect = after_hello();
    let snapshot = serde_json::json!({
        "height": chain.ledger().height(), "hash": chain.head_hash().to_hex()
    });
    transport
        .send_json_counted_until(
            &mut connection,
            &serde_json::json!({
                "type": "hello", "protocolVersion": 1,
                "networkId": native_testnet().network_id(),
                "genesisHash": native_testnet().genesis_hash().to_hex(),
                "head": snapshot
            }),
            4096,
            deadline,
        )
        .unwrap();
    if expect_disconnect {
        assert!(transport
            .recv_json_counted_until::<serde_json::Value>(&mut connection, 4096, deadline)
            .is_err());
        return 0;
    }
    let mut block_requests = 0;
    loop {
        let (request, _): (serde_json::Value, _) = transport
            .recv_json_counted_until(&mut connection, 4096, deadline)
            .unwrap();
        let response = match request["type"].as_str().unwrap() {
            "done" => return block_requests,
            "getHash" => {
                let height = request["height"].as_u64().unwrap();
                let hash = if height == 0 {
                    native_testnet().genesis_hash()
                } else {
                    chain.blocks()[height as usize - 1].hash().unwrap()
                };
                serde_json::json!({
                    "type": "hash", "snapshot": snapshot, "height": height, "hash": hash.to_hex()
                })
            }
            "getBlocks" => {
                block_requests += 1;
                let from = request["from"].as_u64().unwrap() as usize;
                let limit = request["limit"].as_u64().unwrap() as usize;
                let end = (from - 1 + limit).min(chain.blocks().len());
                let mut blocks = chain.blocks()[from - 1..end].to_vec();
                if corrupt_signature {
                    assert_eq!(end, chain.blocks().len());
                    blocks[0].producer_signature = "00".repeat(64);
                }
                serde_json::json!({
                    "type": "blocks", "snapshot": snapshot, "from": from,
                    "blocks": blocks
                })
            }
            "getPending" => serde_json::json!({
                "type": "pending", "snapshot": snapshot, "offset": 0, "total": 0, "transfers": []
            }),
            other => panic!("unexpected peer request: {other}"),
        };
        transport
            .send_json_counted_until(&mut connection, &response, 1024 * 1024, deadline)
            .unwrap();
        if corrupt_signature && request["type"] == "getBlocks" {
            assert!(transport
                .recv_json_counted_until::<serde_json::Value>(&mut connection, 4096, deadline)
                .is_err());
            return block_requests;
        }
    }
}

#[test]
fn an_unchanged_verified_losing_fork_is_not_downloaded_on_every_poll() {
    let local_dir = TestDir::new();
    let remote_dir = TestDir::new();
    let local_node = local_dir.node();
    let remote_node = remote_dir.node();
    let local_miner = SigningKeyV2::from_dev_id("native-repeat-fork-local");
    let remote_miner = SigningKeyV2::from_dev_id("native-repeat-fork-remote");
    mine(&local_node, &local_miner, &local_miner.pk_hex(), 2);
    mine(&remote_node, &remote_miner, &remote_miner.pk_hex(), 1);
    let remote_chain = remote_node.lock().unwrap().chain().clone();
    mine(&remote_node, &remote_miner, &remote_miner.pk_hex(), 3);
    let winning_chain = remote_node.lock().unwrap().chain().clone();
    let expected = local_node.lock().unwrap().chain().clone();
    assert!(!remote_chain.outranks(&expected));
    let local_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let remote_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let remote_address = remote_listener.local_addr().unwrap();
    let local_key = identity();
    let remote_key = identity();
    let transport = TlsTransport::new(
        remote_key.clone(),
        vec![(local_listener.local_addr().unwrap(), local_key.peer_id())],
    )
    .unwrap();
    let final_chain = winning_chain.clone();
    let (observed_tx, observed_rx) = std::sync::mpsc::channel();
    let (advance_tx, advance_rx) = std::sync::mpsc::channel::<()>();
    let remote = std::thread::spawn(move || {
        for index in 0..5 {
            let chain = if index == 4 {
                &winning_chain
            } else {
                &remote_chain
            };
            let requests = serve_fork_round(&remote_listener, &transport, chain, false);
            if observed_tx.send(requests).is_err()
                || (index < 4 && advance_rx.recv_timeout(Duration::from_secs(5)).is_err())
            {
                break;
            }
        }
    });
    let config = NativePeerConfig {
        identity: local_key,
        peers: vec![(remote_address, remote_key.peer_id())],
    };
    let mut service =
        NativePeerService::start(local_listener, local_node.clone(), config.clone()).unwrap();
    let observed = || observed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(observed(), 1, "first candidate must be fully downloaded");
    advance_tx.send(()).unwrap();
    assert_eq!(observed(), 0, "unchanged losing fork was downloaded again");
    assert_eq!(local_node.lock().unwrap().chain(), &expected);
    // The memory belongs to this configured worker's lifetime, not the disk.
    service.stop();
    service = NativePeerService::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        local_node.clone(),
        config,
    )
    .unwrap();
    advance_tx.send(()).unwrap();
    assert_eq!(observed(), 1, "service restart reused stale fork memory");
    mine(&local_node, &local_miner, &local_miner.pk_hex(), 1);
    advance_tx.send(()).unwrap();
    assert_eq!(observed(), 1, "changed local head skipped revalidation");
    advance_tx.send(()).unwrap();
    assert_eq!(observed(), 1, "changed remote head skipped revalidation");
    service.stop();
    remote.join().unwrap();
    assert_eq!(local_node.lock().unwrap().chain(), &final_chain);
    drop(service);
    drop(local_node);
    assert_eq!(
        NativeNode::open(&local_dir.0).unwrap().chain(),
        &final_chain
    );
}

#[test]
fn an_invalid_candidate_cannot_suppress_a_later_valid_body_at_the_same_head() {
    let local_dir = TestDir::new();
    let remote_dir = TestDir::new();
    let local_node = local_dir.node();
    let remote_node = remote_dir.node();
    let local_miner = SigningKeyV2::from_dev_id("native-invalid-cache-local");
    let remote_miner = SigningKeyV2::from_dev_id("native-invalid-cache-remote");
    mine(&local_node, &local_miner, &local_miner.pk_hex(), 2);
    mine(&remote_node, &remote_miner, &remote_miner.pk_hex(), 1);
    let remote_chain = remote_node.lock().unwrap().chain().clone();
    let expected = local_node.lock().unwrap().chain().clone();
    let local_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let remote_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let remote_address = remote_listener.local_addr().unwrap();
    let local_key = identity();
    let remote_key = identity();
    let transport = TlsTransport::new(
        remote_key.clone(),
        vec![(local_listener.local_addr().unwrap(), local_key.peer_id())],
    )
    .unwrap();
    let remote = std::thread::spawn(move || {
        [true, false, false]
            .map(|corrupt| serve_fork_round(&remote_listener, &transport, &remote_chain, corrupt))
    });
    let mut service = NativePeerService::start(
        local_listener,
        local_node.clone(),
        NativePeerConfig {
            identity: local_key,
            peers: vec![(remote_address, remote_key.peer_id())],
        },
    )
    .unwrap();
    assert_eq!(remote.join().unwrap(), [1, 1, 0]);
    service.stop();
    assert!(service.status()[0].failed_rounds >= 1);
    assert_eq!(local_node.lock().unwrap().chain(), &expected);
}

#[test]
fn a_remembered_fork_preference_does_not_hide_storage_loss_during_the_next_hello() {
    let local_dir = TestDir::new();
    let remote_dir = TestDir::new();
    let local_node = local_dir.node();
    let remote_node = remote_dir.node();
    let local_miner = SigningKeyV2::from_dev_id("native-cache-readiness-local");
    let remote_miner = SigningKeyV2::from_dev_id("native-cache-readiness-remote");
    mine(&local_node, &local_miner, &local_miner.pk_hex(), 2);
    mine(&remote_node, &remote_miner, &remote_miner.pk_hex(), 1);
    let remote_chain = remote_node.lock().unwrap().chain().clone();
    let expected = local_node.lock().unwrap().chain().clone();
    let history = local_dir.0.join(boole_node::NATIVE_BLOCKS_FILE);
    let preserved = local_dir.0.join("preserved-history.ndjson");
    let before = std::fs::read(&history).unwrap();
    let local_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let remote_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let remote_address = remote_listener.local_addr().unwrap();
    let local_key = identity();
    let remote_key = identity();
    let transport = TlsTransport::new(
        remote_key.clone(),
        vec![(local_listener.local_addr().unwrap(), local_key.peer_id())],
    )
    .unwrap();
    let history_to_move = history.clone();
    let preserved_copy = preserved.clone();
    let remote = std::thread::spawn(move || {
        assert_eq!(
            serve_fork_round(&remote_listener, &transport, &remote_chain, false),
            1
        );
        serve_fork_round_after_hello(&remote_listener, &transport, &remote_chain, false, || {
            // The local hello has already read a ready node and its unchanged
            // head. Move only this disposable fixture's file, preserving bytes.
            std::fs::rename(history_to_move, preserved_copy).unwrap();
            true
        });
    });
    let mut service = NativePeerService::start(
        local_listener,
        local_node.clone(),
        NativePeerConfig {
            identity: local_key,
            peers: vec![(remote_address, remote_key.peer_id())],
        },
    )
    .unwrap();
    remote.join().unwrap();
    await_condition(|| service.status()[0].failed_rounds >= 1);
    assert_eq!(service.status()[0].last_failure_stage, Some("local_state"));
    service.stop();
    assert!(local_node.lock().unwrap().ensure_ready().is_err());
    assert_eq!(local_node.lock().unwrap().chain(), &expected);
    assert_eq!(std::fs::read(&preserved).unwrap(), before);
    drop(service);
    drop(local_node);
    std::fs::rename(preserved, history).unwrap();
    assert_eq!(NativeNode::open(&local_dir.0).unwrap().chain(), &expected);
}

#[test]
fn a_stalled_authenticated_peer_does_not_delay_another_peers_verified_blocks() {
    let local_dir = TestDir::new();
    let healthy_dir = TestDir::new();
    let local_node = local_dir.node();
    let healthy_node = healthy_dir.node();
    let miner = SigningKeyV2::from_dev_id("native-peer-isolation-miner");
    mine(&healthy_node, &miner, &miner.pk_hex(), 1);
    let expected = healthy_node.lock().unwrap().chain().clone();
    let local_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let local_address = local_listener.local_addr().unwrap();
    let slow_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let slow_address = slow_listener.local_addr().unwrap();
    let healthy_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let healthy_address = healthy_listener.local_addr().unwrap();
    let local_key = identity();
    let slow_key = identity();
    let healthy_key = identity();
    let mut healthy = NativePeerService::start(
        healthy_listener,
        healthy_node,
        NativePeerConfig {
            identity: healthy_key.clone(),
            // This node cannot push/synchronize into local_node: the only path
            // to the new block is local_node's second outbound peer.
            peers: vec![("127.0.0.1:1".parse().unwrap(), local_key.peer_id())],
        },
    )
    .unwrap();
    let slow_transport =
        TlsTransport::new(slow_key.clone(), vec![(local_address, local_key.peer_id())]).unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let stalled = std::thread::spawn(move || {
        let (socket, _) = slow_listener.accept().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut connection = slow_transport
            .accept_stream_until(socket, deadline)
            .unwrap();
        let (hello, _): (serde_json::Value, _) = slow_transport
            .recv_json_counted_until(&mut connection, 4096, deadline)
            .unwrap();
        assert_eq!(hello["type"], "hello");
        entered_tx.send(()).unwrap();
        // Hold the authenticated socket without replying. Its ten-second
        // round timeout must not delay the independently pinned healthy peer.
        let _ = release_rx.recv_timeout(Duration::from_secs(5));
        drop(connection);
    });
    let mut local = NativePeerService::start(
        local_listener,
        local_node.clone(),
        NativePeerConfig {
            identity: local_key,
            peers: vec![
                (slow_address, slow_key.peer_id()),
                (healthy_address, healthy_key.peer_id()),
            ],
        },
    )
    .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while local_node.lock().unwrap().chain() != &expected {
        assert!(
            Instant::now() < deadline,
            "one stalled authenticated peer blocked the healthy peer"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(release_tx);
    local.stop();
    healthy.stop();
    stalled.join().unwrap();
    drop(local);
    drop(local_node);
    assert_eq!(NativeNode::open(&local_dir.0).unwrap().chain(), &expected);
}

#[test]
fn all_eight_outbound_peers_have_bounded_rounds_and_shutdown_interrupts_them() {
    let dir = TestDir::new();
    let node = dir.node();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let local_address = listener.local_addr().unwrap();
    let local_key = identity();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let mut remotes = Vec::new();
    let mut peers = Vec::new();
    for _ in 0..8 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let key = identity();
        peers.push((listener.local_addr().unwrap(), key.peer_id()));
        let transport = TlsTransport::new(key, vec![(local_address, local_key.peer_id())]).unwrap();
        let entered = entered_tx.clone();
        remotes.push(std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut connection = transport.accept_stream_until(socket, deadline).unwrap();
            let (hello, _): (serde_json::Value, _) = transport
                .recv_json_counted_until(&mut connection, 4096, deadline)
                .unwrap();
            assert_eq!(hello["type"], "hello");
            entered.send(()).unwrap();
            assert!(transport
                .recv_json_counted_until::<serde_json::Value>(&mut connection, 4096, deadline)
                .is_err());
        }));
    }
    let mut config = NativePeerConfig {
        identity: local_key,
        peers,
    };
    config
        .peers
        .push(("127.0.0.1:1".parse().unwrap(), identity().peer_id()));
    assert!(
        config.validate().is_err(),
        "ninth peer exceeded worker capacity"
    );
    config.peers.pop();
    let mut service = NativePeerService::start(listener, node.clone(), config).unwrap();
    let monitor = service.monitor();
    for _ in 0..8 {
        entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    }
    let before = monitor.snapshot();
    assert_eq!(before.limits.max_outbound_workers, 8);
    assert_eq!(before.active_outbound_rounds, 8);
    assert_eq!(before.peak_outbound_rounds, 8);
    assert_eq!(before.peers.len(), 8);
    let stopped_at = Instant::now();
    service.stop();
    assert!(stopped_at.elapsed() < Duration::from_secs(1));
    let after = monitor.snapshot();
    assert!(!after.running);
    assert_eq!(after.active_outbound_rounds, 0);
    assert_eq!(after.peak_outbound_rounds, 8);
    for remote in remotes {
        remote.join().unwrap();
    }
    drop(service);
    drop(node);
    assert_eq!(
        NativeNode::open(&dir.0).unwrap().chain().ledger().height(),
        0
    );
}

#[test]
fn simultaneous_peer_pulls_share_one_durable_transfer_and_confirm_it_once() {
    let local_dir = TestDir::new();
    let local_node = local_dir.node();
    let owner = SigningKeyV2::from_dev_id("native-peer-concurrent-owner");
    let recipient = SigningKeyV2::from_dev_id("native-peer-concurrent-recipient").pk_hex();
    mine(&local_node, &owner, &owner.pk_hex(), 10);
    let initial = local_node.lock().unwrap().chain().blocks().to_vec();
    let transfer = NativeTransfer::try_from(
        &owner
            .sign_for_network(
                &serde_json::json!({
                    "schema": "boole.transfer.v1", "from": owner.pk_hex(), "to": recipient,
                    "amount": "100000000", "fee": "1000", "nonce": "0", "validBefore": "100"
                }),
                Some(native_testnet().network_id()),
            )
            .unwrap(),
    )
    .unwrap();
    let local_key = identity();
    let mut remote_dirs = Vec::new();
    let mut services = Vec::new();
    let mut peers = Vec::new();
    for _ in 0..3 {
        let dir = TestDir::new();
        let node = dir.node();
        {
            let mut node = node.lock().unwrap();
            assert!(node.adopt_chain(&initial).unwrap());
            assert!(node.submit_transfer(transfer.clone()).unwrap());
        }
        let key = identity();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        peers.push((listener.local_addr().unwrap(), key.peer_id()));
        services.push(
            NativePeerService::start(
                listener,
                node,
                NativePeerConfig {
                    identity: key,
                    peers: vec![("127.0.0.1:1".parse().unwrap(), local_key.peer_id())],
                },
            )
            .unwrap(),
        );
        remote_dirs.push(dir);
    }
    let mut local = NativePeerService::start(
        TcpListener::bind("127.0.0.1:0").unwrap(),
        local_node.clone(),
        NativePeerConfig {
            identity: local_key,
            peers,
        },
    )
    .unwrap();
    await_condition(|| {
        local
            .status()
            .iter()
            .all(|status| status.successful_rounds >= 1)
    });
    local.stop();
    for remote in &mut services {
        remote.stop();
    }
    assert_eq!(
        local_node.lock().unwrap().pending(),
        std::slice::from_ref(&transfer)
    );
    mine(&local_node, &owner, &owner.pk_hex(), 1);
    drop(local);
    drop(local_node);
    let recovered = NativeNode::open(&local_dir.0).unwrap();
    assert_eq!(recovered.confirmed_height(&transfer.id()), Some(11));
    assert!(recovered.pending().is_empty());
    assert_eq!(recovered.chain().ledger().next_nonce(&owner.pk_hex()), 1);
    assert_eq!(recovered.chain().ledger().balance(&recipient), 100_000_000);
    drop(services);
    drop(remote_dirs);
}

#[test]
fn an_authenticated_key_cannot_start_unlimited_rounds_and_recovers_after_cooldown() {
    let dir = TestDir::new();
    let node_key = identity();
    let client_key = identity();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let client =
        TlsTransport::new(client_key.clone(), vec![(address, node_key.peer_id())]).unwrap();
    let mut service = NativePeerService::start(
        listener,
        dir.node(),
        NativePeerConfig {
            identity: node_key,
            peers: vec![("127.0.0.1:1".parse().unwrap(), client_key.peer_id())],
        },
    )
    .unwrap();
    let try_round = || {
        let mut connection = client.connect(&address).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        if client
            .send_json_counted_until(&mut connection, &wire_hello(), 4096, deadline)
            .is_err()
        {
            return false;
        }
        if client
            .recv_json_counted_until::<serde_json::Value>(&mut connection, 4096, deadline)
            .is_err()
        {
            return false;
        }
        client
            .send_json_counted_until(
                &mut connection,
                &serde_json::json!({"type": "done"}),
                4096,
                deadline,
            )
            .unwrap();
        true
    };
    assert!(try_round());
    await_condition(|| service.monitor().snapshot().completed_inbound_rounds == 1);
    assert!(
        !try_round(),
        "same authenticated key bypassed its per-key cooldown"
    );
    std::thread::sleep(Duration::from_millis(550));
    assert!(try_round());
    await_condition(|| service.monitor().snapshot().completed_inbound_rounds == 2);
    service.stop();
    assert_eq!(service.status().len(), 1);
}
