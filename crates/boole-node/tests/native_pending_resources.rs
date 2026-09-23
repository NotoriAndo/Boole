use std::path::PathBuf;
use std::time::{Duration, Instant};

use boole_core::native_chain::{NativeBlock, NativeChain, NativeTransfer};
use boole_core::native_network::native_testnet;
use boole_core::SigningKeyV2;
use boole_node::NativeNode;

struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boole-pending-resources-{}",
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

fn signed_transfer(key: &SigningKeyV2, to: &str, nonce: u64, valid_before: u64) -> NativeTransfer {
    NativeTransfer::try_from(
        &key.sign_for_network(
            &serde_json::json!({
                "schema": "boole.transfer.v1", "from": key.pk_hex(), "to": to,
                "amount": "1", "fee": "1000", "nonce": nonce.to_string(),
                "validBefore": valid_before.to_string()
            }),
            Some(native_testnet().network_id()),
        )
        .unwrap(),
    )
    .unwrap()
}

fn empty_block(chain: &NativeChain, key: &SigningKeyV2) -> NativeBlock {
    let mined = chain
        .template(
            &key.pk_hex(),
            &key.pk_hex(),
            (chain.ledger().height() + 1) * 60_000,
            &[],
        )
        .unwrap()
        .mine(0, 2_000_000)
        .unwrap()
        .unwrap();
    let auth = key
        .sign_for_network(
            &mined.authorization_payload().unwrap(),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    mined.authorize(&auth).unwrap()
}

#[test]
#[ignore = "explicit preregistered developer-Mac exact-retry measurement; not routine CI"]
fn exact_known_transfer_retries_have_bounded_cpu_cost() {
    let dir = TestDir::new();
    let key = SigningKeyV2::from_dev_id("native-exact-retry-owner");
    let receiver = SigningKeyV2::from_dev_id("native-exact-retry-recipient").pk_hex();
    let mut node = NativeNode::open(&dir.0).unwrap();
    for _ in 0..10 {
        node.submit_block(empty_block(node.chain(), &key)).unwrap();
    }
    let transfers: Vec<_> = (0..512)
        .map(|nonce| signed_transfer(&key, &receiver, nonce, 1000))
        .collect();
    for transfer in &transfers {
        assert!(node.submit_transfer(transfer.clone()).unwrap());
    }
    let canonical = node.chain().clone();
    let files = [
        boole_node::NATIVE_BLOCKS_FILE,
        boole_node::NATIVE_MEMPOOL_FILE,
    ];
    let original: Vec<_> = files
        .iter()
        .map(|file| std::fs::read(dir.0.join(file)).unwrap())
        .collect();
    let available = node
        .pending_view()
        .unwrap()
        .available_balance(&key.pk_hex());
    let started = Instant::now();
    for _ in 0..16 {
        for transfer in &transfers {
            assert!(!node.submit_transfer(transfer.clone()).unwrap());
        }
    }
    let elapsed = started.elapsed();
    assert_eq!(node.chain(), &canonical);
    assert_eq!(node.pending(), &transfers);
    assert_eq!(node.pending_view().unwrap().next_nonce(&key.pk_hex()), 512);
    assert_eq!(
        node.pending_view()
            .unwrap()
            .available_balance(&key.pk_hex()),
        available
    );
    for (file, bytes) in files.iter().zip(original) {
        assert_eq!(std::fs::read(dir.0.join(file)).unwrap(), bytes);
    }
    eprintln!("exact-transfer-retries {{\"retries\":8192,\"pending\":512,\"elapsedMicros\":{},\"journalsUnchanged\":true}}", elapsed.as_micros());
    assert!(
        elapsed < Duration::from_secs(1),
        "exact known retries repeated too much immutable signature work: {elapsed:?}"
    );
}

#[test]
fn pending_reservations_follow_maturity_expiry_rejection_and_restart() {
    let dir = TestDir::new();
    let owner = SigningKeyV2::from_dev_id("pending-boundaries-owner");
    let recipient = SigningKeyV2::from_dev_id("pending-boundaries-recipient").pk_hex();
    let mut node = NativeNode::open(&dir.0).unwrap();
    for _ in 1..=9 {
        node.submit_block(empty_block(node.chain(), &owner))
            .unwrap();
    }
    assert_eq!(
        node.pending_view()
            .unwrap()
            .available_balance(&owner.pk_hex()),
        0
    );
    let expires = signed_transfer(&owner, &recipient, 0, 11);
    assert!(
        node.submit_transfer(expires.clone()).is_err(),
        "reward not mature yet"
    );
    node.ensure_ready().unwrap();
    node.submit_block(empty_block(node.chain(), &owner))
        .unwrap();
    let reward = 50_000 * 100_000_000;
    assert_eq!(
        node.pending_view()
            .unwrap()
            .available_balance(&owner.pk_hex()),
        reward
    );
    assert!(node.submit_transfer(expires.clone()).unwrap());
    assert!(!node.submit_transfer(expires.clone()).unwrap());
    assert!(node
        .submit_transfer(signed_transfer(&owner, &recipient, 2, 100))
        .is_err());
    assert_eq!(node.pending_view().unwrap().next_nonce(&owner.pk_hex()), 1);
    assert_eq!(
        node.pending_view()
            .unwrap()
            .available_balance(&owner.pk_hex()),
        reward - 1001
    );
    assert_eq!(
        node.pending_view().unwrap().available_balance(&recipient),
        1
    );
    node.ensure_ready().unwrap();
    drop(node);
    let mut node = NativeNode::open(&dir.0).unwrap();
    assert_eq!(node.pending_view().unwrap().next_nonce(&owner.pk_hex()), 1);
    assert_eq!(node.pending(), std::slice::from_ref(&expires));
    // A peer may mine an empty block instead of including our pending transfer.
    node.submit_block(empty_block(node.chain(), &owner))
        .unwrap();
    assert!(node.pending().is_empty());
    assert_eq!(node.pending_view().unwrap().next_nonce(&owner.pk_hex()), 0);
    assert_eq!(
        node.pending_view()
            .unwrap()
            .available_balance(&owner.pk_hex()),
        2 * reward
    );
    assert_eq!(
        node.pending_view().unwrap().available_balance(&recipient),
        0
    );
    assert!(
        node.submit_transfer(expires).is_err(),
        "expired at the next candidate height"
    );
    let replacement = signed_transfer(&owner, &recipient, 0, 100);
    assert!(node.submit_transfer(replacement.clone()).unwrap());
    assert_eq!(node.pending_view().unwrap().next_nonce(&owner.pk_hex()), 1);
    node.submit_block(empty_block(node.chain(), &owner))
        .unwrap();
    assert_eq!(node.pending(), std::slice::from_ref(&replacement));
    assert_eq!(node.pending_view().unwrap().next_nonce(&owner.pk_hex()), 1);
    assert_eq!(
        node.pending_view()
            .unwrap()
            .available_balance(&owner.pk_hex()),
        3 * reward - 1001
    );
}

#[test]
fn full_pending_queue_remains_usable_for_admission_reads_and_confirmation() {
    let dir = TestDir::new();
    let key = SigningKeyV2::from_dev_id("pending-resource-owner");
    let mut node = NativeNode::open(&dir.0).unwrap();
    for height in 1..=10 {
        let block = node
            .template(&key.pk_hex(), &key.pk_hex(), height * 60_000)
            .unwrap()
            .mine(0, 2_000_000)
            .unwrap()
            .unwrap();
        let signature = key
            .sign_for_network(
                &block.authorization_payload().unwrap(),
                Some(native_testnet().network_id()),
            )
            .unwrap();
        node.submit_block(block.authorize(&signature).unwrap())
            .unwrap();
    }
    let transfers: Vec<_> = (0..512)
        .map(|nonce| {
            let recipient =
                SigningKeyV2::from_dev_id(&format!("pending-resource-recipient-{nonce}"));
            signed_transfer(&key, &recipient.pk_hex(), nonce, 1000)
        })
        .collect();
    let admission = Instant::now();
    for (index, transfer) in transfers.iter().enumerate() {
        assert!(node.submit_transfer(transfer.clone()).unwrap());
        if (index + 1).is_multiple_of(128) {
            eprintln!(
                "{} durable pending admissions: {:?}",
                index + 1,
                admission.elapsed()
            );
            // A generous regression budget, not a production latency SLA.
            assert!(
                admission.elapsed() < Duration::from_secs(20),
                "bounded queue admission replays too much prior work"
            );
        }
    }
    assert_eq!(node.pending().len(), 512);
    assert!(!node.submit_transfer(transfers[0].clone()).unwrap());
    assert!(node
        .submit_transfer(signed_transfer(&key, &key.pk_hex(), 512, 1000))
        .is_err());
    node.ensure_ready().unwrap();
    assert_eq!(node.pending().len(), 512);
    let reads = Instant::now();
    let last_id = transfers.last().unwrap().id();
    for _ in 0..100 {
        let view = node.pending_view().unwrap();
        assert_eq!(view.next_nonce(&key.pk_hex()), 512);
        assert!(node.is_pending(&last_id));
        assert!(
            reads.elapsed() < Duration::from_secs(5),
            "account reads replay the full pending queue"
        );
    }
    eprintln!(
        "100 full-queue pending account reads: {:?}",
        reads.elapsed()
    );
    assert_eq!(node.chain().ledger().next_nonce(&key.pk_hex()), 0);
    let block = node
        .template(&key.pk_hex(), &key.pk_hex(), 660_000)
        .unwrap()
        .mine(0, 2_000_000)
        .unwrap()
        .unwrap();
    let signature = key
        .sign_for_network(
            &block.authorization_payload().unwrap(),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    node.submit_block(block.authorize(&signature).unwrap())
        .unwrap();
    assert!(node.pending().is_empty());
    assert!(!node.is_pending(&last_id));
    assert_eq!(node.pending_view().unwrap().next_nonce(&key.pk_hex()), 512);
    assert_eq!(node.chain().ledger().next_nonce(&key.pk_hex()), 512);
    let expected = node.chain().clone();
    drop(node);
    assert_eq!(NativeNode::open(&dir.0).unwrap().chain(), &expected);
}
