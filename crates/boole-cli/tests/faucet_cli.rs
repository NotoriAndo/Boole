//! P2.10 — `boole faucet claim` subcommand + `--network` preset.
//!
//! The faucet client is a thin HTTP POST to a configurable faucet URL.
//! Operators select a network with `--network testnet|dev|mainnet`; the
//! preset both resolves a default faucet URL (overridable via
//! `--faucet-url`) AND stamps the canonical network_id into the request
//! body so a faucet operator can refuse cross-network claims.
//!
//! These tests pin:
//!
//!   1. `--network testnet --faucet-url <url>` POSTs the address and
//!      the canonical `"boole-testnet"` network_id to the supplied URL.
//!      A successful response is forwarded verbatim under the unified
//!      P2.5 envelope when `--json` is set.
//!   2. `--network mainnet` with no `--faucet-url` errors with a clear
//!      "no default faucet URL" message — mainnet does not ship with a
//!      canonical faucet, so silent localhost fallback would be a foot
//!      gun.
//!   3. `--network dev` defaults to `http://127.0.0.1:8081/claim` for
//!      local developer ergonomics, with the same `--faucet-url`
//!      override available.
//!
//! A real faucet server is not required: each test spawns an inline
//! TCP listener on `127.0.0.1:0` and parses the single HTTP request to
//! assert the body shape the CLI sent.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::Value;

const ADDRESS_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn cli_bin() -> &'static str {
    env!("CARGO_BIN_EXE_boole-cli")
}

/// Spawn a one-shot mock faucet listening on `127.0.0.1:0`. The thread
/// accepts a single connection, reads the request body, sends back
/// `response_body` as a 200 application/json, and returns the parsed
/// body to the caller via `req_rx`.
fn spawn_mock_faucet(response_body: &str) -> (SocketAddr, mpsc::Receiver<MockRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock faucet");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let response_owned = response_body.to_string();
    thread::spawn(move || {
        listener
            .set_nonblocking(false)
            .expect("blocking accept on mock faucet");
        let (mut stream, _) = listener.accept().expect("mock faucet accept");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read timeout");

        let mut raw = Vec::with_capacity(2048);
        let mut buf = [0u8; 1024];
        let mut header_end: Option<usize> = None;
        let mut content_length: usize = 0;
        loop {
            let n = stream.read(&mut buf).expect("read request");
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n]);
            if header_end.is_none() {
                if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    header_end = Some(pos);
                    let header_text = std::str::from_utf8(&raw[..pos]).expect("header utf8");
                    for line in header_text.split("\r\n").skip(1) {
                        if let Some(v) = line
                            .to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|s| s.trim().to_string())
                        {
                            content_length = v.parse().expect("content-length usize");
                        }
                    }
                }
            }
            if let Some(pos) = header_end {
                if raw.len() >= pos + 4 + content_length {
                    break;
                }
            }
        }
        let pos = header_end.expect("HTTP request must have header terminator");
        let header_text = std::str::from_utf8(&raw[..pos])
            .expect("header utf8")
            .to_string();
        let body_bytes = raw[pos + 4..pos + 4 + content_length].to_vec();
        let method_path = header_text
            .lines()
            .next()
            .expect("request line")
            .to_string();

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_owned}",
            response_owned.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
        let _ = stream.flush();
        let _ = tx.send(MockRequest {
            request_line: method_path,
            body: body_bytes,
        });
    });
    (addr, rx)
}

struct MockRequest {
    request_line: String,
    body: Vec<u8>,
}

