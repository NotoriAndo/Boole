//! Eight distinct, valid forks with preregistered resource/recovery bounds.
use super::*;
use std::collections::BTreeSet;
use std::sync::atomic::AtomicUsize;

struct Candidate {
    head: boole_core::Hex32,
    amount: u64,
    suffix: Arc<Vec<NativeBlock>>,
    hashes: Arc<Vec<boole_core::Hex32>>,
}

struct CandidateOutcome {
    head: boole_core::Hex32,
    confirmed: Vec<(boole_core::Hex32, u64)>,
}

fn candidate_transfer(owner: &SigningKeyV2, nonce: u64, amount: u64) -> NativeTransfer {
    NativeTransfer::try_from(
        &owner
            .sign_for_network(
                &json!({
                    "schema": "boole.transfer.v1", "from": owner.pk_hex(), "to": owner.pk_hex(),
                    "amount": amount.to_string(), "fee": native_testnet().minimum_fee().to_string(),
                    "nonce": nonce.to_string(), "validBefore": "1000"
                }),
                Some(native_testnet().network_id()),
            )
            .unwrap(),
    )
    .unwrap()
}

fn check_winner(
    node: &NativeNode,
    owner: &str,
    recipients: u64,
    per_block: u64,
    common_height: u64,
    candidates: &[CandidateOutcome],
    winner: usize,
) {
    check_canonical(node, owner, recipients, 16 * per_block);
    assert_eq!(node.chain().head_hash(), candidates[winner].head);
    assert_eq!(node.chain().ledger().height(), common_height + 16);
    assert!(node.pending().is_empty());
    for (index, candidate) in candidates.iter().enumerate() {
        for (id, height) in &candidate.confirmed {
            let expected = (index == winner).then_some(*height);
            assert_eq!(node.confirmed_height(id), expected);
            assert!(!node.is_pending(id));
        }
    }
}

