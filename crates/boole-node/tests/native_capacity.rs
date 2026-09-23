//! The ignored scenario has preregistered bounds in
//! docs/native-capacity-qualification-2026-09.md. The small case runs in CI.
use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use boole_core::native_chain::{NativeBlock, NativeTransfer};
use boole_core::native_network::{native_testnet, NATIVE_COIN_UNIT};
use boole_core::SigningKeyV2;
use boole_node::NativeNode;
use serde_json::{json, Value};

const PER_BLOCK: u64 = 512;
const REWARD: u128 = 50_000 * NATIVE_COIN_UNIT;
const HISTORY_BUDGET: u64 = 96 * 1024 * 1024;

struct TestDir {
    path: PathBuf,
    identity: (u64, u64),
}

impl TestDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boole-native-capacity-{}",
            boole_testkit::rand_suffix()
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        let meta = fs::symlink_metadata(&path).unwrap();
        Self {
            path,
            identity: (meta.dev(), meta.ino()),
        }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path).is_ok_and(|meta| {
            meta.is_dir()
                && !meta.file_type().is_symlink()
                && (meta.dev(), meta.ino()) == self.identity
        }) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn transfer(owner: &SigningKeyV2, to: &str, nonce: u64) -> NativeTransfer {
    NativeTransfer::try_from(
        &owner
            .sign_for_network(
                &json!({
                    "schema": "boole.transfer.v1", "from": owner.pk_hex(), "to": to,
                    "amount": "1", "fee": native_testnet().minimum_fee().to_string(),
                    "nonce": nonce.to_string(), "validBefore": "1000"
                }),
                Some(native_testnet().network_id()),
            )
            .unwrap(),
    )
    .unwrap()
}

fn record_phase(start: Instant, maximum: &mut Duration, name: &str) {
    let elapsed = start.elapsed();
    *maximum = (*maximum).max(elapsed);
    assert!(
        elapsed < Duration::from_secs(10),
        "{name} exceeded 10s: {elapsed:?}"
    );
}

