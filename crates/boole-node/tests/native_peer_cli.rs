use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn info(addr: SocketAddr) -> Option<serde_json::Value> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(200)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
    write!(
        stream,
        "GET /native/info HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut out = String::new();
    stream.read_to_string(&mut out).ok()?;
    serde_json::from_str(out.split("\r\n\r\n").nth(1)?).ok()
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boole-native-peer-cli-{}",
            boole_testkit::rand_suffix()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn generated_transport_key_is_private_loadable_and_never_printed_or_overwritten() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new();
    let key = dir.0.join("transport.pk8");
    let generate = || {
        Command::new(env!("CARGO_BIN_EXE_boole-node"))
            .args(["peer-keygen", "--file"])
            .arg(&key)
            .output()
            .unwrap()
    };
    let output = generate();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let public: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let bytes = std::fs::read(&key).unwrap();
    let identity = boole_p2p::TlsIdentity::from_pkcs8(&bytes).unwrap();
    assert_eq!(public["peerId"], identity.peer_id().to_hex());
    assert_eq!(public.as_object().unwrap().len(), 2);
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&hex::encode(&bytes)));
    assert_eq!(
        std::fs::metadata(&key).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!generate().status.success());
    assert_eq!(std::fs::read(&key).unwrap(), bytes);
    assert_eq!(
        boole_node::load_native_peer_key(&key).unwrap().peer_id(),
        identity.peer_id()
    );
}

#[test]
fn unsafe_key_files_and_public_or_incomplete_peer_configuration_fail_before_state_creation() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = Scratch::new();
    let key = dir.0.join("key.pk8");
    let id = boole_node::create_native_peer_key(&key).unwrap();
    let original = std::fs::read(&key).unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(boole_node::load_native_peer_key(&key).is_err());
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.0.join("link.pk8");
    symlink(&key, &link).unwrap();
    assert!(boole_node::load_native_peer_key(&link).is_err());
    assert!(boole_node::create_native_peer_key(&link).is_err());
    let hard = dir.0.join("hard.pk8");
    std::fs::hard_link(&key, &hard).unwrap();
    assert!(boole_node::load_native_peer_key(&key).is_err());
    std::fs::remove_file(&hard).unwrap();
    let fifo = dir.0.join("fifo");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let before = Instant::now();
    assert!(boole_node::load_native_peer_key(&fifo).is_err());
    assert!(before.elapsed() < Duration::from_secs(1));
    assert!(boole_node::load_native_peer_key(&dir.0).is_err());
    let oversized = dir.0.join("oversized.pk8");
    std::fs::write(&oversized, vec![0u8; 4097]).unwrap();
    std::fs::set_permissions(&oversized, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(boole_node::load_native_peer_key(&oversized).is_err());
    let other =
        boole_p2p::TlsIdentity::from_pkcs8(&boole_p2p::TlsIdentity::generate_pkcs8().unwrap())
            .unwrap();
    let key_text = key.to_str().unwrap();
    let public_peer = format!("{}@192.0.2.1:1234", other.peer_id().to_hex());
    let self_peer = format!("{}@127.0.0.1:1234", id.to_hex());
    let unknown_name = format!("{}@localhost:1234", other.peer_id().to_hex());
    for args in [
        vec!["--p2p-addr", "0.0.0.0:0", "--peer-key", key_text],
        vec![
            "--p2p-addr",
            "127.0.0.1:0",
            "--peer-key",
            key_text,
            "--peer",
            &public_peer,
        ],
        vec![
            "--p2p-addr",
            "127.0.0.1:0",
            "--peer-key",
            key_text,
            "--peer",
            &self_peer,
        ],
        vec![
            "--p2p-addr",
            "127.0.0.1:0",
            "--peer-key",
            key_text,
            "--peer",
            &unknown_name,
        ],
        vec!["--p2p-addr", "127.0.0.1:0"],
        vec!["--peer-key", key_text],
    ] {
        let state = dir.0.join("must-not-exist");
        let output = Command::new(env!("CARGO_BIN_EXE_boole-node"))
            .args(["run-native-local", "--addr", "127.0.0.1:0", "--state-dir"])
            .arg(&state)
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!state.exists(), "invalid peer configuration mutated state");
    }
    assert_eq!(std::fs::read(&key).unwrap(), original);
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(boole_node::load_native_peer_key(&key).is_err());
    let new_key = dir.0.join("must-not-create.pk8");
    assert!(boole_node::create_native_peer_key(&new_key).is_err());
    assert!(!new_key.exists());
    std::fs::set_permissions(&dir.0, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn actual_native_node_processes_load_pinned_transport_keys_sync_and_stop_cleanly() {
    let dir = Scratch::new();
    let miner = boole_core::SigningKeyV2::from_dev_id("native-peer-cli-miner");
    let mut node = boole_node::NativeNode::open(&dir.0.join("a")).unwrap();
    let block = node
        .template(&miner.pk_hex(), &miner.pk_hex(), 60_000)
        .unwrap()
        .mine(0, 2_000_000)
        .unwrap()
        .unwrap();
    let signature = miner
        .sign_for_network(
            &block.authorization_payload().unwrap(),
            Some(boole_core::native_network::native_testnet().network_id()),
        )
        .unwrap();
    node.submit_block(block.authorize(&signature).unwrap())
        .unwrap();
    let hash = node.chain().head_hash().to_hex();
    drop(node);
    let key_a = dir.0.join("a.pk8");
    let key_b = dir.0.join("b.pk8");
    let id_a = boole_node::create_native_peer_key(&key_a).unwrap();
    let id_b = boole_node::create_native_peer_key(&key_b).unwrap();
    let reserved: Vec<_> = (0..4)
        .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
        .collect();
    let addresses: Vec<_> = reserved
        .iter()
        .map(|listener| listener.local_addr().unwrap())
        .collect();
    drop(reserved);
    let spawn =
        |name: &str, rpc: SocketAddr, p2p: SocketAddr, key: &std::path::Path, peer: String| {
            Child(
                Command::new(env!("CARGO_BIN_EXE_boole-node"))
                    .args([
                        "run-native-local",
                        "--addr",
                        &rpc.to_string(),
                        "--p2p-addr",
                        &p2p.to_string(),
                        "--peer",
                        &peer,
                        "--peer-key",
                    ])
                    .arg(key)
                    .arg("--state-dir")
                    .arg(dir.0.join(name))
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::inherit())
                    .spawn()
                    .unwrap(),
            )
        };
    let mut a = spawn(
        "a",
        addresses[0],
        addresses[1],
        &key_a,
        format!("{}@{}", id_b.to_hex(), addresses[3]),
    );
    let mut b = spawn(
        "b",
        addresses[2],
        addresses[3],
        &key_b,
        format!("{}@{}", id_a.to_hex(), addresses[1]),
    );
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if info(addresses[2]).is_some_and(|v| v["height"] == "1" && v["headHash"] == hash) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "CLI peer options did not synchronize native state"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    for child in [&mut a, &mut b] {
        assert!(Command::new("kill")
            .args(["-TERM", &child.0.id().to_string()])
            .status()
            .unwrap()
            .success());
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline, "peer process did not drain");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    assert_eq!(
        boole_node::NativeNode::open(&dir.0.join("b"))
            .unwrap()
            .chain()
            .head_hash()
            .to_hex(),
        hash
    );
    for address in addresses {
        assert!(TcpListener::bind(address).is_ok());
    }
}
