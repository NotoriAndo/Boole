//! N5.2 (ADR-0014) — per-network genesis binding + mismatch refusal.
//!
//! Three enforcement surfaces, all keyed on `GenesisSpec.hash()` (N5.1):
//! `Hello.genesis_hash` carries the SPEC hash (not the raw chain anchor),
//! so peers that agree on the anchor but differ on any committed
//! parameter refuse to gossip; `state.manifest.json` records the spec
//! hash and boot refuses a state dir written under a foreign genesis; a
//! node booted under a COMPILED network name (`boole-dev`/`boole-testnet`)
//! must match that network's compiled-in preset exactly.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use boole_core::CONSENSUS_RULE_VERSION;
use boole_node::{serve_local_node_with_p2p, LocalNodeConfig, P2pConfig};
use boole_p2p::{Frame, FrameError, HeadSummary, TcpTransport, Transport, PROTOCOL_VERSION};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};
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

fn scenario_genesis_c() -> String {
    let raw = fs::read_to_string(scenario_path()).expect("scenario fixture");
    let doc: Value = serde_json::from_str(&raw).expect("scenario json");
    doc["genesisC"].as_str().expect("genesisC").to_string()
}

struct Boot {
    addr: SocketAddr,
    dir: PathBuf,
    shutdown: Arc<Notify>,
    handle: thread::JoinHandle<anyhow::Result<()>>,
}

/// Boot a node; `state_dir`/`network_id`/`genesis_override`/`p2p_listener`
/// are the knobs the binding tests exercise. The caller inspects the join
/// result for refusal cases.
fn boot(
    tag: &str,
    state_dir: Option<PathBuf>,
    network_id: Option<String>,
    genesis_override: Option<String>,
    p2p_listener: Option<TcpListener>,
) -> Boot {
    boot_with_scenario(
        tag,
        scenario_path(),
        state_dir,
        network_id,
        genesis_override,
        p2p_listener,
    )
}

/// SC.7 — variant taking the scenario path so a test can boot from a
/// patched calibration (e.g. a non-consensus MinShareScoreMultiplier)
/// while keeping the preset-matching genesis.
fn boot_with_scenario(
    tag: &str,
    scenario: PathBuf,
    state_dir: Option<PathBuf>,
    network_id: Option<String>,
    genesis_override: Option<String>,
    p2p_listener: Option<TcpListener>,
) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-n52-binding-{tag}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("tmp dir");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind http");
    let addr = listener.local_addr().expect("http addr");
    let (tx, rx) = mpsc::channel();
    let block_path = dir.join("blocks.ndjson");
    let rewards = dir.join("rewards.ndjson");
    let shutdown = Arc::new(Notify::new());
    let shutdown_for_node = shutdown.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node_with_p2p(
            listener,
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
                genesis_override,
                state_dir,
                network_id,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: false,
            },
            P2pConfig {
                listener: p2p_listener,
                peers: vec!["127.0.0.1:1".parse().expect("allowlist addr")],
                rate_limit_per_60s: boole_node::DEFAULT_P2P_RATE_LIMIT_PER_60S,
            },
            Some(shutdown_for_node),
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Boot {
        addr,
        dir,
        shutdown,
        handle,
    }
}

fn stop(boot: Boot) {
    boot.shutdown.notify_one();
    boot.handle.join().expect("server thread").expect("exits");
    let _ = fs::remove_dir_all(&boot.dir);
}

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    let raw = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(raw.as_bytes()).expect("write");
    let mut buf = Vec::new();
    match stream.read_to_end(&mut buf) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::ConnectionReset && !buf.is_empty() => {}
        Err(err) => panic!("read response: {err}"),
    }
    let text = String::from_utf8(buf).expect("utf8 response");
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let (_, body) = text.split_once("\r\n\r\n").expect("body break");
    (status, serde_json::from_str(body).expect("json body"))
}

