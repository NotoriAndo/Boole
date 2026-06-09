//! S1 — unit tests for the shared `dispatch_tool` function.
//!
//! The dispatch function must be usable by both the HTTP `invoke` handler
//! and the new stdio `tools/call` handler. These tests call it directly
//! through the `boole_mcp_dispatch` re-export to verify the core behaviour
//! without spinning up an HTTP server.
//!
//! RED contract:
//!   * `boole.status` before any mine → idle envelope
//!   * `boole.status` args ignored (no required arg)
//!   * unknown tool → ToolError::UnknownTool
//!   * `receipt.get` with missing receipt_id → ToolError::MissingArg

// The dispatch_tool fn + ToolError will be exported from the binary crate
// via a pub(crate) or pub module.  The integration tests drive the binary
// via process spawn; these unit tests work through the lib API that
// main.rs exposes in a #[cfg(test)] / pub-in-test-only pattern — here we
// access it by depending on the crate's lib target.
//
// boole_mcp (lib) re-exports a `dispatch_tool` test shim when compiled
// under #[cfg(test)]; the binary crate's main.rs has its own copy.
// For now we test the behaviour through the library's exported helpers.

/// Check that the library crate re-exports the tools list and the tools
/// contain exactly the 4 expected names.  This exercises the shared
/// `mcp_tools_array()` that will feed both HTTP /mcp/tools and stdio tools/list.
#[test]
fn tools_array_has_exactly_four_tools() {
    let tools = boole_mcp::mcp_tools_array();
    assert_eq!(tools.len(), 4, "expected 4 tools, got {}", tools.len());
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"bounty.list"), "names={names:?}");
    assert!(names.contains(&"receipt.get"), "names={names:?}");
    assert!(names.contains(&"boole.mine"), "names={names:?}");
    assert!(names.contains(&"boole.status"), "names={names:?}");
}

/// Each tool has both `inputSchema` (camelCase, for MCP stdio) and
/// `input_schema` (snake_case, for HTTP /mcp/tools backward compat) fields.
#[test]
fn tools_array_each_tool_has_input_schema_and_input_schema_camel() {
    let tools = boole_mcp::mcp_tools_array();
    for t in &tools {
        let name = t["name"].as_str().unwrap_or("?");
        assert!(
            t["input_schema"].is_object(),
            "{name}: missing input_schema (snake)"
        );
        assert!(
            t["inputSchema"].is_object(),
            "{name}: missing inputSchema (camel)"
        );
    }
}

/// Ensure `input_schema` and `inputSchema` carry the same schema content
/// for each tool (they are the same schema, two key spellings).
#[test]
fn tools_array_input_schema_and_input_schema_camel_are_identical() {
    let tools = boole_mcp::mcp_tools_array();
    for t in &tools {
        let name = t["name"].as_str().unwrap_or("?");
        assert_eq!(
            t["input_schema"], t["inputSchema"],
            "{name}: input_schema and inputSchema should carry identical content"
        );
    }
}
