//! BF.6a — fail-closed package receive/fetch into the node-owned local CAS.

use std::fs;
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use boole_core::{
    CanonicalPackage, LocalPackageStore, LocalPackageStoreConfig, PackageFile,
    CONSENSUS_RULE_VERSION, PACKAGE_OBJECTS_DIRECTORY,
};
use boole_node::{
    serve_local_node_with_p2p, LocalNodeConfig, P2pConfig, PackageFetchRequest,
    PackageFetchingConfig,
};
use boole_p2p::{Frame, FrameError, HeadSummary, TcpTransport, Transport, PROTOCOL_VERSION};
use boole_testkit::rand_suffix;
use tokio::sync::Notify;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn scenario_path() -> PathBuf {
    repo_root().join("fixtures/protocol/runtime-smoke/v1.json")
}

fn enabled_store_config() -> LocalPackageStoreConfig {
    LocalPackageStoreConfig {
        enabled: true,
        max_pending_packages: 8,
        max_pending_bytes: 16 * 1024 * 1024,
    }
}

struct Boot {
    http_addr: SocketAddr,
    dir: PathBuf,
    shutdown: Arc<Notify>,
    handle: thread::JoinHandle<anyhow::Result<()>>,
}

fn boot(peer: SocketAddr, package_fetching: PackageFetchingConfig) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-bf6a-fetch-node-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&dir).expect("temporary node directory");
    let http_listener = TcpListener::bind("127.0.0.1:0").expect("bind HTTP");
    let http_addr = http_listener.local_addr().expect("HTTP address");
    let shutdown = Arc::new(Notify::new());
    let shutdown_for_node = shutdown.clone();
    let scenario = scenario_path();
    let block_path = dir.join("blocks.ndjson");
    let rewards = dir.join("rewards.ndjson");
    let handle = thread::spawn(move || {
        serve_local_node_with_p2p(
            http_listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path,
                reward_ledger_path: Some(rewards),
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                operator_signer_pks: vec![],
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                proof_dedup_ledger_path: None,
                max_requests: None,
                genesis_override: None,
                state_dir: None,
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: true,
            },
            P2pConfig {
                listener: None,
                peers: vec![peer],
                rate_limit_per_60s: boole_node::DEFAULT_P2P_RATE_LIMIT_PER_60S,
                package_serving: None,
                package_fetching: Some(package_fetching),
            },
            Some(shutdown_for_node),
        )
    });
    Boot {
        http_addr,
        dir,
        shutdown,
        handle,
    }
}

fn stop(boot: Boot) {
    boot.shutdown.notify_one();
    boot.handle
        .join()
        .expect("node thread")
        .expect("node exits");
    fs::remove_dir_all(boot.dir).expect("remove temporary node directory");
}

fn serve_package_once(package: CanonicalPackage) -> (SocketAddr, thread::JoinHandle<()>) {
    let expected_root = package.root();
    serve_package_response_once(
        expected_root,
        Frame::Package {
            root: expected_root.to_hex(),
            canonical_bytes: Some(package.canonical_bytes().to_vec()),
        },
    )
}

fn serve_package_response_once(
    expected_root: boole_core::PackageRoot,
    response: Frame,
) -> (SocketAddr, thread::JoinHandle<()>) {
    serve_package_responses(expected_root, vec![response])
}

