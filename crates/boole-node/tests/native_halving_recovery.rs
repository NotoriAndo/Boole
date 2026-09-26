//! Actual-height qualification and its small public-interface companion.
//! Fixed criteria: docs/native-halving-recovery-qualification-2026-09.md.
use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use boole_core::native_chain::{NativeBlock, NativeBlockTemplate, NativeTransfer};
use boole_core::native_network::{native_testnet, NATIVE_COIN_UNIT};
use boole_core::SigningKeyV2;
use boole_node::{audit_native_state, NativeNode};
use serde_json::{json, Value};

const FULL_REWARD: u128 = 50_000 * NATIVE_COIN_UNIT;
const FEE: u128 = 1_000;

struct Scratch {
    path: PathBuf,
    identity: (u64, u64),
}

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "boole-native-halving-{}",
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

impl Drop for Scratch {
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

#[derive(Default)]
struct Timings {
    template: Duration,
    append: Duration,
    candidate: Duration,
}

fn phase(begin: Instant, maximum: &mut Duration, label: &str) {
    let elapsed = begin.elapsed();
    *maximum = (*maximum).max(elapsed);
    assert!(elapsed < Duration::from_secs(10), "{label}: {elapsed:?}");
}

// Independent, explicit arithmetic for the two epochs used by this scenario.
// Do not ask the production emission implementation to produce its own oracle.
fn reward(height: u64) -> u128 {
    assert!((1..20_000).contains(&height));
    if height < 10_000 {
        FULL_REWARD
    } else {
        FULL_REWARD / 2
    }
}

fn issued(height: u64) -> u128 {
    u128::from(height.min(9_999)) * FULL_REWARD
        + u128::from(height.saturating_sub(9_999)) * (FULL_REWARD / 2)
}

fn authorize(template: NativeBlockTemplate, producer: &SigningKeyV2) -> NativeBlock {
    let mined = template.mine(0, 2_000_000).unwrap().expect("bounded PoW");
    let auth = producer
        .sign_for_network(
            &mined.authorization_payload().unwrap(),
            Some(native_testnet().network_id()),
        )
        .unwrap();
    mined.authorize(&auth).unwrap()
}

fn extend(node: &mut NativeNode, producer: &SigningKeyV2, reward_pk: &str, t: &mut Timings) {
    let height = node.chain().ledger().height() + 1;
    let before = node.chain().ledger().issued();
    let begin = Instant::now();
    let template = node
        .template(&producer.pk_hex(), reward_pk, height * 60_000)
        .unwrap();
    phase(begin, &mut t.template, "template");
    let block = authorize(template, producer);
    let begin = Instant::now();
    assert!(node.submit_block(block).unwrap());
    phase(begin, &mut t.append, "durable append");
    assert_eq!(node.chain().ledger().issued() - before, reward(height));
    assert_eq!(node.chain().ledger().issued(), issued(height));
}

fn files(path: &Path) -> Vec<Option<String>> {
    [
        "native-blocks.ndjson",
        "native-mempool.ndjson",
        "state.manifest.json",
    ]
    .into_iter()
    .map(|name| match fs::read(path.join(name)) {
        Ok(bytes) => Some(blake3::hash(&bytes).to_hex().to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("state read: {error}"),
    })
    .collect()
}

fn reject_unchanged(node: &mut NativeNode, path: &Path, transfer: &NativeTransfer) {
    let before_files = files(path);
    let before_head = node.chain().head_hash();
    let before_ledger = node.chain().ledger().clone();
    assert!(node.submit_transfer(transfer.clone()).is_err());
    node.ensure_ready().unwrap();
    assert_eq!(node.chain().head_hash(), before_head);
    assert_eq!(node.chain().ledger(), &before_ledger);
    assert!(node.pending().is_empty());
    assert_eq!(files(path), before_files);
}

fn check_state(
    node: &NativeNode,
    producer: &str,
    owner: &str,
    recipient: &str,
    owner_reward_height: Option<u64>,
    confirmed: bool,
) {
    node.ensure_ready().unwrap();
    let ledger = node.chain().ledger();
    let height = ledger.height();
    let owner_reward = owner_reward_height.map_or(0, reward);
    let owner_locked = owner_reward_height
        .filter(|h| height < h + 10)
        .map_or(0, reward);
    let total_locked: u128 = ((height.saturating_sub(10) + 1)..=height).map(reward).sum();
    let debit = if confirmed { NATIVE_COIN_UNIT + FEE } else { 0 };
    let amount = if confirmed { NATIVE_COIN_UNIT } else { 0 };
    let fee = if confirmed { FEE } else { 0 };
    assert_eq!(ledger.issued(), issued(height));
    assert_eq!(
        ledger.balance(producer),
        issued(height) - owner_reward + fee
    );
    assert_eq!(ledger.balance(owner), owner_reward - debit);
    assert_eq!(ledger.balance(recipient), amount);
    assert_eq!(ledger.next_nonce(owner), u64::from(confirmed));
    assert_eq!(ledger.next_nonce(producer), 0);
    assert_eq!(ledger.next_nonce(recipient), 0);
    assert_eq!(ledger.locked_balance(owner), owner_locked);
    assert_eq!(
        ledger.spendable_balance(owner),
        owner_reward - debit - owner_locked
    );
    assert_eq!(ledger.locked_balance(producer), total_locked - owner_locked);
    assert_eq!(ledger.locked_balance(recipient), 0);
    let accounting = ledger.audit().unwrap();
    assert_eq!(accounting.total_balance, issued(height));
    assert_eq!(accounting.total_locked, total_locked);
    assert_eq!(accounting.total_spendable, issued(height) - total_locked);
    assert_eq!(accounting.pending_reward_entries, 10);
    let resources = node.resource_usage().unwrap();
    assert_eq!(resources.history_blocks as u64, height);
    assert_eq!(resources.confirmed_transfers, usize::from(confirmed));
    assert!(resources.history_bytes < 96 * 1024 * 1024);
}

fn reopen(path: &Path) -> (NativeNode, u128) {
    let begin = Instant::now();
    let node = NativeNode::open(path).unwrap();
    let elapsed = begin.elapsed();
    assert!(elapsed < Duration::from_secs(120), "reopen: {elapsed:?}");
    (node, elapsed.as_millis())
}

fn run(special_height: u64) -> Value {
    let started = Instant::now();
    let scratch = Scratch::new();
    let producer = SigningKeyV2::from_dev_id("native-halving-producer");
    let owner = SigningKeyV2::from_dev_id("native-halving-owner");
    let recipient = SigningKeyV2::from_dev_id("native-halving-recipient").pk_hex();
    let producer_pk = producer.pk_hex();
    let owner_pk = owner.pk_hex();
    let tx = NativeTransfer::try_from(
        &owner
            .sign_for_network(
                &json!({
                    "schema": "boole.transfer.v1", "from": owner_pk, "to": recipient,
                    "amount": NATIVE_COIN_UNIT.to_string(), "fee": FEE.to_string(),
                    "nonce": "0", "validBefore": (special_height + 100).to_string()
                }),
                Some(native_testnet().network_id()),
            )
            .unwrap(),
    )
    .unwrap();
    let id = tx.id();
    let signed_bytes = serde_json::to_vec(&tx).unwrap();
    let mut node = NativeNode::open(&scratch.path).unwrap();
    let manifest = fs::read(scratch.path.join("state.manifest.json")).unwrap();
    let mut timings = Timings::default();
    for height in 1..=special_height + 8 {
        let destination = if height == special_height {
            &owner_pk
        } else {
            &producer_pk
        };
        extend(&mut node, &producer, destination, &mut timings);
        assert!(started.elapsed() < Duration::from_secs(1_800));
        if height.is_multiple_of(512) || height == 9_999 || height == 10_000 {
            eprintln!(
                "halving-progress {}",
                json!({
                    "height": height, "elapsedMs": started.elapsed().as_millis(),
                    "issuedAtoms": node.chain().ledger().issued().to_string(),
                    "lastRewardAtoms": reward(height).to_string(),
                    "head": node.chain().head_hash().to_hex(), "resources": node.resource_usage().unwrap()
                })
            );
        }
    }
    check_state(
        &node,
        &producer_pk,
        &owner_pk,
        &recipient,
        Some(special_height),
        false,
    );
    reject_unchanged(&mut node, &scratch.path, &tx);
    extend(&mut node, &producer, &producer_pk, &mut timings);
    assert_eq!(node.chain().ledger().spendable_balance(&owner_pk), 0);
    assert!(node.submit_transfer(tx.clone()).unwrap());
    assert_eq!(node.pending_view().unwrap().next_nonce(&owner_pk), 1);
    assert_eq!(
        node.pending_view().unwrap().available_balance(&owner_pk),
        reward(special_height) - NATIVE_COIN_UNIT - FEE
    );
    extend(&mut node, &producer, &producer_pk, &mut timings);
    assert_eq!(node.confirmed_height(&id), Some(special_height + 10));
    extend(&mut node, &producer, &producer_pk, &mut timings);
    check_state(
        &node,
        &producer_pk,
        &owner_pk,
        &recipient,
        Some(special_height),
        true,
    );
    assert!(node.pending().is_empty());
    let old_head = node.chain().head_hash();

    let common = special_height - 2;
    let mut candidate = node.chain().fork_at(common).unwrap();
    while candidate.ledger().height() < special_height + 13 {
        let height = candidate.ledger().height() + 1;
        let begin = Instant::now();
        let template = candidate
            .template(&producer_pk, &producer_pk, height * 60_000, &[])
            .unwrap();
        phase(begin, &mut timings.template, "candidate template");
        let block = authorize(template, &producer);
        let begin = Instant::now();
        candidate.append(block).unwrap();
        phase(begin, &mut timings.candidate, "candidate append");
    }
    let fork_head = candidate.head_hash();
    let begin = Instant::now();
    assert!(node
        .adopt_recent_suffix(common, &candidate.blocks()[common as usize..])
        .unwrap());
    let adoption = begin.elapsed();
    assert!(
        adoption < Duration::from_secs(10),
        "recent fork: {adoption:?}"
    );
    drop(candidate);
    assert_ne!(old_head, fork_head);
    assert_eq!(node.chain().head_hash(), fork_head);
    assert_eq!(node.confirmed_height(&id), None);
    assert!(!node.is_pending(&id));
    check_state(&node, &producer_pk, &owner_pk, &recipient, None, false);
    reject_unchanged(&mut node, &scratch.path, &tx);
    let fork_files = files(&scratch.path);
    drop(node);
    let (mut node, fork_reopen_ms) = reopen(&scratch.path);
    assert_eq!(node.chain().head_hash(), fork_head);
    assert_eq!(files(&scratch.path), fork_files);
    check_state(&node, &producer_pk, &owner_pk, &recipient, None, false);
    assert_eq!(node.confirmed_height(&id), None);
    assert!(node.pending().is_empty());

    extend(&mut node, &producer, &owner_pk, &mut timings);
    while node.chain().ledger().height() < special_height + 22 {
        extend(&mut node, &producer, &producer_pk, &mut timings);
    }
    check_state(
        &node,
        &producer_pk,
        &owner_pk,
        &recipient,
        Some(special_height + 14),
        false,
    );
    reject_unchanged(&mut node, &scratch.path, &tx);
    extend(&mut node, &producer, &producer_pk, &mut timings);
    assert_eq!(node.chain().ledger().spendable_balance(&owner_pk), 0);
    assert!(node.submit_transfer(tx.clone()).unwrap());
    assert_eq!(node.pending().len(), 1);
    assert_eq!(node.pending()[0].id(), id);
    extend(&mut node, &producer, &producer_pk, &mut timings);
    assert_eq!(node.confirmed_height(&id), Some(special_height + 24));
    assert!(node.pending().is_empty());
    let before_repeat = files(&scratch.path);
    assert!(!node.submit_transfer(tx.clone()).unwrap());
    assert_eq!(files(&scratch.path), before_repeat);
    assert_eq!(serde_json::to_vec(&tx).unwrap(), signed_bytes);
    check_state(
        &node,
        &producer_pk,
        &owner_pk,
        &recipient,
        Some(special_height + 14),
        true,
    );
    let final_head = node.chain().head_hash();
    let final_ledger = node.chain().ledger().clone();
    drop(node);
    let (mut node, final_reopen_ms) = reopen(&scratch.path);
    assert_eq!(node.chain().head_hash(), final_head);
    assert_eq!(node.chain().ledger(), &final_ledger);
    assert_eq!(node.confirmed_height(&id), Some(special_height + 24));
    assert!(node.pending().is_empty());
    assert!(!node.submit_transfer(tx).unwrap());
    assert_eq!(files(&scratch.path), before_repeat);
    check_state(
        &node,
        &producer_pk,
        &owner_pk,
        &recipient,
        Some(special_height + 14),
        true,
    );
    drop(node);
    let begin = Instant::now();
    let audit = audit_native_state(&scratch.path, Some(&final_head.to_hex())).unwrap();
    let audit_elapsed = begin.elapsed();
    assert!(
        audit_elapsed < Duration::from_secs(120),
        "audit: {audit_elapsed:?}"
    );
    let expected_issued = issued(special_height + 24);
    let expected_locked: u128 = ((special_height + 15)..=special_height + 24)
        .map(reward)
        .sum();
    assert_eq!(audit.accounting.issued_atoms, expected_issued.to_string());
    assert_eq!(audit.accounting.balance_atoms, expected_issued.to_string());
    assert_eq!(audit.accounting.locked_atoms, expected_locked.to_string());
    assert_eq!(
        audit.accounting.spendable_atoms,
        (expected_issued - expected_locked).to_string()
    );
    assert_eq!(audit.accounting.pending_reward_entries, 10);
    assert_eq!(audit.confirmed_transfers.count, 1);
    assert_eq!(
        audit.confirmed_transfers.amount_atoms,
        NATIVE_COIN_UNIT.to_string()
    );
    assert_eq!(audit.confirmed_transfers.fee_atoms, FEE.to_string());
    assert_eq!(audit.height, (special_height + 24).to_string());
    assert_eq!(files(&scratch.path), before_repeat);
    assert_eq!(
        fs::read(scratch.path.join("state.manifest.json")).unwrap(),
        manifest
    );
    let report = json!({
        "specialRewardHeight": special_height, "specialRewardAtoms": reward(special_height).to_string(),
        "oldHead": old_head.to_hex(), "forkHead": fork_head.to_hex(),
        "transferId": id.to_hex(), "reconfirmedHeight": special_height + 24,
        "maxTemplateMs": timings.template.as_millis(), "maxAppendMs": timings.append.as_millis(),
        "maxCandidateAppendMs": timings.candidate.as_millis(), "forkAdoptionMs": adoption.as_millis(),
        "forkReopenMs": fork_reopen_ms, "finalReopenMs": final_reopen_ms,
        "auditMs": audit_elapsed.as_millis(), "elapsedMs": started.elapsed().as_millis(),
        "finalFilesBlake3": before_repeat, "audit": audit,
    });
    eprintln!("halving-result {report}");
    assert!(started.elapsed() < Duration::from_secs(1_800));
    report
}

#[test]
fn small_reward_maturity_fork_recovery_preserves_original_transfer() {
    let result = run(16);
    assert_eq!(result["reconfirmedHeight"], 40);
}

#[test]
#[ignore = "preregistered developer-Mac actual first-halving qualification"]
fn actual_first_halving_fork_recovery_10024_blocks() {
    let result = run(10_000);
    assert_eq!(
        result["audit"]["accounting"]["issuedAtoms"],
        "50057500000000000"
    );
}
