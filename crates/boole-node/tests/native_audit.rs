use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use boole_core::native_chain::{NativeBlock, NativeTransfer};
use boole_core::native_network::native_testnet;
use boole_core::SigningKeyV2;
use boole_node::{NativeNode, NATIVE_BLOCKS_FILE, NATIVE_MEMPOOL_FILE};
use serde_json::{json, Value};

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boole-native-audit-{}",
            boole_testkit::rand_suffix()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn transfer(owner: &SigningKeyV2, to: &str, nonce: u64, fee: u128) -> NativeTransfer {
    NativeTransfer::try_from(&owner.sign_for_network(&json!({
        "schema": "boole.transfer.v1", "from": owner.pk_hex(), "to": to,
        "amount": "100000000", "fee": fee.to_string(), "nonce": nonce.to_string(), "validBefore": "100"
    }), Some(native_testnet().network_id())).unwrap()).unwrap()
}

fn mine(node: &NativeNode, producer: &SigningKeyV2, transfers: &[NativeTransfer]) -> NativeBlock {
    let block = node
        .chain()
        .template(
            &producer.pk_hex(),
            &producer.pk_hex(),
            (node.chain().ledger().height() + 1) * 60_000,
            transfers,
        )
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
    block.authorize(&signature).unwrap()
}

fn audit(state: &Path, expected: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_boole-node"));
    command.arg("native-audit").arg("--state-dir").arg(state);
    if let Some(expected) = expected {
        command.arg("--expected-head").arg(expected);
    }
    command.output().unwrap()
}

fn source_bytes(state: &Path) -> Vec<Option<Vec<u8>>> {
    [
        "state.manifest.json",
        NATIVE_BLOCKS_FILE,
        NATIVE_MEMPOOL_FILE,
    ]
    .iter()
    .map(|name| match std::fs::read(state.join(name)) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("source snapshot: {error}"),
    })
    .collect()
}

fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn rejected_unchanged(state: &Path, expected: Option<&str>) {
    let before = source_bytes(state);
    let output = audit(state, expected);
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "failed audit emitted a success-like report"
    );
    assert_eq!(source_bytes(state), before);
}

#[test]
fn offline_audit_replays_confirmed_accounting_and_excludes_pending_without_changing_source() {
    let dir = TestDir::new();
    let state = dir.0.join("node");
    let owner = SigningKeyV2::from_dev_id("native-audit-owner");
    let producer = SigningKeyV2::from_dev_id("native-audit-distinct-producer");
    let recipient = SigningKeyV2::from_dev_id("native-audit-recipient").pk_hex();
    let mut node = NativeNode::open(&state).unwrap();
    for _ in 0..10 {
        node.submit_block(mine(&node, &owner, &[])).unwrap();
    }
    let confirmed = transfer(&owner, &recipient, 0, 1_000);
    node.submit_block(mine(&node, &producer, std::slice::from_ref(&confirmed)))
        .unwrap();
    node.submit_transfer(transfer(&owner, &recipient, 1, 2_000))
        .unwrap();
    let head = node.chain().head_hash().to_hex();
    drop(node);
    let before = source_bytes(&state);
    let report = success(audit(&state, Some(&head)));
    assert_eq!(report["schema"], "boole.native.audit.v1");
    assert_eq!(report["scope"], "confirmed_canonical");
    assert_eq!(report["networkId"], native_testnet().network_id());
    assert_eq!(
        report["genesisHash"],
        native_testnet().genesis_hash().to_hex()
    );
    assert_eq!(report["headHash"], head);
    assert_eq!(report["height"], "11");
    assert_eq!(report["accounting"]["issuedAtoms"], "55000000000000");
    assert_eq!(report["accounting"]["balanceAtoms"], "55000000000000");
    assert_eq!(report["accounting"]["supplyCapAtoms"], "100000000000000000");
    assert_eq!(report["accounting"]["lockedAtoms"], "50000000000000");
    assert_eq!(report["accounting"]["spendableAtoms"], "5000000000000");
    assert_eq!(report["accounting"]["pendingRewardEntries"], 10);
    assert_eq!(report["confirmedTransfers"]["count"], 1);
    assert_eq!(report["confirmedTransfers"]["amountAtoms"], "100000000");
    assert_eq!(report["confirmedTransfers"]["feeAtoms"], "1000");
    assert_eq!(report["resources"]["pendingTransfers"], 1);
    assert_eq!(report["resources"]["balanceEntries"], 3);
    assert_eq!(source_bytes(&state), before);
    assert_eq!(success(audit(&state, None)), report);
    assert_eq!(source_bytes(&state), before);
    let reopened = NativeNode::open(&state).unwrap();
    assert_eq!(reopened.chain().head_hash().to_hex(), head);
    assert_eq!(reopened.confirmed_height(&confirmed.id()), Some(11));
    assert_eq!(reopened.pending().len(), 1);
}

