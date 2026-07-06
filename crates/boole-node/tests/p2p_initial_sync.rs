//! N3.4 — initial sync via `GetBlocks`/`Blocks`.
//!
//! A fresh (empty-store) node learns a peer's head from the `Hello`
//! exchange, downloads the missing range with `GetBlocks` (paginated by
//! the wire contract's 256-block range cap) and replays each block
//! through the exact N3.3 verify-then-append path — strict replay
//! validation per block, no second policy — until its head is identical
//! to the peer's. The serving side answers `GetBlocks` from its own
//! block cache.
//!
//! Reject path pinned here: a peer serving a tampered (evidence-less)
//! chain during sync is refused block-by-block — the fresh node stays at
//! its last valid height instead of adopting the forged chain.
//!
//! Non-goals (spec): competing-chain selection (N4), parallel /
//! headers-first sync optimizations.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use boole_core::{block_hash, Hex32};
use boole_node::{serve_local_node_with_p2p, LocalNodeConfig, P2pConfig};
use boole_p2p::{Frame, HeadSummary, TcpTransport, Transport, PROTOCOL_VERSION};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};
use tokio::sync::Notify;

const DEFAULT_RATE_LIMIT: usize = 600;

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

fn multiminer_steps() -> Vec<Value> {
    let raw =
        fs::read_to_string(repo_root().join("fixtures/protocol/runtime-smoke/multiminer.v1.json"))
            .expect("multiminer fixture");
    let doc: Value = serde_json::from_str(&raw).expect("multiminer json");
    doc["steps"].as_array().expect("steps array").clone()
}

fn submit_envelope(step: &Value) -> Value {
    json!({
        "body": step["body"].clone(),
        "canonTag": step["canonTag"].clone(),
        "ts": step["ts"].clone(),
    })
}

struct Boot {
    addr: SocketAddr,
    dir: PathBuf,
    shutdown: Arc<Notify>,
    handle: thread::JoinHandle<anyhow::Result<()>>,
}

fn boot_with_p2p(
    tag: &str,
    p2p_listener: Option<TcpListener>,
    peers: Vec<SocketAddr>,
    allow_anonymous_submit: bool,
) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-n34-sync-{tag}-{}-{}",
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
    let scenario = scenario_path();
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
                // None: the admission-layer dedup cache is orthogonal to
                // what this test pins (sync convergence); dedup behavior
                // has its own tests (no_duplicate_proof_credit) and its
                // consensus-level rule its own suite (consensus_proof_dedup).
                proof_dedup_ledger_path: None,
                max_requests: None,
                genesis_override: None,
                state_dir: None,
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit,
            },
            P2pConfig {
                listener: p2p_listener,
                peers,
                rate_limit_per_60s: DEFAULT_RATE_LIMIT,
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

fn http_request(addr: SocketAddr, raw: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
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
        .unwrap_or_else(|| panic!("status not parseable: {text}"));
    let (_, body_text) = text
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("response missing body: {text}"));
    (status, body_text.to_string())
}

fn http_post(addr: SocketAddr, path: &str, body: &Value) -> (u16, Value) {
    let body_str = serde_json::to_string(body).expect("body json");
    let raw = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
        body_str.len()
    );
    let (status, text) = http_request(addr, &raw);
    let value: Value = serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("body not JSON: {err}, raw={text}"));
    (status, value)
}

fn http_get_json(addr: SocketAddr, path: &str) -> Value {
    let raw = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let (_, text) = http_request(addr, &raw);
    serde_json::from_str(&text).unwrap_or_else(|err| panic!("body not JSON: {err}, raw={text}"))
}

fn height(addr: SocketAddr) -> u64 {
    http_get_json(addr, "/status")["height"]
        .as_u64()
        .expect("height")
}

fn metric_value(addr: SocketAddr, name: &str) -> u64 {
    let raw = "GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let (_, text) = http_request(addr, raw);
    for line in text.lines() {
        if let Some(value) = line.strip_prefix(name).and_then(|r| r.strip_prefix(' ')) {
            if let Ok(v) = value.trim().parse::<u64>() {
                return v;
            }
        }
    }
    panic!("metric {name} not found in:\n{text}");
}

