use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const PASS: &str = "disposable-native-cli-test-only";
struct Fixture {
    dir: PathBuf,
    child: Child,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn sibling(name: &str) -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_boole-cli"))
        .parent()
        .unwrap()
        .join(name)
}
fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(args)
        .env("BOOLE_WALLET_AGENT_BIN", sibling("boole-wallet-agent"))
        .env("BOOLE_WALLET_PASSPHRASE", PASS)
        .output()
        .unwrap()
}
fn ok(args: &[&str]) -> Value {
    let output = cli(args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn ok_stdin(args: &[&str]) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(args)
        .env("BOOLE_WALLET_AGENT_BIN", sibling("boole-wallet-agent"))
        .env(
            "BOOLE_WALLET_PASSPHRASE",
            "stale-environment-must-not-override-explicit-stdin",
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(), "{PASS}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains(PASS));
    serde_json::from_slice(&output.stdout).unwrap()
}

fn spawn_node(dir: PathBuf) -> (Fixture, String) {
    std::fs::create_dir(&dir).unwrap();
    start_node(dir)
}

fn start_node(dir: PathBuf) -> (Fixture, String) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let child = Command::new(sibling("boole-node"))
        .args([
            "run-native-local",
            "--addr",
            &addr.to_string(),
            "--state-dir",
        ])
        .arg(dir.join("node"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let fixture = Fixture { dir, child };
    let url = format!("http://{addr}");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let output = cli(&["native", "--node", &url, "info"]);
        if output.status.success() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    (fixture, url)
}

fn node_json(args: &[&str]) -> Value {
    let output = Command::new(sibling("boole-node"))
        .args(args)
        .env_remove("BOOLE_WALLET_PASSPHRASE")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn restart_node(fixture: &mut Fixture, url: &str) {
    fixture.child.kill().unwrap();
    fixture.child.wait().unwrap();
    fixture.child = Command::new(sibling("boole-node"))
        .args([
            "run-native-local",
            "--addr",
            url.strip_prefix("http://").unwrap(),
            "--state-dir",
        ])
        .arg(fixture.dir.join("node"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if cli(&["native", "--node", url, "info"]).status.success() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native restart readiness deadline"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn encrypted_owner_cli_mines_transfers_and_retries_only_the_saved_signed_transaction() {
    assert!(Command::new(env!("CARGO"))
        .args(["build", "-p", "boole-node", "-p", "boole-wallet-agent"])
        .status()
        .unwrap()
        .success());
    let dir =
        std::env::temp_dir().join(format!("boole-native-cli-{}", boole_testkit::rand_suffix()));
    let (mut fixture, url) = spawn_node(dir);
    assert_eq!(ok(&["native", "--node", &url, "peers"])["enabled"], false);
    // Real process/CLI recovery diagnostics remain readable when the canonical
    // file is unavailable, but normal node info must still refuse readiness.
    let history = fixture.dir.join("node/native-blocks.ndjson");
    let preserved_history = fixture.dir.join("preserved-initial-history.ndjson");
    let history_bytes = std::fs::read(&history).unwrap();
    std::fs::rename(&history, &preserved_history).unwrap();
    let diagnostic = ok(&["native", "--node", &url, "diagnostics"]);
    assert_eq!(diagnostic["ledgerReadiness"], "not_checked");
    assert_eq!(diagnostic["peers"]["enabled"], false);
    assert!(diagnostic.get("ready").is_none());
    let unavailable = cli(&["native", "--node", &url, "info"]);
    assert!(!unavailable.status.success());
    assert!(String::from_utf8_lossy(&unavailable.stderr).contains("503"));
    assert!(!history.exists());
    assert_eq!(std::fs::read(&preserved_history).unwrap(), history_bytes);
    std::fs::rename(&preserved_history, &history).unwrap();
    // Renaming back preserves bytes, not the recorded ctime. The live writer
    // must remain fenced; a new process revalidates the preserved history.
    assert!(!cli(&["native", "--node", &url, "info"]).status.success());
    restart_node(&mut fixture, &url);
    let vault = fixture.dir.join("owner.vault");
    let mut init = Command::new(sibling("boole-wallet-agent"))
        .arg("init")
        .arg("--vault")
        .arg(&vault)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(init.stdin.take().unwrap(), "{PASS}").unwrap();
    let owner = init.wait_with_output().unwrap();
    assert!(owner.status.success());
    let owner = String::from_utf8(owner.stdout).unwrap().trim().to_string();
    let receiver = boole_core::SigningKeyV2::from_dev_id("native-cli-receiver").pk_hex();
    for height in 1..=10 {
        ok_stdin(&[
            "native",
            "--node",
            &url,
            "mine",
            "--passphrase-stdin",
            "--vault",
            vault.to_str().unwrap(),
            "--timestamp-ms",
            &(height * 60_000u64).to_string(),
            "--attempts",
            "2000000",
        ]);
    }
    let backup = fixture.dir.join("owner.backup.json");
    let restored = fixture.dir.join("recovered-owner.vault");
    assert_eq!(
        ok_stdin(&[
            "wallet",
            "backup",
            "--vault",
            vault.to_str().unwrap(),
            "--output",
            backup.to_str().unwrap(),
            "--json"
        ])["result"]["address"],
        owner
    );
    std::fs::rename(&vault, fixture.dir.join("offline-original.vault")).unwrap();
    assert_eq!(
        ok_stdin(&[
            "wallet",
            "restore",
            "--backup",
            backup.to_str().unwrap(),
            "--vault",
            restored.to_str().unwrap(),
            "--json"
        ])["result"]["address"],
        owner
    );
    let vault = restored;
    let outbox = fixture.dir.join("transfer.json");
    let transfer = ok_stdin(&[
        "native",
        "--node",
        &url,
        "transfer",
        "--passphrase-stdin",
        "--vault",
        vault.to_str().unwrap(),
        "--to",
        &receiver,
        "--amount",
        "1.25",
        "--outbox",
        outbox.to_str().unwrap(),
    ]);
    assert_eq!(transfer["status"], "pending");
    let signed = std::fs::read(&outbox).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&outbox).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let tx: boole_core::native_chain::NativeTransfer = serde_json::from_slice(&signed).unwrap();
    assert_eq!(tx.payload.amount, "125000000");
    assert_eq!(tx.payload.from, owner);
    tx.validated_fields().unwrap();
    let inspection = ok(&[
        "native",
        "--node",
        "offline-no-rpc",
        "inspect-transfer",
        "--file",
        outbox.to_str().unwrap(),
    ]);
    assert_eq!(inspection["txid"], transfer["txid"]);
    assert_eq!(inspection["amountAtoms"], "125000000");
    assert_eq!(inspection["from"], owner);
    assert_eq!(inspection["chainStatus"], "not_checked");
    assert_eq!(
        ok(&[
            "native",
            "--node",
            &url,
            "submit",
            "--file",
            outbox.to_str().unwrap()
        ])["txid"],
        transfer["txid"]
    );
    ok(&[
        "native",
        "--node",
        &url,
        "mine",
        "--vault",
        vault.to_str().unwrap(),
        "--timestamp-ms",
        "660000",
        "--attempts",
        "2000000",
    ]);
    assert_eq!(
        ok(&["native", "--node", &url, "account", "--pk", &receiver])["balance"],
        "125000000"
    );
    assert_eq!(
        ok(&[
            "native",
            "--node",
            &url,
            "submit",
            "--file",
            outbox.to_str().unwrap()
        ])["status"],
        "confirmed"
    );
    let repeated = cli(&[
        "native",
        "--node",
        &url,
        "transfer",
        "--vault",
        vault.to_str().unwrap(),
        "--to",
        &receiver,
        "--amount",
        "9",
        "--outbox",
        outbox.to_str().unwrap(),
    ]);
    assert!(
        !repeated.status.success(),
        "must not replace the saved signature with a new nonce"
    );
    assert_eq!(std::fs::read(&outbox).unwrap(), signed);
    let (_replica, replica_url) = spawn_node(fixture.dir.join("replica"));
    assert_eq!(
        ok(&["native", "--node", &replica_url, "sync", "--from", &url])["adopted"],
        true
    );
    assert_eq!(
        ok(&["native", "--node", &replica_url, "info"]),
        ok(&["native", "--node", &url, "info"])
    );
    assert_eq!(
        ok(&[
            "native",
            "--node",
            &replica_url,
            "account",
            "--pk",
            &receiver
        ])["balance"],
        "125000000"
    );
    // Real process crash/restart; a confirmed transfer must remain exactly
    // once and the saved outbox remains usable without decrypting the vault.
    restart_node(&mut fixture, &url);
    assert_eq!(
        ok(&[
            "native",
            "--node",
            &url,
            "submit",
            "--file",
            fixture.dir.join("transfer.json").to_str().unwrap()
        ])["status"],
        "confirmed"
    );
    assert_eq!(
        ok(&["native", "--node", &url, "account", "--pk", &receiver])["balance"],
        "125000000"
    );

    // Operator recovery: preserve the stopped original, restore canonical
    // history into a fresh node, compare audits and reconcile the old outbox
    // before spending with the already-restored owner vault.
    let source_info = ok(&["native", "--node", &url, "info"]);
    let source_head = source_info["headHash"].as_str().unwrap();
    fixture.child.kill().unwrap();
    fixture.child.wait().unwrap();
    let source = fixture.dir.join("node");
    let source_files = [
        boole_node::NATIVE_BLOCKS_FILE,
        boole_node::NATIVE_MEMPOOL_FILE,
        "state.manifest.json",
    ];
    let source_bytes: Vec<_> = source_files
        .iter()
        .map(|file| std::fs::read(source.join(file)).unwrap())
        .collect();
    let source_audit = node_json(&[
        "native-audit",
        "--state-dir",
        source.to_str().unwrap(),
        "--expected-head",
        source_head,
    ]);
    let archive = fixture.dir.join("verified-chain.ndjson");
    let exported = node_json(&[
        "native-export",
        "--state-dir",
        source.to_str().unwrap(),
        "--output",
        archive.to_str().unwrap(),
    ]);
    assert_eq!(exported["headHash"], source_info["headHash"]);
    let restored_dir = fixture.dir.join("recovered-node");
    std::fs::create_dir(&restored_dir).unwrap();
    let restored_state = restored_dir.join("node");
    let imported = node_json(&[
        "native-import",
        "--state-dir",
        restored_state.to_str().unwrap(),
        "--blocks",
        archive.to_str().unwrap(),
        "--expected-head",
        source_head,
    ]);
    assert_eq!(imported["adopted"], true);
    assert_eq!(imported["headHash"], exported["headHash"]);
    assert_eq!(
        node_json(&[
            "native-audit",
            "--state-dir",
            restored_state.to_str().unwrap(),
            "--expected-head",
            source_head
        ]),
        source_audit
    );
    let (mut recovered, recovered_url) = start_node(restored_dir);
    assert_eq!(
        ok(&["native", "--node", &recovered_url, "info"])["headHash"],
        source_info["headHash"]
    );
    let offline = ok(&[
        "native",
        "--node",
        "offline-no-rpc",
        "inspect-transfer",
        "--file",
        outbox.to_str().unwrap(),
    ]);
    assert_eq!(offline["txid"], transfer["txid"]);
    assert_eq!(
        ok(&[
            "native",
            "--node",
            &recovered_url,
            "transaction",
            "--txid",
            offline["txid"].as_str().unwrap()
        ])["status"],
        "confirmed"
    );
    assert_eq!(
        ok(&[
            "native",
            "--node",
            &recovered_url,
            "submit",
            "--file",
            outbox.to_str().unwrap()
        ])["status"],
        "confirmed"
    );
    assert_eq!(
        ok(&[
            "native",
            "--node",
            &recovered_url,
            "account",
            "--pk",
            &receiver
        ])["balance"],
        "125000000"
    );
    assert_eq!(
        ok(&[
            "native",
            "--node",
            &recovered_url,
            "account",
            "--pk",
            &owner
        ])["confirmedNonce"],
        "1"
    );

    let next_outbox = fixture.dir.join("after-recovery-transfer.json");
    let next = ok_stdin(&[
        "native",
        "--node",
        &recovered_url,
        "transfer",
        "--passphrase-stdin",
        "--vault",
        vault.to_str().unwrap(),
        "--to",
        &receiver,
        "--amount",
        "0.5",
        "--outbox",
        next_outbox.to_str().unwrap(),
    ]);
    let next_signed: boole_core::native_chain::NativeTransfer =
        serde_json::from_slice(&std::fs::read(&next_outbox).unwrap()).unwrap();
    assert_eq!(next_signed.payload.nonce, "1");
    assert_eq!(next["status"], "pending");
    ok_stdin(&[
        "native",
        "--node",
        &recovered_url,
        "mine",
        "--passphrase-stdin",
        "--vault",
        vault.to_str().unwrap(),
        "--timestamp-ms",
        "720000",
        "--attempts",
        "2000000",
    ]);
    assert_eq!(
        ok(&[
            "native",
            "--node",
            &recovered_url,
            "account",
            "--pk",
            &receiver
        ])["balance"],
        "175000000"
    );
    assert_eq!(
        ok(&[
            "native",
            "--node",
            &recovered_url,
            "account",
            "--pk",
            &owner
        ])["confirmedNonce"],
        "2"
    );
    assert_eq!(
        ok(&[
            "native",
            "--node",
            &recovered_url,
            "transaction",
            "--txid",
            next["txid"].as_str().unwrap()
        ])["status"],
        "confirmed"
    );
    let resumed = ok(&["native", "--node", &recovered_url, "info"]);
    recovered.child.kill().unwrap();
    recovered.child.wait().unwrap();
    let resumed_audit = node_json(&[
        "native-audit",
        "--state-dir",
        restored_state.to_str().unwrap(),
        "--expected-head",
        resumed["headHash"].as_str().unwrap(),
    ]);
    assert_eq!(resumed_audit["height"], "12");
    assert_eq!(resumed_audit["confirmedTransfers"]["count"], 2);
    assert_eq!(
        resumed_audit["confirmedTransfers"]["amountAtoms"],
        "175000000"
    );
    assert_eq!(resumed_audit["confirmedTransfers"]["feeAtoms"], "2000");
    assert_eq!(std::fs::read(&outbox).unwrap(), signed);
    for (file, bytes) in source_files.iter().zip(source_bytes) {
        assert_eq!(std::fs::read(source.join(file)).unwrap(), bytes);
    }
}
