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
//! Since N3.3 landed, A's committed block may legitimately propagate to B
//! on a parallel connection, so share-crossing is asserted via the
//! monotonic admitted counter, never a pool-size or height snapshot.
//!
//! SC.1-b adds the reward-authorization roundtrip: the signed work.v2
//! envelope must survive egress → wire → ingress → candidate → evidence.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use boole_core::CONSENSUS_RULE_VERSION;
use boole_node::{serve_local_node_with_p2p, LocalNodeConfig, P2pConfig, RuntimeConfig};
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

/// N5.2 — the GenesisSpec hash every node booted from the runtime-smoke
/// scenario advertises in `Hello.genesis_hash` (the spec identity, not the
/// raw chain anchor). Computed exactly the way the node does at boot.
fn scenario_spec_hash() -> String {
    let raw = fs::read_to_string(scenario_path()).expect("scenario fixture");
    let doc: Value = serde_json::from_str(&raw).expect("scenario json");
    let cfg: boole_core::CalibrationReport =
        serde_json::from_value(doc["cfg"].clone()).expect("scenario cfg");
    let config = RuntimeConfig::from_calibration_report(cfg, 60_000).expect("runtime config");
    config
        .genesis_spec("boole-mvp", doc["genesisC"].as_str().expect("genesisC"))
        .hash()
        .to_hex()
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
    boot_with_p2p_sessions(tag, p2p_listener, peers, allow_anonymous_submit, false)
}

/// SC.1-b — same boot, optionally with the session registry + submit-nonce
/// ledger wired in so the node accepts session-bound (signed-work) submits.
fn boot_with_p2p_sessions(
    tag: &str,
    p2p_listener: Option<TcpListener>,
    peers: Vec<SocketAddr>,
    allow_anonymous_submit: bool,
    with_sessions: bool,
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
    let sessions = with_sessions.then(|| dir.join("sessions.ndjson"));
    let nonces = with_sessions.then(|| dir.join("submit-nonces.ndjson"));
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
                session_registry_path: sessions,
                submit_nonce_ledger_path: nonces,
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

    // The share must cross to B via ShareAnnounce and re-enter B's
    // admission path. Wait on the ADMITTED counter, not a pool-size
    // snapshot: A's egress sends the ShareAnnounce and the BlockAnnounce
    // for its own committed block on separate connections that B handles
    // concurrently, so B may legitimately adopt A's block (N3.3
    // announce/pull) and prune the pool entry before a poll observes it.
    // The counter is monotonic and therefore race-free. (The former
    // `height == 0` scope pin dated from pre-N3.3, when block propagation
    // did not exist; it only held by that same timing accident.)
    wait_until(
        "B to count the gossip-admitted share",
        Duration::from_secs(10),
        || metric_value(b.addr, "boole_p2p_ingress_shares_admitted_total") == 1,
    );

    stop(a);
    stop(b);
}

// ------------------------------------------------------------------
// SC.1-b (ADR-0015 (b-1)) — reward authorization must survive gossip.
// Minimal session/signing helpers mirroring submit_session_policy.rs
// (the canonical gate contract lives there; these only produce a valid
// session-bound submit for the roundtrip).
// ------------------------------------------------------------------

const AUTH_AGENT_PK: &str = "3434343434343434343434343434343434343434343434343434343434343434";
const AUTH_RECIPIENT: &str = "5656565656565656565656565656565656565656565656565656565656565656";
const AUTH_ROOT: &str = "7878787878787878787878787878787878787878787878787878787878787878";

fn register_session(addr: SocketAddr, session_pk: &str, fixed_reward_recipient: &str) {
    let owner = boole_core::SigningKeyV2::from_dev_id("n32-roundtrip-owner");
    let payload = json!({
        "schema": "boole.sessions.register.v1",
        "session": {
            "sessionPk": session_pk,
            "ownerPk": owner.pk_hex(),
            "agentPk": AUTH_AGENT_PK,
            "fixedRewardRecipient": fixed_reward_recipient,
            "allowedFamilyRoot": AUTH_ROOT,
            "maxFeePerRequest": "12",
            "activationHeight": 0,
            "expiryHeight": 100,
            "revoked": false,
            "policyHash": AUTH_ROOT,
        },
        "currentHeight": 0,
        "validBefore": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() + 60)
            .unwrap_or(u64::MAX / 2),
        "nonce": format!("reg-{}", rand_suffix()),
    });
    let signed = owner.sign(&payload).expect("sign register payload");
    let envelope = json!({
        "schema": signed.schema,
        "payload": signed.payload,
        "pk": signed.pk,
        "signature": signed.signature,
    });
    let (status, value) = http_post(addr, "/sessions", &envelope);
    assert_eq!(status, 200, "register failed: status={status}, {value}");
}

