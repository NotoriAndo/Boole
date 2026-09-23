use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use boole_core::native_chain::{NativeBlock, NativeChain, NativeTransfer};
use boole_core::native_network::native_testnet;
use boole_core::SigningKeyV2;
use boole_node::NativeNode;

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boole-native-archive-{}",
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

fn mine(chain: &NativeChain, key: &SigningKeyV2, transfers: &[NativeTransfer]) -> NativeBlock {
    let block = chain
        .template(
            &key.pk_hex(),
            &key.pk_hex(),
            (chain.ledger().height() + 1) * 60_000,
            transfers,
        )
        .unwrap()
        .mine(0, 2_000_000)
        .unwrap()
        .expect("bounded PoW");
    let auth = key
        .sign_for_network(
            &block.authorization_payload().unwrap(),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    block.authorize(&auth).unwrap()
}

fn export(state: &Path, archive: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .arg("native-export")
        .arg("--state-dir")
        .arg(state)
        .arg("--output")
        .arg(archive)
        .output()
        .unwrap()
}
fn import(state: &Path, archive: &Path, head: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_boole-node"))
        .arg("native-import")
        .arg("--state-dir")
        .arg(state)
        .arg("--blocks")
        .arg(archive)
        .arg("--expected-head")
        .arg(head)
        .output()
        .unwrap()
}
fn success(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn offline_archive_restores_verified_balances_nonces_and_confirmation_exactly_once() {
    let dir = TestDir::new();
    let source = dir.0.join("source");
    let destination = dir.0.join("restored");
    let archive = dir.0.join("chain.ndjson");
    let alice = SigningKeyV2::from_dev_id("archive-alice");
    let bob = SigningKeyV2::from_dev_id("archive-bob");
    let transfer = NativeTransfer::try_from(
        &alice
            .sign_for_network(
                &serde_json::json!({
                    "schema": "boole.transfer.v1", "from": alice.pk_hex(), "to": bob.pk_hex(),
                    "amount": "100000000", "fee": "1000", "nonce": "0", "validBefore": "100"
                }),
                Some(native_testnet().network_id()),
            )
            .unwrap(),
    )
    .unwrap();
    let expected = {
        let mut node = NativeNode::open(&source).unwrap();
        for _ in 0..10 {
            node.submit_block(mine(node.chain(), &alice, &[])).unwrap();
        }
        node.submit_block(mine(node.chain(), &alice, std::slice::from_ref(&transfer)))
            .unwrap();
        node.chain().clone()
    };
    let exported = success(export(&source, &archive));
    assert_eq!(exported["headHash"], expected.head_hash().to_hex());
    let before = std::fs::read(&archive).unwrap();
    let imported = success(import(
        &destination,
        &archive,
        &expected.head_hash().to_hex(),
    ));
    assert_eq!(imported["adopted"], true);
    for _ in 0..2 {
        let node = NativeNode::open(&destination).unwrap();
        assert_eq!(node.chain(), &expected);
        assert_eq!(node.confirmed_height(&transfer.id()), Some(11));
        assert_eq!(node.chain().ledger().next_nonce(&alice.pk_hex()), 1);
        assert_eq!(node.chain().ledger().balance(&bob.pk_hex()), 100_000_000);
    }
    assert_eq!(
        success(import(
            &destination,
            &archive,
            &expected.head_hash().to_hex()
        ))["adopted"],
        false
    );
    assert_eq!(std::fs::read(&archive).unwrap(), before);
    assert_eq!(NativeNode::open(&source).unwrap().chain(), &expected);
}

#[test]
fn invalid_archive_is_rejected_before_destination_creation_and_source_is_untouched() {
    let dir = TestDir::new();
    let key = SigningKeyV2::from_dev_id("archive-invalid");
    let block = mine(&NativeChain::new().unwrap(), &key, &[]);
    let head = block.hash().unwrap().to_hex();
    let line = serde_json::to_string(&block).unwrap();
    let mut forged = block.clone();
    forged.producer_signature = "00".repeat(64);
    let inputs = [
        (format!("{line}\n"), "00".repeat(32)),
        (line.clone(), head.clone()),
        (format!("{line}\n{{"), head.clone()),
        (
            format!("{}\n", serde_json::to_string(&forged).unwrap()),
            head.clone(),
        ),
        (format!("{{\"unknown\":true,{}\n", &line[1..]), head.clone()),
        (format!("{line}\n\n"), head.clone()),
        (
            "x".repeat(native_testnet().max_block_bytes() + 2),
            head.clone(),
        ),
    ];
    for (index, (bytes, expected)) in inputs.iter().enumerate() {
        let archive = dir.0.join(format!("bad-{index}.ndjson"));
        let destination = dir.0.join(format!("unused-{index}"));
        std::fs::write(&archive, bytes).unwrap();
        let result = import(&destination, &archive, expected);
        assert!(!result.status.success(), "case {index} accepted");
        assert!(!destination.exists(), "case {index} touched destination");
        assert_eq!(std::fs::read_to_string(&archive).unwrap(), *bytes);
    }
}

#[test]
fn export_refuses_missing_source_instead_of_creating_a_genesis_backup() {
    let dir = TestDir::new();
    let source = dir.0.join("mistyped-state");
    let archive = dir.0.join("must-not-exist.ndjson");
    let result = export(&source, &archive);
    assert!(
        !result.status.success(),
        "missing source was reported as a successful backup"
    );
    assert!(!source.exists());
    assert!(!archive.exists());
}

#[test]
fn offline_recovery_crosses_online_fork_and_rpc_caps_without_losing_orphaned_transfers() {
    let started = std::time::Instant::now();
    let dir = TestDir::new();
    let key = SigningKeyV2::from_dev_id("archive-long-fork");
    let bob = SigningKeyV2::from_dev_id("archive-long-fork-bob");
    let mut chain = NativeChain::new().unwrap();
    for _ in 0..10 {
        chain.append(mine(&chain, &key, &[])).unwrap();
    }
    let transfer = NativeTransfer::try_from(
        &key.sign_for_network(
            &serde_json::json!({
                "schema": "boole.transfer.v1", "from": key.pk_hex(), "to": bob.pk_hex(),
                "amount": "100000000", "fee": "1000", "nonce": "0", "validBefore": "2000"
            }),
            Some(native_testnet().network_id()),
        )
        .unwrap(),
    )
    .unwrap();
    let destination = dir.0.join("old-fork");
    {
        let mut node = NativeNode::open(&destination).unwrap();
        node.adopt_chain(chain.blocks()).unwrap();
        node.submit_block(mine(node.chain(), &key, std::slice::from_ref(&transfer)))
            .unwrap();
        assert_eq!(node.confirmed_height(&transfer.id()), Some(11));
    }
    for _ in 10..1025 {
        chain.append(mine(&chain, &key, &[])).unwrap();
    }
    let source = dir.0.join("long-fork");
    {
        let mut node = NativeNode::open(&source).unwrap();
        node.adopt_chain(chain.blocks()).unwrap();
    }
    let archive = dir.0.join("long.ndjson");
    success(export(&source, &archive));
    success(import(&destination, &archive, &chain.head_hash().to_hex()));
    let node = NativeNode::open(&destination).unwrap();
    assert_eq!(node.chain(), &chain);
    assert_eq!(node.pending(), std::slice::from_ref(&transfer));
    assert_eq!(node.confirmed_height(&transfer.id()), None);
    assert_eq!(node.chain().ledger().balance(&bob.pk_hex()), 0);
    assert_eq!(node.chain().ledger().next_nonce(&key.pk_hex()), 0);
    eprintln!(
        "1025-block offline recovery including mining and all replays: {:?}",
        started.elapsed()
    );
}

#[test]
fn export_cannot_publish_an_archive_as_an_uncreated_state_role() {
    let dir = TestDir::new();
    let source = dir.0.join("source");
    let key = SigningKeyV2::from_dev_id("archive-role-collision");
    let expected = {
        let mut node = NativeNode::open(&source).unwrap();
        node.submit_block(mine(node.chain(), &key, &[])).unwrap();
        node.chain().clone()
    };
    let pool = source.join(boole_node::NATIVE_MEMPOOL_FILE);
    assert!(!pool.exists());
    assert!(!export(&source, &pool).status.success());
    assert!(
        !pool.exists(),
        "export corrupted a previously absent state role"
    );
    assert_eq!(NativeNode::open(&source).unwrap().chain(), &expected);
}

#[test]
fn offline_files_refuse_aliases_special_files_overwrite_and_parallel_state_owners() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = TestDir::new();
    let source = dir.0.join("source");
    let key = SigningKeyV2::from_dev_id("archive-file-boundaries");
    let mut node = NativeNode::open(&source).unwrap();
    node.submit_block(mine(node.chain(), &key, &[])).unwrap();
    let expected = node.chain().clone();
    let archive = dir.0.join("blocks.ndjson");
    assert!(
        !export(&source, &archive).status.success(),
        "live owner was bypassed"
    );
    assert!(!archive.exists());
    drop(node);
    success(export(&source, &archive));
    let bytes = std::fs::read(&archive).unwrap();
    assert!(
        !export(&source, &archive).status.success(),
        "overwrite was allowed"
    );
    assert_eq!(std::fs::read(&archive).unwrap(), bytes);
    let head = expected.head_hash().to_hex();
    let destination = dir.0.join("destination");
    let symlink_path = dir.0.join("alias");
    symlink(&archive, &symlink_path).unwrap();
    assert!(!import(&destination, &symlink_path, &head).status.success());
    let hardlink = dir.0.join("hardlink");
    std::fs::hard_link(&archive, &hardlink).unwrap();
    assert!(!import(&destination, &archive, &head).status.success());
    std::fs::remove_file(&hardlink).unwrap();
    std::fs::set_permissions(&archive, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert!(!import(&destination, &archive, &head).status.success());
    std::fs::set_permissions(&archive, std::fs::Permissions::from_mode(0o600)).unwrap();
    let fifo = dir.0.join("fifo");
    assert!(Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    let start = std::time::Instant::now();
    assert!(!import(&destination, &fifo, &head).status.success());
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    assert!(!import(&destination, &dir.0, &head).status.success());
    let large = dir.0.join("large");
    std::fs::File::create(&large)
        .unwrap()
        .set_len(256 * 1024 * 1024 + 1)
        .unwrap();
    assert!(!import(&destination, &large, &head).status.success());
    assert!(!destination.exists());
    let held = NativeNode::open(&destination).unwrap();
    assert!(!import(&destination, &archive, &head).status.success());
    assert_eq!(held.chain().ledger().height(), 0);
    drop(held);
    success(import(&destination, &archive, &head));
    assert_eq!(NativeNode::open(&destination).unwrap().chain(), &expected);
    assert_eq!(std::fs::read(&archive).unwrap(), bytes);
}

#[test]
fn offline_import_never_forces_a_rollback_or_overwrites_a_corrupt_node() {
    let dir = TestDir::new();
    let key = SigningKeyV2::from_dev_id("archive-recovery-not-reset");
    let source = dir.0.join("source");
    let first = {
        let mut node = NativeNode::open(&source).unwrap();
        node.submit_block(mine(node.chain(), &key, &[])).unwrap();
        node.chain().clone()
    };
    let archive = dir.0.join("one.ndjson");
    success(export(&source, &archive));
    let destination = dir.0.join("ahead");
    let expected = {
        let mut node = NativeNode::open(&destination).unwrap();
        node.adopt_chain(first.blocks()).unwrap();
        node.submit_block(mine(node.chain(), &key, &[])).unwrap();
        node.chain().clone()
    };
    assert!(!import(&destination, &archive, &first.head_hash().to_hex())
        .status
        .success());
    assert_eq!(NativeNode::open(&destination).unwrap().chain(), &expected);
    let blocks = destination.join(boole_node::NATIVE_BLOCKS_FILE);
    let corrupt = b"{\"doNotErase\":true}\n";
    std::fs::write(&blocks, corrupt).unwrap();
    assert!(!import(&destination, &archive, &first.head_hash().to_hex())
        .status
        .success());
    assert_eq!(std::fs::read(&blocks).unwrap(), corrupt);
    let fresh = dir.0.join("fresh-recovery");
    success(import(&fresh, &archive, &first.head_hash().to_hex()));
    assert_eq!(NativeNode::open(&fresh).unwrap().chain(), &first);
    assert_eq!(std::fs::read(&blocks).unwrap(), corrupt);
}

#[test]
fn export_is_source_preserving_even_when_restart_would_repair_a_torn_tail() {
    use std::io::Write;
    let dir = TestDir::new();
    let source = dir.0.join("source");
    let key = SigningKeyV2::from_dev_id("archive-preserves-torn-source");
    {
        let mut node = NativeNode::open(&source).unwrap();
        node.submit_block(mine(node.chain(), &key, &[])).unwrap();
    }
    let blocks = source.join(boole_node::NATIVE_BLOCKS_FILE);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&blocks)
        .unwrap()
        .write_all(b"{torn-evidence")
        .unwrap();
    let before = std::fs::read(&blocks).unwrap();
    let archive = dir.0.join("should-not-publish");
    assert!(
        !export(&source, &archive).status.success(),
        "export silently repaired source"
    );
    assert_eq!(std::fs::read(&blocks).unwrap(), before);
    assert!(!archive.exists());
}
