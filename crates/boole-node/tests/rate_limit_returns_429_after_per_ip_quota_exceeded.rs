//! P1.7 — per-source-IP HTTP rate limit.
//!
//! Master plan line 188 lists `tower_governor rate limit` as the last
//! remaining P1.7 sub-item: `boole-node` ships no HTTP-layer rate cap
//! today, so a single peer can hammer `/status`, `/bounties`, the
//! `/submit` write path, etc. and starve every other client of file
//! descriptors, connection slots, and the read/write lock on
//! `LocalNodeState`. Without a per-IP ceiling, the production-readiness
//! gate cannot guarantee a noisy-neighbor cannot DoS the node.
//!
//! Contract this test pins:
//!
//!   1. When `LocalNodeConfig.http_rate_limit_per_60s = Some(3)`, the
//!      first three HTTP requests from a single source IP within a 60s
//!      window are admitted (HTTP 200). The fourth same-IP request is
//!      short-circuited at middleware time with HTTP 429 carrying a
//!      typed JSON envelope (`reason: "rate_limited"`, plus `quota`
//!      and `windowMs` so callers can compute a sane back-off).
//!   2. The cap does NOT apply to `/live` or `/ready`. Orchestrators
//!      flooding the readiness probes during incident response must not
//!      be able to self-blackhole the node. So after the /status quota
//!      is exhausted, both probes must continue to return 200.
//!
//! The test uses fresh TCP connections per request (`Connection:
//! close`) so connection-keep-alive does not mask a per-request count.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::Value;

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("write timeout");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf).to_string();
    let status: u16 = raw
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = raw
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .unwrap_or_default();
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    (status, parsed)
}

#[test]
fn http_rate_limit_returns_429_after_per_ip_quota_exceeded() {
    let dir = std::env::temp_dir().join(format!(
        "boole-rate-limit-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (ready_tx, ready_rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_for_thread = block_path.clone();

    // max_requests = 6 = 3 admitted /status + 1 rate-limited /status +
    // 1 /live + 1 /ready, each a Connection: close request.
    let handle = thread::spawn(move || {
        ready_tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path: block_for_thread,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: Some(6),
                operator_signer_pks: vec![],
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                genesis_override: None,
                state_dir: None,
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: Some(3),
                allow_anonymous_submit: true,
            },
        )
    });
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));

    for i in 1..=3 {
        let (status, body) = http_get(addr, "/status");
        assert_eq!(
            status, 200,
            "GET /status #{i} must be admitted while the per-IP quota \
             (3 per 60s) is not yet exhausted. body: {body}"
        );
    }

    let (status, body) = http_get(addr, "/status");
    assert_eq!(
        status, 429,
        "GET /status #4 must be rejected with HTTP 429 once the per-IP \
         quota is exhausted within the 60s window. A quiet 200 here \
         would let a single peer monopolise the node. body: {body}"
    );
    assert_eq!(
        body.get("ok"),
        Some(&Value::Bool(false)),
        "rate-limit envelope must report ok=false, got {body}"
    );
    assert_eq!(
        body.get("reason").and_then(Value::as_str),
        Some("rate_limited"),
        "rate-limit envelope must tag reason=\"rate_limited\" so \
         clients can branch without scraping logs; got {body}"
    );
    assert_eq!(
        body.get("quota").and_then(Value::as_u64),
        Some(3),
        "rate-limit envelope must echo the configured quota so callers \
         can compute back-off; got {body}"
    );
    assert_eq!(
        body.get("windowMs").and_then(Value::as_u64),
        Some(60_000),
        "rate-limit envelope must echo the configured window so callers \
         can compute back-off; got {body}"
    );

    let (live_status, live_body) = http_get(addr, "/live");
    assert_eq!(
        live_status, 200,
        "/live must remain reachable after the /status quota is \
         exhausted; readiness probes can never be rate-limited or an \
         orchestrator's incident-response flood would self-blackhole \
         the node. body: {live_body}"
    );

    let (ready_status, ready_body) = http_get(addr, "/ready");
    assert_eq!(
        ready_status, 200,
        "/ready must remain reachable after the /status quota is \
         exhausted; same reasoning as /live above. body: {ready_body}"
    );

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");

    let _ = std::fs::remove_dir_all(&dir);
}
