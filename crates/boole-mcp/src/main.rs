//! P2.1 / P2.2 — `boole-mcp` binary. Carves out the Model Context
//! Protocol server crate and (P2.2) wires the first two read-only
//! tools that proxy to the upstream boole-node:
//!
//!   * `bounty.list`  -> upstream GET /work
//!   * `receipt.get`  -> upstream GET /receipts/{receipt_id}
//!
//! Surface:
//!   * `serve --node-url <url> --listen <host:port>`
//!   * resolved bind address echoed to stderr as
//!     `boole-mcp listening on http://<addr>` so the launcher can grab
//!     the ephemeral port when `:0` is requested
//!   * GET  /healthz       -> 200 `boole-mcp.v0`
//!   * GET  /mcp/tools     -> 200 JSON tool registry
//!   * POST /mcp/invoke    -> 200 with raw upstream JSON, or typed
//!     4xx/5xx error envelope (see below)
//!
//! Typed error shapes (always JSON):
//!   * unknown tool        -> 400 {"error":"unknown-tool","tool":"<name>"}
//!   * missing required arg-> 400 {"error":"missing-arg","arg":"<name>"}
//!   * upstream unreachable-> 502 {"error":"upstream-unreachable"}
//!
//! No signing, no key material, no mutation routes -- this is a
//! read-only proxy. The mutation/wallet surface lives in the signed
//! boole-cli / boole-wallet-agent path, not here.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;

#[derive(Parser)]
#[command(name = "boole-mcp", about = "Boole MCP server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        node_url: String,
        #[arg(long, default_value = "127.0.0.1:0")]
        listen: String,
    },
}

#[derive(Clone)]
struct AppState {
    node_url: String,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct InvokeRequest {
    tool: String,
    #[serde(default)]
    args: Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve { node_url, listen } => serve(&node_url, &listen).await,
    }
}

async fn serve(node_url: &str, listen: &str) -> Result<()> {
    let listener = TcpListener::bind(listen).await?;
    let addr: SocketAddr = listener.local_addr()?;
    eprintln!("boole-mcp listening on http://{addr}");
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .build()?;
    let state = Arc::new(AppState {
        node_url: node_url.trim_end_matches('/').to_string(),
        client,
    });
    let app = build_router(state);
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/mcp/tools", get(tools_list))
        .route("/mcp/invoke", post(invoke))
        .fallback(not_found)
        .with_state(state)
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "boole-mcp.v0")
}

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "")
}

async fn tools_list() -> impl IntoResponse {
    let body = json!({
        "tools": [
            {
                "name": "bounty.list",
                "description": "List currently-open bounties/work units from the upstream boole-node.",
                "input_schema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            },
            {
                "name": "receipt.get",
                "description": "Fetch a single proof receipt by id from the upstream boole-node.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "receipt_id": { "type": "string" }
                    },
                    "required": ["receipt_id"],
                    "additionalProperties": false
                }
            }
        ]
    });
    (StatusCode::OK, Json(body))
}

async fn invoke(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InvokeRequest>,
) -> (StatusCode, Json<Value>) {
    match req.tool.as_str() {
        "bounty.list" => proxy_get(&state, "/work").await,
        "receipt.get" => match req.args.get("receipt_id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => {
                let path = format!("/receipts/{id}");
                proxy_get(&state, &path).await
            }
            _ => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"missing-arg","arg":"receipt_id"})),
            ),
        },
        other => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"unknown-tool","tool":other})),
        ),
    }
}

async fn proxy_get(state: &AppState, path: &str) -> (StatusCode, Json<Value>) {
    let url = format!("{}{}", state.node_url, path);
    match state.client.get(&url).send().await {
        Ok(resp) => {
            let status = resp.status();
            match resp.text().await {
                Ok(body) => {
                    let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                    let mapped =
                        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
                    (mapped, Json(parsed))
                }
                Err(_) => (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"error":"upstream-unreachable"})),
                ),
            }
        }
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error":"upstream-unreachable"})),
        ),
    }
}