fn serve_package_responses(
    expected_root: boole_core::PackageRoot,
    responses: Vec<Frame>,
) -> (SocketAddr, thread::JoinHandle<()>) {
    assert!(!responses.is_empty(), "at least one response");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind synthetic package peer");
    listener
        .set_nonblocking(true)
        .expect("nonblocking synthetic peer");
    let addr = listener.local_addr().expect("synthetic peer address");
    let responses = Arc::new(Mutex::new(std::collections::VecDeque::from(responses)));
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let (served_tx, served_rx) = mpsc::channel();
        loop {
            if served_rx.try_recv().is_ok() {
                return;
            }
            let (stream, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(error)
                    if error.kind() == ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => panic!("accept synthetic peer: {error}"),
            };
            let responses = responses.clone();
            let served_tx = served_tx.clone();
            thread::spawn(move || {
                stream
                    .set_nonblocking(false)
                    .expect("blocking synthetic peer stream");
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("read timeout");
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .expect("write timeout");
                let transport = TcpTransport::new();
                let mut conn = TcpTransport::conn_from_stream(stream).expect("peer connection");
                let Ok(hello) = transport.recv_frame(&mut conn) else {
                    return;
                };
                let (network_id, genesis_hash) = match hello {
                    Frame::Hello {
                        protocol_version,
                        consensus_rule_version,
                        network_id,
                        genesis_hash,
                        ..
                    } => {
                        assert_eq!(protocol_version, PROTOCOL_VERSION);
                        assert_eq!(consensus_rule_version, CONSENSUS_RULE_VERSION);
                        (network_id, genesis_hash)
                    }
                    other => panic!("expected Hello, got {other:?}"),
                };
                transport
                    .send_frame(
                        &mut conn,
                        &Frame::Hello {
                            protocol_version: PROTOCOL_VERSION,
                            consensus_rule_version: CONSENSUS_RULE_VERSION,
                            network_id,
                            genesis_hash,
                            head: HeadSummary {
                                height: 0,
                                c: "00".repeat(32),
                            },
                        },
                    )
                    .expect("send Hello");
                match transport.recv_frame(&mut conn) {
                    Err(FrameError::ConnectionClosed | FrameError::Io(_)) => {}
                    Err(error) => panic!("receive synthetic request: {error}"),
                    Ok(Frame::GetPackage { root }) => {
                        assert_eq!(root, expected_root.to_hex());
                        let (response, finished) = {
                            let mut responses = responses.lock().expect("response queue");
                            let response = responses.pop_front().expect("remaining response");
                            let finished = responses.is_empty();
                            (response, finished)
                        };
                        transport
                            .send_frame(&mut conn, &response)
                            .expect("send package");
                        if finished {
                            let _ = served_tx.send(());
                        }
                    }
                    Ok(Frame::GetBlocks { .. }) => {
                        transport
                            .send_frame(&mut conn, &Frame::Blocks { blocks: vec![] })
                            .expect("answer sync probe");
                    }
                    Ok(other) => panic!("unexpected synthetic request: {other:?}"),
                }
            });
        }
    });
    (addr, handle)
}

fn metric_value(addr: SocketAddr, name: &str) -> u64 {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let mut stream = TcpStream::connect(addr).expect("connect HTTP");
    stream
        .write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("write metrics request");
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("read metrics response");
    let body = response.split_once("\r\n\r\n").expect("HTTP body").1;
    body.lines()
        .find_map(|line| {
            let (metric, value) = line.split_once(' ')?;
            (metric == name).then(|| value.parse().expect("metric value"))
        })
        .unwrap_or_else(|| panic!("missing metric {name}"))
}

