//! BF.6a — read-only useful-work package serving over the real P2P ingress.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use boole_core::{
    CalibrationReport, CanonicalPackage, LocalPackageStore, LocalPackageStoreConfig, PackageFile,
    CONSENSUS_RULE_VERSION, PACKAGE_OBJECTS_DIRECTORY,
};
use boole_node::{
    serve_local_node_with_p2p, LocalNodeConfig, P2pConfig, PackageServingConfig, RuntimeConfig,
};
use boole_p2p::{Frame, HeadSummary, TcpTransport, Transport, PROTOCOL_VERSION};
use boole_testkit::rand_suffix;
use serde_json::Value;
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

fn scenario_identity() -> (String, String) {
    let raw = fs::read_to_string(scenario_path()).expect("scenario fixture");
    let doc: Value = serde_json::from_str(&raw).expect("scenario json");
    let cfg: CalibrationReport = serde_json::from_value(doc["cfg"].clone()).expect("scenario cfg");
    let config = RuntimeConfig::from_calibration_report(cfg, 60_000).expect("runtime config");
    let network_id = "boole-mvp".to_string();
    let genesis_hash = config
        .genesis_spec(&network_id, doc["genesisC"].as_str().expect("genesisC"))
        .hash()
        .to_hex();
    (network_id, genesis_hash)
}

struct Boot {
    http_addr: SocketAddr,
    p2p_addr: SocketAddr,
    dir: PathBuf,
    shutdown: Arc<Notify>,
    handle: thread::JoinHandle<anyhow::Result<()>>,
}

