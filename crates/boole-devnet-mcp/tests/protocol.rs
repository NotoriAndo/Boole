use serde_json::{json, Value};
#[test]
fn strict_stdio_dispatch_lists_tools_and_never_executes_notifications() {
    let mut called = 0;
    let mut invoke = |_: &str, _: &Value| {
        called += 1;
        Ok(json!({"ok":true}))
    };
    let out = boole_devnet_mcp::protocol::dispatch(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        &mut invoke,
    )
    .unwrap();
    assert_eq!(out["result"]["tools"].as_array().unwrap().len(), 6);
    assert!(boole_devnet_mcp::protocol::dispatch(r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"boole.devnet_commit","arguments":{}}}"#,&mut invoke).is_none());
    let bad=boole_devnet_mcp::protocol::dispatch(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"boole.devnet_commit","arguments":{"extra":true}}}"#,&mut invoke).unwrap();
    assert_eq!(bad["result"]["isError"], true);
    assert_eq!(called, 0);
}
#[test]
fn frames_are_bounded_and_partial_eof_is_refused() {
    use std::io::Cursor;
    assert!(boole_devnet_mcp::protocol::read_frame(&mut Cursor::new(b"{}".as_slice())).is_err());
    assert!(boole_devnet_mcp::protocol::read_frame(&mut Cursor::new(vec![b'a'; 65538])).is_err());
    assert_eq!(
        boole_devnet_mcp::protocol::read_frame(&mut Cursor::new(b"{}\r\n".as_slice())).unwrap(),
        Some("{}".into())
    );
}
