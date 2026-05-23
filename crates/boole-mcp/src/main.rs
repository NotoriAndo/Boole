//! P2.1 — `boole-mcp` scaffolding. This slice carves out the binary
//! crate that will host the Model Context Protocol tool surface in
//! P2.2 (slice 34). The scope here is intentionally narrow:
//!
//! - `serve --node-url <url> --listen <host:port>` subcommand
//! - HTTP server bound on the requested address; the resolved address
//!   is printed to stderr as `boole-mcp listening on http://<addr>`
//!   so a launcher invoking with `:0` can grab the ephemeral port
//! - `GET /healthz` returns `200 boole-mcp.v0`
//! - `GET /mcp/tools` returns 404 placeholder until P2.2 wires the
//!   bounty/receipt tool schema
//!
//! No `boole-core` / `boole-node` coupling yet — those land with the
//! actual MCP tool implementations. Keeping the dependency graph
//! shallow lets this slice ship without a recompile cascade.
//!
//! `--node-url` is accepted (and required) at this slice even though
//! no requests are issued yet; this keeps the launcher contract
//! stable across P2.1 -> P2.2 so the surrounding tooling does not
//! have to re-learn the flag set.

use std::net::SocketAddr;

use anyhow::Result;
use axum::{http::StatusCode, response::IntoResponse, routing::get, Router};
use clap::{Parser, Subcommand};
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve { node_url, listen } => serve(&node_url, &listen).await,
    }
}

async fn serve(_node_url: &str, listen: &str) -> Result<()> {
    let listener = TcpListener::bind(listen).await?;
    let addr: SocketAddr = listener.local_addr()?;
    eprintln!("boole-mcp listening on http://{addr}");
    let app = build_router();
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router() -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .fallback(not_found)
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "boole-mcp.v0")
}

async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, "")
}