#[test]
fn missing_canonical_history_is_never_reported_or_reopened_as_empty_genesis() {
    let dir = TestDir::new();
    let state = dir.0.join("lost-history");
    let owner = SigningKeyV2::from_dev_id("native-audit-lost-history");
    let mut node = NativeNode::open(&state).unwrap();
    node.submit_block(mine(&node, &owner, &[])).unwrap();
    assert_eq!(node.chain().ledger().height(), 1);
    drop(node);
    std::fs::remove_file(state.join(NATIVE_BLOCKS_FILE)).unwrap();
    let before = source_bytes(&state);
    let result = audit(&state, None);
    assert!(
        !result.status.success(),
        "missing history was reported as valid genesis: {}",
        String::from_utf8_lossy(&result.stdout)
    );
    assert!(result.stdout.is_empty());
    assert_eq!(source_bytes(&state), before);
    assert!(
        NativeNode::open(&state).is_err(),
        "restart reset a previously funded node to genesis"
    );
    assert_eq!(source_bytes(&state), before);
}

#[test]
fn explicit_genesis_is_auditable_but_missing_busy_and_wrong_head_sources_are_refused() {
    let dir = TestDir::new();
    let missing = dir.0.join("missing");
    assert!(!audit(&missing, None).status.success());
    assert!(!missing.exists());
    let empty = dir.0.join("empty");
    std::fs::create_dir(&empty).unwrap();
    assert!(!audit(&empty, None).status.success());
    assert_eq!(std::fs::read_dir(&empty).unwrap().count(), 0);
    let state = dir.0.join("genesis");
    let node = NativeNode::open(&state).unwrap();
    assert_eq!(std::fs::read(state.join(NATIVE_BLOCKS_FILE)).unwrap(), b"");
    rejected_unchanged(&state, None);
    drop(node);
    let genesis = native_testnet().genesis_hash().to_hex();
    let report = success(audit(&state, Some(&genesis)));
    assert_eq!(report["height"], "0");
    assert_eq!(report["accounting"]["issuedAtoms"], "0");
    assert_eq!(report["accounting"]["balanceAtoms"], "0");
    assert_eq!(report["confirmedTransfers"]["count"], 0);
    rejected_unchanged(&state, Some(&"00".repeat(32)));
    for invalid in [
        "not-a-hash".to_string(),
        genesis.to_uppercase(),
        format!(" {genesis}"),
    ] {
        rejected_unchanged(&state, Some(&invalid));
        let missing = dir.0.join("invalid-head-must-not-create");
        let output = audit(&missing, Some(&invalid));
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("canonical lowercase"));
        assert!(!missing.exists());
    }
}

