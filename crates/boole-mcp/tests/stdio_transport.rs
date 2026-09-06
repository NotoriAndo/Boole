//! S6 — integration test for the `stdio` subcommand.
//!
//! Spawns the built binary with `boole-mcp stdio`, drives it via
//! newline-delimited JSON-RPC 2.0 over its stdin/stdout, and
//! asserts the full handshake:
//!
//!   1. `initialize`  → protocolVersion "2024-11-05", serverInfo.name = "boole-mcp"
//!   2. `notifications/initialized` → no response (notification)
//!   3. `tools/list`  → exactly 5 tools
//!   4. `tools/call`  boole.status → idle envelope in content[0].text
//!
//! Drive via std::process::Command with piped stdin/stdout.
//! The test intentionally implements its own line client instead of using the
//! server's framer, so it catches shared framing mistakes.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

fn bin_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
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

struct StdioChild {
    child: Child,
}

impl Drop for StdioChild {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Write one MCP stdio JSON-RPC message without using the server framer.
fn write_frame(writer: &mut impl Write, body: &str) {
    writer.write_all(body.as_bytes()).expect("write body");
    writer.write_all(b"\n").expect("write delimiter");
    writer.flush().expect("flush");
}

/// Read one MCP stdio line without using the server framer.
fn read_frame(reader: &mut impl BufRead) -> String {
    let mut line = String::new();
    reader.read_line(&mut line).expect("read response line");
    assert!(
        line.ends_with('\n'),
        "MCP response must end in a newline: {line:?}"
    );
    assert!(
        !line.starts_with("Content-Length:"),
        "MCP response must not use LSP Content-Length framing: {line:?}"
    );
    line.pop();
    line
}

fn spawn_stdio() -> (
    StdioChild,
    std::process::ChildStdin,
    BufReader<std::process::ChildStdout>,
) {
    let mut child = Command::new(bin_path())
        .arg("stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn boole-mcp stdio");
    let stdin = child.stdin.take().expect("stdin");
    let stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let guard = StdioChild { child };
    // Give the process a moment to start up.
    std::thread::sleep(Duration::from_millis(50));
    (guard, stdin, stdout)
}

#[test]
fn stdio_initialize_returns_protocol_version() {
    let (_guard, mut stdin, mut stdout) = spawn_stdio();

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "0.1" }
        }
    });
    write_frame(&mut stdin, &req.to_string());

    let resp_str = read_frame(&mut stdout);
    let resp: Value = serde_json::from_str(&resp_str).expect("valid json response");
    assert_eq!(resp["jsonrpc"], "2.0", "resp={resp_str}");
    assert_eq!(resp["id"], 1, "resp={resp_str}");
    assert_eq!(
        resp["result"]["protocolVersion"], "2024-11-05",
        "protocolVersion pinned; resp={resp_str}"
    );
    assert_eq!(
        resp["result"]["serverInfo"]["name"], "boole-mcp",
        "serverInfo.name; resp={resp_str}"
    );
}

#[test]
fn stdio_tools_list_has_five_tools() {
    let (_guard, mut stdin, mut stdout) = spawn_stdio();

    // Perform initialize first (required by MCP protocol).
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "0" }
        }
    });
    write_frame(&mut stdin, &init.to_string());
    let _init_resp = read_frame(&mut stdout); // consume

    // Send notifications/initialized (no response expected).
    let notif = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
    write_frame(&mut stdin, &notif.to_string());
    // No frame to read for a notification.

    // tools/list
    let list_req = json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
    write_frame(&mut stdin, &list_req.to_string());
    let list_resp_str = read_frame(&mut stdout);
    let list_resp: Value = serde_json::from_str(&list_resp_str).expect("valid json");
    let tools = list_resp["result"]["tools"]
        .as_array()
        .expect("tools array");
    assert_eq!(
        tools.len(),
        5,
        "expected 5 tools; got {}; resp={list_resp_str}",
        tools.len()
    );
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in [
        "bounty.list",
        "receipt.get",
        "boole.mine",
        "boole.status",
        "boole.verify_native",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} missing; names={names:?}"
        );
    }
}

#[test]
fn stdio_tools_call_boole_status_returns_idle_in_content() {
    let (_guard, mut stdin, mut stdout) = spawn_stdio();

    // initialize
    let init = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "0" }
        }
    });
    write_frame(&mut stdin, &init.to_string());
    let _init_resp = read_frame(&mut stdout);

    // tools/call boole.status
    let call_req = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": { "name": "boole.status", "arguments": {} }
    });
    write_frame(&mut stdin, &call_req.to_string());
    let call_resp_str = read_frame(&mut stdout);
    let call_resp: Value = serde_json::from_str(&call_resp_str).expect("valid json");

    let content = call_resp["result"]["content"]
        .as_array()
        .expect("content array");
    assert!(
        !content.is_empty(),
        "content must not be empty; resp={call_resp_str}"
    );
    assert_eq!(content[0]["type"], "text", "resp={call_resp_str}");
    let text = content[0]["text"].as_str().expect("text string");
    let inner: Value = serde_json::from_str(text).expect("text is valid json");
    assert_eq!(
        inner["state"], "idle",
        "boole.status before any mine → idle; text={text}"
    );
    let is_error = call_resp["result"]["isError"].as_bool().unwrap_or(false);
    assert!(!is_error, "isError must be false; resp={call_resp_str}");
}

#[test]
fn stdio_malformed_json_returns_parse_error_and_keeps_the_pipe_usable() {
    let (_guard, mut stdin, mut stdout) = spawn_stdio();
    write_frame(&mut stdin, "not-json");
    let error: Value = serde_json::from_str(&read_frame(&mut stdout)).expect("parse response");
    assert_eq!(error["error"]["code"], -32700, "response={error}");

    let request = json!({"jsonrpc":"2.0","id":9,"method":"tools/list"});
    write_frame(&mut stdin, &request.to_string());
    let response: Value = serde_json::from_str(&read_frame(&mut stdout)).expect("next response");
    assert_eq!(response["id"], 9, "response={response}");
}
