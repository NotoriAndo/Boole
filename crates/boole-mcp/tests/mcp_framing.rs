//! MCP stdio framing contract.
//!
//! RED contract (must fail until lib.rs exports the framing functions):
//!   * `write_mcp_frame` / `read_mcp_frame` round-trip a newline-delimited UTF-8 payload
//!     through an in-memory `Cursor<Vec<u8>>`.
//!   * A clean EOF (empty cursor) → `read_mcp_frame` returns `None`.
//!   * An overlong or truncated line is rejected without unbounded allocation.

use std::io::Cursor;

use boole_mcp::{read_mcp_frame, write_mcp_frame};

#[test]
fn round_trip_simple_payload() {
    let payload = r#"{"jsonrpc":"2.0","method":"initialize","id":1}"#;
    let mut buf: Vec<u8> = Vec::new();
    write_mcp_frame(&mut buf, payload).expect("write");
    let raw = String::from_utf8(buf.clone()).expect("utf8");
    assert_eq!(
        raw,
        format!("{payload}\n"),
        "MCP stdio uses one JSON message per line"
    );
    let mut cursor = Cursor::new(buf);
    let result = read_mcp_frame(&mut cursor).expect("read").expect("some");
    assert_eq!(result, payload);
}

#[test]
fn empty_reader_returns_none() {
    let mut cursor = Cursor::new(vec![]);
    let result = read_mcp_frame(&mut cursor).expect("no error on EOF");
    assert!(result.is_none(), "expected None on empty reader");
}

#[test]
fn large_payload_round_trips() {
    // A bigger JSON body (simulate a tools/list response with many fields).
    let payload = serde_json::to_string(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 42,
        "result": {
            "tools": (0..10).map(|i| serde_json::json!({"name": format!("tool.{i}"), "description": "x"})).collect::<Vec<_>>()
        }
    })).unwrap();
    let mut buf: Vec<u8> = Vec::new();
    write_mcp_frame(&mut buf, &payload).expect("write");
    let mut cursor = Cursor::new(buf);
    let result = read_mcp_frame(&mut cursor).expect("read").expect("some");
    assert_eq!(result, payload);
}

#[test]
fn stdio_frame_over_max_line_is_rejected() {
    let frame = vec![b'x'; 16 * 1024 * 1024 + 1];
    let mut cursor = Cursor::new(frame);
    let err = read_mcp_frame(&mut cursor).expect_err("over-cap line must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("cap") || msg.contains("exceeds"),
        "rejection must cite the frame-size cap, got: {msg}"
    );
}

#[test]
fn truncated_line_is_error() {
    let mut cursor = Cursor::new(br#"{"id":1}"#.to_vec());
    let err = read_mcp_frame(&mut cursor).expect_err("EOF before a newline is truncated");
    assert!(err.to_string().contains("truncated"));
}