fn wait_until(what: &str, timeout: Duration, mut check: impl FnMut() -> bool) {
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

#[test]
#[ignore = "needs-multiprocess"]
fn fresh_node_downloads_and_replays_chain_to_identical_head() {
    let steps = multiminer_steps();

    let a_p2p = TcpListener::bind("127.0.0.1:0").expect("bind a p2p");
    let a_p2p_addr = a_p2p.local_addr().expect("a p2p addr");

    // A's only configured peer is a dead loopback address: its announce
    // egress can never deliver anything (each attempt fails, counted),
    // while the entry still puts 127.0.0.1 on A's inbound allowlist so
    // B's sync dials are accepted. B runs no ingress listener at all —
    // the ONLY way B can converge is the N3.4 initial-sync pull.
    let a = boot_with_p2p(
        "a",
        Some(a_p2p),
        vec!["127.0.0.1:1".parse().expect("dead peer addr")],
        true,
    );
    let (s0, v0) = http_post(a.addr, "/submit", &submit_envelope(&steps[0]));
    assert_eq!(s0, 200, "step0 to A: {v0}");
    assert!(v0["block"].is_object(), "step0 must commit on A: {v0}");
    let head = v0["c"].as_str().expect("head after step0").to_string();
    let mut step1 = submit_envelope(&steps[1]);
    step1["body"]["c"] = json!(head);
    let (s1, v1) = http_post(a.addr, "/submit", &step1);
    assert_eq!(s1, 200, "step1 to A: {v1}");
    assert!(v1["block"].is_object(), "step1 must commit on A: {v1}");
    assert_eq!(height(a.addr), 2, "A must hold a 2-block chain");

    // Fresh B boots with an empty store and A as its only peer.
    let b = boot_with_p2p("b-fresh", None, vec![a_p2p_addr], false);

    wait_until(
        "fresh B to sync A's chain to the identical head",
        Duration::from_secs(15),
        || height(b.addr) == 2,
    );

    // Byte-identical convergence, block by block.
    for h in 0..2u64 {
        let a_block = http_get_json(a.addr, &format!("/block/{h}"));
        let b_block = http_get_json(b.addr, &format!("/block/{h}"));
        assert_eq!(
            a_block["block"], b_block["block"],
            "block {h} must be identical on both nodes"
        );
    }
    assert_eq!(
        metric_value(b.addr, "boole_p2p_sync_blocks_applied_total"),
        2,
        "B must count exactly the two sync-applied blocks"
    );
    assert!(
        metric_value(a.addr, "boole_p2p_ingress_get_blocks_served_total") >= 1,
        "A must count the served GetBlocks request"
    );

    stop(a);
    stop(b);
}

#[test]
#[ignore = "needs-multiprocess"]
fn sync_rejects_tampered_chain_from_peer() {
    // A hand-rolled "peer" that serves a forged, evidence-less block during
    // the sync exchange. The fresh node must refuse it (strict replay —
    // N3-pre.1) and keep its genesis head instead of adopting the chain.
    let fake_listener = TcpListener::bind("127.0.0.1:0").expect("bind fake peer");
    let fake_addr = fake_listener.local_addr().expect("fake addr");
    let genesis = scenario_genesis_c();

    let prev = Hex32::from_hex(&genesis).expect("genesis hex");
    let share_hash_hex = "11".repeat(32);
    let share_hash = Hex32::from_hex(&share_hash_hex).expect("share hash hex");
    let forged_c = block_hash(&prev, &[share_hash]).to_hex();
    let pk = "bb".repeat(32);
    let forged = json!({
        "height": 0,
        "prevC": genesis,
        "c": forged_c,
        "proposerPk": pk,
        "selectedShareHashes": [share_hash_hex],
        "selectedSharePks": [pk],
        "minShareScore": "0x1",
        "kmaxApplied": 1,
        "difficultyEpoch": 0,
        "tBlock": format!("0x{}{}", "f".repeat(63), "e"),
        "tShare": format!("0x{}", "f".repeat(64)),
        "difficultyWeight": "1",
        "droppedBelowMinScore": 0,
        "droppedKernelReject": 0,
        "truncatedByKmax": 0,
        "ts": 1_700_000_000_123u64,
    });

    let genesis_for_peer = genesis.clone();
    let fake_peer = thread::spawn(move || {
        let transport = TcpTransport::new();
        // Serve exactly one sync exchange, then exit; later poll rounds
        // simply fail to connect (a normal, counted sync failure).
        let (stream, _) = fake_listener.accept().expect("accept sync dial");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("read timeout");
        let mut conn = TcpTransport::conn_from_stream(stream).expect("conn");
        match transport.recv_frame(&mut conn).expect("dialer hello") {
            Frame::Hello { .. } => {}
            other => panic!("expected the dialer's Hello, got {other:?}"),
        }
        transport
            .send_frame(
                &mut conn,
                &Frame::Hello {
                    protocol_version: PROTOCOL_VERSION,
                    network_id: "boole-mvp".to_string(),
                    genesis_hash: genesis_for_peer.clone(),
                    head: HeadSummary {
                        height: 1,
                        c: forged_c.clone(),
                    },
                },
            )
            .expect("send fake hello");
        match transport.recv_frame(&mut conn).expect("sync GetBlocks") {
            Frame::GetBlocks { from, to } => {
                assert_eq!((from, to), (0, 0), "sync must request the missing range");
            }
            other => panic!("expected GetBlocks, got {other:?}"),
        }
        transport
            .send_frame(
                &mut conn,
                &Frame::Blocks {
                    blocks: vec![forged],
                },
            )
            .expect("send forged chain");
    });

    let b = boot_with_p2p("b-tampered", None, vec![fake_addr], false);

    wait_until(
        "B to reject the tampered sync chain",
        Duration::from_secs(15),
        || metric_value(b.addr, "boole_p2p_ingress_blocks_rejected_total") >= 1,
    );
    assert_eq!(height(b.addr), 0, "the forged chain must not be adopted");
    assert_eq!(
        metric_value(b.addr, "boole_p2p_sync_blocks_applied_total"),
        0,
        "no forged block may count as sync-applied"
    );

    fake_peer.join().expect("fake peer thread");
    stop(b);
}
