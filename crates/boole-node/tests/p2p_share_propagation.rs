//! N3.2 — share gossip: egress announce + ingress re-admit.
//!
//! A share submitted to node A must appear in node B's candidate pool via a
//! `ShareAnnounce` frame, with B re-admitting it through the exact local
//! admission path (`admit_parsed_submission_typed`) — no second validation
//! policy (ADR-0009 (e)). The reject paths are pinned too: an inbound
//! connection from a non-allowlisted address is dropped at accept, and a
//! `Hello` carrying a mismatched `network_id` is a typed disconnect before
//! any frame is processed (ADR-0009 (d)/(e)).
//!
//! Block propagation is explicitly out of scope (N3.3): B's height must stay
//! at genesis even after the gossiped share lands in its pool.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

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

/// The genesis `c` pinned by `fixtures/protocol/runtime-smoke/v1.json`; the
/// `Hello.genesis_hash` both nodes exchange must equal it.
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

/// Boot an in-process node with the N3.2 gossip surface. `p2p_listener`
/// pre-binds the gossip port (None = egress-only node); `peers` is the
/// static peer set that doubles as the inbound IP allowlist (ADR-0009 (d)).
fn boot_with_p2p(
    tag: &str,
    p2p_listener: Option<TcpListener>,
    peers: Vec<SocketAddr>,
    allow_anonymous_submit: bool,
) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-n32-gossip-{tag}-{}-{}",
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
    let dedup = dir.join("proof-dedup.ndjson");
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
                proof_dedup_ledger_path: Some(dedup),
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

fn share_pool_size(addr: SocketAddr) -> u64 {
    http_get_json(addr, "/status")["sharePoolSize"]
        .as_u64()
        .expect("sharePoolSize")
}

fn height(addr: SocketAddr) -> u64 {
    http_get_json(addr, "/status")["height"]
        .as_u64()
        .expect("height")
}

/// Scrape one counter value from `/metrics` (Prometheus text format).
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
fn share_submitted_to_a_appears_in_b_candidate_pool() {
    let steps = multiminer_steps();

    // Pre-bind both gossip listeners so each node can name the other as a
    // static peer before boot (ADR-0009 (d) static peer set).
    let a_p2p = TcpListener::bind("127.0.0.1:0").expect("bind a p2p");
    let b_p2p = TcpListener::bind("127.0.0.1:0").expect("bind b p2p");
    let a_p2p_addr = a_p2p.local_addr().expect("a p2p addr");
    let b_p2p_addr = b_p2p.local_addr().expect("b p2p addr");

    let a = boot_with_p2p("a", Some(a_p2p), vec![b_p2p_addr], true);
    // B never opens its HTTP submit surface to anonymous callers: gossip
    // ingress bypasses the HTTP session gate by design (the gate is an HTTP
    // surface policy; admission is the consensus-level validation).
    let b = boot_with_p2p("b", Some(b_p2p), vec![a_p2p_addr], false);

    let (status, v0) = http_post(a.addr, "/submit", &submit_envelope(&steps[0]));
    assert_eq!(status, 200, "submit to A: {v0}");
    assert_eq!(v0["accepted"], json!(true), "A must admit the share: {v0}");

    // The share must cross to B via ShareAnnounce and re-enter B's pool
    // through the same admission path.
    wait_until(
        "share to appear in B's candidate pool",
        Duration::from_secs(10),
        || share_pool_size(b.addr) == 1,
    );

    // Scope pin: share gossip only. Block propagation is N3.3, so B's chain
    // must still be at genesis height even though A committed a block.
    assert_eq!(
        height(b.addr),
        0,
        "B must not gain a block from N3.2 gossip"
    );
    assert_eq!(
        metric_value(b.addr, "boole_p2p_ingress_shares_admitted_total"),
        1,
        "B must count exactly one gossip-admitted share"
    );

    stop(a);
    stop(b);
}

#[test]
#[ignore = "needs-multiprocess"]
fn ingress_drops_share_from_non_allowlisted_peer() {
    let steps = multiminer_steps();

    let b_p2p = TcpListener::bind("127.0.0.1:0").expect("bind b p2p");
    let b_p2p_addr = b_p2p.local_addr().expect("b p2p addr");

    // B's static peer set is EMPTY: every inbound gossip connection is
    // outside the allowlist and must be dropped at accept (ADR-0009 (d)).
    let b = boot_with_p2p("b-empty-allowlist", Some(b_p2p), vec![], false);
    let a = boot_with_p2p("a-not-allowlisted", None, vec![b_p2p_addr], true);

    let (status, v0) = http_post(a.addr, "/submit", &submit_envelope(&steps[0]));
    assert_eq!(status, 200, "submit to A: {v0}");
    assert_eq!(v0["accepted"], json!(true), "A must admit the share: {v0}");

    // Positive signal first (a bare sleep would race the egress thread):
    // B must COUNT the dropped connection, then its pool must still be empty.
    wait_until(
        "B to count the non-allowlisted drop",
        Duration::from_secs(10),
        || metric_value(b.addr, "boole_p2p_ingress_not_allowlisted_drops_total") >= 1,
    );
    assert_eq!(
        share_pool_size(b.addr),
        0,
        "a non-allowlisted peer's share must never reach B's pool"
    );

    stop(a);
    stop(b);
}

#[test]
#[ignore = "needs-multiprocess"]
fn ingress_disconnects_on_network_id_mismatch_hello() {
    let b_p2p = TcpListener::bind("127.0.0.1:0").expect("bind b p2p");
    let b_p2p_addr = b_p2p.local_addr().expect("b p2p addr");

    // Loopback is allowlisted (the fake peer below connects from 127.0.0.1),
    // so the drop this test observes can only come from the Hello check.
    let b = boot_with_p2p(
        "b-hello-mismatch",
        Some(b_p2p),
        vec!["127.0.0.1:1".parse().expect("allowlist addr")],
        false,
    );

    let transport = TcpTransport::new();
    let mut conn = transport.connect(&b_p2p_addr).expect("connect to B");
    transport
        .send_frame(
            &mut conn,
            &Frame::Hello {
                protocol_version: PROTOCOL_VERSION,
                network_id: "not-the-b-network".to_string(),
                genesis_hash: scenario_genesis_c(),
                head: HeadSummary {
                    height: 0,
                    c: scenario_genesis_c(),
                },
            },
        )
        .expect("send mismatched hello");

    // ADR-0009 (e): typed disconnect after the mismatched Hello — B must
    // close without replying, and count the drop.
    match transport.recv_frame(&mut conn) {
        Err(FrameError::ConnectionClosed) | Err(FrameError::Io(_)) => {}
        other => panic!("B must disconnect after a mismatched Hello, got {other:?}"),
    }
    wait_until(
        "B to count the hello mismatch",
        Duration::from_secs(10),
        || metric_value(b.addr, "boole_p2p_ingress_hello_mismatch_drops_total") >= 1,
    );

    // The share never had a path in: pool stays empty.
    assert_eq!(
        share_pool_size(b.addr),
        0,
        "no share may enter B's pool after a mismatched Hello"
    );

    stop(b);
}
