//! P1.7 — streaming-aware request body cap.
//!
//! `body_cap_middleware` rejects requests whose `Content-Length` header
//! advertises more than `MAX_HTTP_BODY_BYTES`. HTTP/1.1 chunked transfer
//! encoding has no `Content-Length` to inspect at middleware time, so a
//! hand-rolled chunked POST can bypass the header check and stream an
//! arbitrary number of bytes into the handler's extractor, defeating
//! the per-request memory cap that the middleware was meant to enforce.
//!
//! Contract: an HTTP/1.1 POST with `Transfer-Encoding: chunked` whose
//! aggregate chunk payload exceeds `MAX_HTTP_BODY_BYTES` must be rejected
//! with HTTP 413 before the handler observes the body. A header-only
//! middleware cannot satisfy this contract — only a stream-counting body
//! limit can.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn http_post_chunked(addr: SocketAddr, path: &str, body: &[u8]) -> u16 {
    let chunk_header = format!("{:x}\r\n", body.len());
    let mut request = Vec::new();
    request.extend_from_slice(format!("POST {path} HTTP/1.1\r\n").as_bytes());
    request.extend_from_slice(b"Host: localhost\r\n");
    request.extend_from_slice(b"Transfer-Encoding: chunked\r\n");
    request.extend_from_slice(b"Content-Type: application/json\r\n");
    request.extend_from_slice(b"Connection: close\r\n\r\n");
    request.extend_from_slice(chunk_header.as_bytes());
    request.extend_from_slice(body);
    request.extend_from_slice(b"\r\n0\r\n\r\n");

    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("write timeout");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    let _ = stream.write_all(&request);
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf).to_string();
    raw.lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

#[test]
fn chunked_post_exceeding_body_cap_returns_413() {
    let dir = std::env::temp_dir().join(format!(
        "boole-node-body-cap-stream-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (ready_tx, ready_rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path = dir.join("blocks.ndjson");

    let handle = thread::spawn(move || {
        ready_tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: Some(1),
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
                http_rate_limit_per_60s: None,
            },
        )
    });
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));

    let oversized = vec![b'a'; 1_536_000];
    let status = http_post_chunked(addr, "/sessions", &oversized);
    assert_eq!(
        status, 413,
        "chunked POST exceeding MAX_HTTP_BODY_BYTES must be rejected with 413; \
         the header-only Content-Length middleware misses chunked-encoded bodies."
    );

    handle
        .join()
        .expect("server thread joined")
        .expect("server exits cleanly");
    let _ = std::fs::remove_dir_all(&dir);
}