fn run_competing_scenario(funded_blocks: u64, per_block: u64) {
    let started = Instant::now();
    let dir = TestDir::new();
    let owner = SigningKeyV2::from_dev_id("native-capacity-producer");
    let mut node = NativeNode::open(&dir.path).unwrap();
    let mut max_template = Duration::ZERO;
    let mut max_append = Duration::ZERO;
    let mut max_candidate_append = Duration::ZERO;
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
                "competing-progress fundedBlocks={} elapsedMs={}",
                index + 1,
                started.elapsed().as_millis()
            );
        }
    }
    let recipients = funded_blocks * PER_BLOCK;
    check_canonical(&node, &owner.pk_hex(), recipients, 0);
    let common = node.chain().clone();
    let common_height = common.ledger().height();
    let common_hash = common.head_hash();
    let local = mine(&node, &owner, Some(&[]), &mut max_template);
    append(&mut node, local, &mut max_append);
    let initial_head = node.chain().head_hash();
    let mut candidates = Vec::new();
    let mut candidate_work = None;
    for amount in 1..=8 {
        let mut branch = common.clone();
        for index in 0..16 {
            let transfers: Vec<_> = (recipients + index * per_block
                ..recipients + (index + 1) * per_block)
                .map(|nonce| candidate_transfer(&owner, nonce, amount))
                .collect();
            let begin = Instant::now();
            let template = branch
                .template(
                    &owner.pk_hex(),
                    &owner.pk_hex(),
                    (branch.ledger().height() + 1) * 60_000,
                    &transfers,
                )
                .unwrap();
            record_phase(begin, &mut max_template, "candidate template");
            let mined = template
                .mine(0, 2_000_000)
                .unwrap()
                .expect("bounded candidate PoW");
            let signature = owner
                .sign_for_network(
                    &mined.authorization_payload().unwrap(),
                    Some(native_testnet().network_id()),
                )
                .unwrap();
            let block = mined.authorize(&signature).unwrap();
            let begin = Instant::now();
            branch.append(block).unwrap();
            record_phase(begin, &mut max_candidate_append, "candidate validation");
            assert!(started.elapsed() < Duration::from_secs(900));
        }
        assert!(branch.outranks(node.chain()));
        if let Some(expected) = &candidate_work {
            assert_eq!(branch.cumulative_work(), expected);
        } else {
            candidate_work = Some(branch.cumulative_work().clone());
        }
        candidates.push(Candidate {
            head: branch.head_hash(),
            amount,
            suffix: Arc::new(branch.blocks()[common_height as usize..].to_vec()),
            hashes: Arc::new(
                std::iter::once(native_testnet().genesis_hash())
                    .chain(branch.blocks().iter().map(|block| block.hash().unwrap()))
                    .collect(),
            ),
        });
        eprintln!(
            "competing-candidate amount={amount} head={} elapsedMs={}",
            branch.head_hash().to_hex(),
            started.elapsed().as_millis()
        );
    }
    drop(common);
    let heads: BTreeSet<_> = candidates.iter().map(|candidate| candidate.head).collect();
    assert_eq!(heads.len(), 8);
    // All candidates have identical work; the existing rule prefers lower hash.
    let winner = candidates
        .iter()
        .enumerate()
        .min_by_key(|(_, candidate)| candidate.head)
        .unwrap()
        .0;
    let winning_head = candidates[winner].head;
    // Keep only public expected IDs/heights for independent replay. Candidate
    // bodies and all network fixtures are released before the node is reopened.
    let outcomes: Vec<_> = candidates
        .iter()
        .map(|candidate| CandidateOutcome {
            head: candidate.head,
            confirmed: candidate
                .suffix
                .iter()
                .enumerate()
                .flat_map(|(offset, block)| {
                    block
                        .transfers
                        .iter()
                        .map(move |tx| (tx.id(), common_height + offset as u64 + 1))
                })
                .collect(),
        })
        .collect();
    let before_digests = journal_digests(&dir);
    let node = Arc::new(Mutex::new(node));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let local_address = listener.local_addr().unwrap();
    let local_key = peer_identity();
    let stop_fixture = Arc::new(AtomicBool::new(false));
    let stable = Arc::new((0..8).map(|_| AtomicUsize::new(0)).collect::<Vec<_>>());
    let (paused_tx, paused_rx) = mpsc::channel();
    let mut peers = Vec::new();
    let mut remotes = Vec::new();
    let mut releases = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let remote_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        remote_listener.set_nonblocking(true).unwrap();
        let identity = peer_identity();
        peers.push((remote_listener.local_addr().unwrap(), identity.peer_id()));
        let transport =
            TlsTransport::new(identity, vec![(local_address, local_key.peer_id())]).unwrap();
        let suffix = candidate.suffix.clone();
        let hashes = candidate.hashes.clone();
        let advertised = json!({"height": common_height + 16, "hash": candidate.head.to_hex()});
        let stopped = stop_fixture.clone();
        let stable = stable.clone();
        let paused = paused_tx.clone();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        releases.push(release_tx);
        remotes.push(std::thread::spawn(move || -> anyhow::Result<Vec<Value>> {
            let mut reports = Vec::new();
            for round in 0..256 {
                let round_started = Instant::now();
                let accept_deadline = round_started + Duration::from_secs(120);
                let socket = loop {
                    if stopped.load(Ordering::Acquire) { return Ok(reports); }
                    match remote_listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < accept_deadline => {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => return Err(error.into()),
                    }
                };
                let deadline = Instant::now() + Duration::from_secs(12);
                let mut connection = match transport.accept_stream_until(socket, deadline) {
                    Ok(connection) => connection,
                    Err(_) if stopped.load(Ordering::Acquire) => return Ok(reports),
                    Err(_) => { reports.push(json!({"round": round+1, "completed": false, "phase": "tls"})); continue; }
                };
                let (hello, mut received_bytes): (Value, _) = match transport.recv_json_counted_until(&mut connection, 4096, deadline) {
                    Ok(hello) => hello,
                    Err(_) if stopped.load(Ordering::Acquire) => return Ok(reports),
                    Err(_) => { reports.push(json!({"round": round+1, "completed": false, "phase": "hello"})); continue; }
                };
                anyhow::ensure!(hello["type"] == "hello", "hello required");
                if round == 0 { anyhow::ensure!(hello["head"]["hash"] == initial_head.to_hex(), "initial head mismatch"); }
                let final_head_round = hello["head"]["hash"] == winning_head.to_hex();
                let greeting = json!({"type": "hello", "protocolVersion": 1,
                    "networkId": native_testnet().network_id(), "genesisHash": native_testnet().genesis_hash().to_hex(),
                    "head": advertised});
                let mut sent_bytes = transport.send_json_counted_until(&mut connection, &greeting, 4096, deadline)?;
                let mut sent_blocks = 0usize;
                let mut block_requests = 0;
                let mut hash_requests = 0;
                let mut pending_requests = 0;
                let completed = loop {
                    let (request, bytes): (Value, _) = match transport.recv_json_counted_until(&mut connection, 4096, deadline) {
                        Ok(request) => request,
                        Err(_) if stopped.load(Ordering::Acquire) => return Ok(reports),
                        // A concurrent winner can invalidate any current local
                        // snapshot. Completion is still required at the best head.
                        Err(_) => break false,
                    };
                    received_bytes += bytes;
                    if request["type"] == "done" { break true; }
                    anyhow::ensure!(request["snapshot"] == advertised, "wrong candidate snapshot");
                    let response = match request["type"].as_str() {
                        Some("getHash") => {
                            hash_requests += 1;
                            let height = request["height"].as_u64().unwrap();
                            json!({"type": "hash", "snapshot": advertised, "height": height, "hash": hashes[height as usize].to_hex()})
                        }
                        Some("getBlocks") => {
                            block_requests += 1;
                            let from = request["from"].as_u64().unwrap();
                            anyhow::ensure!(from == common_height + 1 + sent_blocks as u64, "unexpected competing block range");
                            let count = 3usize.min(suffix.len() - sent_blocks);
                            anyhow::ensure!(count > 0 && request["limit"].as_u64().unwrap() >= count as u64, "invalid page size");
                            if round == 0 && sent_blocks == 15 {
                                paused.send((index, sent_bytes))?;
                                release_rx.recv_timeout(Duration::from_secs(10))?;
                            }
                            let blocks = &suffix[sent_blocks..sent_blocks + count];
                            sent_blocks += count;
                            json!({"type": "blocks", "snapshot": advertised, "from": from, "blocks": blocks})
                        }
                        Some("getPending") => {
                            pending_requests += 1;
                            anyhow::ensure!(request["offset"] == 0, "unexpected pending offset");
                            json!({"type": "pending", "snapshot": advertised, "offset": 0, "total": 0, "transfers": []})
                        }
                        _ => anyhow::bail!("unexpected competing request"),
                    };
                    sent_bytes += transport.send_json_counted_until(&mut connection, &response, 1024 * 1024, deadline)?;
                };
                anyhow::ensure!(sent_bytes + received_bytes < 8 * 1024 * 1024, "competing round byte cap");
                anyhow::ensure!(hash_requests + block_requests + pending_requests < 64, "competing round request cap including hello");
                anyhow::ensure!(sent_blocks <= 16, "competing round block cap");
                if round == 0 { anyhow::ensure!(sent_blocks == 16, "initial candidate incomplete"); }
                if completed && final_head_round {
                    if stable[index].load(Ordering::Acquire) > 0 {
                        anyhow::ensure!(hash_requests == 0 && block_requests == 0, "stable final-head poll redownloaded candidate");
                    }
                    stable[index].fetch_add(1, Ordering::Release);
                }
                reports.push(json!({"peer": index, "round": round+1, "clientHead": hello["head"],
                    "completed": completed, "finalHeadRound": final_head_round, "elapsedMicros": round_started.elapsed().as_micros(),
                    "sentBlocks": sent_blocks, "sentBytes": sent_bytes, "receivedBytes": received_bytes,
                    "hashRequests": hash_requests, "blockRequests": block_requests, "pendingRequests": pending_requests}));
            }
            anyhow::bail!("competing fixture round limit")
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
    let mut held_bytes = [0; 8];
    for _ in 0..8 {
        let (index, bytes) = paused_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("all eight distinct candidate buffers must pause");
        assert_eq!(held_bytes[index], 0);
        held_bytes[index] = bytes;
    }
    assert_eq!(service.monitor().snapshot().active_outbound_rounds, 8);
    assert_eq!(journal_digests(&dir), before_digests);
    {
        let node = node.lock().unwrap();
        assert_eq!(node.chain().head_hash(), initial_head);
        check_canonical(&node, &owner.pk_hex(), recipients, 0);
    }
    for release in releases {
        release.send(()).unwrap();
    }
    let mut observed_heads = BTreeSet::new();
    let mut observed_samples = 0usize;
    let mut busy_samples = 0usize;
    let completed = loop {
        if let Ok(node) = node.try_lock() {
            node.ensure_ready().unwrap();
            let head = node.chain().head_hash();
            assert!(head == initial_head || heads.contains(&head));
            let extra = if head == initial_head {
                0
            } else {
                16 * per_block
            };
            let height = common_height + if head == initial_head { 1 } else { 16 };
            assert_eq!(node.chain().ledger().height(), height);
            assert_eq!(
                node.chain().ledger().next_nonce(&owner.pk_hex()),
                recipients + extra
            );
            assert_eq!(
                node.resource_usage().unwrap().confirmed_transfers as u64,
                recipients + extra
            );
            assert_eq!(
                node.chain().ledger().balance(&owner.pk_hex()),
                u128::from(height) * REWARD - u128::from(recipients)
            );
            assert!(node.pending().is_empty());
            observed_heads.insert(head.to_hex());
            observed_samples += 1;
        } else {
            busy_samples += 1;
        }
        let snapshot = service.monitor().snapshot();
        if stable
            .iter()
            .all(|count| count.load(Ordering::Acquire) >= 2)
            && snapshot.peers.iter().enumerate().all(|(index, status)| {
                status.consecutive_failures == 0
                    && status.state
                        == if index == winner {
                            "snapshot_match"
                        } else {
                            "local_chain_preferred"
                        }
            })
        {
            break snapshot;
        }
        assert!(
            network_started.elapsed() < Duration::from_secs(120),
            "competing forks did not stabilize: {snapshot:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    let network_elapsed = network_started.elapsed();
    assert!(network_elapsed < Duration::from_secs(120));
    assert_eq!(completed.peak_outbound_rounds, 8);
    stop_fixture.store(true, Ordering::Release);
    let stopping_at = Instant::now();
    service.stop();
    let stopping = stopping_at.elapsed();
    assert!(stopping < Duration::from_secs(1));
    assert_eq!(service.monitor().snapshot().active_outbound_rounds, 0);
    let rounds: Vec<_> = remotes
        .into_iter()
        .map(|remote| remote.join().unwrap().unwrap())
        .collect();
    check_winner(
        &node.lock().unwrap(),
        &owner.pk_hex(),
        recipients,
        per_block,
        common_height,
        &outcomes,
        winner,
    );
    let after_digests = journal_digests(&dir);
    assert_ne!(after_digests[0], before_digests[0]);
    eprintln!(
        "competing-network {}",
        json!({
            "fundedBlocks": funded_blocks, "perCandidateBlock": per_block, "commonHeight": common_height,
            "commonHash": common_hash.to_hex(), "initialHead": initial_head.to_hex(), "winningPeer": winner,
            "winningHead": winning_head.to_hex(), "winningAmount": candidates[winner].amount,
            "candidateHeads": candidates.iter().map(|candidate| candidate.head.to_hex()).collect::<Vec<_>>(),
            "heldBytesPerPeer": held_bytes, "networkMs": network_elapsed.as_millis(), "stopMicros": stopping.as_micros(),
            "observedHeadsLowerBound": observed_heads, "observedSamples": observed_samples, "busySamples": busy_samples,
            "rounds": rounds, "peersBeforeStop": completed, "elapsedMs": started.elapsed().as_millis()
        })
    );
    drop(service);
    drop(node);
    drop(candidates);
    let reopened_at = Instant::now();
    let node = NativeNode::open(&dir.path).unwrap();
    let restart = reopened_at.elapsed();
    assert!(restart < Duration::from_secs(120));
    check_winner(
        &node,
        &owner.pk_hex(),
        recipients,
        per_block,
        common_height,
        &outcomes,
        winner,
    );
    assert_eq!(journal_digests(&dir), after_digests);
    eprintln!(
        "competing-result {}",
        json!({
            "fundedBlocks": funded_blocks, "perCandidateBlock": per_block, "networkId": native_testnet().network_id(),
            "genesisHash": native_testnet().genesis_hash().to_hex(), "head": winning_head.to_hex(),
            "resources": node.resource_usage().unwrap(), "issued": node.chain().ledger().issued().to_string(),
            "restartMs": restart.as_millis(), "elapsedMs": started.elapsed().as_millis(),
            "maxTemplateMs": max_template.as_millis(), "maxAppendMs": max_append.as_millis(),
            "maxCandidateAppendMs": max_candidate_append.as_millis()
        })
    );
    assert!(started.elapsed() < Duration::from_secs(900));
}

#[test]
fn small_distinct_competing_candidates_choose_one_ledger_and_recover() {
    run_competing_scenario(2, 16);
}

#[test]
#[ignore = "explicit preregistered distinct competing-fork qualification; not routine CI"]
fn native_eight_distinct_forks_131072_accounts() {
    run_competing_scenario(256, 512);
}
