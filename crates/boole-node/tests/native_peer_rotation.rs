//! Closed-local transport membership lifecycle; no external enrollment authority.
use boole_core::native_chain::{NativeBlockTemplate, NativeTransfer};
use boole_core::native_network::native_testnet;
use boole_core::SigningKeyV2;
use boole_node::NativeNode;
use boole_p2p::{PeerId, TlsIdentity, TlsTransport};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Scratch {
    path: PathBuf,
    identity: (u64, u64),
}
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boole-native-peer-rotation-{}",
            boole_testkit::rand_suffix()
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        Self {
            path,
            identity: (metadata.dev(), metadata.ino()),
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
            metadata.is_dir() && (metadata.dev(), metadata.ino()) == self.identity
        }) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn rpc(address: SocketAddr, method: &str, path: &str, body: Value) -> Option<Value> {
    let body = if method == "GET" {
        String::new()
    } else {
        body.to_string()
    };
    let mut socket = TcpStream::connect_timeout(&address, Duration::from_millis(200)).ok()?;
    socket.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .ok()?;
    write!(socket, "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).ok()?;
    let mut response = String::new();
    socket.take(131_072).read_to_string(&mut response).ok()?;
    if !response.starts_with("HTTP/1.1 200 ") {
        return None;
    }
    serde_json::from_str(response.split_once("\r\n\r\n")?.1).ok()
}

fn wait(label: &str, mut predicate: impl FnMut() -> bool) {
    let started = Instant::now();
    while !predicate() {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "phase deadline: {label}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "phase deadline: {label}"
    );
    eprintln!(
        "native-peer-rotation-phase {label} elapsedMs={}",
        started.elapsed().as_millis()
    );
}

fn generate_key(path: &Path) -> PeerId {
    let output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .args(["peer-keygen", "--file"])
        .arg(path)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .output()
        .unwrap();
    assert!(output.status.success(), "key generation failed");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    let identity = boole_node::load_native_peer_key(path).unwrap();
    assert_eq!(result["peerId"], identity.peer_id().to_hex());
    assert_eq!(result.as_object().unwrap().len(), 2);
    identity.peer_id()
}

struct Actor {
    child: Option<Child>,
    state: PathBuf,
    key: PathBuf,
    peer: PeerId,
    rpc: SocketAddr,
    p2p: SocketAddr,
}
impl Actor {
    fn new(root: &Path, name: &str, rpc: SocketAddr, p2p: SocketAddr) -> Self {
        let key = root.join(format!("{name}.pk8"));
        let peer = generate_key(&key);
        Self {
            child: None,
            state: root.join(name),
            key,
            peer,
            rpc,
            p2p,
        }
    }
    fn endpoint(&self) -> (SocketAddr, PeerId) {
        (self.p2p, self.peer)
    }
    fn start(&mut self, peers: &[(SocketAddr, PeerId)]) {
        assert!(self.child.is_none());
        let mut command = Command::new(env!("CARGO_BIN_EXE_boole-node"));
        command
            .args([
                "run-native-local",
                "--addr",
                &self.rpc.to_string(),
                "--p2p-addr",
                &self.p2p.to_string(),
                "--peer-key",
            ])
            .arg(&self.key)
            .arg("--state-dir")
            .arg(&self.state)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (address, key) in peers {
            command.args(["--peer", &format!("{}@{address}", key.to_hex())]);
        }
        self.child = Some(command.spawn().unwrap());
        wait("process-ready", || {
            assert!(
                self.child.as_mut().unwrap().try_wait().unwrap().is_none(),
                "node exited"
            );
            rpc(self.rpc, "GET", "/native/info", Value::Null).is_some()
        });
        assert_eq!(self.get("/native/peers")["localPeerId"], self.peer.to_hex());
    }
    fn stop(&mut self) {
        let child = self.child.as_mut().expect("running node");
        assert!(child.try_wait().unwrap().is_none());
        let started = Instant::now();
        assert!(Command::new("/bin/kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success());
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "normal stop failed: {status}");
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(3),
                "normal stop deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(started.elapsed() < Duration::from_secs(3));
        self.child = None;
        eprintln!(
            "native-peer-rotation-stop elapsedMs={}",
            started.elapsed().as_millis()
        );
    }
    fn get(&self, path: &str) -> Value {
        rpc(self.rpc, "GET", path, Value::Null).unwrap_or_else(|| panic!("GET {path}"))
    }
    fn post(&self, path: &str, body: Value) -> Value {
        rpc(self.rpc, "POST", path, body).unwrap_or_else(|| panic!("POST {path}"))
    }
}
impl Drop for Actor {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn await_head(actors: &[&Actor], head: &str, height: u64) {
    wait("head-agreement", || {
        actors.iter().all(|actor| {
            rpc(actor.rpc, "GET", "/native/info", Value::Null).is_some_and(|info| {
                info["headHash"] == head && info["height"] == height.to_string()
            })
        })
    });
}

fn await_membership(actors: &[&Actor], count: usize) {
    wait("reciprocal-membership", || {
        actors.iter().all(|actor| {
            rpc(actor.rpc, "GET", "/native/peers", Value::Null).is_some_and(|status| {
                status["peers"].as_array().is_some_and(|peers| {
                    peers.len() == count
                        && peers.iter().all(|peer| {
                            peer["successfulRounds"].as_u64().unwrap() > 0
                                && peer["state"] == "snapshot_match"
                        })
                })
            })
        })
    });
}

fn await_refused(actor: &Actor) {
    wait("unapproved-identity-refused", || {
        let status = actor.get("/native/peers");
        status["peers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|peer| peer["failedRounds"].as_u64().unwrap() > 0 && peer["successfulRounds"] == 0)
    });
}

fn transfer(owner: &SigningKeyV2, recipient: &str, nonce: u64) -> NativeTransfer {
    NativeTransfer::try_from(
        &owner
            .sign_for_network(
                &json!({"schema":"boole.transfer.v1", "from":owner.pk_hex(), "to":recipient,
            "amount":"100000000", "fee":"1000", "nonce":nonce.to_string(), "validBefore":"100"}),
                Some(native_testnet().network_id()),
            )
            .unwrap(),
    )
    .unwrap()
}

fn mine(actor: &Actor, producer: &SigningKeyV2, height: u64) -> String {
    let template: NativeBlockTemplate = serde_json::from_value(actor.post(
        "/native/template",
        json!({"producerPk":producer.pk_hex(), "rewardPk":producer.pk_hex(),
            "timestampMs":height * 60_000}),
    ))
    .unwrap();
    let block = template.mine(0, 2_000_000).unwrap().unwrap();
    let signature = producer
        .sign_for_network(
            &block.authorization_payload().unwrap(),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    let block = block.authorize(&signature).unwrap();
    let hash = block.hash().unwrap().to_hex();
    assert_eq!(actor.post("/native/blocks", json!(block))["accepted"], true);
    hash
}

fn hello() -> Value {
    json!({"type":"hello", "protocolVersion":1, "networkId":native_testnet().network_id(),
        "genesisHash":native_testnet().genesis_hash().to_hex(),
        "head":{"height":0, "hash":native_testnet().genesis_hash().to_hex()}})
}

fn refuse_old_key(identity: &TlsIdentity, actor: &Actor) {
    let before = actor.get("/native/peers")["authenticationFailures"]
        .as_u64()
        .unwrap();
    let transport = TlsTransport::new(identity.clone(), vec![actor.endpoint()]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let accepted_hello = transport
        .connect_until(&actor.p2p, deadline)
        .ok()
        .is_some_and(|mut connection| {
            transport
                .send_json_counted_until(&mut connection, &hello(), 4096, deadline)
                .is_ok()
                && transport
                    .recv_json_counted_until::<Value>(&mut connection, 4096, deadline)
                    .is_ok()
        });
    assert!(
        !accepted_hello,
        "retired identity obtained an authenticated native hello"
    );
    wait("retired-key-authentication-failure", || {
        actor.get("/native/peers")["authenticationFailures"]
            .as_u64()
            .unwrap()
            > before
    });
}

#[test]
fn stopped_configuration_revokes_old_transport_key_and_explicit_reenrollment_preserves_ledger() {
    let started = Instant::now();
    let scratch = Scratch::new();
    let ports: Vec<_> = (0..6)
        .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
        .collect();
    let addresses: Vec<_> = ports
        .iter()
        .map(|port| port.local_addr().unwrap())
        .collect();
    let mut a = Actor::new(&scratch.path, "a", addresses[0], addresses[1]);
    let mut b = Actor::new(&scratch.path, "b", addresses[2], addresses[3]);
    let mut c = Actor::new(&scratch.path, "c", addresses[4], addresses[5]);
    let old_key_path = b.key.clone();
    let old_key_bytes = std::fs::read(&old_key_path).unwrap();
    let old_identity = boole_node::load_native_peer_key(&old_key_path).unwrap();
    let producer = SigningKeyV2::from_dev_id("native-peer-rotation-producer");
    let owner = SigningKeyV2::from_dev_id("native-peer-rotation-wallet-owner");
    let recipient = SigningKeyV2::from_dev_id("native-peer-rotation-recipient").pk_hex();
    assert_ne!(owner.pk_hex(), b.peer.to_hex());
    let initial_head = {
        let mut node = NativeNode::open(&a.state).unwrap();
        for height in 1..=10 {
            let block = node
                .template(&producer.pk_hex(), &owner.pk_hex(), height * 60_000)
                .unwrap()
                .mine(0, 2_000_000)
                .unwrap()
                .unwrap();
            let signature = producer
                .sign_for_network(
                    &block.authorization_payload().unwrap(),
                    Some(native_testnet().network_id()),
                )
                .unwrap();
            node.submit_block(block.authorize(&signature).unwrap())
                .unwrap();
        }
        node.chain().head_hash().to_hex()
    };
    drop(ports);
    a.start(&[b.endpoint(), c.endpoint()]);
    b.start(&[a.endpoint(), c.endpoint()]);
    c.start(&[a.endpoint(), b.endpoint()]);
    await_head(&[&a, &b, &c], &initial_head, 10);
    await_membership(&[&a, &b, &c], 2);
    let initial_history = std::fs::read(b.state.join(boole_node::NATIVE_BLOCKS_FILE)).unwrap();
    let manifests: Vec<_> = [&a, &b, &c]
        .iter()
        .map(|actor| std::fs::read(actor.state.join("state.manifest.json")).unwrap())
        .collect();

    // An authenticated old-key connection must not survive normal retirement.
    b.stop();
    let old_client = TlsTransport::new(old_identity.clone(), vec![a.endpoint()]).unwrap();
    let mut held = None;
    wait("old-key-live-authenticated-connection", || {
        let deadline = Instant::now() + Duration::from_secs(2);
        if let Ok(mut connection) = old_client.connect_until(&a.p2p, deadline) {
            if old_client
                .send_json_counted_until(&mut connection, &hello(), 4096, deadline)
                .is_ok()
                && old_client
                    .recv_json_counted_until::<Value>(&mut connection, 4096, deadline)
                    .is_ok()
            {
                held = Some(connection);
                return true;
            }
        }
        false
    });
    a.stop();
    c.stop();
    assert!(old_client
        .recv_json_counted_until::<Value>(
            &mut held.take().unwrap(),
            4096,
            Instant::now() + Duration::from_secs(1)
        )
        .is_err());
    a.start(&[c.endpoint()]);
    c.start(&[a.endpoint()]);
    await_membership(&[&a, &c], 1);
    b.start(&[a.endpoint(), c.endpoint()]);
    await_refused(&b);
    refuse_old_key(&old_identity, &a);
    refuse_old_key(&old_identity, &c);
    assert_eq!(b.get("/native/info")["headHash"], initial_head);

    // A new file/identity alone does not enroll it at the other nodes.
    b.stop();
    let new_key_path = scratch.path.join("b-replacement.pk8");
    b.peer = generate_key(&new_key_path);
    b.key = new_key_path.clone();
    assert_ne!(b.peer, old_identity.peer_id());
    let replacement_bytes = std::fs::read(&new_key_path).unwrap();
    let denied_before: Vec<_> = [&a, &c]
        .iter()
        .map(|actor| {
            actor.get("/native/peers")["authenticationFailures"]
                .as_u64()
                .unwrap()
        })
        .collect();
    b.start(&[a.endpoint(), c.endpoint()]);
    await_refused(&b);
    wait("replacement-key-server-authentication-refusal", || {
        [&a, &c].iter().zip(&denied_before).all(|(actor, before)| {
            actor.get("/native/peers")["authenticationFailures"]
                .as_u64()
                .unwrap()
                > *before
        })
    });
    let tx0 = transfer(&owner, &recipient, 0);
    assert_eq!(c.post("/native/transfers", json!(tx0))["status"], "pending");
    wait("healthy-peer-transfer-propagation", || {
        a.get(&format!("/native/transactions/{}", tx0.id().to_hex()))["status"] == "pending"
    });
    let head11 = mine(&a, &producer, 11);
    await_head(&[&a, &c], &head11, 11);
    assert_eq!(b.get("/native/info")["headHash"], initial_head);

    // The deliberate reciprocal mapping update takes effect only on restart.
    a.stop();
    c.stop();
    a.start(&[b.endpoint(), c.endpoint()]);
    c.start(&[a.endpoint(), b.endpoint()]);
    await_head(&[&a, &b, &c], &head11, 11);
    await_membership(&[&a, &b, &c], 2);
    for actor in [&a, &b, &c] {
        let status = actor.get("/native/peers");
        assert!(status["peers"]
            .as_array()
            .unwrap()
            .iter()
            .all(|peer| { peer["peerId"] != old_identity.peer_id().to_hex() }));
        assert_eq!(
            actor.post("/native/transfers", json!(tx0))["status"],
            "confirmed"
        );
    }
    refuse_old_key(&old_identity, &a);
    refuse_old_key(&old_identity, &c);
    let tx1 = transfer(&owner, &recipient, 1);
    assert_eq!(b.post("/native/transfers", json!(tx1))["status"], "pending");
    wait("replacement-peer-transfer-propagation", || {
        [&a, &c].iter().all(|actor| {
            actor.get(&format!("/native/transactions/{}", tx1.id().to_hex()))["status"] == "pending"
        })
    });
    let final_head = mine(&a, &producer, 12);
    await_head(&[&a, &b, &c], &final_head, 12);
    await_membership(&[&a, &b, &c], 2);
    for actor in [&a, &b, &c] {
        let account = actor.get(&format!("/native/accounts/{}", owner.pk_hex()));
        assert_eq!(account["confirmedNonce"], "2");
        assert_eq!(account["balance"], "49999799998000");
        assert_eq!(
            actor.get(&format!("/native/accounts/{recipient}"))["balance"],
            "200000000"
        );
        for tx in [&tx0, &tx1] {
            assert_eq!(
                actor.get(&format!("/native/transactions/{}", tx.id().to_hex()))["status"],
                "confirmed"
            );
            assert_eq!(
                actor.post("/native/transfers", json!(tx))["status"],
                "confirmed"
            );
        }
        assert_eq!(
            actor.get("/native/info")["resources"]["pendingTransfers"],
            0
        );
    }
    for actor in [&mut a, &mut b, &mut c] {
        actor.stop();
    }
    let final_history = std::fs::read(a.state.join(boole_node::NATIVE_BLOCKS_FILE)).unwrap();
    assert!(final_history.starts_with(&initial_history));
    let mut audits = Vec::new();
    for (actor, manifest) in [&a, &b, &c].iter().zip(manifests) {
        assert_eq!(
            std::fs::read(actor.state.join("state.manifest.json")).unwrap(),
            manifest
        );
        assert_eq!(
            std::fs::read(actor.state.join(boole_node::NATIVE_BLOCKS_FILE)).unwrap(),
            final_history
        );
        let audit = serde_json::to_value(
            boole_node::audit_native_state(&actor.state, Some(&final_head)).unwrap(),
        )
        .unwrap();
        assert_eq!(audit["height"], "12");
        assert_eq!(audit["accounting"]["issuedAtoms"], "60000000000000");
        assert_eq!(audit["accounting"]["balanceAtoms"], "60000000000000");
        assert_eq!(audit["confirmedTransfers"]["count"], 2);
        assert_eq!(audit["confirmedTransfers"]["amountAtoms"], "200000000");
        assert_eq!(audit["confirmedTransfers"]["feeAtoms"], "2000");
        assert_eq!(audit["resources"]["pendingTransfers"], 0);
        audits.push(audit);
    }
    assert_eq!(audits[0], audits[1]);
    assert_eq!(audits[1], audits[2]);
    assert_eq!(std::fs::read(&old_key_path).unwrap(), old_key_bytes);
    assert_eq!(std::fs::read(&new_key_path).unwrap(), replacement_bytes);
    for address in addresses {
        assert!(TcpListener::bind(address).is_ok());
    }
    assert!(
        started.elapsed() < Duration::from_secs(120),
        "scenario deadline"
    );
    eprintln!(
        "native-peer-rotation-result {}",
        json!({
            "elapsedMs":started.elapsed().as_millis(), "retiredPeerId":old_identity.peer_id().to_hex(),
            "replacementPeerId":b.peer.to_hex(), "ownerPk":owner.pk_hex(),
            "head":final_head, "audit":audits[0], "preservedOriginalKey":true,
            "closedOldConnection":true, "explicitReenrollment":true
        })
    );
}
