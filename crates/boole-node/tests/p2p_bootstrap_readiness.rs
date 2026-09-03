//! N5.3 — a node that explicitly requires static-bootstrap synchronization
//! must not advertise readiness merely because its local state is healthy.
//! Only an outbound, identity-checked sync round that ends at the peer's exact
//! `(height, c)` head may open `/ready`.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use boole_node::{serve_local_node_with_p2p, LocalNodeConfig, P2pConfig};
use boole_p2p::{Frame, HeadSummary, TcpTransport, Transport};
use boole_testkit::rand_suffix;
use serde_json::Value;
use tokio::sync::Notify;

const RATE_LIMIT: usize = 600;

fn scenario_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario")
}

fn node_config(dir: &Path) -> LocalNodeConfig {
    LocalNodeConfig {
        scenario_path: scenario_path(),
        block_path: dir.join("blocks.ndjson"),
        reward_ledger_path: Some(dir.join("rewards.ndjson")),
        work_manifests_path: None,
        bounties_path: None,
        bounty_event_ledger_path: None,
        bounty_verifiers: None,
        family_manifests_dir: None,
        max_requests: None,
        operator_signer_pks: vec![],
        session_registry_path: None,
        submit_nonce_ledger_path: None,
        signed_nonce_ledger_path: None,
        submit_receipt_ledger_path: None,
        receipt_commitment_ledger_path: None,
        proof_dedup_ledger_path: None,
        genesis_override: None,
        state_dir: None,
        network_id: None,
        lean_checker_dir: None,
        lean_checker_disabled: true,
        http_rate_limit_per_60s: None,
        allow_anonymous_submit: true,
    }
}

struct Boot {
    http: SocketAddr,
    shutdown: Arc<Notify>,
    handle: thread::JoinHandle<anyhow::Result<()>>,
    dir: PathBuf,
}

fn boot(label: &str, p2p: P2pConfig) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-n53-bootstrap-ready-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let config = node_config(&dir);
    boot_with_config(dir, config, p2p)
}

fn boot_with_config(dir: PathBuf, config: LocalNodeConfig, p2p: P2pConfig) -> Boot {
    let listener = TcpListener::bind("127.0.0.1:0").expect("http bind");
    let http = listener.local_addr().expect("http addr");
    let shutdown = Arc::new(Notify::new());
    let shutdown_for_node = shutdown.clone();
    let handle = thread::spawn(move || {
        serve_local_node_with_p2p(listener, config, p2p, Some(shutdown_for_node))
    });
    wait_until("HTTP listener", Duration::from_secs(5), || {
        TcpStream::connect_timeout(&http, Duration::from_millis(50)).is_ok()
    });
    Boot {
        http,
        shutdown,
        handle,
        dir,
    }
}

fn stop(boot: Boot) {
    boot.shutdown.notify_one();
    boot.handle.join().expect("node thread").expect("node exit");
    let _ = std::fs::remove_dir_all(boot.dir);
}

fn get_json(addr: SocketAddr, path: &str) -> (u16, Value) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .expect("request");
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).expect("response");
    let raw = String::from_utf8(bytes).expect("utf8");
    let status = raw
        .split_whitespace()
        .nth(1)
        .expect("status")
        .parse()
        .expect("status code");
    let body = raw.split_once("\r\n\r\n").expect("body").1;
    (status, serde_json::from_str(body).expect("json"))
}

fn metric_value(addr: SocketAddr, name: &str) -> u64 {
    let mut stream = TcpStream::connect(addr).expect("connect metrics");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("metrics timeout");
    write!(
        stream,
        "GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .expect("metrics request");
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).expect("metrics response");
    let raw = String::from_utf8(bytes).expect("metrics utf8");
    let body = raw.split_once("\r\n\r\n").expect("metrics body").1;
    let prefix = format!("{name} ");
    body.lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("metric {name} missing from:\n{body}"))
        .parse()
        .expect("u64 metric")
}

