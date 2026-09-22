//! Closed-local, keyless native RPC. No public/untrusted transport claim.

use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use boole_core::native_chain::{NativeBlock, NativeTransfer};
use boole_core::Hex32;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{Notify, Semaphore};

use crate::local_node::{BoundedHttpListener, HttpRemoteAddr};
use crate::native_node::NativeNode;

/// Bounded full-chain import for this first local prototype. Beyond this,
/// incremental authenticated sync/checkpoint work is required, not truncation.
pub const MAX_NATIVE_SYNC_BLOCKS: usize = 1024;
pub const MAX_NATIVE_SYNC_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
struct Api {
    node: Arc<Mutex<NativeNode>>,
    workers: Arc<Semaphore>,
    authority: SocketAddr,
}

pub fn bind_loopback(addr: SocketAddr) -> anyhow::Result<TcpListener> {
    anyhow::ensure!(addr.ip().is_loopback(), "native RPC is closed-local only");
    Ok(TcpListener::bind(addr)?)
}

/// Listener must already be numeric loopback; callers must check before bind
/// too. Signals are translated into `shutdown` by the executable wrapper.
pub async fn serve(
    listener: TcpListener,
    node: NativeNode,
    shutdown: Arc<Notify>,
) -> anyhow::Result<()> {
    let authority = listener.local_addr()?;
    anyhow::ensure!(
        authority.ip().is_loopback(),
        "native RPC is closed-local only"
    );
    listener.set_nonblocking(true)?;
    let listener = BoundedHttpListener::new(tokio::net::TcpListener::from_std(listener)?);
    let api = Api {
        node: Arc::new(Mutex::new(node)),
        workers: Arc::new(Semaphore::new(8)),
        authority,
    };
    let app = Router::new()
        .route("/native/info", get(info))
        .route("/ready", get(info))
        .route("/native/accounts/{pk}", get(account))
        .route("/native/transactions/{id}", get(transaction))
        .route("/native/blocks/{height}", get(block_by_height))
        .route(
            "/native/transfers",
            post(transfer).layer(DefaultBodyLimit::max(4096)),
        )
        .route(
            "/native/template",
            post(template).layer(DefaultBodyLimit::max(1024)),
        )
        .route("/native/blocks", post(block))
        .route(
            "/native/chain",
            post(adopt).layer(DefaultBodyLimit::max(MAX_NATIVE_SYNC_BYTES)),
        )
        .layer(DefaultBodyLimit::max(
            boole_core::native_network::native_testnet().max_block_bytes(),
        ))
        .layer(from_fn_with_state(api.clone(), boundary))
        .with_state(api);
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<HttpRemoteAddr>(),
    )
    .with_graceful_shutdown(async move { shutdown.notified().await });
    server.await?;
    Ok(())
}

async fn boundary(State(api): State<Api>, request: Request, next: Next) -> Response {
    if let Some(ConnectInfo(remote)) = request.extensions().get::<ConnectInfo<HttpRemoteAddr>>() {
        remote.mark_header_received();
    }
    let allowed_host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<SocketAddr>().ok())
        == Some(api.authority);
    let mut response = if !allowed_host
        || request.headers().contains_key(header::ORIGIN)
        || request.headers().contains_key("sec-fetch-site")
    {
        error(StatusCode::FORBIDDEN, "closed_local_client_required")
    } else {
        match tokio::time::timeout(Duration::from_secs(10), next.run(request)).await {
            Ok(response) => response,
            Err(_) => error(
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout_query_status_before_retry",
            ),
        }
    };
    response.headers_mut().insert(
        header::CONNECTION,
        header::HeaderValue::from_static("close"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({"error": code}))).into_response()
}

async fn operation<F>(api: Api, action: F) -> Response
where
    F: FnOnce(&mut NativeNode) -> anyhow::Result<Value> + Send + 'static,
{
    let Ok(permit) = api.workers.clone().try_acquire_owned() else {
        return error(StatusCode::TOO_MANY_REQUESTS, "native_worker_limit");
    };
    // Permit lives with the actual work, even if the HTTP caller times out.
    match tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut node = api.node.lock().map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "native state lock poisoned".to_string(),
            )
        })?;
        node.ensure_ready()
            .map_err(|error| (StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
        action(&mut node).map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))
    })
    .await
    {
        Ok(Ok(value)) => Json(value).into_response(),
        Ok(Err((status, message))) => error(status, &message),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "native_worker_failed"),
    }
}

