//! N2.1 — `/submit` ownership proof is mandatory by default.
//!
//! A `/submit` envelope without an agent-wallet `session` block carries only a
//! bare prover pk and cannot prove it owns the reward it claims. With the
//! secure production default (`allow_anonymous_submit = false`) such a submit
//! is rejected with `401 unauthenticated_submit` *before* admission, so no
//! credit can accrue. Operators opt into the legacy unauthenticated path
//! explicitly; only then does a session-less submit reach admission.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scenario_path() -> PathBuf {
    repo_root().join("fixtures/protocol/runtime-smoke/v1.json")
}

/// A representative session-less `/submit` body (a bare prover envelope, the
/// pre-wallet shape). The exact admission outcome is irrelevant to N2.1 — what
/// matters is whether the ownership gate lets it reach admission at all.
fn anonymous_submit_body() -> Value {
    let raw = std::fs::read_to_string(scenario_path()).expect("scenario file");
    let scenario: Value = serde_json::from_str(&raw).expect("scenario json");
    scenario["steps"][0]["body"].clone()
}

struct Boot {
    addr: SocketAddr,
    handle: thread::JoinHandle<anyhow::Result<()>>,
}

fn boot(allow_anonymous_submit: bool, max_requests: usize) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-n21-ownership-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
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
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                genesis_override: None,
                state_dir: None,
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Boot { addr, handle }
}

fn http_post(addr: SocketAddr, path: &str, body: &Value) -> (u16, Value) {
    let body_str = serde_json::to_string(body).expect("body json");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
        body_str.len()
    );
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = Vec::new();
    match stream.read_to_end(&mut buf) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::ConnectionReset && !buf.is_empty() => {}
        Err(err) => panic!("read response: {err}"),
    }
    let raw = String::from_utf8(buf).expect("utf8 response");
    let status = raw
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("status not parseable: {raw}"));
    let (_, body_text) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("response missing body: {raw}"));
    let body: Value = serde_json::from_str(body_text)
        .unwrap_or_else(|err| panic!("body not JSON: {err}, raw={body_text}"));
    (status, body)
}

#[test]
fn submit_without_session_or_signature_is_rejected_when_anonymous_disabled() {
    let boot = boot(false, 1);
    let (status, body) = http_post(boot.addr, "/submit", &anonymous_submit_body());
    assert_eq!(
        status, 401,
        "anonymous /submit must be rejected before admission, got {status}: {body}"
    );
    assert_eq!(
        body["reason"], "unauthenticated_submit",
        "rejection must be the typed ownership-proof reason, got {body}"
    );
    // Rejected before admission ⇒ never accepted, so no reward could accrue.
    assert_ne!(body["accepted"], json!(true));
    boot.handle.join().expect("join").expect("server exits");
}

#[test]
fn legacy_anonymous_submit_only_under_explicit_opt_in() {
    let boot = boot(true, 1);
    let (status, body) = http_post(boot.addr, "/submit", &anonymous_submit_body());
    // With the legacy path explicitly enabled, the ownership gate no longer
    // short-circuits: the submit reaches admission. It may still be rejected
    // for unrelated reasons (POW/c), but never as `unauthenticated_submit`.
    assert_ne!(
        status, 401,
        "opt-in anonymous submit must pass the ownership gate, got {status}: {body}"
    );
    assert_ne!(
        body["reason"], "unauthenticated_submit",
        "opt-in anonymous submit must not hit the ownership reject, got {body}"
    );
    boot.handle.join().expect("join").expect("server exits");
}
