//! Closed-local, keyless native RPC. No public/untrusted transport claim.

use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use boole_core::native_chain::{NativeBlock, NativeTransfer};
use boole_core::Hex32;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};

use crate::local_node::{BoundedHttpListener, HttpRemoteAddr};
use crate::native_node::NativeNode;
use crate::p2p_lifecycle::P2pLifecycle;
use crate::{NativePeerConfig, NativePeerMonitor, NativePeerService};

/// Bounded full-chain import for this first local prototype. Beyond this,
/// incremental authenticated sync/checkpoint work is required, not truncation.
pub const MAX_NATIVE_SYNC_BLOCKS: usize = 1024;
pub const MAX_NATIVE_SYNC_BYTES: usize = 8 * 1024 * 1024;
const MAX_NATIVE_REQUESTS: usize = 8;
const MAX_NATIVE_ERROR_BYTES: usize = 4096;

/// One admission slot from before body extraction through actual blocking work.
/// Axum extensions clone this handle, not the underlying semaphore permit.
#[derive(Clone)]
struct RequestLease {
    _permit: Arc<OwnedSemaphorePermit>,
}

#[derive(Clone)]
struct Api {
    node: Arc<Mutex<NativeNode>>,
    workers: Arc<Semaphore>,
    authority: SocketAddr,
    lifecycle: Arc<P2pLifecycle>,
    peers: Option<NativePeerMonitor>,
}

pub fn bind_loopback(addr: SocketAddr) -> anyhow::Result<TcpListener> {
    anyhow::ensure!(addr.ip().is_loopback(), "native RPC is closed-local only");
    Ok(TcpListener::bind(addr)?)
}

pub async fn serve_with_peers(
    listener: TcpListener,
    node: NativeNode,
    peer_listener: TcpListener,
    peer_config: NativePeerConfig,
    shutdown: Arc<Notify>,
) -> anyhow::Result<()> {
    serve_inner(listener, node, Some((peer_listener, peer_config)), shutdown).await
}

/// Listener must already be numeric loopback; callers must check before bind
/// too. Signals are translated into `shutdown` by the executable wrapper.
pub async fn serve(
    listener: TcpListener,
    node: NativeNode,
    shutdown: Arc<Notify>,
) -> anyhow::Result<()> {
    serve_inner(listener, node, None, shutdown).await
}

struct StopGuard {
    lifecycle: Arc<P2pLifecycle>,
    peers: Option<NativePeerService>,
}

impl Drop for StopGuard {
    fn drop(&mut self) {
        self.lifecycle.stop();
        if let Some(peers) = &mut self.peers {
            peers.stop();
        }
    }
}

async fn serve_inner(
    listener: TcpListener,
    node: NativeNode,
    peers: Option<(TcpListener, NativePeerConfig)>,
    shutdown: Arc<Notify>,
) -> anyhow::Result<()> {
    let authority = listener.local_addr()?;
    anyhow::ensure!(
        authority.ip().is_loopback(),
        "native RPC is closed-local only"
    );
    listener.set_nonblocking(true)?;
    let listener = BoundedHttpListener::new(tokio::net::TcpListener::from_std(listener)?);
    let node = Arc::new(Mutex::new(node));
    let lifecycle = Arc::new(P2pLifecycle::new());
    let peers = peers
        .map(|(listener, config)| {
            NativePeerService::start_with_lifecycle(
                listener,
                node.clone(),
                config,
                lifecycle.clone(),
            )
        })
        .transpose()?;
    let peer_monitor = peers.as_ref().map(NativePeerService::monitor);
    let guard = StopGuard {
        lifecycle: lifecycle.clone(),
        peers,
    };
    let api = Api {
        node,
        workers: Arc::new(Semaphore::new(MAX_NATIVE_REQUESTS)),
        authority,
        lifecycle: lifecycle.clone(),
        peers: peer_monitor,
    };
    let app = router(api);
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<HttpRemoteAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown.notified().await;
        // Close both network mutation boundaries before draining HTTP.
        lifecycle.request_stop();
    });
    let result = server.await;
    // Timed-out HTTP requests retain their mutation permit until their actual
    // blocking work finishes. State ownership is not released before this.
    tokio::task::spawn_blocking(move || drop(guard)).await?;
    result?;
    Ok(())
}

fn router(api: Api) -> Router {
    Router::new()
        .route("/native/info", get(info))
        .route("/native/peers", get(peer_status))
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
        .with_state(api)
}

