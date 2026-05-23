//! P2.2 — boole-mcp MCP tool surface. Slice 34 wires two read-only
//! tools that proxy to the upstream boole-node:
//!
//!   * `bounty.list`  -> upstream GET /work
//!   * `receipt.get`  -> upstream GET /receipts/{receipt_id}
//!
//! Contract under test:
//!   * GET /mcp/tools -> 200 JSON `{ "tools": [{name, description, input_schema}, ...] }`
//!     containing both `bounty.list` and `receipt.get`.
//!   * POST /mcp/invoke `{"tool":"bounty.list","args":{}}` -> 200 with upstream /work body.
//!   * POST /mcp/invoke `{"tool":"receipt.get","args":{"receipt_id":"abc"}}` -> 200 with
//!     upstream /receipts/abc body.
//!   * POST /mcp/invoke `{"tool":"nope","args":{}}` -> 400 `{"error":"unknown-tool","tool":"nope"}`.
//!   * POST /mcp/invoke `{"tool":"receipt.get","args":{}}` -> 400
//!     `{"error":"missing-arg","arg":"receipt_id"}`.
//!   * POST /mcp/invoke with upstream-unreachable node-url -> 502
//!     `{"error":"upstream-unreachable"}`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
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

/// Tiny mock upstream that responds to GET /work and GET /receipts/{id}
/// with canned JSON. Spawns a worker thread per accepted connection.
/// `shutdown` is set by the guard's Drop to break the accept loop.
struct MockUpstream {
    addr: SocketAddr,
    shutdown: Arc<Mutex<bool>>,
    seen_paths: Arc<Mutex<Vec<String>>>,
}

impl MockUpstream {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
        let addr = listener.local_addr().expect("addr");
        let shutdown = Arc::new(Mutex::new(false));
        let seen_paths = Arc::new(Mutex::new(Vec::<String>::new()));
        let shutdown_clone = shutdown.clone();
        let seen_clone = seen_paths.clone();
        thread::spawn(move || {
            // Blocking accept loop; the Drop impl breaks it by
            // connecting to its own addr to wake accept(), and the
            // shutdown flag tells the handler to bail without touching
            // seen_paths.
            for stream in listener.incoming() {
                if *shutdown_clone.lock().unwrap() {
                    return;
                }
                if let Ok(stream) = stream {
                    let seen = seen_clone.clone();
                    thread::spawn(move || handle_mock_conn(stream, seen));
                }
            }
        });
        Self {
            addr,
            shutdown,
            seen_paths,
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn seen(&self) -> Vec<String> {
        self.seen_paths.lock().unwrap().clone()
    }
}

impl Drop for MockUpstream {
    fn drop(&mut self) {
        *self.shutdown.lock().unwrap() = true;
        // Poke the listener to break accept().
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(100));
    }
}

fn handle_mock_conn(mut stream: TcpStream, seen: Arc<Mutex<Vec<String>>>) {
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
    let mut buf = [0u8; 2048];
    let n = match stream.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return,
    };
    let req = String::from_utf8_lossy(&buf[..n]).to_string();
    let request_line = req.lines().next().unwrap_or("");
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    seen.lock().unwrap().push(path.to_string());

    let (status, body) = if path == "/work" {
        (
            200,
            r#"{"work":[{"id":"w-1","description":"sample"}]}"#.to_string(),
        )
    } else if let Some(rest) = path.strip_prefix("/receipts/") {
        (
            200,
            format!(r#"{{"receipt_id":"{rest}","status":"accepted"}}"#),
        )
    } else {
        (404, "{}".to_string())
    };

    let response = format!(
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

#[test]
fn tools_endpoint_lists_bounty_list_and_receipt_get_with_input_schemas() {
    let upstream = MockUpstream::start();
    let (_guard, addr) = spawn_serve(&upstream.url());
    let (status, body) = http_get(addr, "/mcp/tools");
    assert_eq!(status, 200, "body={body}");
    let v: Value = serde_json::from_str(&body).expect("json");
    let tools = v["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"bounty.list"), "names={names:?}");
    assert!(names.contains(&"receipt.get"), "names={names:?}");
    for t in tools {
        assert!(
            t["description"].is_string(),
            "each tool needs description: {t}"
        );
        assert!(
            t["input_schema"].is_object(),
            "each tool needs input_schema: {t}"
        );
    }
}

#[test]
fn invoke_bounty_list_proxies_to_upstream_work_endpoint() {
    let upstream = MockUpstream::start();
    let (_guard, addr) = spawn_serve(&upstream.url());
    let req_body = json!({"tool":"bounty.list","args":{}}).to_string();
    let (status, body) = http_post_json(addr, "/mcp/invoke", &req_body);
    assert_eq!(status, 200, "body={body}");
    let v: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(v["work"][0]["id"], "w-1");
    assert!(
        upstream.seen().iter().any(|p| p == "/work"),
        "upstream should have seen /work; seen={:?}",
        upstream.seen()
    );
}

#[test]
fn invoke_receipt_get_proxies_to_upstream_receipts_endpoint_with_path_param() {
    let upstream = MockUpstream::start();
    let (_guard, addr) = spawn_serve(&upstream.url());
    let req_body = json!({"tool":"receipt.get","args":{"receipt_id":"r-42"}}).to_string();
    let (status, body) = http_post_json(addr, "/mcp/invoke", &req_body);
    assert_eq!(status, 200, "body={body}");
    let v: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(v["receipt_id"], "r-42");
    assert!(
        upstream.seen().iter().any(|p| p == "/receipts/r-42"),
        "seen={:?}",
        upstream.seen()
    );
}

#[test]
fn invoke_with_unknown_tool_returns_typed_400() {
    let upstream = MockUpstream::start();
    let (_guard, addr) = spawn_serve(&upstream.url());
    let req_body = json!({"tool":"nope.fake","args":{}}).to_string();
    let (status, body) = http_post_json(addr, "/mcp/invoke", &req_body);
    assert_eq!(status, 400, "body={body}");
    let v: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(v["error"], "unknown-tool");
    assert_eq!(v["tool"], "nope.fake");
}

#[test]
fn invoke_receipt_get_missing_receipt_id_returns_typed_400() {
    let upstream = MockUpstream::start();
    let (_guard, addr) = spawn_serve(&upstream.url());
    let req_body = json!({"tool":"receipt.get","args":{}}).to_string();
    let (status, body) = http_post_json(addr, "/mcp/invoke", &req_body);
    assert_eq!(status, 400, "body={body}");
    let v: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(v["error"], "missing-arg");
    assert_eq!(v["arg"], "receipt_id");
}

#[test]
fn invoke_with_upstream_unreachable_returns_typed_502() {
    // Bind+drop to grab a free port, then point boole-mcp at it.
    let dead_listener = TcpListener::bind("127.0.0.1:0").expect("bind dead");
    let dead_addr = dead_listener.local_addr().expect("dead addr");
    drop(dead_listener);
    let dead_url = format!("http://{dead_addr}");
    let (_guard, addr) = spawn_serve(&dead_url);
    let req_body = json!({"tool":"bounty.list","args":{}}).to_string();
    let (status, body) = http_post_json(addr, "/mcp/invoke", &req_body);
    assert_eq!(status, 502, "body={body}");
    let v: Value = serde_json::from_str(&body).expect("json");
    assert_eq!(v["error"], "upstream-unreachable");
}