async fn account(State(api): State<Api>, Path(pk): Path<String>) -> Response {
    operation(api, move |node| {
        anyhow::ensure!(
            Hex32::from_hex(&pk)?.to_hex() == pk,
            "noncanonical account key"
        );
        let ledger = node.chain().ledger();
        let pending = node.pending_view()?;
        Ok(json!({"pk": pk, "height": ledger.height().to_string(),
            "balance": ledger.balance(&pk).to_string(),
            "locked": ledger.locked_balance(&pk).to_string(),
            "spendable": ledger.spendable_balance(&pk).to_string(),
            "nextBlockAvailable": pending.available_balance(&pk).to_string(),
            "confirmedNonce": ledger.next_nonce(&pk).to_string(),
            "pendingNonce": pending.next_nonce(&pk).to_string()}))
    })
    .await
}

fn transaction_status(node: &NativeNode, id: &Hex32) -> Value {
    if let Some(height) = node.confirmed_height(id) {
        json!({"txid": id.to_hex(), "status": "confirmed", "height": height.to_string(),
            "confirmations": (node.chain().ledger().height() - height + 1).to_string(), "final": false})
    } else {
        let pending = node.pending().iter().any(|transfer| transfer.id() == *id);
        json!({"txid": id.to_hex(), "status": if pending { "pending" } else { "unknown" }, "final": false})
    }
}

async fn transaction(State(api): State<Api>, Path(id): Path<String>) -> Response {
    operation(api, move |node| {
        Ok(transaction_status(node, &Hex32::from_hex(&id)?))
    })
    .await
}

async fn block_by_height(State(api): State<Api>, Path(height): Path<u64>) -> Response {
    operation(api, move |node| {
        let index = usize::try_from(
            height
                .checked_sub(1)
                .ok_or_else(|| anyhow::anyhow!("height starts at 1"))?,
        )?;
        Ok(serde_json::to_value(
            node.chain()
                .blocks()
                .get(index)
                .ok_or_else(|| anyhow::anyhow!("block not found"))?,
        )?)
    })
    .await
}

async fn transfer(State(api): State<Api>, Json(transfer): Json<NativeTransfer>) -> Response {
    operation(api, move |node| {
        let id = transfer.id();
        node.submit_transfer(transfer)?;
        Ok(transaction_status(node, &id))
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TemplateRequest {
    producer_pk: String,
    reward_pk: String,
    timestamp_ms: u64,
}

async fn template(State(api): State<Api>, Json(input): Json<TemplateRequest>) -> Response {
    operation(api, move |node| {
        Ok(serde_json::to_value(node.template(
            &input.producer_pk,
            &input.reward_pk,
            input.timestamp_ms,
        )?)?)
    })
    .await
}

async fn block(State(api): State<Api>, Json(block): Json<NativeBlock>) -> Response {
    operation(api, move |node| {
        let hash = block.hash()?.to_hex();
        let accepted = node.submit_block(block)?;
        Ok(json!({"accepted": accepted, "blockHash": hash, "height": node.chain().ledger().height().to_string()}))
    }).await
}

async fn adopt(State(api): State<Api>, Json(blocks): Json<Vec<NativeBlock>>) -> Response {
    operation(api, move |node| {
        anyhow::ensure!(blocks.len() <= MAX_NATIVE_SYNC_BLOCKS, "native RPC sync block limit");
        let adopted = node.adopt_chain(&blocks)?;
        Ok(json!({"adopted": adopted, "headHash": node.chain().head_hash().to_hex(), "height": node.chain().ledger().height().to_string()}))
    }).await
}

async fn info(State(api): State<Api>) -> Response {
    operation(api, |node| {
        let policy = boole_core::native_network::native_testnet();
        Ok(
            json!({"policy": policy, "genesisHash": policy.genesis_hash().to_hex(),
            "height": node.chain().ledger().height().to_string(),
            "headHash": node.chain().head_hash().to_hex(),
            "cumulativeWork": node.chain().cumulative_work().to_string(),
            "minimumTimestampMs": node.chain().minimum_timestamp_ms()?.to_string(),
            "issued": node.chain().ledger().issued().to_string(), "pending": node.pending().len(),
            "transport": "closed-local", "ready": true}),
        )
    })
    .await
}
