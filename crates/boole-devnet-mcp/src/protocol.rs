use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::io::BufRead;

pub fn read_frame(reader: &mut impl BufRead) -> Result<Option<String>> {
    let mut bytes = vec![];
    loop {
        let part = reader.fill_buf()?;
        if part.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            bail!("partial stdio frame");
        }
        let end = part.iter().position(|b| *b == b'\n');
        let n = end.map_or(part.len(), |i| i + 1);
        if bytes.len() + n > 65537 {
            bail!("stdio request exceeds 64 KiB");
        }
        bytes.extend(&part[..n]);
        reader.consume(n);
        if end.is_some() {
            break;
        }
    }
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    Ok(Some(String::from_utf8(bytes)?))
}
pub fn dispatch(
    message: &str,
    invoke: &mut impl FnMut(&str, &Value) -> Result<Value>,
) -> Option<Value> {
    let msg: Value = match serde_json::from_str(message) {
        Ok(v) => v,
        Err(_) => {
            return Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
            )
        }
    };
    let id = msg.get("id").cloned();
    if msg["jsonrpc"] != "2.0"
        || !msg["method"].is_string()
        || id
            .as_ref()
            .is_some_and(|id| !id.is_string() && !id.is_number())
    {
        return Some(
            json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request"}}),
        );
    }
    // No notification can trigger a write, including tools/call without an id.
    let id = id?;
    let result = match msg["method"].as_str().unwrap_or("") {
        "initialize" => {
            json!({"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"boole-devnet-mcp","version":env!("CARGO_PKG_VERSION")}})
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools":crate::tools()}),
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let args = msg["params"].get("arguments").cloned().unwrap_or(json!({}));
            let result = (|| {
                let tools = crate::tools();
                let tool = tools
                    .iter()
                    .find(|t| t["name"] == name)
                    .ok_or_else(|| anyhow::anyhow!("unknown devnet tool"))?;
                let object = args
                    .as_object()
                    .ok_or_else(|| anyhow::anyhow!("arguments must be an object"))?;
                let props = tool["inputSchema"]["properties"]
                    .as_object()
                    .expect("tool schema");
                anyhow::ensure!(
                    object.keys().all(|k| props.contains_key(k)),
                    "unknown tool argument"
                );
                anyhow::ensure!(
                    tool["inputSchema"]["required"]
                        .as_array()
                        .expect("schema")
                        .iter()
                        .all(|k| object.contains_key(k.as_str().expect("schema key"))),
                    "missing required argument"
                );
                invoke(name, &args)
            })();
            match result {
                Ok(value) => {
                    json!({"content":[{"type":"text","text":value.to_string()}],"isError":false})
                }
                Err(error) => {
                    json!({"content":[{"type":"text","text":error.to_string()}],"isError":true})
                }
            }
        }
        _ => {
            return Some(
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}}),
            )
        }
    };
    Some(json!({"jsonrpc":"2.0","id":id,"result":result}))
}
