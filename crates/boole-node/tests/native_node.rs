use std::path::PathBuf;

use boole_core::native_chain::{NativeBlock, NativeChain, NativeTransfer};
use boole_core::native_network::native_testnet;
use boole_core::signed_envelope::SigningKeyV2;
use boole_node::NativeNode;

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boole-native-node-test-{}",
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

fn mine(chain: &NativeChain, key: &SigningKeyV2, reward: &str, ts: u64) -> NativeBlock {
    let block = chain
        .template(&key.pk_hex(), reward, ts, &[])
        .unwrap()
        .mine(0, 2_000_000)
        .unwrap()
        .expect("bounded PoW");
    let signature = key
        .sign_for_network(
            &block.authorization_payload().unwrap(),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    block.authorize(&signature).unwrap()
}

#[test]
fn native_node_persists_a_verified_reward_and_recovers_it_exactly_once() {
    let dir = TestDir::new();
    let miner = SigningKeyV2::from_dev_id("native-durable-miner");
    let reward = SigningKeyV2::from_dev_id("native-durable-reward").pk_hex();
    let expected;
    {
        let mut node = NativeNode::open(&dir.0).unwrap();
        let block = mine(node.chain(), &miner, &reward, 60_000);
        node.submit_block(block).unwrap();
        expected = node.chain().clone();
        assert!(NativeNode::open(&dir.0).is_err(), "single writer ownership");
    }
    for _ in 0..2 {
        let node = NativeNode::open(&dir.0).unwrap();
        assert_eq!(node.chain(), &expected);
        assert_eq!(node.chain().ledger().balance(&reward), 50_000 * 100_000_000);
    }
}

#[test]
fn native_recovery_refuses_oversized_manifest_without_rewriting_it() {
    let dir = TestDir::new();
    drop(NativeNode::open(&dir.0).unwrap());
    let path = dir.0.join("state.manifest.json");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend(std::iter::repeat_n(b' ', 65_536));
    std::fs::write(&path, &bytes).unwrap();
    assert!(
        NativeNode::open(&dir.0).is_err(),
        "unbounded manifest was accepted"
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn native_manifest_symlinks_hardlinks_and_fifos_fail_without_following_or_blocking() {
    use std::os::unix::fs::symlink;
    let dir = TestDir::new();
    drop(NativeNode::open(&dir.0).unwrap());
    let path = dir.0.join("state.manifest.json");
    let saved = dir.0.join("saved-manifest.json");
    std::fs::rename(&path, &saved).unwrap();
    let bytes = std::fs::read(&saved).unwrap();
    symlink(&saved, &path).unwrap();
    assert!(NativeNode::open(&dir.0).is_err());
    std::fs::remove_file(&path).unwrap();
    std::fs::hard_link(&saved, &path).unwrap();
    assert!(NativeNode::open(&dir.0).is_err());
    std::fs::remove_file(&path).unwrap();
    assert!(std::process::Command::new("mkfifo")
        .arg(&path)
        .status()
        .unwrap()
        .success());
    let start = std::time::Instant::now();
    assert!(NativeNode::open(&dir.0).is_err());
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(std::fs::read(&saved).unwrap(), bytes);
    std::fs::remove_file(&path).unwrap();
    std::fs::rename(&saved, &path).unwrap();
    assert!(NativeNode::open(&dir.0).is_ok());
}

#[test]
fn pending_owner_transfer_survives_restart_and_is_removed_only_after_durable_inclusion() {
    let dir = TestDir::new();
    let alice = SigningKeyV2::from_dev_id("native-pending-alice");
    let bob = SigningKeyV2::from_dev_id("native-pending-bob").pk_hex();
    let miner = SigningKeyV2::from_dev_id("native-pending-miner");
    let signed = alice
        .sign_for_network(
            &serde_json::json!({
                "schema": "boole.transfer.v1", "from": alice.pk_hex(), "to": bob,
                "amount": "100000000", "fee": "1000", "nonce": "0", "validBefore": "100"
            }),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    let transfer = NativeTransfer::try_from(&signed).unwrap();
    {
        let mut node = NativeNode::open(&dir.0).unwrap();
        for height in 1..=10 {
            let reward = if height == 1 {
                alice.pk_hex()
            } else {
                miner.pk_hex()
            };
            node.submit_block(mine(node.chain(), &miner, &reward, height * 60_000))
                .unwrap();
        }
        assert!(node.submit_transfer(transfer.clone()).unwrap());
        assert!(!node.submit_transfer(transfer.clone()).unwrap());
        assert_eq!(node.pending().len(), 1);
        assert_eq!(node.chain().ledger().next_nonce(&alice.pk_hex()), 0);
    }
    {
        let mut node = NativeNode::open(&dir.0).unwrap();
        assert_eq!(node.pending(), std::slice::from_ref(&transfer));
        let block = node
            .template(&miner.pk_hex(), &miner.pk_hex(), 660_000)
            .unwrap()
            .mine(0, 2_000_000)
            .unwrap()
            .unwrap();
        let authorization = miner
            .sign_for_network(
                &block.authorization_payload().unwrap(),
                Some(native_testnet().network_id()),
            )
            .unwrap();
        node.submit_block(block.authorize(&authorization).unwrap())
            .unwrap();
        assert!(node.pending().is_empty());
        assert_eq!(node.chain().ledger().balance(&bob), 100_000_000);
        assert_eq!(node.chain().ledger().next_nonce(&alice.pk_hex()), 1);
        assert!(!node.submit_transfer(transfer.clone()).unwrap());
    }
    let node = NativeNode::open(&dir.0).unwrap();
    assert!(node.pending().is_empty());
    assert_eq!(node.chain().ledger().balance(&bob), 100_000_000);
}

#[test]
fn heavier_fork_rebuilds_balances_and_restores_an_orphaned_transfer_for_safe_retry() {
    let dir = TestDir::new();
    let alice = SigningKeyV2::from_dev_id("native-reorg-alice");
    let bob = SigningKeyV2::from_dev_id("native-reorg-bob").pk_hex();
    let miner = SigningKeyV2::from_dev_id("native-reorg-miner");
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
    let candidate;
    let former;
    {
        let mut node = NativeNode::open(&dir.0).unwrap();
        for height in 1..=10 {
            let reward = if height == 1 {
                alice.pk_hex()
            } else {
                miner.pk_hex()
            };
            node.submit_block(mine(node.chain(), &miner, &reward, height * 60_000))
                .unwrap();
        }
        let mut alternative = node.chain().clone();
        node.submit_transfer(transfer.clone()).unwrap();
        let block = node
            .template(&miner.pk_hex(), &miner.pk_hex(), 660_000)
            .unwrap()
            .mine(0, 2_000_000)
            .unwrap()
            .unwrap();
        let auth = miner
            .sign_for_network(
                &block.authorization_payload().unwrap(),
                Some(native_testnet().network_id()),
            )
            .unwrap();
        node.submit_block(block.authorize(&auth).unwrap()).unwrap();
        assert_eq!(node.chain().ledger().balance(&bob), 100_000_000);
        former = node.chain().clone();
        for height in 11..=12 {
            alternative
                .append(mine(&alternative, &miner, &miner.pk_hex(), height * 60_000))
                .unwrap();
        }
        assert!(node.adopt_chain(alternative.blocks()).unwrap());
        assert_eq!(node.chain(), &alternative);
        assert_eq!(node.chain().ledger().balance(&bob), 0);
        assert_eq!(node.chain().ledger().next_nonce(&alice.pk_hex()), 0);
        assert_eq!(node.pending(), std::slice::from_ref(&transfer));
        assert_eq!(node.confirmed_height(&transfer.id()), None);
        candidate = alternative;
    }
    let node = NativeNode::open(&dir.0).unwrap();
    assert_eq!(node.chain(), &candidate);
    assert_eq!(node.pending(), std::slice::from_ref(&transfer));
    drop(node);
    // Valid crash-boundary snapshots: the recovery union has reached disk,
    // while the atomic canonical-file replacement may or may not have won.
    for (chain, pending) in [(&former, false), (&candidate, true)] {
        let mut bytes = String::new();
        for block in chain.blocks() {
            bytes.push_str(&serde_json::to_string(block).unwrap());
            bytes.push('\n');
        }
        std::fs::write(dir.0.join(boole_node::NATIVE_BLOCKS_FILE), bytes).unwrap();
        std::fs::write(
            dir.0.join(boole_node::NATIVE_MEMPOOL_FILE),
            format!("{}\n", serde_json::to_string(&transfer).unwrap()),
        )
        .unwrap();
        let recovered = NativeNode::open(&dir.0).unwrap();
        assert_eq!(recovered.chain(), chain);
        assert_eq!(recovered.pending().len(), usize::from(pending));
        assert_eq!(
            recovered.confirmed_height(&transfer.id()).is_none(),
            pending
        );
    }
}

#[test]
fn future_drift_is_rejected_at_ingress_reorg_and_boot_without_putting_a_clock_in_consensus() {
    let dir = TestDir::new();
    let miner = SigningKeyV2::from_dev_id("native-future-miner");
    let now = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let empty = NativeChain::new().unwrap();
    let future = mine(&empty, &miner, &miner.pk_hex(), now + 3 * 60 * 60 * 1000);
    assert!(
        NativeChain::replay(std::slice::from_ref(&future)).is_ok(),
        "core replay is clock-free"
    );
    {
        let mut node = NativeNode::open(&dir.0).unwrap();
        assert!(node.submit_block(future.clone()).is_err());
        assert!(node.adopt_chain(std::slice::from_ref(&future)).is_err());
        assert!(node
            .template(&miner.pk_hex(), &miner.pk_hex(), future.header.timestamp_ms)
            .is_err());
        assert_eq!(node.chain(), &empty);
    }
    std::fs::write(
        dir.0.join(boole_node::NATIVE_BLOCKS_FILE),
        format!("{}\n", serde_json::to_string(&future).unwrap()),
    )
    .unwrap();
    assert!(NativeNode::open(&dir.0).is_err());
}

#[test]
fn losing_or_replacing_live_state_files_cannot_append_from_stale_memory() {
    for replace in [false, true] {
        let dir = TestDir::new();
        let miner = SigningKeyV2::from_dev_id("native-state-file-loss");
        let mut node = NativeNode::open(&dir.0).unwrap();
        node.submit_block(mine(node.chain(), &miner, &miner.pk_hex(), 60_000))
            .unwrap();
        let second = mine(node.chain(), &miner, &miner.pk_hex(), 120_000);
        let path = dir.0.join(boole_node::NATIVE_BLOCKS_FILE);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        if replace {
            std::fs::write(&path, &bytes).unwrap();
        }
        assert!(node.submit_block(second).is_err());
        assert_eq!(node.chain().ledger().height(), 1);
        if replace {
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        } else {
            assert!(!path.exists());
        }
    }
}

#[test]
fn oversized_recovery_files_are_rejected_without_reading_or_truncating_them() {
    for (name, limit) in [
        (boole_node::NATIVE_BLOCKS_FILE, 256 * 1024 * 1024),
        (boole_node::NATIVE_MEMPOOL_FILE, 5 * 1024 * 1024),
    ] {
        let dir = TestDir::new();
        let path = dir.0.join(name);
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(limit + 1).unwrap();
        drop(file);
        assert!(NativeNode::open(&dir.0).is_err());
        assert_eq!(std::fs::metadata(path).unwrap().len(), limit + 1);
    }
}