#[test]
fn corrupt_and_legacy_sources_are_not_repaired_or_upgraded_by_audit() {
    let dir = TestDir::new();
    let state = dir.0.join("source");
    let owner = SigningKeyV2::from_dev_id("native-audit-corruption");
    let mut node = NativeNode::open(&state).unwrap();
    let block = mine(&node, &owner, &[]);
    node.submit_block(block.clone()).unwrap();
    let head = node.chain().head_hash().to_hex();
    drop(node);
    let block_path = state.join(NATIVE_BLOCKS_FILE);
    let original = std::fs::read(&block_path).unwrap();
    let mut forged = block;
    forged.producer_signature = "00".repeat(64);
    for corrupt in [
        [original.as_slice(), b"{torn-evidence"].concat(),
        format!("{}\n", serde_json::to_string(&forged).unwrap()).into_bytes(),
    ] {
        std::fs::write(&block_path, corrupt).unwrap();
        rejected_unchanged(&state, None);
    }
    // A complete valid prefix cannot prove that newer history was never lost.
    // The independently retained expected head catches even this empty prefix.
    std::fs::write(&block_path, []).unwrap();
    rejected_unchanged(&state, Some(&head));
    std::fs::write(&block_path, &original).unwrap();

    let manifest_path = state.join("state.manifest.json");
    let original_manifest = std::fs::read(&manifest_path).unwrap();
    let manifest: Value = serde_json::from_slice(&original_manifest).unwrap();
    let mut legacy = manifest.clone();
    legacy["genesis_hash"] = json!("");
    let mut foreign = manifest;
    foreign["schema_versions"]["native_storage"] = json!(999);
    for changed in [legacy, foreign] {
        std::fs::write(&manifest_path, serde_json::to_vec(&changed).unwrap()).unwrap();
        rejected_unchanged(&state, None);
    }
    std::fs::write(&manifest_path, &original_manifest).unwrap();
    let pool = state.join(NATIVE_MEMPOOL_FILE);
    let pending = transfer(&owner, &owner.pk_hex(), 0, 1_000);
    // Signature-valid but immature, so a normal boot would remove this row.
    std::fs::write(
        &pool,
        format!("{}\n", serde_json::to_string(&pending).unwrap()),
    )
    .unwrap();
    let before = source_bytes(&state);
    let report = success(audit(&state, Some(&head)));
    assert_eq!(report["resources"]["pendingTransfers"], 0);
    assert_eq!(report["confirmedTransfers"]["count"], 0);
    assert_eq!(report["confirmedTransfers"]["feeAtoms"], "0");
    assert_eq!(
        source_bytes(&state),
        before,
        "audit removed a stale pending row"
    );
    let mut forged = pending;
    forged.signature = "00".repeat(64);
    std::fs::write(
        &pool,
        format!("{}\n", serde_json::to_string(&forged).unwrap()),
    )
    .unwrap();
    rejected_unchanged(&state, None);
}

#[test]
fn missing_manifest_is_not_recreated_over_existing_native_history() {
    let dir = TestDir::new();
    let state = dir.0.join("source");
    let owner = SigningKeyV2::from_dev_id("native-audit-lost-manifest");
    let mut node = NativeNode::open(&state).unwrap();
    node.submit_block(mine(&node, &owner, &[])).unwrap();
    drop(node);
    std::fs::remove_file(state.join("state.manifest.json")).unwrap();
    let before = source_bytes(&state);
    rejected_unchanged(&state, None);
    assert!(NativeNode::open(&state).is_err());
    assert_eq!(source_bytes(&state), before);

    // A crash between first history publication and first manifest publication
    // is deliberately ambiguous. Preserve even an empty file; do not infer
    // permission to finish initialization over an existing journal.
    let interrupted = dir.0.join("interrupted-bootstrap");
    std::fs::create_dir(&interrupted).unwrap();
    std::fs::write(interrupted.join(NATIVE_BLOCKS_FILE), []).unwrap();
    let before = source_bytes(&interrupted);
    rejected_unchanged(&interrupted, None);
    assert!(NativeNode::open(&interrupted).is_err());
    assert_eq!(source_bytes(&interrupted), before);
}

#[test]
fn audit_rejects_unsafe_source_aliases_permissions_and_oversized_history() {
    use std::io::Read;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::time::{Duration, Instant};
    let dir = TestDir::new();
    let state = dir.0.join("source");
    drop(NativeNode::open(&state).unwrap());
    let alias = dir.0.join("alias");
    symlink(&state, &alias).unwrap();
    rejected_unchanged(&alias, None);
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o777)).unwrap();
    rejected_unchanged(&state, None);
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["state.manifest.json", NATIVE_BLOCKS_FILE] {
        let link = dir.0.join("hardlink");
        std::fs::hard_link(state.join(name), &link).unwrap();
        rejected_unchanged(&state, None);
        std::fs::remove_file(&link).unwrap();
    }
    let blocks = state.join(NATIVE_BLOCKS_FILE);
    let oversized = 256 * 1024 * 1024 + 1;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&blocks)
        .unwrap()
        .set_len(oversized)
        .unwrap();
    let started = Instant::now();
    let output = audit(&state, None);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(std::fs::metadata(&blocks).unwrap().len(), oversized);
    let mut prefix = Vec::new();
    std::fs::File::open(blocks)
        .unwrap()
        .take(4096)
        .read_to_end(&mut prefix)
        .unwrap();
    assert_eq!(prefix, vec![0; 4096]);
}