#[test]
fn faucet_claim_testnet_posts_address_and_canonical_network_id_in_envelope() {
    let response_body = r#"{"status":"queued","tx_id":"faucet-tx-abc"}"#;
    let (addr, req_rx) = spawn_mock_faucet(response_body);
    let faucet_url = format!("http://{addr}/claim");

    let output = Command::new(cli_bin())
        .args([
            "faucet",
            "claim",
            "--network",
            "testnet",
            "--address",
            ADDRESS_HEX,
            "--faucet-url",
            &faucet_url,
            "--json",
        ])
        .output()
        .expect("spawn boole-cli faucet claim");
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(
        output.status.success(),
        "faucet claim must exit 0; stderr={stderr}"
    );

    let req = req_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("mock faucet must receive exactly one request");
    assert!(
        req.request_line.starts_with("POST /claim "),
        "CLI must POST to the supplied faucet URL path; got {:?}",
        req.request_line
    );
    let body: Value = serde_json::from_slice(&req.body)
        .unwrap_or_else(|e| panic!("faucet body not JSON: {e}: {:?}", req.body));
    assert_eq!(
        body.get("address").and_then(Value::as_str),
        Some(ADDRESS_HEX),
        "faucet body must echo the operator-supplied address verbatim",
    );
    assert_eq!(
        body.get("network_id").and_then(Value::as_str),
        Some("boole-testnet"),
        "faucet body must carry the canonical network_id derived from \
         --network testnet; missing or wrong network_id would let a \
         faucet operator be tricked into funding a different network",
    );

    let parsed: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("CLI stdout not JSON: {e}: {stdout}"));
    assert_eq!(
        parsed.get("ok"),
        Some(&Value::Bool(true)),
        "faucet claim envelope ok must be true; got {parsed}"
    );
    assert_eq!(
        parsed.get("version").and_then(Value::as_str),
        Some("v1"),
        "faucet claim must use unified envelope schema v1; got {parsed}"
    );
    assert_eq!(
        parsed.get("command").and_then(Value::as_str),
        Some("faucet.claim"),
        "faucet claim envelope command must be dotted 'faucet.claim'",
    );
    let result = parsed
        .get("result")
        .unwrap_or_else(|| panic!("envelope missing result field; got {parsed}"));
    assert_eq!(
        result.get("status").and_then(Value::as_str),
        Some("queued"),
        "faucet server response must be forwarded under result; got {parsed}"
    );
    assert_eq!(
        result.get("tx_id").and_then(Value::as_str),
        Some("faucet-tx-abc"),
        "faucet server response tx_id must be forwarded under result",
    );
}

#[test]
fn faucet_claim_mainnet_without_explicit_faucet_url_errors_with_clear_reason() {
    // Mainnet does not ship with a default faucet URL. Silently
    // falling through to localhost (or worse, a placeholder URL that
    // routes nowhere) would be a foot gun, so the CLI must fail loudly
    // and tell the operator to pass --faucet-url.
    let output = Command::new(cli_bin())
        .args([
            "faucet",
            "claim",
            "--network",
            "mainnet",
            "--address",
            ADDRESS_HEX,
            "--json",
        ])
        .output()
        .expect("spawn boole-cli faucet claim mainnet");
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(
        !output.status.success(),
        "mainnet without --faucet-url must exit non-zero; stdout={stdout} stderr={stderr}"
    );
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        combined.contains("faucet-url") || combined.contains("faucet_url"),
        "error must mention --faucet-url so the operator knows the fix; got {combined}"
    );
    assert!(
        combined.contains("mainnet"),
        "error must mention the network so the operator can match it; got {combined}"
    );
}

#[test]
fn faucet_claim_dev_preset_stamps_canonical_network_id() {
    // The `dev` preset must stamp `network_id=boole-dev` so a developer
    // running a local faucet (or a CI harness wrapping a mock one) can
    // distinguish dev claims from testnet/mainnet without parsing the
    // operator's CLI invocation. We don't pin the default dev URL in an
    // integration test (it would race other localhost listeners on the
    // host), but we DO pin the network_id contract by passing
    // --faucet-url and asserting the body shape.
    let response_body = r#"{"status":"queued","tx_id":"dev-tx"}"#;
    let (addr, req_rx) = spawn_mock_faucet(response_body);
    let faucet_url = format!("http://{addr}/claim");

    let output = Command::new(cli_bin())
        .args([
            "faucet",
            "claim",
            "--network",
            "dev",
            "--address",
            ADDRESS_HEX,
            "--faucet-url",
            &faucet_url,
            "--json",
        ])
        .output()
        .expect("spawn boole-cli faucet claim dev");
    assert!(
        output.status.success(),
        "dev preset with explicit --faucet-url must succeed; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let req = req_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("dev mock faucet must receive a request");
    let body: Value = serde_json::from_slice(&req.body)
        .unwrap_or_else(|e| panic!("dev faucet body not JSON: {e}"));
    assert_eq!(
        body.get("network_id").and_then(Value::as_str),
        Some("boole-dev"),
        "dev preset must stamp canonical network_id=boole-dev; got {body}"
    );
}