fn metric_value(addr: SocketAddr, name: &str) -> u64 {
    let mut stream = TcpStream::connect(addr).expect("connect metrics");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    let raw = "GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    stream.write_all(raw.as_bytes()).expect("write");
    let mut buf = Vec::new();
    match stream.read_to_end(&mut buf) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::ConnectionReset && !buf.is_empty() => {}
        Err(err) => panic!("read metrics: {err}"),
    }
    let text = String::from_utf8_lossy(&buf);
    text.lines()
        .find(|line| line.starts_with(name))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn wait_until(what: &str, timeout: Duration, check: impl Fn() -> bool) {
    let deadline = Instant::now() + timeout;
    loop {
        if check() {
            return;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for {what}");
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn hello_with_genesis(genesis_hash: &str) -> Frame {
    Frame::Hello {
        protocol_version: PROTOCOL_VERSION,
        consensus_rule_version: CONSENSUS_RULE_VERSION,
        network_id: "boole-mvp".to_string(),
        genesis_hash: genesis_hash.to_string(),
        head: HeadSummary {
            height: 0,
            c: scenario_genesis_c(),
        },
    }
}

#[test]
#[ignore = "needs-multiprocess"]
fn node_refuses_peer_with_mismatched_genesis() {
    let p2p = TcpListener::bind("127.0.0.1:0").expect("bind p2p");
    let p2p_addr = p2p.local_addr().expect("p2p addr");
    let b = boot("peer-genesis", None, None, None, Some(p2p));

    // N5.2 semantic upgrade: Hello.genesis_hash carries the SPEC hash.
    let (status, doc) = http_get(b.addr, "/status");
    assert_eq!(status, 200, "status: {doc}");
    let spec_hash = doc["genesisSpecHash"]
        .as_str()
        .expect("status must expose genesisSpecHash")
        .to_string();
    assert_ne!(
        spec_hash,
        scenario_genesis_c(),
        "the spec hash must not be the raw chain anchor"
    );

    // A peer presenting the spec hash completes the handshake.
    let transport = TcpTransport::new();
    let mut conn = transport.connect(&p2p_addr).expect("connect");
    transport
        .send_frame(&mut conn, &hello_with_genesis(&spec_hash))
        .expect("send matching hello");
    match transport.recv_frame(&mut conn).expect("hello reply") {
        Frame::Hello { .. } => {}
        other => panic!("expected a Hello reply for the matching spec hash, got {other:?}"),
    }
    drop(conn);

    // A peer presenting the raw anchor (same chain bytes, uncommitted
    // params) is refused — that was exactly the pre-N5.2 hole.
    let mut conn = transport.connect(&p2p_addr).expect("connect 2");
    transport
        .send_frame(&mut conn, &hello_with_genesis(&scenario_genesis_c()))
        .expect("send anchor-only hello");
    match transport.recv_frame(&mut conn) {
        Err(FrameError::ConnectionClosed) | Err(FrameError::Io(_)) => {}
        other => panic!("raw-anchor Hello must be disconnected, got {other:?}"),
    }
    wait_until(
        "the hello mismatch drop to be counted",
        Duration::from_secs(10),
        || metric_value(b.addr, "boole_p2p_ingress_hello_mismatch_drops_total") >= 1,
    );

    stop(b);
}

#[test]
#[ignore = "needs-multiprocess"]
fn boot_refuses_state_dir_with_foreign_genesis() {
    let state_dir = std::env::temp_dir().join(format!(
        "boole-n52-state-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = fs::remove_dir_all(&state_dir);
    fs::create_dir_all(&state_dir).expect("state dir");

    // First boot writes the manifest with the genesis spec hash.
    let a = boot("state-first", Some(state_dir.clone()), None, None, None);
    stop(a);
    let manifest_path = state_dir.join("state.manifest.json");
    let raw = fs::read_to_string(&manifest_path).expect("manifest");
    let mut manifest: Value = serde_json::from_str(&raw).expect("manifest json");
    let recorded = manifest["genesis_hash"]
        .as_str()
        .expect("manifest must record genesis_hash")
        .to_string();
    assert_eq!(recorded.len(), 64, "genesis_hash is a hex32: {recorded}");

    // Tamper: the state dir now claims a foreign genesis. Boot must refuse.
    manifest["genesis_hash"] = json!("11".repeat(32));
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).expect("serialize"),
    )
    .expect("tamper manifest");

    let b = boot("state-foreign", Some(state_dir.clone()), None, None, None);
    b.shutdown.notify_one();
    let joined = b.handle.join().expect("server thread");
    let err = joined.expect_err("boot must refuse a foreign-genesis state dir");
    assert!(
        err.to_string().contains("genesis_hash"),
        "refusal must name the genesis_hash mismatch: {err}"
    );
    let _ = fs::remove_dir_all(&b.dir);
    let _ = fs::remove_dir_all(&state_dir);
}

#[test]
#[ignore = "needs-multiprocess"]
fn named_network_boot_binds_to_compiled_preset() {
    // `boole-dev` is a compiled-in network: its preset matches the standard
    // runtime-smoke scenario, so booting under that name succeeds...
    let ok = boot("preset-ok", None, Some("boole-dev".to_string()), None, None);
    let (status, doc) = http_get(ok.addr, "/status");
    assert_eq!(status, 200, "status: {doc}");
    stop(ok);

    // ...but a node whose effective genesis diverges from the compiled
    // preset (here: a foreign anchor via --genesis) must refuse to boot
    // under that network name instead of silently forking it.
    let bad = boot(
        "preset-diverged",
        None,
        Some("boole-dev".to_string()),
        Some("22".repeat(32)),
        None,
    );
    bad.shutdown.notify_one();
    let joined = bad.handle.join().expect("server thread");
    let err = joined.expect_err("a diverging genesis must refuse to boot under a compiled network");
    assert!(
        err.to_string().contains("boole-dev"),
        "refusal must name the network: {err}"
    );
    let _ = fs::remove_dir_all(&bad.dir);
}

// SC.7 (4th review) — on a NAMED network the admission floor must be the
// consensus floor: a calibration whose MinShareScoreMultiplier diverges
// from the Tier-2 rule constant refuses to boot (the builder commits the
// constant regardless, so the divergent knob could only skew what this
// node admits away from what every replay enforces). Unnamed/fixture
// runs keep the knob as node-local ops config (ADR-0014 Tier-3).
#[test]
fn named_network_boot_fails_fast_on_non_consensus_multiplier() {
    let raw = fs::read_to_string(scenario_path()).expect("scenario fixture");
    let mut doc: Value = serde_json::from_str(&raw).expect("scenario json");
    doc["cfg"]["MinShareScoreMultiplier"] = serde_json::json!(2.0);
    let dir = std::env::temp_dir().join(format!(
        "boole-sc7-multiplier-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("tmp dir");
    let patched = dir.join("scenario.json");
    fs::write(&patched, serde_json::to_string_pretty(&doc).expect("json")).expect("write");

    let bad = boot_with_scenario(
        "multiplier-diverged",
        patched,
        None,
        Some("boole-dev".to_string()),
        None,
        None,
    );
    bad.shutdown.notify_one();
    let joined = bad.handle.join().expect("server thread");
    let err =
        joined.expect_err("a non-consensus multiplier must refuse to boot on a named network");
    assert!(
        err.to_string().contains("MinShareScoreMultiplier"),
        "refusal must name the knob: {err}"
    );
    let _ = fs::remove_dir_all(&bad.dir);
    let _ = fs::remove_dir_all(&dir);
}
