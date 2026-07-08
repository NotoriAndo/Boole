//! N3.3 — block announce + linkage-checked ingest.
//!
//! A block committed on node A reaches node B as `BlockAnnounce` (summary);
//! B pulls the body with `GetBlocks` on the same connection (the wire
//! contract moves block bodies only inside `Blocks` frames, ADR-0009 (b))
//! and validates it through the exact strict replay path — linkage, hash
//! re-derivation, evidence-mandatory (N3-pre.1), canonical selection
//! (N3-pre.2), median-time-past (N3-pre.3) and the boundary future-drift
//! guard — BEFORE appending. B converges to A's byte-identical head.
//!
//! Reject paths pinned here: an evidence-less block (strict replay must
//! refuse it — the N3-pre.1 truth boundary applies to gossip ingest), and
//! a peer flooding frames past the per-peer rate limit (ADR-0009 (c)).
//! Fork-choice/reorg stay out of scope (N4): only a block extending the
//! current head by exactly one is ingested.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use boole_core::{block_hash, CONSENSUS_RULE_VERSION};
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
    rate_limit_per_60s: usize,
    allow_anonymous_submit: bool,
) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-n33-block-{tag}-{}-{}",
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
                rate_limit_per_60s,
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

fn share_pool_size(addr: SocketAddr) -> u64 {
    http_get_json(addr, "/status")["sharePoolSize"]
        .as_u64()
        .expect("sharePoolSize")
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

fn hello_frame(genesis: &str, head: HeadSummary) -> Frame {
    Frame::Hello {
        protocol_version: PROTOCOL_VERSION,
        consensus_rule_version: CONSENSUS_RULE_VERSION,
        network_id: "boole-mvp".to_string(),
        genesis_hash: genesis.to_string(),
        head,
    }
}

/// Drive a peer's block ingress by hand over one connection: Hello handshake,
/// announce `block`, then answer the peer's `GetBlocks` pull with the block
/// body. Used to feed a node both a tampered block (which strict replay must
/// reject) and its untampered twin (a control that must be ingested).
fn announce_block_to(peer_p2p: &SocketAddr, genesis: &str, block: &Value) {
    let block_height = block["height"].as_u64().expect("block height");
    let block_c = block["c"].as_str().expect("block c").to_string();
    let transport = TcpTransport::new();
    let mut conn = transport.connect(peer_p2p).expect("connect to peer");
    transport
        .send_frame(
            &mut conn,
            &hello_frame(
                genesis,
                HeadSummary {
                    height: block_height + 1,
                    c: block_c.clone(),
                },
            ),
        )
        .expect("send hello");
    match transport.recv_frame(&mut conn).expect("peer hello reply") {
        Frame::Hello { .. } => {}
        other => panic!("expected peer's hello reply, got {other:?}"),
    }
    transport
        .send_frame(
            &mut conn,
            &Frame::BlockAnnounce {
                height: block_height,
                c: block_c.clone(),
            },
        )
        .expect("send block announce");
    match transport
        .recv_frame(&mut conn)
        .expect("peer must pull the block")
    {
        Frame::GetBlocks { from, to } => {
            assert_eq!(
                (from, to),
                (block_height, block_height),
                "peer must request exactly the announced block"
            );
        }
        other => panic!("expected GetBlocks, got {other:?}"),
    }
    transport
        .send_frame(
            &mut conn,
            &Frame::Blocks {
                blocks: vec![block.clone()],
            },
        )
        .expect("send block body");
}

#[test]
#[ignore = "needs-multiprocess"]
fn block_committed_on_a_is_ingested_and_replayed_identically_on_b() {
    let steps = multiminer_steps();

    let a_p2p = TcpListener::bind("127.0.0.1:0").expect("bind a p2p");
    let b_p2p = TcpListener::bind("127.0.0.1:0").expect("bind b p2p");
    let a_p2p_addr = a_p2p.local_addr().expect("a p2p addr");
    let b_p2p_addr = b_p2p.local_addr().expect("b p2p addr");

    let a = boot_with_p2p("a", Some(a_p2p), vec![b_p2p_addr], DEFAULT_RATE_LIMIT, true);
    let b = boot_with_p2p(
        "b",
        Some(b_p2p),
        vec![a_p2p_addr],
        DEFAULT_RATE_LIMIT,
        false,
    );

    let (status, v0) = http_post(a.addr, "/submit", &submit_envelope(&steps[0]));
    assert_eq!(status, 200, "submit to A: {v0}");
    assert_eq!(v0["accepted"], json!(true), "A must admit the share: {v0}");
    // A block-producing submit carries the committed block object (the
    // `blockProduced:false` key exists only on the NoProposer shape).
    assert!(
        v0["block"].is_object(),
        "step0 must commit a block on A: {v0}"
    );

    // The block must cross to B (BlockAnnounce -> GetBlocks -> Blocks),
    // survive full strict validation, and land as B's new head.
    wait_until("B to ingest A's block", Duration::from_secs(10), || {
        height(b.addr) == 1
    });

    // Byte-identical convergence on the persisted head block.
    let a_latest = http_get_json(a.addr, "/block/latest");
    let b_latest = http_get_json(b.addr, "/block/latest");
    assert_eq!(
        a_latest["block"], b_latest["block"],
        "B's ingested head block must be identical to A's"
    );
    assert_eq!(
        metric_value(b.addr, "boole_p2p_ingress_blocks_ingested_total"),
        1,
        "B must count exactly one ingested block"
    );
    // The N3.2-gossiped share was bound to the genesis head; after the block
    // lands it is either pruned (admitted first) or rejected stale — the
    // pool must not carry a stale share either way.
    assert_eq!(
        share_pool_size(b.addr),
        0,
        "B's pool must hold no stale share after the block ingest"
    );

    stop(a);
    stop(b);
}

#[test]
#[ignore = "needs-multiprocess"]
fn ingress_rejects_evidence_less_block() {
    let b_p2p = TcpListener::bind("127.0.0.1:0").expect("bind b p2p");
    let b_p2p_addr = b_p2p.local_addr().expect("b p2p addr");

    // Loopback is allowlisted, so the reject can only come from validation.
    let b = boot_with_p2p(
        "b-evidence-less",
        Some(b_p2p),
        vec!["127.0.0.1:1".parse().expect("allowlist addr")],
        DEFAULT_RATE_LIMIT,
        false,
    );
    let genesis = scenario_genesis_c();

    // A shape-valid block whose `c` re-derives correctly but that carries NO
    // selected_share_evidence: the strict replay path (N3-pre.1) must refuse
    // it at the gossip trust boundary — evidence-less blocks are a legacy
    // local-replay concession, never a peer-ingest one.
    let share_hash_hex = "11".repeat(32);
    let pk = "bb".repeat(32);
    let mut forged = json!({
        "height": 0,
        "prevC": genesis,
        "c": "",
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
    let parsed: boole_core::PersistedBlock =
        serde_json::from_value(forged.clone()).expect("forged block parses");
    let forged_c = block_hash(&parsed).to_hex();
    forged["c"] = json!(forged_c);

    let transport = TcpTransport::new();
    let mut conn = transport.connect(&b_p2p_addr).expect("connect to B");
    transport
        .send_frame(
            &mut conn,
            &hello_frame(
                &genesis,
                HeadSummary {
                    height: 1,
                    c: forged_c.clone(),
                },
            ),
        )
        .expect("send hello");
    match transport.recv_frame(&mut conn).expect("B hello reply") {
        Frame::Hello { .. } => {}
        other => panic!("expected B's hello reply, got {other:?}"),
    }
    transport
        .send_frame(
            &mut conn,
            &Frame::BlockAnnounce {
                height: 0,
                c: forged_c.clone(),
            },
        )
        .expect("send block announce");
    match transport
        .recv_frame(&mut conn)
        .expect("B must pull the block")
    {
        Frame::GetBlocks { from, to } => {
            assert_eq!(
                (from, to),
                (0, 0),
                "B must request exactly the announced block"
            );
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
        .expect("send forged block");

    wait_until(
        "B to count the rejected block",
        Duration::from_secs(10),
        || metric_value(b.addr, "boole_p2p_ingress_blocks_rejected_total") >= 1,
    );
    assert_eq!(height(b.addr), 0, "the evidence-less block must not land");

    stop(b);
}

/// N4.4 — regression defence for the fork-choice ingress boundary: a peer that
/// forges its block's cumulative-work weight (a bid to win canonical head with
/// a lie) must be refused. A validity-preserving twin of the same block IS
/// ingested, proving the refusal comes from B's strict replay, not a broken
/// transfer. Pins the reject guarantee that N3.3 established so a later change
/// cannot silently start trusting tampered peer blocks.
#[test]
#[ignore = "needs-multiprocess"]
fn ingress_rejects_tampered_peer_block() {
    let steps = multiminer_steps();
    let genesis = scenario_genesis_c();

    // A produces one real, fully-valid block over HTTP. A has no p2p peers, so
    // it never announces the block — the test carries it by hand, reading the
    // exact wire bytes the p2p layer would serve straight from A's block store.
    let a = boot_with_p2p("a-tamper-src", None, vec![], DEFAULT_RATE_LIMIT, true);
    let (status, v0) = http_post(a.addr, "/submit", &submit_envelope(&steps[0]));
    assert_eq!(status, 200, "submit to A: {v0}");
    assert_eq!(v0["accepted"], json!(true), "A must admit the share: {v0}");
    assert!(
        v0["block"].is_object(),
        "step0 must commit a block on A: {v0}"
    );
    let store = fs::read_to_string(a.dir.join("blocks.ndjson")).expect("read A's block store");
    let real_block: Value =
        serde_json::from_str(store.lines().last().expect("A stored at least one block"))
            .expect("parse A's persisted block");
    assert_eq!(
        real_block["height"].as_u64(),
        Some(0),
        "A's committed block is height 0"
    );

    // Tampered twin: A's real block with only difficultyWeight forged far above
    // the value this block's target (tBlock) actually earns — a bid to overstate
    // cumulative work and steal canonical head at the fork-choice boundary. B
    // re-derives the weight from tBlock during strict replay and must reject the
    // inflated claim, leaving its head untouched. (The scenario's near-max
    // tBlock earns weight "1", so the lie has to inflate, not shrink.) B_reject
    // trusts only a dummy peer address (nothing auto-connects); loopback stays
    // allowlisted, so the reject can only come from validation.
    let mut tampered = real_block.clone();
    assert_eq!(
        real_block["difficultyWeight"],
        json!("1"),
        "scenario's near-max tBlock earns weight 1; the forgery must differ from it"
    );
    tampered["difficultyWeight"] = json!("1000000000000");
    let reject_p2p = TcpListener::bind("127.0.0.1:0").expect("bind reject p2p");
    let reject_p2p_addr = reject_p2p.local_addr().expect("reject p2p addr");
    let b_reject = boot_with_p2p(
        "b-tamper-reject",
        Some(reject_p2p),
        vec!["127.0.0.1:1".parse().expect("allowlist addr")],
        DEFAULT_RATE_LIMIT,
        false,
    );
    announce_block_to(&reject_p2p_addr, &genesis, &tampered);
    wait_until(
        "B to count the rejected tampered block",
        Duration::from_secs(10),
        || metric_value(b_reject.addr, "boole_p2p_ingress_blocks_rejected_total") >= 1,
    );
    assert_eq!(height(b_reject.addr), 0, "the tampered block must not land");

    // Control on a fresh node: the untampered block IS accepted over the very
    // same hand-driven path, proving the rejection above was validation — not a
    // malformed frame or a broken transfer. It needs a separate node because
    // the twin shares A's block `c`, which B_reject now treats as already-seen.
    let accept_p2p = TcpListener::bind("127.0.0.1:0").expect("bind accept p2p");
    let accept_p2p_addr = accept_p2p.local_addr().expect("accept p2p addr");
    let b_accept = boot_with_p2p(
        "b-tamper-accept",
        Some(accept_p2p),
        vec!["127.0.0.1:1".parse().expect("allowlist addr")],
        DEFAULT_RATE_LIMIT,
        false,
    );
    announce_block_to(&accept_p2p_addr, &genesis, &real_block);
    wait_until(
        "fresh B to ingest the untampered block",
        Duration::from_secs(10),
        || height(b_accept.addr) == 1,
    );

    stop(a);
    stop(b_reject);
    stop(b_accept);
}

#[test]
#[ignore = "needs-multiprocess"]
fn ingress_rate_limits_flooding_peer() {
    let b_p2p = TcpListener::bind("127.0.0.1:0").expect("bind b p2p");
    let b_p2p_addr = b_p2p.local_addr().expect("b p2p addr");

    // Tight quota so the flood trips fast: 3 frames per 60s window.
    let b = boot_with_p2p(
        "b-rate-limit",
        Some(b_p2p),
        vec!["127.0.0.1:1".parse().expect("allowlist addr")],
        3,
        false,
    );
    let genesis = scenario_genesis_c();

    let transport = TcpTransport::new();
    let mut conn = transport.connect(&b_p2p_addr).expect("connect to B");
    transport
        .send_frame(
            &mut conn,
            &hello_frame(
                &genesis,
                HeadSummary {
                    height: 0,
                    c: genesis.clone(),
                },
            ),
        )
        .expect("send hello");
    match transport.recv_frame(&mut conn).expect("B hello reply") {
        Frame::Hello { .. } => {}
        other => panic!("expected B's hello reply, got {other:?}"),
    }
    // Flood well past the quota. Sends may start failing once B drops the
    // connection — that is the expected enforcement, not a test failure.
    for _ in 0..10 {
        let _ = transport.send_frame(
            &mut conn,
            &Frame::BlockAnnounce {
                height: 99,
                c: "22".repeat(32),
            },
        );
    }

    wait_until(
        "B to count the rate-limited drop",
        Duration::from_secs(10),
        || metric_value(b.addr, "boole_p2p_ingress_rate_limited_drops_total") >= 1,
    );
    assert_eq!(height(b.addr), 0, "no block may land from the flood");

    stop(b);
}