fn boot(tag: &str, package_serving: Option<PackageServingConfig>) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-bf6a-p2p-{tag}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&dir).expect("temporary node directory");
    let http_listener = TcpListener::bind("127.0.0.1:0").expect("bind HTTP");
    let http_addr = http_listener.local_addr().expect("HTTP address");
    let p2p_listener = TcpListener::bind("127.0.0.1:0").expect("bind P2P");
    let p2p_addr = p2p_listener.local_addr().expect("P2P address");
    let shutdown = Arc::new(Notify::new());
    let shutdown_for_node = shutdown.clone();
    let scenario = scenario_path();
    let block_path = dir.join("blocks.ndjson");
    let rewards = dir.join("rewards.ndjson");
    let (ready_tx, ready_rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        ready_tx.send(()).expect("node thread started");
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
                listener: Some(p2p_listener),
                peers: vec!["127.0.0.1:1".parse().expect("dummy allowlisted peer")],
                rate_limit_per_60s: boole_node::DEFAULT_P2P_RATE_LIMIT_PER_60S,
                package_serving,
                package_fetching: None,
            },
            Some(shutdown_for_node),
        )
    });
    ready_rx.recv().expect("node thread ready");
    thread::sleep(Duration::from_millis(50));
    Boot {
        http_addr,
        p2p_addr,
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

fn request_package(addr: SocketAddr, root: &str) -> Frame {
    let stream = TcpStream::connect(addr).expect("connect P2P");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("write timeout");
    let transport = TcpTransport::new();
    let mut conn = TcpTransport::conn_from_stream(stream).expect("wrap P2P stream");
    let (network_id, genesis_hash) = scenario_identity();
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
    assert!(matches!(
        transport.recv_frame(&mut conn).expect("receive Hello"),
        Frame::Hello { .. }
    ));
    transport
        .send_frame(
            &mut conn,
            &Frame::GetPackage {
                root: root.to_string(),
            },
        )
        .expect("send GetPackage");
    transport.recv_frame(&mut conn).expect("receive Package")
}

fn http_request(addr: SocketAddr, raw: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("connect HTTP");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("HTTP read timeout");
    stream.write_all(raw.as_bytes()).expect("write HTTP");
    let mut buf = Vec::new();
    match stream.read_to_end(&mut buf) {
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::ConnectionReset && !buf.is_empty() => {}
        Err(error) => panic!("read HTTP response: {error}"),
    }
    let response = String::from_utf8(buf).expect("UTF-8 HTTP response");
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .expect("HTTP status");
    let body = response
        .split_once("\r\n\r\n")
        .expect("HTTP body")
        .1
        .to_string();
    (status, body)
}

fn metric_value(addr: SocketAddr, name: &str) -> u64 {
    let request = "GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let (status, body) = http_request(addr, request);
    assert_eq!(status, 200);
    body.lines()
        .find_map(|line| {
            let (metric, value) = line.split_once(' ')?;
            (metric == name).then(|| value.parse().expect("metric value"))
        })
        .unwrap_or_else(|| panic!("missing metric {name}"))
}

fn status(addr: SocketAddr) -> Value {
    let request = "GET /status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let (http_status, body) = http_request(addr, request);
    assert_eq!(http_status, 200);
    serde_json::from_str(&body).expect("status JSON")
}

fn enabled_store_config() -> LocalPackageStoreConfig {
    LocalPackageStoreConfig {
        enabled: true,
        max_pending_packages: 8,
        max_pending_bytes: 16 * 1024 * 1024,
    }
}

#[test]
fn staged_package_is_served_byte_identically_over_p2p() {
    let parent = std::env::temp_dir().join(format!(
        "boole-bf6a-p2p-store-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&parent).expect("store parent");
    let package = CanonicalPackage::new(vec![
        PackageFile::new(b"README.md", b"useful work"),
        PackageFile::new(b"src/lib.rs", b"pub fn answer() -> u8 { 42 }"),
    ])
    .expect("canonical package");
    let expected = package.canonical_bytes().to_vec();
    let root = package.root().to_hex();
    let mut store = LocalPackageStore::open(parent.join("store"), enabled_store_config())
        .expect("open package store");
    store
        .stage(&package, "receipt:test")
        .expect("stage package");

    let boot = boot(
        "served",
        Some(PackageServingConfig::new(Arc::new(store), [package.root()])),
    );
    assert_eq!(
        request_package(boot.p2p_addr, &root),
        Frame::Package {
            root,
            canonical_bytes: Some(expected),
        }
    );
    assert_eq!(
        metric_value(
            boot.http_addr,
            "boole_p2p_ingress_get_packages_served_total"
        ),
        1
    );
    stop(boot);
    fs::remove_dir_all(parent).expect("remove store parent");
}

#[test]
fn authorized_but_missing_package_is_explicitly_unavailable() {
    let parent = std::env::temp_dir().join(format!(
        "boole-bf6a-p2p-missing-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&parent).expect("store parent");
    let package = CanonicalPackage::new(vec![PackageFile::new(b"missing.txt", b"not staged")])
        .expect("canonical package");
    let root = package.root().to_hex();
    let store = LocalPackageStore::open(parent.join("store"), enabled_store_config())
        .expect("open empty package store");

    let boot = boot(
        "missing",
        Some(PackageServingConfig::new(Arc::new(store), [package.root()])),
    );
    assert_eq!(
        request_package(boot.p2p_addr, &root),
        Frame::Package {
            root,
            canonical_bytes: None,
        }
    );
    assert_eq!(
        metric_value(
            boot.http_addr,
            "boole_p2p_ingress_get_packages_unavailable_total"
        ),
        1
    );
    assert_eq!(
        metric_value(
            boot.http_addr,
            "boole_p2p_ingress_get_packages_store_error_total"
        ),
        0
    );
    stop(boot);
    fs::remove_dir_all(parent).expect("remove store parent");
}

#[test]
fn disabled_store_is_explicitly_unavailable_without_touching_disk() {
    let store_path = std::env::temp_dir().join(format!(
        "boole-bf6a-p2p-disabled-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    assert!(!store_path.exists());
    let package = CanonicalPackage::new(vec![PackageFile::new(b"disabled.txt", b"never read")])
        .expect("canonical package");
    let root = package.root().to_hex();
    let store = LocalPackageStore::open(&store_path, LocalPackageStoreConfig::default())
        .expect("open disabled store");

    let boot = boot(
        "disabled",
        Some(PackageServingConfig::new(Arc::new(store), [package.root()])),
    );
    assert_eq!(
        request_package(boot.p2p_addr, &root),
        Frame::Package {
            root,
            canonical_bytes: None,
        }
    );
    assert_eq!(
        metric_value(
            boot.http_addr,
            "boole_p2p_ingress_get_packages_unavailable_total"
        ),
        1
    );
    assert_eq!(
        metric_value(
            boot.http_addr,
            "boole_p2p_ingress_get_packages_store_error_total"
        ),
        0
    );
    stop(boot);
    assert!(
        !store_path.exists(),
        "disabled package serving must stay a disk no-op"
    );
}

#[test]
fn staged_but_unapproved_root_is_not_disclosed() {
    let parent = std::env::temp_dir().join(format!(
        "boole-bf6a-p2p-unapproved-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&parent).expect("store parent");
    let package = CanonicalPackage::new(vec![PackageFile::new(b"private.txt", b"not shared")])
        .expect("canonical package");
    let root = package.root().to_hex();
    let mut store = LocalPackageStore::open(parent.join("store"), enabled_store_config())
        .expect("open package store");
    store
        .stage(&package, "receipt:unapproved")
        .expect("stage package");

    let boot = boot(
        "unapproved",
        Some(PackageServingConfig::new(
            Arc::new(store),
            std::iter::empty(),
        )),
    );
    assert_eq!(
        request_package(boot.p2p_addr, &root),
        Frame::Package {
            root,
            canonical_bytes: None,
        }
    );
    assert_eq!(
        metric_value(
            boot.http_addr,
            "boole_p2p_ingress_get_packages_unavailable_total"
        ),
        1
    );
    assert_eq!(
        metric_value(
            boot.http_addr,
            "boole_p2p_ingress_get_packages_served_total"
        ),
        0
    );
    stop(boot);
    fs::remove_dir_all(parent).expect("remove store parent");
}

#[test]
fn corrupt_authorized_object_is_unavailable_without_mutating_chain_state() {
    let parent = std::env::temp_dir().join(format!(
        "boole-bf6a-p2p-corrupt-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    fs::create_dir_all(&parent).expect("store parent");
    let store_path = parent.join("store");
    let package = CanonicalPackage::new(vec![PackageFile::new(b"result.bin", b"valid bytes")])
        .expect("canonical package");
    let root = package.root().to_hex();
    let mut store =
        LocalPackageStore::open(&store_path, enabled_store_config()).expect("open package store");
    store
        .stage(&package, "receipt:corrupt")
        .expect("stage package");
    fs::write(
        store_path
            .join(PACKAGE_OBJECTS_DIRECTORY)
            .join(format!("{root}.pkg")),
        b"corrupt object",
    )
    .expect("corrupt synthetic object");

    let boot = boot(
        "corrupt",
        Some(PackageServingConfig::new(Arc::new(store), [package.root()])),
    );
    let height_before = status(boot.http_addr)["height"]
        .as_u64()
        .expect("height before");
    let rejected_before = metric_value(boot.http_addr, "boole_p2p_ingress_blocks_rejected_total");
    let block_bytes_before = fs::read(boot.dir.join("blocks.ndjson")).ok();

    assert_eq!(
        request_package(boot.p2p_addr, &root),
        Frame::Package {
            root,
            canonical_bytes: None,
        }
    );
    assert_eq!(
        metric_value(
            boot.http_addr,
            "boole_p2p_ingress_get_packages_store_error_total"
        ),
        1
    );
    assert_eq!(
        metric_value(
            boot.http_addr,
            "boole_p2p_ingress_get_packages_unavailable_total"
        ),
        0
    );
    assert_eq!(
        status(boot.http_addr)["height"].as_u64(),
        Some(height_before)
    );
    assert_eq!(
        metric_value(boot.http_addr, "boole_p2p_ingress_blocks_rejected_total"),
        rejected_before
    );
    assert_eq!(
        fs::read(boot.dir.join("blocks.ndjson")).ok(),
        block_bytes_before,
        "package availability failures must not touch the block store"
    );

    stop(boot);
    fs::remove_dir_all(parent).expect("remove store parent");
}

#[test]
fn default_off_node_returns_explicit_unavailable_without_a_store() {
    let package = CanonicalPackage::new(vec![PackageFile::new(b"absent.txt", b"not configured")])
        .expect("canonical package");
    let root = package.root().to_hex();
    let boot = boot("default-off", None);

    assert_eq!(
        request_package(boot.p2p_addr, &root),
        Frame::Package {
            root,
            canonical_bytes: None,
        }
    );
    assert_eq!(
        metric_value(
            boot.http_addr,
            "boole_p2p_ingress_get_packages_unavailable_total"
        ),
        1
    );
    assert_eq!(
        metric_value(
            boot.http_addr,
            "boole_p2p_ingress_get_packages_served_total"
        ),
        0
    );
    assert_eq!(
        metric_value(
            boot.http_addr,
            "boole_p2p_ingress_get_packages_store_error_total"
        ),
        0
    );
    stop(boot);
}
