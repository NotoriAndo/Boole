//! The ignored scenario has preregistered bounds in
//! docs/native-capacity-qualification-2026-09.md. The small case runs in CI.
use std::fs;
use std::io::Read;
use std::net::TcpListener;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use boole_core::native_chain::{NativeBlock, NativeTransfer};
use boole_core::native_network::{native_testnet, NATIVE_COIN_UNIT};
use boole_core::SigningKeyV2;
use boole_node::{NativeNode, NativePeerConfig, NativePeerService};
use boole_p2p::{TlsIdentity, TlsTransport};
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

fn run_recent_fork_scenario(funded_blocks: u64) {
    let started = Instant::now();
    let dir = TestDir::new();
    let owner = SigningKeyV2::from_dev_id("native-capacity-producer");
    let replacement = SigningKeyV2::from_dev_id("native-capacity-replacement");
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
        assert!(started.elapsed() < Duration::from_secs(900));
        if (index + 1).is_multiple_of(32) {
            eprintln!(
                "fork-progress fundedBlocks={} elapsedMs={}",
                index + 1,
                started.elapsed().as_millis()
            );
        }
    }
    let common_height = node.chain().ledger().height();
    let common_hash = node.chain().head_hash();
    let recipients = funded_blocks * PER_BLOCK;
    check_canonical(&node, &owner.pk_hex(), recipients, 0);
    let mut candidate = node.chain().clone();
    let orphan = transfer(&owner, &owner.pk_hex(), recipients);
    assert!(node.submit_transfer(orphan.clone()).unwrap());
    let block = mine(&node, &owner, None, &mut max_template);
    append(&mut node, block, &mut max_append);
    assert_eq!(node.confirmed_height(&orphan.id()), Some(common_height + 1));
    for height in common_height + 1..=common_height + 2 {
        let block = candidate
            .template(
                &replacement.pk_hex(),
                &replacement.pk_hex(),
                height * 60_000,
                &[],
            )
            .unwrap()
            .mine(0, 2_000_000)
            .unwrap()
            .unwrap();
        let auth = replacement
            .sign_for_network(
                &block.authorization_payload().unwrap(),
                Some(native_testnet().network_id()),
            )
            .unwrap();
        candidate.append(block.authorize(&auth).unwrap()).unwrap();
    }
    let begin = Instant::now();
    assert!(node.adopt_chain(candidate.blocks()).unwrap());
    let adoption = begin.elapsed();
    assert_eq!(node.chain(), &candidate);
    assert_eq!(node.pending(), std::slice::from_ref(&orphan));
    assert_eq!(node.confirmed_height(&orphan.id()), None);
    assert_eq!(
        node.chain().ledger().next_nonce(&owner.pk_hex()),
        recipients
    );
    assert_eq!(
        node.pending_view().unwrap().next_nonce(&owner.pk_hex()),
        recipients + 1
    );
    assert_eq!(
        node.resource_usage().unwrap().confirmed_transfers as u64,
        recipients
    );
    for index in 0..recipients {
        assert_eq!(node.chain().ledger().balance(&format!("{index:064x}")), 1);
    }
    eprintln!(
        "fork-adoption {}",
        json!({
            "fundedBlocks": funded_blocks, "commonHeight": common_height, "commonHash": common_hash.to_hex(),
            "head": node.chain().head_hash().to_hex(), "adoptionMs": adoption.as_millis(),
            "elapsedMs": started.elapsed().as_millis(), "resources": node.resource_usage().unwrap(),
        })
    );
    assert!(
        adoption < Duration::from_secs(10),
        "recent fork adoption exceeded 10s: {adoption:?}"
    );
    drop(candidate);
    let expected_head = node.chain().head_hash();
    drop(node);
    let begin = Instant::now();
    let node = NativeNode::open(&dir.path).unwrap();
    let restart = begin.elapsed();
    assert_eq!(node.chain().head_hash(), expected_head);
    assert_eq!(node.pending(), std::slice::from_ref(&orphan));
    assert_eq!(
        node.chain().ledger().next_nonce(&owner.pk_hex()),
        recipients
    );
    assert_eq!(
        node.resource_usage().unwrap().confirmed_transfers as u64,
        recipients
    );
    assert!(restart < Duration::from_secs(120));
    eprintln!(
        "fork-result {}",
        json!({
            "adoptionMs": adoption.as_millis(), "restartMs": restart.as_millis(),
            "elapsedMs": started.elapsed().as_millis(), "resources": node.resource_usage().unwrap(),
        })
    );
    assert!(started.elapsed() < Duration::from_secs(900));
}