async fn boundary(State(api): State<Api>, mut request: Request, next: Next) -> Response {
    if let Some(ConnectInfo(remote)) = request.extensions().get::<ConnectInfo<HttpRemoteAddr>>() {
        remote.mark_header_received();
    }
    let allowed_host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<SocketAddr>().ok())
        == Some(api.authority);
    let mut admission = None;
    let mut response = if !allowed_host
        || request.headers().contains_key(header::ORIGIN)
        || request.headers().contains_key("sec-fetch-site")
    {
        error(StatusCode::FORBIDDEN, "closed_local_client_required")
    } else if let Ok(permit) = api.workers.clone().try_acquire_owned() {
        // This precedes Json/body extraction, including 100-continue. Keep the
        // same permit with the handler and then its real blocking operation;
        // cancellation must not admit replacement work while a mutation runs.
        let lease = RequestLease {
            _permit: Arc::new(permit),
        };
        request.extensions_mut().insert(lease.clone());
        admission = Some(lease);
        match tokio::time::timeout(Duration::from_secs(10), next.run(request)).await {
            Ok(response) => response,
            Err(_) => error(
                StatusCode::REQUEST_TIMEOUT,
                "request_timeout_query_status_before_retry",
            ),
        }
    } else {
        error(StatusCode::TOO_MANY_REQUESTS, "native_worker_limit")
    };
    // Serde diagnostics can reflect attacker-controlled field names more than
    // once. Do not retain large error bodies on slowly draining connections.
    // Keep admission until this bounded response conversion is complete.
    if response.status().is_client_error() || response.status().is_server_error() {
        let (parts, body) = response.into_parts();
        response = match to_bytes(body, MAX_NATIVE_ERROR_BYTES).await {
            Ok(bytes) => Response::from_parts(parts, Body::from(bytes)),
            Err(_) => error(parts.status, "native_error_response_limit"),
        };
    }
    response.headers_mut().insert(
        header::CONNECTION,
        header::HeaderValue::from_static("close"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    drop(admission);
    response
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({"error": code}))).into_response()
}

async fn peer_status(
    State(api): State<Api>,
    Extension(lease): Extension<RequestLease>,
) -> Response {
    let monitor = api.peers.clone();
    operation(api, lease, move |_| match monitor {
        Some(monitor) => Ok(serde_json::to_value(monitor.snapshot())?),
        None => Ok(json!({"enabled": false, "running": false, "peers": []})),
    })
    .await
}

async fn operation<F>(api: Api, lease: RequestLease, action: F) -> Response
where
    F: FnOnce(&mut NativeNode) -> anyhow::Result<Value> + Send + 'static,
{
    // Permit lives with the actual work, even if the HTTP caller times out.
    match tokio::task::spawn_blocking(move || {
        let _permit = lease;
        let _mutation = api.lifecycle.begin_mutation().ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "native node stopping".to_string(),
            )
        })?;
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

