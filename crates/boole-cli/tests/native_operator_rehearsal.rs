//! One-host operational workflow only; never public-network or R2/R3 evidence.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PASS: &str = "disposable-three-process-rehearsal-only";

struct Scratch {
    path: PathBuf,
    identity: (u64, u64),
}
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boole-native-operator-rehearsal-{}",
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

fn sibling(name: &str) -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_boole-cli"))
        .parent()
        .unwrap()
        .join(name)
}

fn command_json(binary: &Path, args: &[&str], password: bool) -> Value {
    let mut child = Command::new(binary)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("BOOLE_WALLET_AGENT_BIN", sibling("boole-wallet-agent"))
        .stdin(if password {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if password {
        writeln!(child.stdin.take().unwrap(), "{PASS}").unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(!String::from_utf8_lossy(&output.stdout).contains(PASS));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(PASS));
    assert!(
        output.status.success(),
        "command {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn cli(args: &[&str], password: bool) -> Value {
    command_json(Path::new(env!("CARGO_BIN_EXE_boole-cli")), args, password)
}

fn node_command(args: &[&str]) -> Value {
    command_json(&sibling("boole-node"), args, false)
}

fn rpc(addr: SocketAddr, path: &str) -> Option<Value> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_millis(100)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(1)))
        .ok()?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut response = String::new();
    stream.take(131_072).read_to_string(&mut response).ok()?;
    if !response.starts_with("HTTP/1.1 200 ") {
        return None;
    }
    serde_json::from_str(response.split_once("\r\n\r\n")?.1).ok()
}

fn await_phase(label: &str, mut predicate: impl FnMut() -> bool) {
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
        "native-operator-phase {label} elapsedMs={}",
        started.elapsed().as_millis()
    );
}

struct Actor {
    child: Option<Child>,
    state: PathBuf,
    vault: PathBuf,
    transport_key: PathBuf,
    owner: String,
    peer: String,
    rpc: SocketAddr,
    p2p: SocketAddr,
}
impl Drop for Actor {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Actor {
    fn new(root: &Path, name: &str, rpc: SocketAddr, p2p: SocketAddr) -> Self {
        let dir = root.join(name);
        std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        let vault = dir.join("owner.vault");
        let owner = cli(
            &[
                "wallet",
                "init",
                "--vault",
                vault.to_str().unwrap(),
                "--json",
            ],
            true,
        )["result"]["address"]
            .as_str()
            .unwrap()
            .to_owned();
        let transport_key = dir.join("transport.pk8");
        let peer = node_command(&["peer-keygen", "--file", transport_key.to_str().unwrap()])
            ["peerId"]
            .as_str()
            .unwrap()
            .to_owned();
        Self {
            child: None,
            state: dir.join("state"),
            vault,
            transport_key,
            owner,
            peer,
            rpc,
            p2p,
        }
    }

    fn start(&mut self, peers: &[(String, SocketAddr)]) {
        assert!(self.child.is_none());
        let mut command = Command::new(sibling("boole-node"));
        command
            .args([
                "run-native-local",
                "--addr",
                &self.rpc.to_string(),
                "--state-dir",
            ])
            .arg(&self.state)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if !peers.is_empty() {
            command
                .args(["--p2p-addr", &self.p2p.to_string(), "--peer-key"])
                .arg(&self.transport_key);
            for (key, address) in peers {
                command.args(["--peer", &format!("{key}@{address}")]);
            }
        }
        self.child = Some(command.spawn().unwrap());
        await_phase("process-ready", || rpc(self.rpc, "/native/info").is_some());
    }

    fn stop(&mut self) {
        let child = self.child.as_mut().expect("running process");
        assert!(
            child.try_wait().unwrap().is_none(),
            "process exited before normal stop"
        );
        let started = Instant::now();
        assert!(Command::new("/bin/kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success());
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "normal native stop failed: {status}");
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
            "native-operator-stop elapsedMs={}",
            started.elapsed().as_millis()
        );
    }

    fn native(&self, args: &[&str], password: bool) -> Value {
        let url = format!("http://{}", self.rpc);
        let mut all = vec!["native", "--node", &url];
        all.extend_from_slice(args);
        cli(&all, password)
    }