#[test]
fn small_recent_fork_preserves_balances_and_recovers_orphans() {
    run_recent_fork_scenario(2);
}

#[test]
#[ignore = "explicit closed-local recent-fork qualification; not routine CI"]
fn native_recent_fork_131072_accounts() {
    run_recent_fork_scenario(256);
}

fn journal_digests(dir: &TestDir) -> Vec<Option<blake3::Hash>> {
    [
        boole_node::NATIVE_BLOCKS_FILE,
        boole_node::NATIVE_MEMPOOL_FILE,
    ]
    .iter()
    .map(|name| {
        let mut file = match fs::File::open(dir.path.join(name)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => panic!("cannot read journal: {error}"),
        };
        let mut digest = blake3::Hasher::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            let count = file.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        Some(digest.finalize())
    })
    .collect()
}

fn peer_identity() -> TlsIdentity {
    TlsIdentity::from_pkcs8(&TlsIdentity::generate_pkcs8().unwrap()).unwrap()
}

fn run_concurrent_peer_scenario(funded_blocks: u64) {
    let started = Instant::now();
    let dir = TestDir::new();
    let owner = SigningKeyV2::from_dev_id("native-capacity-producer");
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
        assert!(started.elapsed() < Duration::from_secs(900));
        if (index + 1).is_multiple_of(32) {
            eprintln!(
                "peer-capacity-progress fundedBlocks={} elapsedMs={}",
                index + 1,
                started.elapsed().as_millis()
            );
        }
    }
    let recipients = funded_blocks * PER_BLOCK;
    check_canonical(&node, &owner.pk_hex(), recipients, 0);
    let common_height = node.chain().ledger().height();
    let common_hash = node.chain().head_hash();
    let mut alternative = node.chain().clone();
    for index in 0..33 {
        let transfers: Vec<_> = (recipients + index * PER_BLOCK
            ..recipients + (index + 1) * PER_BLOCK)
            .map(|nonce| transfer(&owner, &owner.pk_hex(), nonce))
            .collect();
        let height = alternative.ledger().height() + 1;
        let block = alternative
            .template(
                &owner.pk_hex(),
                &owner.pk_hex(),
                height * 60_000,
                &transfers,
            )
            .unwrap()
            .mine(0, 2_000_000)
            .unwrap()
            .unwrap();
        let auth = owner
            .sign_for_network(
                &block.authorization_payload().unwrap(),
                Some(native_testnet().network_id()),
            )
            .unwrap();
        alternative.append(block.authorize(&auth).unwrap()).unwrap();
        assert!(started.elapsed() < Duration::from_secs(900));
    }
    let advertised =
        json!({"height": alternative.ledger().height(), "hash": alternative.head_hash().to_hex()});
    let hashes = Arc::new(
        std::iter::once(native_testnet().genesis_hash())
            .chain(
                alternative
                    .blocks()
                    .iter()
                    .map(|block| block.hash().unwrap()),
            )
            .collect::<Vec<_>>(),
    );
    let suffix = Arc::new(alternative.blocks()[common_height as usize..].to_vec());
    drop(alternative);
    let block = mine(&node, &owner, Some(&[]), &mut max_template);
    append(&mut node, block, &mut max_append);
    let local_head = node.chain().head_hash();
    check_canonical(&node, &owner.pk_hex(), recipients, 0);
    let before_digests = journal_digests(&dir);
    let node = Arc::new(Mutex::new(node));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let local_address = listener.local_addr().unwrap();
    let local_key = peer_identity();
    let (paused_tx, paused_rx) = mpsc::channel();
    let mut remotes = Vec::new();
    let mut releases = Vec::new();
    let mut peers = Vec::new();
    for index in 0..8 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let identity = peer_identity();
        peers.push((listener.local_addr().unwrap(), identity.peer_id()));
        let transport =
            TlsTransport::new(identity, vec![(local_address, local_key.peer_id())]).unwrap();
        let suffix = suffix.clone();
        let hashes = hashes.clone();
        let advertised = advertised.clone();
        let paused = paused_tx.clone();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        releases.push(release_tx);
        remotes.push(std::thread::spawn(move || -> anyhow::Result<usize> {
            let accepted_by = Instant::now() + Duration::from_secs(10);
            let socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < accepted_by => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => return Err(error.into()),
                }
            };
            let deadline = Instant::now() + Duration::from_secs(12);
            let mut connection = transport.accept_stream_until(socket, deadline)?;
            let (hello, _): (Value, _) = transport.recv_json_counted_until(&mut connection, 4096, deadline)?;
            anyhow::ensure!(hello["type"] == "hello", "hello required");
            let greeting = json!({"type": "hello", "protocolVersion": 1,
                "networkId": native_testnet().network_id(), "genesisHash": native_testnet().genesis_hash().to_hex(),
                "head": advertised});
            let mut sent_bytes = transport.send_json_counted_until(&mut connection, &greeting, 4096, deadline)?;
            let mut sent_blocks = 0;
            loop {
                let request: Value = match transport.recv_json_counted_until(&mut connection, 4096, deadline) {
                    Ok((request, _)) => request,
                    Err(_) if sent_blocks >= 30 => return Ok(sent_bytes),
                    Err(error) => return Err(error.into()),
                };
                anyhow::ensure!(request["snapshot"] == advertised, "wrong snapshot request");
                let response = match request["type"].as_str() {
                    Some("getHash") => {
                        let height = request["height"].as_u64().unwrap();
                        json!({"type": "hash", "snapshot": advertised, "height": height,
                            "hash": hashes[height as usize].to_hex()})
                    }
                    Some("getBlocks") => {
                        let from = request["from"].as_u64().unwrap();
                        anyhow::ensure!(from == common_height + 1 + sent_blocks, "unexpected range");
                        anyhow::ensure!(request["limit"].as_u64().unwrap() >= 3, "short page request");
                        if sent_blocks == 30 {
                            // The next request proves all thirty prior blocks
                            // were decoded and retained by the actual client.
                            anyhow::ensure!(sent_bytes < 8 * 1024 * 1024, "partial fork already over budget");
                            paused.send((index, sent_bytes))?;
                            release_rx.recv_timeout(Duration::from_secs(10))?;
                        }
                        let offset = sent_blocks as usize;
                        sent_blocks += 3;
                        json!({"type": "blocks", "snapshot": advertised, "from": from,
                            "blocks": &suffix[offset..offset + 3]})
                    }
                    _ => anyhow::bail!("unexpected request before budget rejection"),
                };
                match transport.send_json_counted_until(&mut connection, &response, 1024 * 1024, deadline) {
                    Ok(bytes) => sent_bytes += bytes,
                    Err(_) if sent_blocks > 30 => return Ok(sent_bytes),
                    Err(error) => return Err(error.into()),
                }
            }
        }));
    }
    let network_started = Instant::now();
    let mut service = NativePeerService::start(
        listener,
        node.clone(),
        NativePeerConfig {
            identity: local_key,
            peers,
        },
    )
    .unwrap();
    let mut partial_bytes = [0; 8];
    for _ in 0..8 {
        let (index, bytes) = paused_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("all eight peers must reach the simultaneous partial fork");
        assert_eq!(partial_bytes[index], 0);
        partial_bytes[index] = bytes;
    }
    let paused = service.monitor().snapshot();
    assert_eq!(paused.active_outbound_rounds, 8);
    assert_eq!(paused.peak_outbound_rounds, 8);
    let query_started = Instant::now();
    for _ in 0..100 {
        let node = node.lock().unwrap();
        node.ensure_ready().unwrap();
        assert_eq!(node.chain().head_hash(), local_head);
        assert_eq!(
            node.resource_usage().unwrap().confirmed_transfers as u64,
            recipients
        );
    }
    let lookups = query_started.elapsed();
    assert!(lookups < Duration::from_secs(5));
    for release in releases {
        release.send(()).unwrap();
    }
    while service
        .status()
        .iter()
        .any(|status| status.failed_rounds == 0)
    {
        assert!(
            network_started.elapsed() < Duration::from_secs(15),
            "all oversized fork rounds must fail"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let stopped_at = Instant::now();
    service.stop();
    let stopping = stopped_at.elapsed();
    assert!(stopping < Duration::from_secs(1));
    let peer_status = service.monitor().snapshot();
    assert_eq!(peer_status.active_outbound_rounds, 0);
    assert_eq!(peer_status.peers.len(), 8);
    for remote in remotes {
        remote.join().unwrap().unwrap();
    }
    let network_elapsed = network_started.elapsed();
    assert!(network_elapsed < Duration::from_secs(15));
    assert_eq!(journal_digests(&dir), before_digests);
    {
        let node = node.lock().unwrap();
        node.ensure_ready().unwrap();
        check_canonical(&node, &owner.pk_hex(), recipients, 0);
        assert_eq!(node.chain().head_hash(), local_head);
        assert!(node.pending().is_empty());
    }
    eprintln!(
        "peer-capacity-network {}",
        json!({
            "fundedBlocks": funded_blocks, "commonHeight": common_height, "commonHash": common_hash.to_hex(),
            "localHead": local_head.to_hex(), "advertised": advertised,
            "partialBytesPerPeer": partial_bytes, "networkMs": network_elapsed.as_millis(),
            "lookup100Micros": lookups.as_micros(), "stopMicros": stopping.as_micros(), "peers": peer_status,
            "elapsedMs": started.elapsed().as_millis(),
        })
    );
    drop(service);
    drop(node);
    drop(suffix);
    drop(hashes);
    let reopened_at = Instant::now();
    let node = NativeNode::open(&dir.path).unwrap();
    let restart = reopened_at.elapsed();
    assert!(restart < Duration::from_secs(120));
    assert_eq!(node.chain().head_hash(), local_head);
    check_canonical(&node, &owner.pk_hex(), recipients, 0);
    assert!(node.pending().is_empty());
    assert_eq!(journal_digests(&dir), before_digests);
    eprintln!(
        "peer-capacity-result {}",
        json!({
            "fundedBlocks": funded_blocks, "networkId": native_testnet().network_id(),
            "genesisHash": native_testnet().genesis_hash().to_hex(), "head": local_head.to_hex(),
            "resources": node.resource_usage().unwrap(), "issued": node.chain().ledger().issued().to_string(),
            "restartMs": restart.as_millis(), "elapsedMs": started.elapsed().as_millis(),
            "maxTemplateMs": max_template.as_millis(), "maxAppendMs": max_append.as_millis(),
        })
    );
    assert!(started.elapsed() < Duration::from_secs(900));
}

#[test]
fn small_concurrent_peer_forks_preserve_state_and_recovery() {
    run_concurrent_peer_scenario(2);
}

#[test]
#[ignore = "explicit preregistered eight-peer capacity qualification; not routine CI"]
fn native_eight_peer_forks_131072_accounts() {
    run_concurrent_peer_scenario(256);
}
