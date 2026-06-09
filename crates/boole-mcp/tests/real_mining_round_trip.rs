//! Closed-local smoke test: `boole.mine` with a real v1-lenbound target emitter
//! and a deterministic intake-valid driver stand-in.
//!
//! Asserts the HONEST pipeline boundary:
//!
//! - tickets_found >= 1: real ticket grind ran with all-ones thresholds
//! - driver_answered >= 1 AND proof_intake_accepted >= 1: emitter to driver to
//!   ProofIntakeV1 traversal completed
//! - verify_accepted == 0 AND verify_rejected >= 1: RejectingVerifier was
//!   REACHED; no Lean claim
//! - shares_accepted == 0: no network submission; NOT a public-mining claim
//! - loop_class == "smoke": open_thresholds (all-ones t_*) marks this as a
//!   closed-local run, not a public-scoring-eligible round
//!
//! This is a CLOSED-LOCAL SMOKE only. No public network. No Lean verify.
//! verify_accepted=0 is CORRECT behavior — do not interpret it as failure.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

fn bin_path() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("target");
    p.push(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    p.push("boole-mcp");
    p
}

struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_serve(node_url: &str) -> (ChildGuard, SocketAddr) {
    let mut child = Command::new(bin_path())
        .arg("serve")
        .arg("--node-url")
        .arg(node_url)
        .arg("--listen")
        .arg("127.0.0.1:0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn boole-mcp");
    let stderr = child.stderr.take().expect("stderr");
    let mut reader = BufReader::new(stderr);
    let mut first_line = String::new();
    reader
        .read_line(&mut first_line)
        .expect("read first stderr line");
    let addr_str = first_line
        .trim()
        .strip_prefix("boole-mcp listening on http://")
        .unwrap_or_else(|| panic!("unexpected stderr line: {first_line:?}"));
    let addr: SocketAddr = addr_str.parse().expect("addr parse");
    let guard = ChildGuard { child };
    wait_for_tcp(addr, Duration::from_secs(5));
    (guard, addr)
}

fn wait_for_tcp(addr: SocketAddr, timeout: Duration) {
    let start = Instant::now();
    loop {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return;
        }
        if start.elapsed() > timeout {
            panic!("server never became reachable at {addr}");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn http_post_json(addr: SocketAddr, path: &str, body: &str) -> (u16, String) {
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
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
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

fn dummy_upstream_url() -> String {
    "http://127.0.0.1:1".to_string()
}

/// Closed-local round-trip smoke: boole.mine with max_cycles=1 using a real
/// v1-lenbound target emitter and intake-valid deterministic driver stand-in.
///
/// Asserts the HONEST pipeline boundary — verify_accepted MUST be 0
/// (RejectingVerifier; no Lean toolchain; correct behavior, not a bug).
/// shares_accepted MUST be 0 — this is NOT public mining.
#[test]
fn real_round_trip_honest_pipeline_counters() {
    let (_guard, addr) = spawn_serve(&dummy_upstream_url());
    let req_body = json!({"tool":"boole.mine","args":{"max_cycles":1}}).to_string();
    let (status, body) = http_post_json(addr, "/mcp/invoke", &req_body);
    assert_eq!(status, 200, "body={body}");
    let v: Value = serde_json::from_str(&body).expect("json parse failed; body={body}");

    // Pipeline traversal: real v1-lenbound instance was generated and processed.
    assert!(
        v["driver_answered"].as_u64().unwrap_or(0) >= 1,
        "driver_answered must be >= 1 (canonical stand-in driver answered); body={body}"
    );
    assert!(
        v["proof_intake_accepted"].as_u64().unwrap_or(0) >= 1,
        "proof_intake_accepted must be >= 1 (intake-valid answer passed ProofIntakeV1); body={body}"
    );

    // Ticket grind: all-ones thresholds guarantee at least one ticket per cycle.
    assert!(
        v["tickets_found"].as_u64().unwrap_or(0) >= 1,
        "tickets_found must be >= 1 (all-ones thresholds; deterministic ticket grind); body={body}"
    );

    // Verifier boundary: RejectingVerifier was REACHED and correctly rejected.
    // verify_accepted == 0 is CORRECT — this is a closed-local smoke with no
    // Lean toolchain.  Presenting verify_accepted=0 as failure would be wrong.
    assert_eq!(
        v["verify_accepted"].as_u64().unwrap_or(1),
        0,
        "verify_accepted must be 0 (RejectingVerifier; no Lean claim); body={body}"
    );
    assert!(
        v["verify_rejected"].as_u64().unwrap_or(0) >= 1,
        "verify_rejected must be >= 1 (verifier was reached and rejected; correct behavior); body={body}"
    );

    // No network submission — this is NOT public mining.
    assert_eq!(
        v["shares_accepted"].as_u64().unwrap_or(1),
        0,
        "shares_accepted must be 0 (closed-local smoke; no public-network submission); body={body}"
    );

    // Loop class must be "smoke" — all-ones thresholds (or other ineligibility
    // reason) guarantee this is never misclassified as public_mining.
    assert_eq!(
        v["loop_class"].as_str().unwrap_or(""),
        "smoke",
        "loop_class must be \"smoke\" for a closed-local run; body={body}"
    );
}

/// After a real round-trip, boole.status reflects the honest counter set
/// including the agent counters (driver_answered, proof_intake_accepted, etc.).
#[test]
fn status_after_real_round_trip_exposes_agent_counters() {
    let (_guard, addr) = spawn_serve(&dummy_upstream_url());
    // Run one cycle first.
    let mine_body = json!({"tool":"boole.mine","args":{"max_cycles":1}}).to_string();
    let (mine_status, mine_resp) = http_post_json(addr, "/mcp/invoke", &mine_body);
    assert_eq!(mine_status, 200, "mine_resp={mine_resp}");

    // Now inspect the stored summary.
    let status_body = json!({"tool":"boole.status","args":{}}).to_string();
    let (status_code, status_resp) = http_post_json(addr, "/mcp/invoke", &status_body);
    assert_eq!(status_code, 200, "status_resp={status_resp}");
    let v: Value = serde_json::from_str(&status_resp).expect("json");
    assert_eq!(v["state"], "completed", "body={status_resp}");

    let s = &v["last_summary"];
    assert!(
        s["driver_answered"].as_u64().unwrap_or(0) >= 1,
        "last_summary must expose driver_answered; body={status_resp}"
    );
    assert!(
        s["proof_intake_accepted"].as_u64().unwrap_or(0) >= 1,
        "last_summary must expose proof_intake_accepted; body={status_resp}"
    );
    assert_eq!(
        s["verify_accepted"].as_u64().unwrap_or(1),
        0,
        "last_summary verify_accepted must be 0; body={status_resp}"
    );
    assert_eq!(
        s["shares_accepted"].as_u64().unwrap_or(1),
        0,
        "last_summary shares_accepted must be 0 (not public mining); body={status_resp}"
    );
}