fn signed_work_session(
    key: &boole_core::SigningKeyV2,
    body: &Value,
    nonce: &str,
    reward_recipient: &str,
) -> Value {
    let payload = json!({
        "schema": "boole.signer.work.v2",
        "route": "/submit",
        "familyId": "boole.protocol-invariant.v01",
        "verifierId": "lean-runner-v01",
        "fee": "0",
        "requestHash": boole_core::canonical_payload_hash_hex(body),
        "nonce": nonce,
        "rewardRecipient": reward_recipient,
        "workPayload": body,
    });
    let signed = key.sign(&payload).expect("sign work payload");
    json!({
        "submittedBy": key.pk_hex(),
        "rewardRecipient": reward_recipient,
        "nonce": nonce,
        "signedWork": {
            "schema": signed.schema,
            "payload": signed.payload,
            "pk": signed.pk,
            "signature": signed.signature,
        }
    })
}

/// SC.1-b — the submitter-signed reward authorization admitted by A must
/// ride `ShareAnnounce` (asserted on the captured wire frame), survive B's
/// re-admission into its candidate pool, and land in the evidence of a
/// block B builds — so a PEER-built block carries the material replay
/// needs to verify reward routing (ADR-0015 (b-1): the authorization
/// survives gossip end-to-end).
#[test]
#[ignore = "needs-multiprocess"]
fn p2p_share_roundtrip_preserves_reward_authorization() {
    let steps = multiminer_steps();

    // The test IS the wire between A and B. On this scenario every accepted
    // submit builds a block instantly, so with a direct A→B link B races
    // between (admit relayed share) and (adopt A's announced block, which
    // prunes the pool) — structurally flaky. Instead the test receives A's
    // egress frames on a relay listener (A's BlockAnnounce goes nowhere)
    // and forwards the captured ShareAnnounce to B, which never sees A's
    // block and deterministically builds its own over the relayed share.
    let relay = TcpListener::bind("127.0.0.1:0").expect("bind relay");
    let relay_addr = relay.local_addr().expect("relay addr");
    let b_p2p = TcpListener::bind("127.0.0.1:0").expect("bind b p2p");
    let b_p2p_addr = b_p2p.local_addr().expect("b p2p addr");

    // A accepts only session-bound submits; B allows one anonymous submit
    // to trigger its own block build over the relayed candidate. B's
    // allowlist covers loopback so the test can connect as its peer.
    let a = boot_with_p2p_sessions("a-auth", None, vec![relay_addr], false, true);
    let b = boot_with_p2p(
        "b-auth",
        Some(b_p2p),
        vec!["127.0.0.1:1".parse().expect("allowlist addr")],
        true,
    );

    let key = boole_core::SigningKeyV2::from_dev_id("n32-roundtrip-auth");
    let session_pk = key.pk_hex();
    register_session(a.addr, &session_pk, AUTH_RECIPIENT);

    // The share mines under the session identity (body.pk == submittedBy,
    // ADR-0015 (b-1) identity chain) and routes its reward to a cold wallet.
    let mut body = steps[0]["body"].clone();
    body["pk"] = json!(session_pk);
    let session = signed_work_session(&key, &body, "n-roundtrip-auth", AUTH_RECIPIENT);
    let signed_work = session["signedWork"].clone();
    let envelope = json!({
        "body": body,
        "session": session,
        "canonTag": steps[0]["canonTag"],
        "ts": steps[0]["ts"],
    });
    let (status, v0) = http_post(a.addr, "/submit", &envelope);
    assert_eq!(status, 200, "submit to A: {v0}");
    assert_eq!(v0["accepted"], json!(true), "A must admit the share: {v0}");

    // Egress half: capture A's ShareAnnounce off the wire. Each egress
    // event dials a fresh connection (Hello exchange first — the dialer
    // validates the reply, so mimic B's identity exactly).
    let transport = TcpTransport::new();
    let our_hello = || Frame::Hello {
        protocol_version: PROTOCOL_VERSION,
        consensus_rule_version: CONSENSUS_RULE_VERSION,
        network_id: "boole-mvp".to_string(),
        genesis_hash: scenario_spec_hash(),
        head: HeadSummary {
            height: 0,
            c: scenario_genesis_c(),
        },
    };
    let captured = {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut captured = None;
        while captured.is_none() {
            assert!(
                Instant::now() < deadline,
                "timed out capturing A's ShareAnnounce on the relay listener"
            );
            let (stream, _) = relay.accept().expect("accept A's egress dial");
            let mut conn = TcpTransport::conn_from_stream(stream).expect("relay conn");
            match transport.recv_frame(&mut conn) {
                Ok(Frame::Hello { .. }) => {}
                other => panic!("A's egress must open with a Hello, got {other:?}"),
            }
            transport
                .send_frame(&mut conn, &our_hello())
                .expect("reply hello to A");
            // One event per connection: keep a ShareAnnounce, drop anything
            // else (e.g. the BlockAnnounce for A's own block).
            if let Ok(Frame::ShareAnnounce { submission }) = transport.recv_frame(&mut conn) {
                captured = Some(submission);
            }
        }
        captured.expect("captured submission")
    };
    let auth_on_wire = captured
        .get("signedWork")
        .cloned()
        .expect("A's egress must attach the signed authorization to the wire submission");
    assert_eq!(auth_on_wire["pk"], signed_work["pk"]);
    assert_eq!(auth_on_wire["signature"], signed_work["signature"]);
    assert_eq!(auth_on_wire["payload"], signed_work["payload"]);

    // Ingress half: forward the captured frame to B as an allowlisted peer.
    let mut conn = transport.connect(&b_p2p_addr).expect("connect to B");
    transport
        .send_frame(&mut conn, &our_hello())
        .expect("hello to B");
    match transport.recv_frame(&mut conn) {
        Ok(Frame::Hello { .. }) => {}
        other => panic!("B must reply with its own Hello, got {other:?}"),
    }
    transport
        .send_frame(
            &mut conn,
            &Frame::ShareAnnounce {
                submission: captured,
            },
        )
        .expect("forward captured share to B");

    wait_until(
        "relayed authorized share to enter B's pool",
        Duration::from_secs(10),
        || share_pool_size(b.addr) >= 1,
    );

    // Trigger a block build on B: its selection draws on the pool holding
    // the relayed (authorized) share plus this anonymous one (K_max = 4).
    let (status, v1) = http_post(b.addr, "/submit", &submit_envelope(&steps[1]));
    assert_eq!(status, 200, "submit to B: {v1}");
    assert_eq!(v1["accepted"], json!(true), "B must admit and build: {v1}");

    // The consensus artifact is the PERSISTED block: the relayed share's
    // evidence must carry the original signed envelope, and the committed
    // reward routing must be the signed recipient.
    let blocks_raw = fs::read_to_string(b.dir.join("blocks.ndjson")).expect("B block store exists");
    let block: Value = serde_json::from_str(blocks_raw.lines().next().expect("B built one block"))
        .expect("B block json");
    let evidence = block["selectedShareEvidence"]
        .as_array()
        .expect("evidence array");
    let idx = evidence
        .iter()
        .position(|entry| entry["pk"] == json!(session_pk))
        .expect("relayed authorized share must be selected into B's block");
    let auth = &evidence[idx]["signedWork"];
    assert_eq!(
        auth["pk"], signed_work["pk"],
        "gossip must preserve the reward authorization into peer evidence: {block}"
    );
    assert_eq!(auth["signature"], signed_work["signature"]);
    assert_eq!(auth["payload"], signed_work["payload"]);
    assert_eq!(
        block["selectedShareRewardPks"][idx],
        json!(AUTH_RECIPIENT),
        "B must commit the signed reward recipient for the relayed share: {block}"
    );

    stop(a);
    stop(b);
}

