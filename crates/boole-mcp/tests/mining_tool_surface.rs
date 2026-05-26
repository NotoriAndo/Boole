//! P2.1 closure (slice 51) — register the two mining-side MCP tools
//! that close P2.1 criterion 1: `boole.mine` and `boole.status`.
//!
//! Scope of this slice is intentionally narrow:
//!   * Both tools appear in `GET /mcp/tools` with name + description +
//!     `input_schema`.
//!   * `POST /mcp/invoke` for either tool returns a typed HTTP 501
//!     `{ "error": "not-implemented", "tool": "<name>" }` envelope so
//!     IDE clients see a structural answer instead of `unknown-tool`.
//!   * Existing tools (`bounty.list`, `receipt.get`) keep working.
//!
//! The actual `run_mining_loop` wiring + the round-trip behaviour ride
//! on slices 52+. Pinning the discoverability + dispatch envelope
//! first means IDE / MCP clients can already see the tools and get
//! a typed answer while the runtime lands.

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

fn http_get(addr: SocketAddr, path: &str) -> (u16, String) {
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
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

fn http_post_json(addr: SocketAddr, path: &str, body: &str) -> (u16, String) {
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
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
    // No upstream is reachable here, but the new tools don't proxy, so
    // they should answer 501 without touching the upstream URL at all.
    "http://127.0.0.1:1".to_string()
}

#[test]
fn tools_endpoint_now_lists_boole_mine_and_boole_status() {
    let (_guard, addr) = spawn_serve(&dummy_upstream_url());
    let (status, body) = http_get(addr, "/mcp/tools");
    assert_eq!(status, 200, "body={body}");
    let v: Value = serde_json::from_str(&body).expect("json");
    let tools = v["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(
        names.contains(&"boole.mine"),
        "boole.mine must be in tools; names={names:?}"
    );
    assert!(
        names.contains(&"boole.status"),
        "boole.status must be in tools; names={names:?}"
    );
    // Existing tools must still appear — slice 51 must not regress
    // P2.2 slice 34's surface.
    assert!(
        names.contains(&"bounty.list"),
        "bounty.list must still be in tools; names={names:?}"
    );
    assert!(
        names.contains(&"receipt.get"),
        "receipt.get must still be in tools; names={names:?}"
    );
    for t in tools {
        assert!(t["description"].is_string(), "each tool needs description");
        assert!(
            t["input_schema"].is_object(),
            "each tool needs input_schema"
        );
    }
}

#[test]
fn invoke_boole_mine_returns_typed_not_implemented_501() {
    let (_guard, addr) = spawn_serve(&dummy_upstream_url());
    let req_body = json!({"tool":"boole.mine","args":{}}).to_string();
    let (status, body) = http_post_json(addr, "/mcp/invoke", &req_body);
    assert_eq!(status, 501, "body={body}");
    let v: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(v["error"], "not-implemented");
    assert_eq!(v["tool"], "boole.mine");
}

/// P2.1 closure (slice 52) — `boole.status` answers a real envelope.
/// With no mining session ever started, the read returns
/// `{"state":"idle"}` and HTTP 200. The `not-implemented` 501 envelope
/// is reserved for `boole.mine` until slice 53 wires the run-loop.
#[test]
fn invoke_boole_status_returns_idle_envelope_200() {
    let (_guard, addr) = spawn_serve(&dummy_upstream_url());
    let req_body = json!({"tool":"boole.status","args":{}}).to_string();
    let (status, body) = http_post_json(addr, "/mcp/invoke", &req_body);
    assert_eq!(status, 200, "body={body}");
    let v: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(v["state"], "idle", "body={body}");
}