fn wait_until(what: &str, timeout: Duration, mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn unreachable_required_bootstrap_keeps_ready_closed() {
    let node = boot(
        "unreachable",
        P2pConfig {
            listener: None,
            peers: vec!["127.0.0.1:9".parse().expect("dead peer")],
            rate_limit_per_60s: RATE_LIMIT,
            package_serving: None,
            package_fetching: None,
            require_head_sync_for_readiness: true,
        },
    );

    let (status, body) = get_json(node.http, "/ready");
    assert_eq!(
        status, 503,
        "unreachable bootstrap must not be ready: {body}"
    );
    assert_eq!(body["reason"], "p2p_head_not_synced");
    assert_eq!(body["checks"]["p2p_head_synced"], false);
    stop(node);
}

#[test]
fn exact_outbound_bootstrap_head_opens_ready() {
    let seed_p2p = TcpListener::bind("127.0.0.1:0").expect("seed p2p");
    let seed_p2p_addr = seed_p2p.local_addr().expect("seed p2p addr");
    let joiner_p2p = TcpListener::bind("127.0.0.1:0").expect("joiner p2p");
    let joiner_p2p_addr = joiner_p2p.local_addr().expect("joiner p2p addr");

    let seed = boot(
        "seed",
        P2pConfig {
            listener: Some(seed_p2p),
            peers: vec![joiner_p2p_addr],
            rate_limit_per_60s: RATE_LIMIT,
            package_serving: None,
            package_fetching: None,
            require_head_sync_for_readiness: false,
        },
    );
    let joiner = boot(
        "joiner",
        P2pConfig {
            listener: Some(joiner_p2p),
            peers: vec![seed_p2p_addr],
            rate_limit_per_60s: RATE_LIMIT,
            package_serving: None,
            package_fetching: None,
            require_head_sync_for_readiness: true,
        },
    );

    wait_until("joiner readiness", Duration::from_secs(10), || {
        let (status, body) = get_json(joiner.http, "/ready");
        status == 200 && body["checks"]["p2p_head_synced"] == true
    });

    stop(joiner);
    stop(seed);
}

#[test]
fn reachable_different_network_bootstrap_keeps_ready_closed() {
    let seed_p2p = TcpListener::bind("127.0.0.1:0").expect("seed p2p");
    let seed_p2p_addr = seed_p2p.local_addr().expect("seed p2p addr");

    let seed_dir = std::env::temp_dir().join(format!(
        "boole-n53-bootstrap-other-network-seed-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let joiner_dir = std::env::temp_dir().join(format!(
        "boole-n53-bootstrap-other-network-joiner-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&seed_dir).expect("seed dir");
    std::fs::create_dir_all(&joiner_dir).expect("joiner dir");
    let mut seed_config = node_config(&seed_dir);
    seed_config.network_id = Some("controlled-network-a".to_string());
    let mut joiner_config = node_config(&joiner_dir);
    joiner_config.network_id = Some("controlled-network-b".to_string());

    let seed = boot_with_config(
        seed_dir,
        seed_config,
        P2pConfig {
            listener: Some(seed_p2p),
            // The ingress allowlist is derived from peer IPs. A same-host
            // dead endpoint allows the joiner's connection to reach the
            // network-identity comparison without creating another node.
            peers: vec!["127.0.0.1:9".parse().expect("allowlisted dead peer")],
            rate_limit_per_60s: RATE_LIMIT,
            package_serving: None,
            package_fetching: None,
            require_head_sync_for_readiness: false,
        },
    );
    let joiner = boot_with_config(
        joiner_dir,
        joiner_config,
        P2pConfig {
            listener: None,
            peers: vec![seed_p2p_addr],
            rate_limit_per_60s: RATE_LIMIT,
            package_serving: None,
            package_fetching: None,
            require_head_sync_for_readiness: true,
        },
    );

    wait_until(
        "foreign-network handshake rejection",
        Duration::from_secs(10),
        || {
            metric_value(joiner.http, "boole_p2p_sync_peer_failures_total") >= 1
                && metric_value(seed.http, "boole_p2p_ingress_hello_mismatch_drops_total") >= 1
        },
    );
    let (status, body) = get_json(joiner.http, "/ready");
    assert_eq!(status, 503, "foreign network must not be ready: {body}");
    assert_eq!(body["reason"], "p2p_head_not_synced");

    stop(joiner);
    stop(seed);
}

#[test]
fn newer_peer_hello_closes_ready_before_its_blocks_arrive() {
    let fake_listener = TcpListener::bind("127.0.0.1:0").expect("fake peer bind");
    let fake_addr = fake_listener.local_addr().expect("fake peer addr");
    let (ahead_tx, ahead_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let fake_peer = thread::spawn(move || {
        let transport = TcpTransport::new();
        for round in 0..2 {
            let (stream, _) = fake_listener.accept().expect("accept sync round");
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("fake read timeout");
            let mut conn = TcpTransport::conn_from_stream(stream).expect("fake conn");
            let hello = transport.recv_frame(&mut conn).expect("dialer hello");
            let Frame::Hello {
                protocol_version,
                consensus_rule_version,
                network_id,
                genesis_hash,
                head,
            } = hello
            else {
                panic!("expected Hello from sync dialer, got {hello:?}");
            };
            let advertised = if round == 0 {
                head
            } else {
                HeadSummary {
                    height: head.height + 1,
                    c: "33".repeat(32),
                }
            };
            transport
                .send_frame(
                    &mut conn,
                    &Frame::Hello {
                        protocol_version,
                        consensus_rule_version,
                        network_id,
                        genesis_hash,
                        head: advertised,
                    },
                )
                .expect("reply Hello");
            if round == 1 {
                assert!(matches!(
                    transport.recv_frame(&mut conn).expect("GetBlocks"),
                    Frame::GetBlocks { from: 0, to: 0 }
                ));
                ahead_tx.send(()).expect("signal newer Hello");
                release_rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("release stalled fake peer");
            }
        }
    });

    let node = boot(
        "newer-hello",
        P2pConfig {
            listener: None,
            peers: vec![fake_addr],
            rate_limit_per_60s: RATE_LIMIT,
            package_serving: None,
            package_fetching: None,
            require_head_sync_for_readiness: true,
        },
    );
    wait_until(
        "initial exact-head readiness",
        Duration::from_secs(10),
        || get_json(node.http, "/ready").0 == 200,
    );
    ahead_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("second sync round reached the stalled block response");

    let (status, body) = get_json(node.http, "/ready");
    assert_eq!(
        status, 503,
        "a newly advertised higher head must close readiness before its blocks arrive: {body}"
    );
    assert_eq!(body["reason"], "p2p_head_not_synced");
    assert_eq!(body["checks"]["p2p_head_synced"], false);

    release_tx.send(()).expect("release fake peer");
    fake_peer.join().expect("fake peer thread");
    stop(node);
}