fn mine(
    node: &NativeNode,
    owner: &SigningKeyV2,
    transfers: Option<&[NativeTransfer]>,
    maximum: &mut Duration,
) -> NativeBlock {
    let height = node.chain().ledger().height() + 1;
    let begin = Instant::now();
    let template = match transfers {
        Some(transfers) => {
            node.chain()
                .template(&owner.pk_hex(), &owner.pk_hex(), height * 60_000, transfers)
        }
        None => node.template(&owner.pk_hex(), &owner.pk_hex(), height * 60_000),
    }
    .unwrap();
    record_phase(begin, maximum, "template");
    let mined = template.mine(0, 2_000_000).unwrap().expect("bounded PoW");
    let auth = owner
        .sign_for_network(
            &mined.authorization_payload().unwrap(),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    mined.authorize(&auth).unwrap()
}

fn append(node: &mut NativeNode, block: NativeBlock, maximum: &mut Duration) {
    let begin = Instant::now();
    assert!(node.submit_block(block).unwrap());
    record_phase(begin, maximum, "durable block submission");
}

fn check_canonical(node: &NativeNode, owner: &str, recipients: u64, extra_nonces: u64) {
    let ledger = node.chain().ledger();
    let issued = u128::from(ledger.height()) * REWARD;
    assert_eq!(ledger.issued(), issued);
    assert_eq!(ledger.balance(owner), issued - u128::from(recipients));
    assert_eq!(ledger.locked_balance(owner), 10 * REWARD);
    assert_eq!(
        ledger.spendable_balance(owner),
        issued - 10 * REWARD - u128::from(recipients)
    );
    assert_eq!(ledger.next_nonce(owner), recipients + extra_nonces);
    let resources = node.resource_usage().unwrap();
    assert_eq!(resources.balance_entries as u64, recipients + 1);
    assert_eq!(resources.nonce_entries, 1);
    assert_eq!(
        resources.confirmed_transfers as u64,
        recipients + extra_nonces
    );
    assert_eq!(resources.history_blocks as u64, ledger.height());
    assert!(resources.history_bytes <= HISTORY_BUDGET);
    for index in 0..recipients {
        assert_eq!(ledger.balance(&format!("{index:064x}")), 1);
    }
}

fn run_scenario(funded_blocks: u64) -> Value {
    let scenario_started = Instant::now();
    let dir = TestDir::new();
    let owner = SigningKeyV2::from_dev_id("native-capacity-producer");
    let owner_pk = owner.pk_hex();
    let mut node = NativeNode::open(&dir.path).unwrap();
    let mut max_template = Duration::ZERO;
    let mut max_append = Duration::ZERO;
    for _ in 0..10 {
        let block = mine(&node, &owner, Some(&[]), &mut max_template);
        append(&mut node, block, &mut max_append);
    }
    for index in 0..funded_blocks {
        let transfers: Vec<_> = (index * PER_BLOCK..(index + 1) * PER_BLOCK)
            .map(|nonce| transfer(&owner, &format!("{nonce:064x}"), nonce))
            .collect();
        let block = mine(&node, &owner, Some(&transfers), &mut max_template);
        append(&mut node, block, &mut max_append);
        assert!(scenario_started.elapsed() < Duration::from_secs(15 * 60));
        if (index + 1).is_multiple_of(32) {
            eprintln!(
                "capacity-progress {}",
                json!({
                    "fundedBlocks": index + 1, "elapsedMs": scenario_started.elapsed().as_millis(),
                "resources": node.resource_usage().unwrap()
                })
            );
        }
    }
    let recipients = funded_blocks * PER_BLOCK;
    check_canonical(&node, &owner_pk, recipients, 0);
    let funded_head = node.chain().head_hash();
    let pending: Vec<_> = (recipients..recipients + PER_BLOCK)
        .map(|nonce| transfer(&owner, &owner_pk, nonce))
        .collect();
    let last_id = pending.last().unwrap().id();
    let begin = Instant::now();
    for tx in pending {
        assert!(node.submit_transfer(tx).unwrap());
    }
    let admission = begin.elapsed();
    assert!(
        admission < Duration::from_secs(20),
        "admission: {admission:?}"
    );
    assert!(node
        .submit_transfer(transfer(&owner, &owner_pk, recipients + PER_BLOCK))
        .is_err());
    node.ensure_ready().unwrap();
    let begin = Instant::now();
    for _ in 0..100 {
        assert_eq!(
            node.pending_view().unwrap().next_nonce(&owner_pk),
            recipients + PER_BLOCK
        );
        assert!(node.is_pending(&last_id));
        assert_eq!(
            node.resource_usage().unwrap().pending_transfers,
            PER_BLOCK as usize
        );
    }
    let lookups = begin.elapsed();
    assert!(lookups < Duration::from_secs(5));
    drop(node);

    let begin = Instant::now();
    let mut node = NativeNode::open(&dir.path).unwrap();
    let restart_pending = begin.elapsed();
    assert!(
        restart_pending < Duration::from_secs(120),
        "restart with pending: {restart_pending:?}"
    );
    assert_eq!(node.chain().head_hash(), funded_head);
    check_canonical(&node, &owner_pk, recipients, 0);
    assert!(node.is_pending(&last_id));
    assert_eq!(
        node.pending_view().unwrap().next_nonce(&owner_pk),
        recipients + PER_BLOCK
    );
    assert_eq!(
        node.pending_view().unwrap().available_balance(&owner_pk),
        node.chain().ledger().spendable_balance(&owner_pk) + REWARD
            - u128::from(PER_BLOCK) * native_testnet().minimum_fee()
    );
    let block = mine(&node, &owner, None, &mut max_template);
    append(&mut node, block, &mut max_append);
    let confirmed_head = node.chain().head_hash();
    drop(node);

    let begin = Instant::now();
    let node = NativeNode::open(&dir.path).unwrap();
    let restart_confirmed = begin.elapsed();
    assert!(
        restart_confirmed < Duration::from_secs(120),
        "restart after confirmation: {restart_confirmed:?}"
    );
    assert_eq!(node.chain().head_hash(), confirmed_head);
    check_canonical(&node, &owner_pk, recipients, PER_BLOCK);
    assert!(node.pending().is_empty());
    assert!(!node.is_pending(&last_id));
    assert_eq!(node.confirmed_height(&last_id), Some(11 + funded_blocks));
    let resources = node.resource_usage().unwrap();
    let elapsed = scenario_started.elapsed();
    let report = json!({
        "fundedBlocks": funded_blocks, "transfersPerBlock": PER_BLOCK,
        "networkId": native_testnet().network_id(), "genesisHash": native_testnet().genesis_hash().to_hex(),
        "fundedHead": funded_head.to_hex(), "confirmedHead": confirmed_head.to_hex(),
        "resources": resources, "issued": node.chain().ledger().issued().to_string(),
        "maxTemplateMs": max_template.as_millis(), "maxAppendMs": max_append.as_millis(),
        "pendingAdmissionMs": admission.as_millis(), "lookup100Micros": lookups.as_micros(),
        "restartPendingMs": restart_pending.as_millis(), "restartConfirmedMs": restart_confirmed.as_millis(),
        "elapsedMs": elapsed.as_millis()
    });
    eprintln!("capacity-result {report}");
    assert!(elapsed < Duration::from_secs(15 * 60));
    // Peak RSS is measured externally for this executable. Qualification is
    // not a pass unless the independent OS measurement also meets the 1GiB cap.
    report
}

#[test]
fn small_capacity_workflow_preserves_observed_resources_and_recovery() {
    let report = run_scenario(2);
    assert_eq!(report["resources"]["balanceEntries"], 1025);
    assert_eq!(report["resources"]["historyBlocks"], 13);
}

#[test]
#[ignore = "explicit preregistered closed-local capacity qualification; not routine CI"]
fn native_capacity_131072_accounts_and_full_pending_queue() {
    let report = run_scenario(256);
    assert_eq!(report["resources"]["balanceEntries"], 131_073);
    assert_eq!(report["resources"]["historyBlocks"], 267);
}
