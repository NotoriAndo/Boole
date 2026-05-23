//! P2.1 — boole-mcp scaffolding contract. This slice carves out the
//! binary and stands up the health endpoint; the actual MCP tool
//! surface (bounty + receipt read APIs) ships in P2.2 (slice 34).
//!
//! Contract under test:
//!   * `serve --node-url <url> --listen <addr>` spawns an HTTP server
//!     bound to `<addr>`; the resolved address is echoed to stderr as
//!     `boole-mcp listening on http://<addr>` so a launcher invoking
//!     with `:0` can grab the ephemeral port.
//!   * `GET /healthz` -> 200 with body `boole-mcp.v0`.
//!   * `GET /mcp/tools` -> 200 once P2.2 wires the tool registry. The
//!     scaffold test asserts the route is reachable; tests/tool_surface.rs
//!     pins the registry body shape.
//!   * `serve` without `--node-url` exits non-zero and the error mentions
//!     the missing flag.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

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

/// Spawn `boole-mcp serve --node-url <url> --listen 127.0.0.1:0` and
/// parse the bound port from stderr's first line.
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
    let addr: SocketAddr = addr_str
        .parse()
        .unwrap_or_else(|_| panic!("addr parse from {addr_str:?}"));
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
        .set_write_timeout(Some(Duration::from_secs(5)))
        .expect("write timeout");
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

#[test]
fn binary_starts_and_prints_listening_address_to_stderr() {
    let (_guard, addr) = spawn_serve("http://127.0.0.1:9");
    assert_eq!(addr.ip().to_string(), "127.0.0.1");
    assert_ne!(addr.port(), 0, "ephemeral port must be resolved");
}

#[test]
fn healthz_returns_boole_mcp_v0() {
    let (_guard, addr) = spawn_serve("http://127.0.0.1:9");
    let (status, body) = http_get(addr, "/healthz");
    assert_eq!(status, 200, "body was {body:?}");
    assert_eq!(body, "boole-mcp.v0");
}

#[test]
fn mcp_tools_endpoint_is_reachable() {
    // P2.1 stood up a 404 placeholder; P2.2 wired the real registry.
    // The scaffold test only asserts the route exists and is not the
    // generic 404 fallback. tests/tool_surface.rs pins the body shape.
    let (_guard, addr) = spawn_serve("http://127.0.0.1:9");
    let (status, _body) = http_get(addr, "/mcp/tools");
    assert_eq!(status, 200, "tools registry must be wired");
}

#[test]
fn serve_requires_node_url_argument() {
    let output = Command::new(bin_path())
        .arg("serve")
        .arg("--listen")
        .arg("127.0.0.1:0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn");
    assert!(!output.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        stderr.contains("--node-url"),
        "stderr should mention --node-url; got {stderr:?}"
    );
}
