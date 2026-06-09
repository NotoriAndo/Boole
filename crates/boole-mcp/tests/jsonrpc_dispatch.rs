//! S5 — unit tests for the JSON-RPC 2.0 dispatch layer.
//!
//! RED contract:
//!   * `initialize` → result with protocolVersion == "2024-11-05"
//!   * `notifications/initialized` (notification, no id) → None
//!   * `tools/list` → result.tools has exactly 4 tools
//!   * `tools/call` boole.status → content[0].type=="text" with idle state
//!   * unknown method → JSON-RPC error code -32601
//!   * malformed JSON → JSON-RPC error code -32700
//!   * missing method field → JSON-RPC error code -32600
//!
//! The protocol version string "2024-11-05" is pinned explicitly so a
//! future bump is deliberate and visible in the diff.

use serde_json::{json, Value};

use boole_mcp::handle_jsonrpc_sync;

fn call(msg: &str) -> Option<Value> {
    let result = handle_jsonrpc_sync(msg);
    result.map(|s| serde_json::from_str(&s).expect("response is valid json"))
}

fn call_str(msg: &Value) -> Option<Value> {
    call(&msg.to_string())
}

// ── initialize ───────────────────────────────────────────────────────────────

#[test]
fn initialize_returns_protocol_version_pinned() {
    let req = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}
    }});
    let resp = call_str(&req).expect("Some");
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    // Pin the exact version string — a bump must be deliberate.
    assert_eq!(
        resp["result"]["protocolVersion"], "2024-11-05",
        "protocolVersion must be pinned to 2024-11-05"
    );
    assert!(resp["result"]["serverInfo"]["name"].is_string());
}

// ── notifications/initialized ─────────────────────────────────────────────

#[test]
fn initialized_notification_returns_none() {
    let notif = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
    let resp = call_str(&notif);
    assert!(
        resp.is_none(),
        "notifications/initialized is a notification; must return None"
    );
}

// ── tools/list ───────────────────────────────────────────────────────────────

#[test]
fn tools_list_returns_exactly_four_tools() {
    let req = json!({"jsonrpc":"2.0","id":2,"method":"tools/list"});
    let resp = call_str(&req).expect("Some");
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 2);
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 4, "expected 4 tools; got {}", tools.len());
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"bounty.list"), "names={names:?}");
    assert!(names.contains(&"receipt.get"), "names={names:?}");
    assert!(names.contains(&"boole.mine"), "names={names:?}");
    assert!(names.contains(&"boole.status"), "names={names:?}");
}

#[test]
fn tools_list_tools_have_input_schema_camel() {
    let req = json!({"jsonrpc":"2.0","id":3,"method":"tools/list"});
    let resp = call_str(&req).expect("Some");
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    for t in tools {
        let name = t["name"].as_str().unwrap_or("?");
        assert!(
            t["inputSchema"].is_object(),
            "{name}: tools/list must use camelCase inputSchema per MCP spec"
        );
    }
}

// ── tools/call boole.status ───────────────────────────────────────────────

#[test]
fn tools_call_boole_status_idle_envelope_in_content() {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": { "name": "boole.status", "arguments": {} }
    });
    let resp = call_str(&req).expect("Some");
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 4);
    let content = resp["result"]["content"].as_array().expect("content array");
    assert!(!content.is_empty(), "content must be non-empty");
    assert_eq!(content[0]["type"], "text", "content[0].type must be 'text'");
    let text = content[0]["text"].as_str().expect("text string");
    let inner: Value = serde_json::from_str(text).expect("text is valid json");
    assert_eq!(
        inner["state"], "idle",
        "boole.status without prior mine → idle"
    );
    // isError must be absent or false for a successful call.
    let is_error = resp["result"]["isError"].as_bool().unwrap_or(false);
    assert!(!is_error, "isError must be false for a successful call");
}

// ── unknown method ────────────────────────────────────────────────────────

#[test]
fn unknown_method_returns_jsonrpc_error_32601() {
    let req = json!({"jsonrpc":"2.0","id":99,"method":"no_such_method"});
    let resp = call_str(&req).expect("Some (error responses still have a response)");
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 99);
    assert_eq!(
        resp["error"]["code"], -32601,
        "unknown method must return -32601 Method Not Found"
    );
}

// ── malformed JSON ────────────────────────────────────────────────────────

#[test]
fn malformed_json_returns_jsonrpc_error_32700() {
    let resp = call("not json at all {{{").expect("Some (parse error response)");
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(
        resp["error"]["code"], -32700,
        "malformed JSON must return -32700 Parse error"
    );
}

// ── missing method field ──────────────────────────────────────────────────

#[test]
fn missing_method_returns_jsonrpc_error_32600() {
    let resp = call_str(&json!({"jsonrpc":"2.0","id":5})).expect("Some");
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(
        resp["error"]["code"], -32600,
        "missing method field must return -32600 Invalid Request"
    );
}
