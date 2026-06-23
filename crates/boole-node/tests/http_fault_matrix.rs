//! P1.7 — route-specific timeout/layer matrix.
//!
//! The bounty-proof route carries a larger body (Lean source + POFP
//! envelope + signature) and runs the Lean verifier, so it gets a higher
//! body cap (8 MiB) and a longer request timeout (90 s) than every other
//! route (1 MiB / 30 s). These tests pin the matrix constants and prove
//! the per-route body cap actually differentiates at the streaming
//! extractor (the chunked-transfer path the global `DefaultBodyLimit`
//! guards), which is exactly where a route-layer override has to win over
//! the global default for the proof route to admit a large proof.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::{
    serve_local_node, LocalNodeConfig, DEFAULT_ROUTE_TIMEOUT, MAX_HTTP_BODY_BYTES,
    PROOF_ROUTE_BODY_BYTES, PROOF_ROUTE_TIMEOUT,
};
use boole_testkit::rand_suffix;

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

/// Hand-rolled HTTP/1.1 chunked POST so the body has no `Content-Length`
/// header — this exercises the streaming `DefaultBodyLimit` extractor path
/// rather than the header-only middleware. Returns the response status.
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
        .set_write_timeout(Some(Duration::from_secs(15)))
        .expect("write timeout");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .expect("read timeout");
    let _ = stream.write_all(&request);
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf)
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

struct Booted {
    addr: SocketAddr,
    handle: thread::JoinHandle<()>,
    dir: PathBuf,
}

impl Booted {
    fn finish(self) {
        self.handle.join().expect("server thread joined");
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `max_requests` must equal the literal connection count each test makes
/// (one chunked POST = one `Connection: close` connection), or the
/// graceful-shutdown counter never reaches its target and `join()` hangs.
fn boot(tag: &str, max_requests: usize) -> Booted {
    let dir = std::env::temp_dir().join(format!(
        "boole-fault-matrix-{tag}-{}-{}",
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
                max_requests: Some(max_requests),
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
                allow_anonymous_submit: true,
            },
        )
        .expect("server exits cleanly");
    });
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Booted { addr, handle, dir }
}

#[test]
fn fault_matrix_constants_match_masterplan() {
    // Bind to locals so the relationship assertions below are not flagged
    // as constant-valued (clippy::assertions_on_constants); the semantic
    // invariant (proof route admits more / waits longer than the default)
    // is the point of the matrix.
    let default_cap = MAX_HTTP_BODY_BYTES;
    let proof_cap = PROOF_ROUTE_BODY_BYTES;
    let default_timeout = DEFAULT_ROUTE_TIMEOUT;
    let proof_timeout = PROOF_ROUTE_TIMEOUT;

    assert_eq!(default_cap, 1_048_576, "default body cap = 1 MiB");
    assert_eq!(proof_cap, 8 * 1_048_576, "proof body cap = 8 MiB");
    assert!(
        proof_cap > default_cap,
        "the proof route must admit a strictly larger body than the default"
    );
    assert_eq!(
        default_timeout,
        Duration::from_secs(30),
        "default route timeout = 30 s"
    );
    assert_eq!(
        proof_timeout,
        Duration::from_secs(90),
        "proof route timeout = 90 s"
    );
    assert!(
        proof_timeout > default_timeout,
        "Lean verification needs a longer budget than a cheap read"
    );
}

#[test]
fn default_route_rejects_chunked_body_over_default_cap() {
    let node = boot("default-cap", 1);
    let oversized = vec![b'a'; MAX_HTTP_BODY_BYTES + 512_000];
    let status = http_post_chunked(node.addr, "/submit", &oversized);
    assert_eq!(
        status, 413,
        "a chunked body above the 1 MiB default cap must be rejected with 413"
    );
    node.finish();
}

#[test]
fn proof_route_admits_chunked_body_above_default_cap() {
    let node = boot("proof-admits", 1);
    // 2 MiB: above the 1 MiB default, below the 8 MiB proof cap. The body
    // cap must NOT reject it — the handler fails later on the bogus
    // envelope, but never with 413. This proves the per-route 8 MiB
    // `DefaultBodyLimit` override wins over the global 1 MiB default.
    let body = vec![b'a'; 2 * 1_048_576];
    let status = http_post_chunked(node.addr, "/bounties/test-id/proof", &body);
    assert_ne!(
        status, 413,
        "the proof route's 8 MiB cap must admit a 2 MiB body (got 413 — the \
         global 1 MiB default leaked through to the proof route)"
    );
    node.finish();
}

#[test]
fn proof_route_rejects_chunked_body_over_proof_cap() {
    let node = boot("proof-cap", 1);
    let oversized = vec![b'a'; PROOF_ROUTE_BODY_BYTES + 1_048_576]; // 9 MiB
    let status = http_post_chunked(node.addr, "/bounties/test-id/proof", &oversized);
    assert_eq!(
        status, 413,
        "a chunked body above the 8 MiB proof cap must be rejected with 413"
    );
    node.finish();
}