    fn mine(&self, height: u64) -> String {
        self.native(
            &[
                "mine",
                "--vault",
                self.vault.to_str().unwrap(),
                "--passphrase-stdin",
                "--timestamp-ms",
                &(height * 60_000).to_string(),
                "--attempts",
                "2000000",
            ],
            true,
        );
        let info = self.native(&["info"], false);
        assert_eq!(info["height"], height.to_string());
        info["headHash"].as_str().unwrap().to_owned()
    }

    fn transfer(&self, recipient: &str, amount: &str, outbox: &Path) -> String {
        let submitted = self.native(
            &[
                "transfer",
                "--vault",
                self.vault.to_str().unwrap(),
                "--passphrase-stdin",
                "--to",
                recipient,
                "--amount",
                amount,
                "--valid-before",
                "100",
                "--outbox",
                outbox.to_str().unwrap(),
            ],
            true,
        );
        assert_eq!(submitted["status"], "pending");
        submitted["txid"].as_str().unwrap().to_owned()
    }
}

fn await_head(actors: &[Actor], head: &str, height: u64) {
    let expected_height = height.to_string();
    await_phase("head-agreement", || {
        actors.iter().all(|actor| {
            rpc(actor.rpc, "/native/info").is_some_and(|info| {
                info["headHash"] == head
                    && info["height"].as_str() == Some(expected_height.as_str())
            })
        })
    });
}

fn await_transaction(actors: &[Actor], txid: &str, status: &str, height: Option<u64>) {
    let expected_height = height.map(|height| height.to_string());
    await_phase("transaction-agreement", || {
        actors.iter().all(|actor| {
            rpc(actor.rpc, &format!("/native/transactions/{txid}")).is_some_and(|tx| {
                tx["status"] == status
                    && tx["final"] == false
                    && expected_height
                        .as_ref()
                        .is_none_or(|height| tx["height"].as_str() == Some(height.as_str()))
            })
        })
    });
}

#[test]
fn three_real_nodes_transfer_partition_rejoin_and_restore_without_duplicate_payment() {
    assert!(Command::new(env!("CARGO"))
        .args(["build", "-p", "boole-node", "-p", "boole-wallet-agent"])
        .status()
        .unwrap()
        .success());
    let started = Instant::now();
    let scratch = Scratch::new();
    let reserved: Vec<_> = (0..6)
        .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
        .collect();
    let addresses: Vec<_> = reserved
        .iter()
        .map(|listener| listener.local_addr().unwrap())
        .collect();
    let mut actors: Vec<_> = ["a", "b", "c"]
        .iter()
        .enumerate()
        .map(|(index, name)| {
            Actor::new(
                &scratch.path,
                name,
                addresses[index * 2],
                addresses[index * 2 + 1],
            )
        })
        .collect();
    let peer_sets: Vec<Vec<_>> = (0..3)
        .map(|local| {
            actors
                .iter()
                .enumerate()
                .filter(|(remote, _)| *remote != local)
                .map(|(_, actor)| (actor.peer.clone(), actor.p2p))
                .collect()
        })
        .collect();
    let transport_bytes: Vec<_> = actors
        .iter()
        .map(|actor| std::fs::read(&actor.transport_key).unwrap())
        .collect();
    drop(reserved);
    for (actor, peers) in actors.iter_mut().zip(&peer_sets) {
        actor.start(peers);
    }
    for actor in &actors {
        let info = actor.native(&["info"], false);
        assert_eq!(info["policy"]["networkId"], "boole-native-testnet-1");
        assert_eq!(
            info["genesisHash"],
            "933a672120674efa9ec6205f344e1830febdec58b0ffcd2603ccc8a9723610c1"
        );
        assert_eq!(info["height"], "0");
    }

    // Preserve B's original vault and use an authenticated restored copy.
    let backup = scratch.path.join("b-owner.backup.json");
    assert_eq!(
        cli(
            &[
                "wallet",
                "backup",
                "--vault",
                actors[1].vault.to_str().unwrap(),
                "--output",
                backup.to_str().unwrap(),
                "--json"
            ],
            true
        )["result"]["address"],
        actors[1].owner
    );
    let backup_bytes = std::fs::read(&backup).unwrap();
    let original_vault = actors[1].vault.clone();
    let original_vault_bytes = std::fs::read(&original_vault).unwrap();
    actors[1].vault = scratch.path.join("b-restored.vault");
    assert_eq!(
        cli(
            &[
                "wallet",
                "restore",
                "--backup",
                backup.to_str().unwrap(),
                "--vault",
                actors[1].vault.to_str().unwrap(),
                "--json"
            ],
            true
        )["result"]["address"],
        actors[1].owner
    );

    let mut head = String::new();
    for height in 1..=10 {
        head = actors[0].mine(height);
    }
    await_head(&actors, &head, 10);
    await_phase("reciprocal-pinned-peers", || {
        actors.iter().all(|actor| {
            rpc(actor.rpc, "/native/peers").is_some_and(|status| {
                status["localPeerId"] == actor.peer
                    && status["peers"].as_array().is_some_and(|peers| {
                        peers.len() == 2
                            && peers.iter().all(|peer| peer["state"] == "snapshot_match")
                    })
            })
        })
    });
    let outboxes: Vec<_> = ["a-to-b.json", "b-to-c.json", "c-to-a.json"]
        .iter()
        .map(|name| scratch.path.join(name))
        .collect();
    let a_tx = actors[0].transfer(&actors[1].owner, "2", &outboxes[0]);
    let a_signed = std::fs::read(&outboxes[0]).unwrap();
    await_transaction(&actors, &a_tx, "pending", None);
    let repeated = actors[2].native(&["submit", "--file", outboxes[0].to_str().unwrap()], false);
    assert_eq!(repeated["txid"], a_tx);
    assert_eq!(repeated["status"], "pending");
    head = actors[1].mine(11);
    await_head(&actors, &head, 11);
    await_transaction(&actors, &a_tx, "confirmed", Some(11));
    let b_tx = actors[1].transfer(&actors[2].owner, "1.5", &outboxes[1]);
    let b_signed = std::fs::read(&outboxes[1]).unwrap();
    await_transaction(&actors, &b_tx, "pending", None);
    head = actors[2].mine(12);
    await_head(&actors, &head, 12);
    await_transaction(&actors, &b_tx, "confirmed", Some(12));

    // A real isolated C process, not a simulated transport response.
    actors[2].stop();
    actors[2].start(&[]);
    assert_eq!(actors[2].native(&["peers"], false)["enabled"], false);
    actors[0].mine(13);
    head = actors[0].mine(14);
    await_head(&actors[..2], &head, 14);
    let c_tx = actors[2].transfer(&actors[0].owner, "0.25", &outboxes[2]);
    let c_signed = std::fs::read(&outboxes[2]).unwrap();
    let isolated_head = actors[2].mine(13);
    assert_ne!(isolated_head, head);
    await_transaction(&actors[2..], &c_tx, "confirmed", Some(13));
    for actor in &actors[..2] {
        assert_eq!(
            actor.native(&["transaction", "--txid", &c_tx], false)["status"],
            "unknown"
        );
    }
    actors[2].stop();
    actors[2].start(&peer_sets[2]);
    await_head(&actors, &head, 14);
    await_transaction(&actors, &c_tx, "pending", None);
    head = actors[0].mine(15);
    await_head(&actors, &head, 15);
    await_transaction(&actors, &c_tx, "confirmed", Some(15));

    let balances = ["64999825000000", "5000050000000", "5000125000000"];
    let locked = ["40000000000000", "5000000000000", "5000000000000"];
    let spendable = ["24999825000000", "50000000", "125000000"];
    for actor in &actors {
        let info = actor.native(&["info"], false);
        assert_eq!(info["issued"], "75000000000000");
        assert_eq!(info["pending"], 0);
        assert_eq!(info["resources"]["confirmedTransfers"], 3);
        for (index, owner) in actors.iter().enumerate() {
            let account = actor.native(&["account", "--pk", &owner.owner], false);
            assert_eq!(account["balance"], balances[index]);
            assert_eq!(account["locked"], locked[index]);
            assert_eq!(account["spendable"], spendable[index]);
            assert_eq!(account["confirmedNonce"], "1");
            assert_eq!(account["pendingNonce"], "1");
        }
    }
    for actor in &mut actors {
        actor.stop();
    }
    let source_files = [
        boole_node::NATIVE_BLOCKS_FILE,
        boole_node::NATIVE_MEMPOOL_FILE,
        "state.manifest.json",
    ];
    let original_states: Vec<_> = actors.iter().map(|actor| actor.state.clone()).collect();
    let originals: Vec<Vec<_>> = original_states
        .iter()
        .map(|state| {
            source_files
                .iter()
                .map(|file| std::fs::read(state.join(file)).unwrap())
                .collect()
        })
        .collect();
    assert!(originals.iter().all(|files| files[0] == originals[0][0]));
    let audits: Vec<_> = original_states
        .iter()
        .map(|state| {
            node_command(&[
                "native-audit",
                "--state-dir",
                state.to_str().unwrap(),
                "--expected-head",
                &head,
            ])
        })
        .collect();
    assert!(audits.iter().all(|audit| audit == &audits[0]));
    let audit = &audits[0];
    assert_eq!(audit["height"], "15");
    assert_eq!(audit["headHash"], head);
    assert_eq!(audit["accounting"]["issuedAtoms"], "75000000000000");
    assert_eq!(audit["accounting"]["balanceAtoms"], "75000000000000");
    assert_eq!(audit["accounting"]["lockedAtoms"], "50000000000000");
    assert_eq!(audit["accounting"]["spendableAtoms"], "25000000000000");
    assert_eq!(
        audit["confirmedTransfers"],
        json!({"count": 3, "amountAtoms": "375000000", "feeAtoms": "3000"})
    );
    let archive = scratch.path.join("a-canonical.ndjson");
    let exported = node_command(&[
        "native-export",
        "--state-dir",
        original_states[0].to_str().unwrap(),
        "--output",
        archive.to_str().unwrap(),
    ]);
    assert_eq!(exported["headHash"], head);
    let archive_bytes = std::fs::read(&archive).unwrap();
    actors[1].state = scratch.path.join("b-restored-state");
    let imported = node_command(&[
        "native-import",
        "--state-dir",
        actors[1].state.to_str().unwrap(),
        "--blocks",
        archive.to_str().unwrap(),
        "--expected-head",
        &head,
    ]);
    assert_eq!(imported["adopted"], true);
    assert_eq!(imported["headHash"], head);
    assert_eq!(
        node_command(&[
            "native-audit",
            "--state-dir",
            actors[1].state.to_str().unwrap(),
            "--expected-head",
            &head
        ]),
        *audit
    );
    actors[1].start(&peer_sets[1]);
    assert_eq!(
        actors[1].native(&["peers"], false)["localPeerId"],
        actors[1].peer
    );
    let before = actors[1].native(&["info"], false);
    let inspection = cli(
        &[
            "native",
            "inspect-transfer",
            "--file",
            outboxes[1].to_str().unwrap(),
        ],
        false,
    );
    assert_eq!(inspection["txid"], b_tx);
    assert_eq!(inspection["chainStatus"], "not_checked");
    let status = actors[1].native(&["transaction", "--txid", &b_tx], false);
    assert_eq!(status["status"], "confirmed");
    assert_eq!(status["height"], "12");
    assert_eq!(
        actors[1].native(&["submit", "--file", outboxes[1].to_str().unwrap()], false),
        status
    );
    assert_eq!(actors[1].native(&["info"], false), before);
    actors[1].stop();
    assert_eq!(
        node_command(&[
            "native-audit",
            "--state-dir",
            actors[1].state.to_str().unwrap(),
            "--expected-head",
            &head
        ]),
        *audit
    );

    for (state, files) in original_states.iter().zip(&originals) {
        for (name, bytes) in source_files.iter().zip(files) {
            assert_eq!(std::fs::read(state.join(name)).unwrap(), *bytes);
        }
    }
    for (outbox, bytes) in outboxes.iter().zip([a_signed, b_signed, c_signed]) {
        assert_eq!(std::fs::read(outbox).unwrap(), bytes);
    }
    assert_eq!(std::fs::read(&archive).unwrap(), archive_bytes);
    assert_eq!(std::fs::read(&backup).unwrap(), backup_bytes);
    assert_eq!(
        std::fs::read(&original_vault).unwrap(),
        original_vault_bytes
    );
    for (actor, bytes) in actors.iter().zip(transport_bytes) {
        assert_eq!(std::fs::read(&actor.transport_key).unwrap(), bytes);
    }
    assert!(started.elapsed() < Duration::from_secs(180));
    eprintln!(
        "native-operator-result {}",
        json!({
            "scope": "one-host-three-loopback-processes", "elapsedMs": started.elapsed().as_millis(),
            "head": head, "orphanedHead": isolated_head, "transactionIds": [a_tx, b_tx, c_tx],
            "owners": actors.iter().map(|actor| &actor.owner).collect::<Vec<_>>(),
            "balancesAtoms": balances, "lockedAtoms": locked, "spendableAtoms": spendable,
            "audit": audit, "originalsPreserved": true, "publicActivation": false
        })
    );
}
