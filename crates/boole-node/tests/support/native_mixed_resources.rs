//! One preregistered combination of actual bounded HTTP and pinned-TLS inputs.
use super::*;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};

const BODY_BYTES: usize = 8 * 1024 * 1024;

fn socket(address: SocketAddr) -> TcpStream {
    let socket = TcpStream::connect_timeout(&address, Duration::from_secs(1)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    socket
}

fn headers(socket: &mut TcpStream) -> String {
    let mut output = Vec::new();
    while !output.ends_with(b"\r\n\r\n") {
        assert!(output.len() < 8192);
        let mut byte = [0];
        socket.read_exact(&mut byte).unwrap();
        output.push(byte[0]);
    }
    String::from_utf8(output).unwrap()
}

fn response(mut socket: TcpStream) -> (String, Value) {
    let header = headers(&mut socket);
    let mut body = String::new();
    socket.take(131_072).read_to_string(&mut body).unwrap();
    (header, serde_json::from_str(&body).unwrap())
}

fn get(address: SocketAddr, path: &str) -> Value {
    let mut socket = socket(address);
    write!(
        socket,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let (header, body) = response(socket);
    assert!(
        header.starts_with("HTTP/1.1 200 "),
        "{path}: {header} {body}"
    );
    body
}

fn diagnostics(address: SocketAddr) -> Value {
    let started = Instant::now();
    let body = get(address, "/native/diagnostics");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "diagnostic response deadline"
    );
    assert_eq!(body["authority"], "local_process_only");
    assert_eq!(body["ledgerReadiness"], "not_checked");
    assert!(body.get("headHash").is_none() && body.get("balance").is_none());
    assert_eq!(body["rpc"]["requestLimit"], 8);
    assert!(body["rpc"]["activeRequests"].as_u64().unwrap() <= 8);
    assert!(body["peers"]["activeInboundWorkers"].as_u64().unwrap() <= 4);
    assert!(body["peers"]["activeOutboundRounds"].as_u64().unwrap() <= 8);
    body
}

fn wait(label: &str, mut predicate: impl FnMut() -> bool) {
    let begin = Instant::now();
    while !predicate() {
        assert!(
            begin.elapsed() < Duration::from_secs(3),
            "mixed phase: {label}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn begin_upload(address: SocketAddr) -> TcpStream {
    let mut socket = socket(address);
    write!(socket, "POST /native/chain HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {BODY_BYTES}\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n").unwrap();
    socket
}

fn hello() -> Value {
    json!({"type":"hello", "protocolVersion":1, "networkId":native_testnet().network_id(),
        "genesisHash":native_testnet().genesis_hash().to_hex(),
        "head":{"height":0,"hash":native_testnet().genesis_hash().to_hex()}})
}

struct ForkGroup {
    peers: Vec<(SocketAddr, boole_p2p::PeerId)>,
    identities: Vec<TlsIdentity>,
    paused: mpsc::Receiver<(usize, usize)>,
    releases: Vec<mpsc::Sender<ForkRelease>>,
    workers: Vec<std::thread::JoinHandle<anyhow::Result<usize>>>,
}

enum ForkRelease {
    SendExcessPage,
    ObserveShutdown,
}

fn require_tls_closed(
    transport: &TlsTransport,
    connection: &mut boole_p2p::TlsConn,
) -> anyhow::Result<()> {
    match transport.recv_json_counted_until::<Value>(
        connection,
        4096,
        Instant::now() + Duration::from_secs(1),
    ) {
        Err(boole_p2p::FrameError::ConnectionClosed) => Ok(()),
        Err(boole_p2p::FrameError::Io(error))
            if matches!(
                error.kind(),
                std::io::ErrorKind::UnexpectedEof
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::BrokenPipe
            ) =>
        {
            Ok(())
        }
        other => anyhow::bail!("held TLS connection did not close: {other:?}"),
    }
}

fn require_http_closed(socket: &mut TcpStream) -> anyhow::Result<()> {
    socket.set_read_timeout(Some(Duration::from_secs(1)))?;
    let mut tail = Vec::new();
    match socket.take(4097).read_to_end(&mut tail) {
        Ok(_) if tail.len() <= 4096 => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
            ) =>
        {
            Ok(())
        }
        other => anyhow::bail!("held HTTP connection did not close: {other:?}"),
    }
}

impl ForkGroup {
    fn start(
        address: SocketAddr,
        key: boole_p2p::PeerId,
        common_height: u64,
        suffix: Arc<Vec<NativeBlock>>,
        hashes: Arc<Vec<boole_core::Hex32>>,
        advertised: Value,
    ) -> Self {
        let (paused_tx, paused) = mpsc::channel();
        let mut group = Self {
            peers: Vec::new(),
            identities: Vec::new(),
            paused,
            releases: Vec::new(),
            workers: Vec::new(),
        };
        for index in 0..8 {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let identity = peer_identity();
            group
                .peers
                .push((listener.local_addr().unwrap(), identity.peer_id()));
            group.identities.push(identity.clone());
            let transport = TlsTransport::new(identity, vec![(address, key)]).unwrap();
            let suffix = suffix.clone();
            let hashes = hashes.clone();
            let advertised = advertised.clone();
            let paused = paused_tx.clone();
            let (release_tx, release_rx) = mpsc::channel();
            group.releases.push(release_tx);
            group.workers.push(std::thread::spawn(move || -> anyhow::Result<usize> {
                let accept_by = Instant::now() + Duration::from_secs(10);
                let socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < accept_by => {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => return Err(error.into()),
                    }
                };
                let deadline = Instant::now() + Duration::from_secs(12);
                let mut connection = transport.accept_stream_until(socket, deadline)?;
                let (hello, _): (Value, _) = transport.recv_json_counted_until(&mut connection, 4096, deadline)?;
                anyhow::ensure!(hello["type"] == "hello", "hello required");
                let greeting = json!({"type":"hello", "protocolVersion":1,
                    "networkId":native_testnet().network_id(), "genesisHash":native_testnet().genesis_hash().to_hex(), "head":advertised});
                let mut sent = transport.send_json_counted_until(&mut connection, &greeting, 4096, deadline)?;
                let mut blocks = 0usize;
                loop {
                    let request: Value = match transport.recv_json_counted_until(&mut connection, 4096, deadline) {
                        Ok((request, _)) => request,
                        Err(_) if blocks >= 30 => return Ok(sent),
                        Err(error) => return Err(error.into()),
                    };
                    anyhow::ensure!(request["snapshot"] == advertised, "wrong snapshot");
                    let reply = match request["type"].as_str() {
                        Some("getHash") => {
                            let height = request["height"].as_u64().unwrap();
                            json!({"type":"hash", "snapshot":advertised, "height":height, "hash":hashes[height as usize].to_hex()})
                        }
                        Some("getBlocks") => {
                            let from = request["from"].as_u64().unwrap();
                            anyhow::ensure!(from == common_height + 1 + blocks as u64, "wrong range");
                            anyhow::ensure!(request["limit"].as_u64().unwrap() >= 3, "short page");
                            if blocks == 30 {
                                anyhow::ensure!(sent < 8 * 1024 * 1024, "partial fork already over budget");
                                paused.send((index, sent))?;
                                match release_rx.recv_timeout(Duration::from_secs(10))? {
                                    ForkRelease::SendExcessPage => {}
                                    ForkRelease::ObserveShutdown => {
                                        require_tls_closed(&transport, &mut connection)?;
                                        return Ok(sent);
                                    }
                                }
                            }
                            let first = blocks;
                            blocks += 3;
                            json!({"type":"blocks", "snapshot":advertised, "from":from, "blocks":&suffix[first..blocks]})
                        }
                        _ => anyhow::bail!("unexpected request before budget rejection"),
                    };
                    match transport.send_json_counted_until(&mut connection, &reply, 1024 * 1024, deadline) {
                        Ok(bytes) => sent += bytes,
                        Err(_) if blocks > 30 => return Ok(sent),
                        Err(error) => return Err(error.into()),
                    }
                }
            }));
        }
        group
    }
}

struct StopOnDrop(Arc<tokio::sync::Notify>);
impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

fn run_mixed_scenario(funded_blocks: u64, stop_with_held_inputs: bool) {
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
                "mixed-progress fundedBlocks={} elapsedMs={}",
                index + 1,
                started.elapsed().as_millis()
            );
        }
    }
    let recipients = funded_blocks * PER_BLOCK;
    check_canonical(&node, &owner.pk_hex(), recipients, 0);
    let common_height = node.chain().ledger().height();
    let mut alternative = node.chain().clone();
    for index in 0..33 {
        let transfers: Vec<_> = (recipients + index * PER_BLOCK
            ..recipients + (index + 1) * PER_BLOCK)
            .map(|nonce| transfer(&owner, &owner.pk_hex(), nonce))
            .collect();
        let begin = Instant::now();
        let template = alternative
            .template(
                &owner.pk_hex(),
                &owner.pk_hex(),
                (alternative.ledger().height() + 1) * 60_000,
                &transfers,
            )
            .unwrap();
        record_phase(begin, &mut max_template, "mixed candidate template");
        let block = template.mine(0, 2_000_000).unwrap().unwrap();
        let auth = owner
            .sign_for_network(
                &block.authorization_payload().unwrap(),
                Some(native_testnet().network_id()),
            )
            .unwrap();
        let begin = Instant::now();
        alternative.append(block.authorize(&auth).unwrap()).unwrap();
        record_phase(begin, &mut max_candidate_append, "mixed candidate append");
    }
    let advertised =
        json!({"height":alternative.ledger().height(),"hash":alternative.head_hash().to_hex()});
    let suffix = Arc::new(alternative.blocks()[common_height as usize..].to_vec());
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
    drop(alternative);
    let block = mine(&node, &owner, Some(&[]), &mut max_template);
    append(&mut node, block, &mut max_append);
    check_canonical(&node, &owner.pk_hex(), recipients, 0);
    let local_head = node.chain().head_hash();
    let resources = serde_json::to_value(node.resource_usage().unwrap()).unwrap();
    let before_digests = journal_digests(&dir);
    let manifest = fs::read(dir.path.join("state.manifest.json")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let rpc_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let rpc_address = rpc_listener.local_addr().unwrap();
    let local_key = peer_identity();
    let local_id = local_key.peer_id();
    let group = ForkGroup::start(
        address,
        local_id,
        common_height,
        suffix.clone(),
        hashes.clone(),
        advertised.clone(),
    );
    let peer_addresses: Vec<_> = group.peers.iter().map(|(address, _)| *address).collect();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let shutdown = Arc::new(tokio::sync::Notify::new());
    let _stop_on_drop = StopOnDrop(shutdown.clone());
    let network_started = Instant::now();
    let mut server = runtime.spawn(boole_node::serve_native_node_with_peers(
        rpc_listener,
        node,
        listener,
        NativePeerConfig {
            identity: local_key,
            peers: group.peers.clone(),
        },
        shutdown.clone(),
    ));
    let mut partial_bytes = [0; 8];
    for _ in 0..8 {
        let (index, bytes) = group
            .paused
            .recv_timeout(Duration::from_secs(10))
            .expect("all eight fork buffers must pause together");
        assert_eq!(partial_bytes[index], 0);
        partial_bytes[index] = bytes;
    }
    assert_eq!(diagnostics(rpc_address)["peers"]["activeOutboundRounds"], 8);
    assert_eq!(journal_digests(&dir), before_digests);
    let mut inbound = Vec::new();
    for identity in &group.identities[..4] {
        let transport = TlsTransport::new(identity.clone(), vec![(address, local_id)]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut connection = transport.connect_until(&address, deadline).unwrap();
        transport
            .send_json_counted_until(&mut connection, &hello(), 4096, deadline)
            .unwrap();
        let (reply, _): (Value, _) = transport
            .recv_json_counted_until(&mut connection, 4096, deadline)
            .unwrap();
        assert_eq!(reply["type"], "hello");
        assert_eq!(reply["head"]["hash"], local_head.to_hex());
        inbound.push((transport, connection));
    }
    let before = diagnostics(rpc_address);
    assert_eq!(before["peers"]["activeInboundWorkers"], 4);
    assert_eq!(before["peers"]["authenticatedConnections"], 4);
    let excess = TlsTransport::new(group.identities[4].clone(), vec![(address, local_id)]).unwrap();
    assert!(excess
        .connect_until(&address, Instant::now() + Duration::from_secs(1))
        .is_err());
    wait("inbound rejection", || {
        diagnostics(rpc_address)["peers"]["rejectedConnections"]
            .as_u64()
            .unwrap()
            > before["peers"]["rejectedConnections"].as_u64().unwrap()
    });

    let whitespace = [b' '; 64 * 1024];
    let mut uploads = Vec::new();
    for _ in 0..8 {
        let mut upload = begin_upload(rpc_address);
        assert!(headers(&mut upload).starts_with("HTTP/1.1 100 "));
        upload.write_all(b"[]").unwrap();
        let mut remaining = BODY_BYTES - 3;
        while remaining > 0 {
            let count = remaining.min(whitespace.len());
            upload.write_all(&whitespace[..count]).unwrap();
            remaining -= count;
        }
        uploads.push(upload);
    }
    let overlap = diagnostics(rpc_address);
    assert_eq!(overlap["rpc"]["activeRequests"], 8);
    assert_eq!(overlap["rpc"]["diagnosticLimit"], 2);
    assert_eq!(overlap["rpc"]["activeDiagnostics"], 1);
    assert_eq!(overlap["peers"]["activeOutboundRounds"], 8);
    assert_eq!(overlap["peers"]["activeInboundWorkers"], 4);
    let (header, refused) = response(begin_upload(rpc_address));
    assert!(
        header.starts_with("HTTP/1.1 429 "),
        "ninth body was admitted: {header}"
    );
    assert!(refused.to_string().contains("native_worker_limit"));
    let mut diagnostic_max = Duration::ZERO;
    for _ in 0..10 {
        let begin = Instant::now();
        let observed = diagnostics(rpc_address);
        diagnostic_max = diagnostic_max.max(begin.elapsed());
        assert_eq!(observed["rpc"]["activeRequests"], 8);
        assert_eq!(observed["peers"]["activeOutboundRounds"], 8);
        assert_eq!(observed["peers"]["activeInboundWorkers"], 4);
    }
    assert_eq!(journal_digests(&dir), before_digests);
    let mut held_clients = None;
    let (drained, stopping, sent_bytes) = if stop_with_held_inputs {
        let stop_started = Instant::now();
        shutdown.notify_one();
        let completion = runtime
            .block_on(async { tokio::time::timeout(Duration::from_secs(7), &mut server).await });
        let stopping = stop_started.elapsed();
        let timely = completion.is_ok();
        if let Ok(result) = completion {
            result.unwrap().unwrap();
        } else {
            // A RED must not strand the server or real state ownership. Close
            // only fixture clients, then join before asserting the deadline.
            uploads.clear();
            inbound.clear();
            runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(5), &mut server)
                    .await
                    .expect("cleanup after held-client deadline")
                    .unwrap()
                    .unwrap();
            });
        }
        let mut closed_http = 0;
        for upload in &mut uploads {
            require_http_closed(upload).unwrap();
            closed_http += 1;
        }
        let mut closed_inbound = 0;
        for (transport, connection) in &mut inbound {
            require_tls_closed(transport, connection).unwrap();
            closed_inbound += 1;
        }
        for release in group.releases {
            release.send(ForkRelease::ObserveShutdown).unwrap();
        }
        let sent_bytes: Vec<_> = group
            .workers
            .into_iter()
            .map(|worker| worker.join().unwrap().unwrap())
            .collect();
        assert!(timely, "held-input shutdown exceeded seven seconds");
        assert_eq!((closed_http, closed_inbound), (8, 4));
        assert_eq!(sent_bytes, partial_bytes);
        // Preserve old client handles through independent fresh ownership.
        held_clients = Some((uploads, inbound));
        (
            json!({"closedHttp":closed_http,"closedInbound":closed_inbound,
                "closedOutbound":sent_bytes.len(),"completedMissingInputs":false}),
            stopping,
            sent_bytes,
        )
    } else {
        for mut upload in uploads {
            upload.write_all(b" ").unwrap();
            let (header, body) = response(upload);
            assert!(header.starts_with("HTTP/1.1 200 "), "{header} {body}");
            assert_eq!(body["adopted"], false);
        }
        wait("ordinary permits drain", || {
            diagnostics(rpc_address)["rpc"]["activeRequests"] == 0
        });
        drop(inbound);
        wait("inbound workers drain", || {
            diagnostics(rpc_address)["peers"]["activeInboundWorkers"] == 0
        });
        for release in group.releases {
            release.send(ForkRelease::SendExcessPage).unwrap();
        }
        wait("all oversized forks refused", || {
            diagnostics(rpc_address)["peers"]["peers"]
                .as_array()
                .unwrap()
                .iter()
                .all(|peer| peer["failedRounds"].as_u64().unwrap() > 0)
        });
        let drained = diagnostics(rpc_address);
        let info = get(rpc_address, "/native/info");
        assert_eq!(info["headHash"], local_head.to_hex());
        assert_eq!(info["resources"], resources);
        assert_eq!(journal_digests(&dir), before_digests);
        assert!(network_started.elapsed() < Duration::from_secs(15));
        let stop_started = Instant::now();
        shutdown.notify_one();
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(3), server)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        });
        let stopping = stop_started.elapsed();
        assert!(stopping < Duration::from_secs(3));
        let sent_bytes: Vec<_> = group
            .workers
            .into_iter()
            .map(|worker| worker.join().unwrap().unwrap())
            .collect();
        (drained, stopping, sent_bytes)
    };
    let network_elapsed = network_started.elapsed();
    assert!(network_elapsed < Duration::from_secs(15));
    for address in [rpc_address, address].into_iter().chain(peer_addresses) {
        assert!(TcpListener::bind(address).is_ok());
    }
    assert_eq!(journal_digests(&dir), before_digests);
    assert_eq!(
        fs::read(dir.path.join("state.manifest.json")).unwrap(),
        manifest
    );
    eprintln!(
        "mixed-network {}",
        json!({"fundedBlocks":funded_blocks, "stopWithHeldInputs":stop_with_held_inputs,
        "localHead":local_head.to_hex(),
        "advertised":advertised, "partialBytesPerPeer":partial_bytes, "finalSentBytesPerPeer":sent_bytes,
        "httpBodyBytesSentPerClient":BODY_BYTES-1, "overlap":overlap, "drained":drained,
        "diagnosticMaxMicros":diagnostic_max.as_micros(), "networkMs":network_elapsed.as_millis(),
        "stopMicros":stopping.as_micros(), "elapsedMs":started.elapsed().as_millis()})
    );
    // In the held-input case, runtime teardown must not be what releases state
    // ownership. Keep both runtime and old client handles alive through reopen.
    let held_runtime = if stop_with_held_inputs {
        Some(runtime)
    } else {
        drop(runtime);
        None
    };
    drop(suffix);
    drop(hashes);
    let reopen_started = Instant::now();
    let node = NativeNode::open(&dir.path).unwrap();
    let replay = reopen_started.elapsed();
    assert!(replay < Duration::from_secs(120));
    node.ensure_ready().unwrap();
    check_canonical(&node, &owner.pk_hex(), recipients, 0);
    assert_eq!(node.chain().head_hash(), local_head);
    assert!(node.pending().is_empty());
    assert_eq!(journal_digests(&dir), before_digests);
    assert_eq!(
        fs::read(dir.path.join("state.manifest.json")).unwrap(),
        manifest
    );
    drop(held_clients);
    drop(held_runtime);
    assert!(started.elapsed() < Duration::from_secs(900));
    eprintln!(
        "mixed-result {}",
        json!({"fundedBlocks":funded_blocks, "stopWithHeldInputs":stop_with_held_inputs,
        "head":local_head.to_hex(),
        "networkId":native_testnet().network_id(), "genesisHash":native_testnet().genesis_hash().to_hex(),
        "resources":node.resource_usage().unwrap(), "issued":node.chain().ledger().issued().to_string(),
        "restartMs":replay.as_millis(), "elapsedMs":started.elapsed().as_millis(),
        "maxTemplateMs":max_template.as_millis(), "maxAppendMs":max_append.as_millis(),
        "maxCandidateAppendMs":max_candidate_append.as_millis()})
    );
}

#[test]
fn small_mixed_p2p_http_pressure_preserves_state_and_diagnostics() {
    run_mixed_scenario(2, false);
}

#[test]
#[ignore = "explicit preregistered combined P2P/HTTP resource qualification; not routine CI"]
fn native_mixed_p2p_http_pressure_131072_accounts() {
    run_mixed_scenario(256, false);
}

#[test]
fn small_mixed_shutdown_closes_held_inputs_and_replays_unchanged() {
    run_mixed_scenario(2, true);
}

#[test]
#[ignore = "explicit preregistered held-input shutdown qualification; not routine CI"]
fn native_mixed_shutdown_131072_accounts() {
    run_mixed_scenario(256, true);
}
