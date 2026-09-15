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

fn spawn_node(dir: PathBuf) -> (Fixture, String) {
    std::fs::create_dir(&dir).unwrap();
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
        ok(&[
            "native",
            "--node",
            &url,
            "mine",
            "--vault",
            vault.to_str().unwrap(),
            "--timestamp-ms",
            &(height * 60_000u64).to_string(),
            "--attempts",
            "2000000",
        ]);
    }
    let outbox = fixture.dir.join("transfer.json");
    let transfer = ok(&[
        "native",
        "--node",
        &url,
        "transfer",
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
    assert_eq!(std::fs::read(outbox).unwrap(), signed);
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
        if cli(&["native", "--node", &url, "info"]).status.success() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native restart readiness deadline"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
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
}
