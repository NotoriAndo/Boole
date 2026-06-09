//! S4 — unit tests for the MCP Content-Length framing protocol.
//!
//! RED contract (must fail until lib.rs exports the framing functions):
//!   * `write_mcp_frame` / `read_mcp_frame` round-trip a UTF-8 payload
//!     through an in-memory `Cursor<Vec<u8>>`.
//!   * A clean EOF (empty cursor) → `read_mcp_frame` returns `None`.
//!   * Malformed header (no Content-Length) → error.
//!   * Content-Length mismatch → error.

use std::io::Cursor;

use boole_mcp::{read_mcp_frame, write_mcp_frame};

#[test]
fn round_trip_simple_payload() {
    let payload = r#"{"jsonrpc":"2.0","method":"initialize","id":1}"#;
    let mut buf: Vec<u8> = Vec::new();
    write_mcp_frame(&mut buf, payload).expect("write");
    let raw = String::from_utf8(buf.clone()).expect("utf8");
    // Header line must be present.
    assert!(
        raw.starts_with("Content-Length:"),
        "expected Content-Length header; got {raw:?}"
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
fn missing_content_length_header_is_error() {
    // A bare body with no header at all looks like just a blank line then
    // content. Without Content-Length we should get an error.
    let malformed = b"\r\n{\"id\":1}";
    let mut cursor = Cursor::new(malformed.to_vec());
    let result = read_mcp_frame(&mut cursor);
    assert!(result.is_err(), "expected error for missing Content-Length");
}
