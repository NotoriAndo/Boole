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
use std::time::{Duration, Instant};

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

#[test]
fn stdio_idless_tool_notification_does_not_hide_the_next_response() {
    let (_guard, mut stdin, mut stdout) = spawn_stdio();

    write_frame(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": { "name": "boole.status", "arguments": {} }
        })
        .to_string(),
    );
    write_frame(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":10,"method":"tools/list"}).to_string(),
    );

    let response: Value = serde_json::from_str(&read_frame(&mut stdout)).expect("list response");
    assert_eq!(
        response["id"], 10,
        "the notification must not have a response"
    );
}

#[test]
fn stdio_mining_stays_responsive_and_cancel_has_one_terminal_response() {
    let (_guard, mut stdin, mut stdout) = spawn_stdio();

    // A high, bounded cycle count keeps the closed-local job alive long enough
    // to prove stdio can serve a normal request instead of serially awaiting it.
    write_frame(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 50,
            "method": "tools/call",
            "params": { "name": "boole.mine", "arguments": { "max_cycles": 100_000 } }
        })
        .to_string(),
    );
    write_frame(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":51,"method":"tools/list"}).to_string(),
    );
    let list: Value = serde_json::from_str(&read_frame(&mut stdout)).expect("list response");
    assert_eq!(list["id"], 51, "mining must not block tools/list: {list}");

    // A cancellation notification is scoped to its requestId; another request
    // must not be able to stop this mine.
    write_frame(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": 999 }
        })
        .to_string(),
    );
    write_frame(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 54,
            "method": "tools/call",
            "params": { "name": "boole.status", "arguments": {} }
        })
        .to_string(),
    );
    let running: Value = serde_json::from_str(&read_frame(&mut stdout)).expect("status response");
    assert_eq!(running["id"], 54, "response={running}");
    let running_text = running["result"]["content"][0]["text"]
        .as_str()
        .expect("running status text");
    assert_eq!(
        serde_json::from_str::<Value>(running_text).expect("running JSON")["state"],
        "running"
    );

    // A second mine while the first is active must be rejected, rather than
    // allowing two in-process miners to run concurrently.
    write_frame(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 52,
            "method": "tools/call",
            "params": { "name": "boole.mine", "arguments": { "max_cycles": 0 } }
        })
        .to_string(),
    );
    let busy: Value = serde_json::from_str(&read_frame(&mut stdout)).expect("busy response");
    assert_eq!(busy["id"], 52, "response={busy}");
    let busy_text = busy["result"]["content"][0]["text"]
        .as_str()
        .expect("busy tool text");
    assert_eq!(
        serde_json::from_str::<Value>(busy_text).expect("busy JSON")["error"],
        "mining-busy"
    );

    write_frame(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": 50 }
        })
        .to_string(),
    );
    let cancelled: Value =
        serde_json::from_str(&read_frame(&mut stdout)).expect("cancelled terminal response");
    assert_eq!(cancelled["id"], 50, "response={cancelled}");
    assert_eq!(cancelled["error"]["code"], -32800, "response={cancelled}");

    // There is exactly one terminal response for request 50: if completion and
    // cancellation both wrote, this ping would instead read a duplicate 50.
    write_frame(
        &mut stdin,
        &json!({"jsonrpc":"2.0","id":53,"method":"ping"}).to_string(),
    );
    let ping: Value = serde_json::from_str(&read_frame(&mut stdout)).expect("ping response");
    assert_eq!(ping["id"], 53, "duplicate mining response leaked: {ping}");
    assert_eq!(ping["result"], json!({}));
}

#[test]
fn stdio_mining_rejects_non_integer_and_over_cap_cycles() {
    let (_guard, mut stdin, mut stdout) = spawn_stdio();

    for (id, max_cycles) in [(60, json!(-1)), (61, json!(100_001))] {
        write_frame(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/call",
                "params": { "name": "boole.mine", "arguments": { "max_cycles": max_cycles } }
            })
            .to_string(),
        );
        let response: Value =
            serde_json::from_str(&read_frame(&mut stdout)).expect("error response");
        assert_eq!(response["id"], id, "response={response}");
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("error tool text");
        assert_eq!(
            serde_json::from_str::<Value>(text).expect("error JSON")["error"],
            "invalid-arg"
        );
    }
}

#[test]
fn stdio_eof_cancels_active_mining_and_exits_bounded() {
    let (mut guard, mut stdin, stdout) = spawn_stdio();
    write_frame(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 70,
            "method": "tools/call",
            "params": { "name": "boole.mine", "arguments": { "max_cycles": 100_000 } }
        })
        .to_string(),
    );
    drop(stdin);
    drop(stdout);

    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(status) = guard.child.try_wait().expect("poll boole-mcp") {
            assert!(status.success(), "EOF shutdown status={status}");
            return;
        }
        assert!(Instant::now() < deadline, "stdio did not stop after EOF");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn assert_framing_error_cancels_active_mining_and_exits_bounded(frame: &[u8]) {
    let (mut guard, mut stdin, stdout) = spawn_stdio();
    write_frame(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0",
            "id": 71,
            "method": "tools/call",
            "params": { "name": "boole.mine", "arguments": { "max_cycles": 100_000 } }
        })
        .to_string(),
    );
    stdin.write_all(frame).expect("write malformed frame");
    stdin.flush().expect("flush malformed frame");
    drop(stdin);
    drop(stdout);

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = guard.child.try_wait().expect("poll boole-mcp") {
            assert!(
                !status.success(),
                "a framing error must still be reported after bounded cancellation"
            );
            return;
        }
        assert!(
            Instant::now() < deadline,
            "stdio did not cancel active mining after a framing error"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn stdio_invalid_utf8_cancels_active_mining_and_exits_bounded() {
    assert_framing_error_cancels_active_mining_and_exits_bounded(b"\xff\n");
}

#[test]
fn stdio_truncated_frame_cancels_active_mining_and_exits_bounded() {
    assert_framing_error_cancels_active_mining_and_exits_bounded(b"{\"");
}
