use boole_core::native_chain::NativeTransfer;
use boole_core::native_network::native_testnet;
use boole_core::SigningKeyV2;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boole-native-inspect-{}",
            boole_testkit::rand_suffix()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn signed_transfer() -> NativeTransfer {
    let owner = SigningKeyV2::from_dev_id("offline-inspection-unfunded-owner");
    let payload = json!({
        "schema": "boole.transfer.v1", "from": owner.pk_hex(),
        "to": SigningKeyV2::from_dev_id("offline-inspection-recipient").pk_hex(),
        "amount": "12345678901234567", "fee": "1000",
        "nonce": "9007199254740993", "validBefore": "0"
    });
    NativeTransfer::try_from(
        &owner
            .sign_for_network(&payload, Some(native_testnet().network_id()))
            .unwrap(),
    )
    .unwrap()
}

fn envelope_bytes(envelope: &boole_core::signed_envelope::SignedEnvelope) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": envelope.schema, "payload": envelope.payload,
        "pk": envelope.pk, "signature": envelope.signature,
        "network_id": envelope.network_id
    }))
    .unwrap()
}

fn inspect(file: &Path, node: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["native", "--node", node, "inspect-transfer", "--file"])
        .arg(file)
        .env("BOOLE_WALLET_AGENT_BIN", file.join("no-agent-here"))
        .env_remove("BOOLE_WALLET_PASSPHRASE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("offline inspection blocked on input, agent or network");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn saved_transfer_can_be_identified_offline_without_wallet_or_chain_validity_claims() {
    let fixture = Fixture::new();
    let file = fixture.0.join("signed.json");
    let transfer = signed_transfer();
    transfer.validated_fields().unwrap();
    let original = serde_json::to_vec_pretty(&transfer).unwrap();
    std::fs::write(&file, &original).unwrap();

    // Even the node URL is unusable: this command must not construct an RPC
    // client, request a password, launch an agent or require a funded account.
    let result = inspect(&file, "offline-do-not-parse-or-connect");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        receipt,
        json!({
            "schema": "boole.native.transfer.inspection.v1",
            "verification": "signature_and_format_only", "chainStatus": "not_checked",
            "networkId": native_testnet().network_id(),
            "compiledGenesisHash": native_testnet().genesis_hash().to_hex(),
            "txid": transfer.id().to_hex(), "from": transfer.payload.from,
            "to": transfer.payload.to, "amountAtoms": "12345678901234567",
            "feeAtoms": "1000", "nonce": "9007199254740993", "validBefore": "0"
        })
    );
    assert_eq!(std::fs::read(&file).unwrap(), original);
    assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
}

#[test]
fn inspection_does_not_contact_even_a_listening_node_endpoint() {
    let fixture = Fixture::new();
    let file = fixture.0.join("signed.json");
    std::fs::write(&file, serde_json::to_vec(&signed_transfer()).unwrap()).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let result = inspect(&file, &format!("http://{}", listener.local_addr().unwrap()));
    assert!(result.status.success(), "{:?}", result.stderr);
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn invalid_signed_files_have_no_inspection_receipt_and_are_not_changed() {
    let fixture = Fixture::new();
    let owner = SigningKeyV2::from_dev_id("offline-inspection-unfunded-owner");
    let valid = signed_transfer();
    let wire = serde_json::to_value(&valid).unwrap();
    let mut cases = Vec::new();
    let mut tampered = wire.clone();
    tampered["payload"]["amount"] = "42".into();
    cases.push((
        "changed-signed-amount",
        serde_json::to_vec(&tampered).unwrap(),
    ));
    let wrong_network = owner
        .sign_for_network(&wire["payload"], Some("different-native-network"))
        .unwrap();
    cases.push(("wrong-network", envelope_bytes(&wrong_network)));
    let session = SigningKeyV2::from_dev_id("offline-inspection-work-session");
    cases.push((
        "not-owner",
        envelope_bytes(
            &session
                .sign_for_network(&wire["payload"], Some(native_testnet().network_id()))
                .unwrap(),
        ),
    ));
    for (field, value) in [
        ("schema", json!("boole.work.v1")),
        ("amount", json!("01")),
        ("amount", json!("0")),
        ("amount", json!(1)),
        ("fee", json!("999")),
        ("nonce", json!("18446744073709551616")),
        ("extra", json!("ambiguous")),
    ] {
        let mut payload = wire["payload"].clone();
        payload[field] = value;
        let signed = owner
            .sign_for_network(&payload, Some(native_testnet().network_id()))
            .unwrap();
        cases.push((field, envelope_bytes(&signed)));
    }
    let mut extra = wire.clone();
    extra["txid"] = valid.id().to_hex().into();
    cases.push((
        "unknown-envelope-field",
        serde_json::to_vec(&extra).unwrap(),
    ));
    let mut no_network = wire.clone();
    no_network.as_object_mut().unwrap().remove("network_id");
    cases.push(("missing-network", serde_json::to_vec(&no_network).unwrap()));
    let canonical = serde_json::to_string(&valid).unwrap();
    let duplicate = canonical.replacen("\"amount\":", "\"amount\":\"1\",\"amount\":", 1);
    assert_ne!(duplicate, canonical);
    cases.push(("duplicate-payload-field", duplicate.into_bytes()));
    cases.push((
        "truncated-json",
        canonical.as_bytes()[..canonical.len() - 1].to_vec(),
    ));
    let mut oversize = serde_json::to_vec(&valid).unwrap();
    oversize.resize(native_testnet().max_transfer_bytes() + 1, b' ');
    cases.push(("oversized-wire", oversize));

    for (index, (name, bytes)) in cases.into_iter().enumerate() {
        let file = fixture.0.join(format!("{index}.json"));
        std::fs::write(&file, &bytes).unwrap();
        let result = inspect(&file, "offline-do-not-parse-or-connect");
        assert!(!result.status.success(), "accepted {name}");
        assert!(result.stdout.is_empty(), "receipt for invalid {name}");
        assert_eq!(std::fs::read(&file).unwrap(), bytes, "rewrote {name}");
    }
}

#[test]
fn inspection_refuses_missing_nonregular_and_symlink_inputs_without_creating_files() {
    let fixture = Fixture::new();
    let valid = fixture.0.join("signed.json");
    let bytes = serde_json::to_vec(&signed_transfer()).unwrap();
    std::fs::write(&valid, &bytes).unwrap();
    let missing = fixture.0.join("absent.json");
    let directory = fixture.0.join("directory");
    std::fs::create_dir(&directory).unwrap();
    let symlink = fixture.0.join("link.json");
    std::os::unix::fs::symlink(&valid, &symlink).unwrap();
    let fifo = fixture.0.join("pipe.json");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    for file in [&missing, &directory, &symlink, &fifo] {
        let result = inspect(file, "offline-do-not-parse-or-connect");
        assert!(!result.status.success(), "accepted {}", file.display());
        assert!(result.stdout.is_empty());
    }
    assert!(!missing.exists());
    assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 4);
    assert_eq!(std::fs::read(valid).unwrap(), bytes);
}