async fn account(
    State(api): State<Api>,
    Extension(lease): Extension<RequestLease>,
    Path(pk): Path<String>,
) -> Response {
    operation(api, lease, move |node| {
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
        let pending = node.is_pending(id);
        json!({"txid": id.to_hex(), "status": if pending { "pending" } else { "unknown" }, "final": false})
    }
}

async fn transaction(
    State(api): State<Api>,
    Extension(lease): Extension<RequestLease>,
    Path(id): Path<String>,
) -> Response {
    operation(api, lease, move |node| {
        Ok(transaction_status(node, &Hex32::from_hex(&id)?))
    })
    .await
}

async fn block_by_height(
    State(api): State<Api>,
    Extension(lease): Extension<RequestLease>,
    Path(height): Path<u64>,
) -> Response {
    operation(api, lease, move |node| {
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

async fn transfer(
    State(api): State<Api>,
    Extension(lease): Extension<RequestLease>,
    Json(transfer): Json<NativeTransfer>,
) -> Response {
    operation(api, lease, move |node| {
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

async fn template(
    State(api): State<Api>,
    Extension(lease): Extension<RequestLease>,
    Json(input): Json<TemplateRequest>,
) -> Response {
    operation(api, lease, move |node| {
        Ok(serde_json::to_value(node.template(
            &input.producer_pk,
            &input.reward_pk,
            input.timestamp_ms,
        )?)?)
    })
    .await
}

async fn block(
    State(api): State<Api>,
    Extension(lease): Extension<RequestLease>,
    Json(block): Json<NativeBlock>,
) -> Response {
    operation(api, lease, move |node| {
        let hash = block.hash()?.to_hex();
        let accepted = node.submit_block(block)?;
        Ok(json!({"accepted": accepted, "blockHash": hash, "height": node.chain().ledger().height().to_string()}))
    }).await
}

async fn adopt(
    State(api): State<Api>,
    Extension(lease): Extension<RequestLease>,
    Json(blocks): Json<Vec<NativeBlock>>,
) -> Response {
    operation(api, lease, move |node| {
        anyhow::ensure!(blocks.len() <= MAX_NATIVE_SYNC_BLOCKS, "native RPC sync block limit");
        let adopted = node.adopt_chain(&blocks)?;
        Ok(json!({"adopted": adopted, "headHash": node.chain().head_hash().to_hex(), "height": node.chain().ledger().height().to_string()}))
    }).await
}

async fn info(State(api): State<Api>, Extension(lease): Extension<RequestLease>) -> Response {
    operation(api, lease, |node| {
        let policy = boole_core::native_network::native_testnet();
        Ok(
            json!({"policy": policy, "genesisHash": policy.genesis_hash().to_hex(),
            "height": node.chain().ledger().height().to_string(),
            "headHash": node.chain().head_hash().to_hex(),
            "cumulativeWork": node.chain().cumulative_work().to_string(),
            "minimumTimestampMs": node.chain().minimum_timestamp_ms()?.to_string(),
            "issued": node.chain().ledger().issued().to_string(), "pending": node.pending().len(),
            "resources": node.resource_usage()?,
            "transport": "closed-local", "ready": true}),
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Instant;

    fn request(address: SocketAddr, path: &str, body: Option<&str>) -> String {
        let mut socket = TcpStream::connect(address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let method = if body.is_some() { "POST" } else { "GET" };
        let body = body.unwrap_or("");
        write!(socket, "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn timed_out_http_callers_do_not_release_admission_while_their_actual_mutations_wait() {
        let dir = crate::durability::PrivateTempDir::new_in(
            &std::env::temp_dir(),
            "boole-native-http-held-mutation",
        )
        .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let node = NativeNode::open(dir.path()).unwrap();
        let owner = boole_core::SigningKeyV2::from_dev_id("native-http-timeout-owner");
        let block = node
            .template(&owner.pk_hex(), &owner.pk_hex(), 60_000)
            .unwrap()
            .mine(0, 2_000_000)
            .unwrap()
            .unwrap();
        let auth = owner
            .sign_for_network(
                &block.authorization_payload().unwrap(),
                Some(boole_core::native_network::native_testnet().network_id()),
            )
            .unwrap();
        let block = block.authorize(&auth).unwrap();
        let api = Api {
            node: Arc::new(Mutex::new(node)),
            workers: Arc::new(Semaphore::new(MAX_NATIVE_REQUESTS)),
            authority: address,
            lifecycle: Arc::new(P2pLifecycle::new()),
            peers: None,
        };
        let app = router(api.clone());
        let stop = Arc::new(Notify::new());
        let shutdown = stop.clone();
        let server = runtime.spawn(async move {
            axum::serve(
                BoundedHttpListener::new(tokio::net::TcpListener::from_std(listener).unwrap()),
                app.into_make_service_with_connect_info::<HttpRemoteAddr>(),
            )
            .with_graceful_shutdown(async move {
                shutdown.notified().await;
            })
            .await
            .unwrap();
        });
        // Fault injection at the actual node lock; all requests still traverse
        // the production router, body extraction, ten-second deadline and block
        // submission. No fake operation replaces the durable mutation path.
        let held = api.node.lock().unwrap();
        let callers: Vec<_> = (0..MAX_NATIVE_REQUESTS)
            .map(|_| {
                let body = serde_json::to_string(&block).unwrap();
                std::thread::spawn(move || request(address, "/native/blocks", Some(&body)))
            })
            .collect();
        for caller in callers {
            let response = caller.join().unwrap();
            assert!(response.starts_with("HTTP/1.1 408"), "{response}");
            assert!(response.contains("query_status_before_retry"));
        }
        assert_eq!(api.workers.available_permits(), 0);
        assert!(request(address, "/native/info", None).starts_with("HTTP/1.1 429"));
        assert_eq!(held.chain().ledger().height(), 0);
        drop(held);
        let deadline = Instant::now() + Duration::from_secs(5);
        while api.workers.available_permits() != MAX_NATIVE_REQUESTS {
            assert!(Instant::now() < deadline, "actual mutations did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(request(address, "/native/info", None).starts_with("HTTP/1.1 200"));
        assert_eq!(
            api.node.lock().unwrap().chain().head_hash(),
            block.hash().unwrap()
        );
        stop.notify_one();
        runtime.block_on(server).unwrap();
        api.lifecycle.stop();
        drop(api);
        let reopened = NativeNode::open(dir.path()).unwrap();
        assert_eq!(reopened.chain().head_hash(), block.hash().unwrap());
        assert_eq!(reopened.chain().ledger().height(), 1);
        assert_eq!(
            reopened.chain().ledger().issued(),
            50_000 * boole_core::native_network::NATIVE_COIN_UNIT
        );
    }
}