/// SC.1-b RED — a relayed share carrying an INVALID authorization is a
/// typed ingress reject, never a silent pool entry: a peer must not
/// launder an envelope the submitter never signed into its candidate set.
#[test]
#[ignore = "needs-multiprocess"]
fn ingress_rejects_share_with_invalid_authorization() {
    let steps = multiminer_steps();

    let b_p2p = TcpListener::bind("127.0.0.1:0").expect("bind b p2p");
    let b_p2p_addr = b_p2p.local_addr().expect("b p2p addr");

    // Loopback is allowlisted; the Hello below matches B's identity, so
    // the only rejection surface left is the share authorization check.
    let b = boot_with_p2p(
        "b-invalid-auth",
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
                consensus_rule_version: CONSENSUS_RULE_VERSION,
                network_id: "boole-mvp".to_string(),
                genesis_hash: scenario_spec_hash(),
                head: HeadSummary {
                    height: 0,
                    c: scenario_genesis_c(),
                },
            },
        )
        .expect("send matching hello");
    match transport.recv_frame(&mut conn) {
        Ok(Frame::Hello { .. }) => {}
        other => panic!("B must reply with its own Hello, got {other:?}"),
    }

    // Well-formed hex shapes but a signature no key ever produced: the
    // envelope-intrinsic verification must fail, and the share must not
    // enter the pool.
    let submission = json!({
        "body": steps[2]["body"].clone(),
        "canonTag": steps[2]["canonTag"].clone(),
        "ts": steps[2]["ts"].clone(),
        "signedWork": {
            "schema": "boole.signed.v1",
            "payload": {
                "schema": "boole.signer.work.v2",
                "route": "/submit",
                "fee": "0",
                "requestHash": "11".repeat(32),
                "nonce": "n-forged",
                "rewardRecipient": "22".repeat(32),
                "workPayload": steps[2]["body"].clone(),
            },
            "pk": steps[2]["body"]["pk"].clone(),
            "signature": "33".repeat(64),
        },
    });
    transport
        .send_frame(&mut conn, &Frame::ShareAnnounce { submission })
        .expect("send share with forged authorization");

    {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if metric_value(b.addr, "boole_p2p_ingress_shares_rejected_total") >= 1 {
                break;
            }
            if Instant::now() >= deadline {
                panic!(
                    "timed out waiting for B to count the rejected share; \
                     admitted={} rejected={} malformed={} pool={}",
                    metric_value(b.addr, "boole_p2p_ingress_shares_admitted_total"),
                    metric_value(b.addr, "boole_p2p_ingress_shares_rejected_total"),
                    metric_value(b.addr, "boole_p2p_ingress_malformed_frame_drops_total"),
                    share_pool_size(b.addr),
                );
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    assert_eq!(
        share_pool_size(b.addr),
        0,
        "a share with a forged authorization must never reach B's pool"
    );

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
                consensus_rule_version: CONSENSUS_RULE_VERSION,
                network_id: "not-the-b-network".to_string(),
                genesis_hash: scenario_spec_hash(),
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

#[test]
#[ignore = "needs-multiprocess"]
fn hello_mismatched_consensus_rule_version_is_dropped() {
    let b_p2p = TcpListener::bind("127.0.0.1:0").expect("bind b p2p");
    let b_p2p_addr = b_p2p.local_addr().expect("b p2p addr");

    // Loopback is allowlisted and every other Hello field matches B's
    // identity, so the drop this test observes can only come from the
    // consensus_rule_version check (ADR-0014 (b): a peer running a
    // different rule set must be disconnected before any frame is
    // processed, or the nodes silently fork).
    let b = boot_with_p2p(
        "b-rule-version-mismatch",
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
                consensus_rule_version: CONSENSUS_RULE_VERSION + 1,
                network_id: "boole-mvp".to_string(),
                genesis_hash: scenario_spec_hash(),
                head: HeadSummary {
                    height: 0,
                    c: scenario_genesis_c(),
                },
            },
        )
        .expect("send mismatched hello");

    // Same typed-disconnect posture as the network_id mismatch above.
    match transport.recv_frame(&mut conn) {
        Err(FrameError::ConnectionClosed) | Err(FrameError::Io(_)) => {}
        other => panic!("B must disconnect after a mismatched rule version, got {other:?}"),
    }
    wait_until(
        "B to count the hello mismatch",
        Duration::from_secs(10),
        || metric_value(b.addr, "boole_p2p_ingress_hello_mismatch_drops_total") >= 1,
    );

    assert_eq!(
        share_pool_size(b.addr),
        0,
        "no share may enter B's pool after a mismatched Hello"
    );

    stop(b);
}