#[test]
fn requested_package_is_strictly_received_and_staged_in_the_node_owned_cas() {
    let parent = std::env::temp_dir().join(format!(
        "boole-bf6a-fetch-store-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&parent).expect("store parent");
    let store_path = parent.join("store");
    let package = CanonicalPackage::new(vec![
        PackageFile::new(b"README.md", b"fetched useful work"),
        PackageFile::new(b"src/lib.rs", b"pub fn answer() -> u8 { 42 }"),
    ])
    .expect("canonical package");
    let expected_root = package.root();
    let expected_bytes = package.canonical_bytes().to_vec();
    let (peer, peer_thread) = serve_package_once(package);
    let store = LocalPackageStore::open(&store_path, enabled_store_config())
        .expect("open receiver-owned store");
    let fetching = PackageFetchingConfig::new(
        store,
        [PackageFetchRequest::new(expected_root, "receipt:block:7").expect("fetch request")],
    )
    .expect("fetching config");
    let boot = boot(peer, fetching);

    let deadline = Instant::now() + Duration::from_secs(10);
    while metric_value(boot.http_addr, "boole_p2p_package_fetch_staged_total") == 0 {
        assert!(
            Instant::now() < deadline,
            "package fetch timed out: recovered={} unavailable={} invalid={} peer_failure={} store_error={}",
            metric_value(
                boot.http_addr,
                "boole_p2p_package_fetch_recovered_total"
            ),
            metric_value(
                boot.http_addr,
                "boole_p2p_package_fetch_unavailable_total"
            ),
            metric_value(boot.http_addr, "boole_p2p_package_fetch_invalid_total"),
            metric_value(
                boot.http_addr,
                "boole_p2p_package_fetch_peer_failure_total"
            ),
            metric_value(
                boot.http_addr,
                "boole_p2p_package_fetch_store_error_total"
            ),
        );
        thread::sleep(Duration::from_millis(20));
    }

    stop(boot);
    peer_thread.join().expect("synthetic peer");
    let reopened = LocalPackageStore::open(&store_path, enabled_store_config())
        .expect("reopen receiver-owned store");
    assert_eq!(reopened.pending().len(), 1);
    assert_eq!(reopened.pending()[0].root(), expected_root);
    assert_eq!(reopened.pending()[0].reference(), "receipt:block:7");
    assert_eq!(
        reopened.read(expected_root).expect("read fetched CAS"),
        expected_bytes
    );
    assert!(store_path
        .join(PACKAGE_OBJECTS_DIRECTORY)
        .join(format!("{}.pkg", expected_root.to_hex()))
        .is_file());
    fs::remove_dir_all(parent).expect("remove store parent");
}

#[test]
fn content_root_mismatch_is_rejected_without_any_cas_or_pending_write() {
    let parent = std::env::temp_dir().join(format!(
        "boole-bf6a-invalid-fetch-store-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&parent).expect("store parent");
    let store_path = parent.join("store");
    let requested = CanonicalPackage::new(vec![PackageFile::new(b"answer", b"expected")])
        .expect("requested package");
    let wrong = CanonicalPackage::new(vec![PackageFile::new(b"answer", b"malicious")])
        .expect("wrong package");
    let requested_root = requested.root();
    let response = Frame::Package {
        root: requested_root.to_hex(),
        canonical_bytes: Some(wrong.canonical_bytes().to_vec()),
    };
    let (peer, peer_thread) = serve_package_response_once(requested_root, response);
    let store = LocalPackageStore::open(&store_path, enabled_store_config())
        .expect("open receiver-owned store");
    let fetching = PackageFetchingConfig::new(
        store,
        [
            PackageFetchRequest::new(requested_root, "receipt:block:invalid")
                .expect("fetch request"),
        ],
    )
    .expect("fetching config");
    let boot = boot(peer, fetching);

    let deadline = Instant::now() + Duration::from_secs(10);
    while metric_value(boot.http_addr, "boole_p2p_package_fetch_invalid_total") == 0 {
        assert!(
            Instant::now() < deadline,
            "invalid response was not counted"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        metric_value(boot.http_addr, "boole_p2p_package_fetch_staged_total"),
        0
    );

    stop(boot);
    peer_thread.join().expect("synthetic peer");
    let reopened = LocalPackageStore::open(&store_path, enabled_store_config())
        .expect("reopen receiver-owned store");
    assert!(reopened.pending().is_empty());
    assert!(reopened.read(requested_root).is_err());
    assert!(reopened.read(wrong.root()).is_err());
    fs::remove_dir_all(parent).expect("remove store parent");
}

#[test]
fn restart_recovers_an_already_staged_fetch_without_dialing_for_it_again() {
    let parent = std::env::temp_dir().join(format!(
        "boole-bf6a-recovered-fetch-store-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&parent).expect("store parent");
    let store_path = parent.join("store");
    let package =
        CanonicalPackage::new(vec![PackageFile::new(b"proof", b"durable")]).expect("package");
    let root = package.root();
    {
        let mut store = LocalPackageStore::open(&store_path, enabled_store_config())
            .expect("open original store");
        store
            .stage(&package, "receipt:block:recovered")
            .expect("stage before restart");
    }
    let store = LocalPackageStore::open(&store_path, enabled_store_config())
        .expect("reopen receiver-owned store");
    let fetching = PackageFetchingConfig::new(
        store,
        [PackageFetchRequest::new(root, "receipt:block:recovered").expect("fetch request")],
    )
    .expect("fetching config");
    let unreachable_peer: SocketAddr = "127.0.0.1:9".parse().expect("unreachable peer");
    let boot = boot(unreachable_peer, fetching);

    let deadline = Instant::now() + Duration::from_secs(10);
    while metric_value(boot.http_addr, "boole_p2p_package_fetch_recovered_total") == 0 {
        assert!(Instant::now() < deadline, "durable fetch was not recovered");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        metric_value(boot.http_addr, "boole_p2p_package_fetch_staged_total"),
        0,
        "recovery must not create a second pending reference"
    );

    stop(boot);
    let reopened = LocalPackageStore::open(&store_path, enabled_store_config())
        .expect("reopen after node shutdown");
    assert_eq!(reopened.pending().len(), 1);
    assert_eq!(reopened.pending()[0].root(), root);
    assert_eq!(reopened.pending()[0].reference(), "receipt:block:recovered");
    fs::remove_dir_all(parent).expect("remove store parent");
}

#[test]
fn restart_recovery_rechecks_the_durable_object_and_fails_closed_on_tamper() {
    let parent = std::env::temp_dir().join(format!(
        "boole-bf6a-corrupt-recovery-store-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&parent).expect("store parent");
    let store_path = parent.join("store");
    let package =
        CanonicalPackage::new(vec![PackageFile::new(b"proof", b"durable")]).expect("package");
    let root = package.root();
    {
        let mut store = LocalPackageStore::open(&store_path, enabled_store_config())
            .expect("open original store");
        store
            .stage(&package, "receipt:block:tamper")
            .expect("stage before restart");
    }
    let store = LocalPackageStore::open(&store_path, enabled_store_config())
        .expect("reopen before synthetic tamper");
    fs::write(
        store_path
            .join(PACKAGE_OBJECTS_DIRECTORY)
            .join(format!("{}.pkg", root.to_hex())),
        b"tampered after store open",
    )
    .expect("tamper durable object after open");
    let fetching = PackageFetchingConfig::new(
        store,
        [PackageFetchRequest::new(root, "receipt:block:tamper").expect("fetch request")],
    )
    .expect("fetching config");
    let unreachable_peer: SocketAddr = "127.0.0.1:9".parse().expect("unreachable peer");
    let boot = boot(unreachable_peer, fetching);

    let deadline = Instant::now() + Duration::from_secs(10);
    while metric_value(boot.http_addr, "boole_p2p_package_fetch_store_error_total") == 0 {
        assert!(
            Instant::now() < deadline,
            "corrupt recovery was not refused"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        metric_value(boot.http_addr, "boole_p2p_package_fetch_recovered_total"),
        0
    );

    stop(boot);
    assert!(LocalPackageStore::open(&store_path, enabled_store_config()).is_err());
    fs::remove_dir_all(parent).expect("remove store parent");
}

#[test]
fn unavailable_response_stays_pending_and_is_retried_until_strict_bytes_arrive() {
    let parent = std::env::temp_dir().join(format!(
        "boole-bf6a-retry-fetch-store-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&parent).expect("store parent");
    let store_path = parent.join("store");
    let package =
        CanonicalPackage::new(vec![PackageFile::new(b"proof", b"arrives later")]).expect("package");
    let root = package.root();
    let (peer, peer_thread) = serve_package_responses(
        root,
        vec![
            Frame::Package {
                root: root.to_hex(),
                canonical_bytes: None,
            },
            Frame::Package {
                root: root.to_hex(),
                canonical_bytes: Some(package.canonical_bytes().to_vec()),
            },
        ],
    );
    let store = LocalPackageStore::open(&store_path, enabled_store_config())
        .expect("open receiver-owned store");
    let fetching = PackageFetchingConfig::new(
        store,
        [PackageFetchRequest::new(root, "receipt:block:retry").expect("fetch request")],
    )
    .expect("fetching config")
    .with_retry_interval(Duration::from_millis(50));
    let boot = boot(peer, fetching);

    let deadline = Instant::now() + Duration::from_secs(10);
    while metric_value(boot.http_addr, "boole_p2p_package_fetch_staged_total") == 0 {
        assert!(Instant::now() < deadline, "pending fetch was not retried");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        metric_value(boot.http_addr, "boole_p2p_package_fetch_unavailable_total"),
        1
    );

    stop(boot);
    peer_thread.join().expect("synthetic peer");
    let reopened = LocalPackageStore::open(&store_path, enabled_store_config())
        .expect("reopen receiver-owned store");
    assert_eq!(reopened.pending().len(), 1);
    assert_eq!(reopened.pending()[0].reference(), "receipt:block:retry");
    fs::remove_dir_all(parent).expect("remove store parent");
}
