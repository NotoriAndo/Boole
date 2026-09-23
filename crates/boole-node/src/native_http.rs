//! Closed-local, keyless native RPC. No public/untrusted transport claim.

use std::future::IntoFuture;
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use boole_core::native_chain::{NativeBlock, NativeTransfer};
use boole_core::Hex32;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};

use crate::local_node::{BoundedHttpListener, HttpRemoteAddr, HTTP_SHUTDOWN_DRAIN_TIMEOUT};
use crate::native_node::NativeNode;
use crate::p2p_lifecycle::P2pLifecycle;
use crate::{NativePeerConfig, NativePeerMonitor, NativePeerService};

/// Bounded full-chain import for this first local prototype. Beyond this,
/// incremental authenticated sync/checkpoint work is required, not truncation.
pub const MAX_NATIVE_SYNC_BLOCKS: usize = 1024;
pub const MAX_NATIVE_SYNC_BYTES: usize = 8 * 1024 * 1024;
const MAX_NATIVE_REQUESTS: usize = 8;
const MAX_NATIVE_DIAGNOSTICS: usize = 2;
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
    diagnostic_workers: Arc<Semaphore>,
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
    http_connections: Arc<P2pLifecycle>,
}

impl Drop for StopGuard {
    fn drop(&mut self) {
        self.http_connections.request_stop();
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
    let http_connections = Arc::new(P2pLifecycle::new());
    let listener = BoundedHttpListener::new(tokio::net::TcpListener::from_std(listener)?)
        .with_socket_lifecycle(http_connections.clone());
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
        http_connections: http_connections.clone(),
    };
    let api = Api {
        node: node.clone(),
        workers: Arc::new(Semaphore::new(MAX_NATIVE_REQUESTS)),
        diagnostic_workers: Arc::new(Semaphore::new(MAX_NATIVE_DIAGNOSTICS)),
        authority,
        lifecycle: lifecycle.clone(),
        peers: peer_monitor,
    };
    let app = router(api);
    let result = serve_http(listener, app, lifecycle, http_connections, shutdown).await;
    // Timed-out or disconnected HTTP callers retain their mutation permit until
    // their actual blocking work finishes. Client-I/O expiry never releases the
    // state owner before this existing durable-mutation barrier is crossed.
    tokio::task::spawn_blocking(move || drop(guard)).await?;
    release_node_owner(node).await?;
    result?;
    Ok(())
}

async fn release_node_owner(mut owner: Arc<Mutex<NativeNode>>) -> anyhow::Result<()> {
    // Connection completion can precede the final service-reference drop.
    // Success must release the real file locks here, not on another task later.
    // This is bounded cleanup after the existing mutation barrier, never a new
    // indefinite wait or permission to force an active owner off its locks.
    tokio::time::timeout(Duration::from_secs(1), async move {
        loop {
            match Arc::try_unwrap(owner) {
                Ok(node) => {
                    drop(node);
                    return;
                }
                Err(retained) => owner = retained,
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!(
            "native shutdown retained state ownership; do not force a replacement owner"
        )
    })
}

async fn serve_http(
    listener: BoundedHttpListener,
    app: Router,
    lifecycle: Arc<P2pLifecycle>,
    http_connections: Arc<P2pLifecycle>,
    shutdown: Arc<Notify>,
) -> std::io::Result<()> {
    let drain_started = Arc::new(Notify::new());
    let signal_drain = drain_started.clone();
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<HttpRemoteAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown.notified().await;
        // Close both network mutation boundaries before draining HTTP.
        lifecycle.request_stop();
        signal_drain.notify_one();
    })
    .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result,
        _ = drain_started.notified() => {
            match tokio::time::timeout(HTTP_SHUTDOWN_DRAIN_TIMEOUT, &mut server).await {
                Ok(result) => result,
                Err(_) => {
                    eprintln!("boole-node: native HTTP drain expired; closing remaining client sockets");
                    // Axum owns independent connection tasks. Dropping only the
                    // outer future would leave their I/O/state references alive.
                    // Wake actual sockets, then wait for those tasks to finish.
                    http_connections.request_stop();
                    server.await
                }
            }
        }
    }
}

fn router(api: Api) -> Router {
    Router::new()
        .route("/native/info", get(info))
        .route("/native/peers", get(peer_status))
        .route("/native/diagnostics", get(diagnostics))
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
    // A tiny, separate observation budget must remain available while normal
    // RPC work waits on the ledger. It grants no state access or readiness.
    let diagnostic =
        request.method() == Method::GET && request.uri().path() == "/native/diagnostics";
    let workers = if diagnostic {
        &api.diagnostic_workers
    } else {
        &api.workers
    };
    let mut response = if !allowed_host
        || request.headers().contains_key(header::ORIGIN)
        || request.headers().contains_key("sec-fetch-site")
    {
        error(StatusCode::FORBIDDEN, "closed_local_client_required")
    } else if let Ok(permit) = workers.clone().try_acquire_owned() {
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
        error(
            StatusCode::TOO_MANY_REQUESTS,
            if diagnostic {
                "native_diagnostic_limit"
            } else {
                "native_worker_limit"
            },
        )
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

async fn diagnostics(
    State(api): State<Api>,
    Extension(_lease): Extension<RequestLease>,
) -> Response {
    // Never acquire the ledger lock or read its files here. This is process/
    // transport observation, explicitly not permission to sign or mutate.
    let peers = match api.peers {
        Some(monitor) => match serde_json::to_value(monitor.snapshot()) {
            Ok(value) => value,
            Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE, "native_diagnostic_failed"),
        },
        None => json!({"enabled": false, "running": false, "peers": []}),
    };
    Json(json!({
        "schema": "boole.native.diagnostics.v1",
        "authority": "local_process_only",
        "ledgerReadiness": "not_checked",
        "stopping": api.lifecycle.is_stopped(),
        "rpc": {
            "activeRequests": MAX_NATIVE_REQUESTS - api.workers.available_permits(),
            "requestLimit": MAX_NATIVE_REQUESTS,
            "activeDiagnostics": MAX_NATIVE_DIAGNOSTICS - api.diagnostic_workers.available_permits(),
            "diagnosticLimit": MAX_NATIVE_DIAGNOSTICS
        },
        "peers": peers
    })).into_response()
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
        request_with_headers(address, path, body, &format!("Host: {address}\r\n"))
    }

    fn request_with_headers(
        address: SocketAddr,
        path: &str,
        body: Option<&str>,
        headers: &str,
    ) -> String {
        let mut socket = TcpStream::connect(address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let method = if body.is_some() { "POST" } else { "GET" };
        let body = body.unwrap_or("");
        write!(socket, "{method} {path} HTTP/1.1\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).unwrap();
        response
    }

    #[test]
    fn shutdown_state_owner_cleanup_is_bounded_and_never_forces_a_retained_owner() {
        let dir = crate::durability::PrivateTempDir::new_in(
            &std::env::temp_dir(),
            "boole-native-http-retained-owner",
        )
        .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let node = Arc::new(Mutex::new(NativeNode::open(dir.path()).unwrap()));
        let retained = node.clone();
        let history = std::fs::read(dir.path().join(crate::NATIVE_BLOCKS_FILE)).unwrap();
        let manifest = std::fs::read(dir.path().join("state.manifest.json")).unwrap();
        let started = Instant::now();
        let error = runtime.block_on(release_node_owner(node)).unwrap_err();
        let elapsed = started.elapsed();
        assert!(error.to_string().contains("retained state ownership"));
        assert!(
            elapsed < Duration::from_secs(3),
            "cleanup is not bounded: {elapsed:?}"
        );
        assert!(
            NativeNode::open(dir.path()).is_err(),
            "cleanup must not force another owner off its locks"
        );
        assert_eq!(
            std::fs::read(dir.path().join(crate::NATIVE_BLOCKS_FILE)).unwrap(),
            history
        );
        assert_eq!(
            std::fs::read(dir.path().join("state.manifest.json")).unwrap(),
            manifest
        );
        drop(retained);
        let reopened = NativeNode::open(dir.path()).unwrap();
        assert_eq!(reopened.chain().ledger().height(), 0);
        runtime
            .block_on(release_node_owner(Arc::new(Mutex::new(reopened))))
            .unwrap();
        assert_eq!(
            NativeNode::open(dir.path())
                .unwrap()
                .chain()
                .ledger()
                .height(),
            0
        );
        eprintln!(
            "native-http-state-owner-timeout elapsedMs={} retainedLockPreserved=true",
            elapsed.as_millis()
        );
    }

    #[test]
    fn shutdown_disconnects_clients_but_waits_for_an_already_admitted_block() {
        let dir = crate::durability::PrivateTempDir::new_in(
            &std::env::temp_dir(),
            "boole-native-http-shutdown-admitted-block",
        )
        .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let node = NativeNode::open(dir.path()).unwrap();
        let owner = boole_core::SigningKeyV2::from_dev_id("native-http-shutdown-admitted-owner");
        let block = node
            .template(&owner.pk_hex(), &owner.pk_hex(), 60_000)
            .unwrap()
            .mine(0, 2_000_000)
            .unwrap()
            .unwrap();
        let authorization = owner
            .sign_for_network(
                &block.authorization_payload().unwrap(),
                Some(boole_core::native_network::native_testnet().network_id()),
            )
            .unwrap();
        let block = block.authorize(&authorization).unwrap();
        let api = Api {
            node: Arc::new(Mutex::new(node)),
            workers: Arc::new(Semaphore::new(MAX_NATIVE_REQUESTS)),
            diagnostic_workers: Arc::new(Semaphore::new(MAX_NATIVE_DIAGNOSTICS)),
            authority: address,
            lifecycle: Arc::new(P2pLifecycle::new()),
            peers: None,
        };
        let http_connections = Arc::new(P2pLifecycle::new());
        let guard = StopGuard {
            lifecycle: api.lifecycle.clone(),
            peers: None,
            http_connections: http_connections.clone(),
        };
        let final_owner = api.node.clone();
        let lifecycle = api.lifecycle.clone();
        let app = router(api.clone());
        let stop = Arc::new(Notify::new());
        let shutdown = stop.clone();
        let mut server = runtime.spawn(async move {
            let result = serve_http(
                BoundedHttpListener::new(tokio::net::TcpListener::from_std(listener).unwrap())
                    .with_socket_lifecycle(http_connections.clone()),
                app,
                lifecycle,
                http_connections,
                shutdown,
            )
            .await;
            tokio::task::spawn_blocking(move || drop(guard))
                .await
                .unwrap();
            release_node_owner(final_owner).await.unwrap();
            result.unwrap();
        });
        // Hold the real node mutex, not a fake mutation. A test-only read of
        // the existing lifecycle gate confirms the real HTTP work was admitted
        // before requesting shutdown; no sleep guesses that ordering.
        let held = api.node.lock().unwrap();
        let mut clients = Vec::new();
        let body = serde_json::to_string(&block).unwrap();
        let mut block_client = TcpStream::connect(address).unwrap();
        write!(block_client, "POST /native/blocks HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        clients.push(block_client);
        let admitted_by = Instant::now() + Duration::from_secs(3);
        while !api.lifecycle.has_active_mutation() {
            assert!(
                Instant::now() < admitted_by,
                "actual block operation was not admitted"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        for _ in 1..MAX_NATIVE_REQUESTS {
            let mut socket = TcpStream::connect(address).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            write!(socket, "POST /native/chain HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: 8192\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n").unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                assert!(headers.len() < 8192);
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                headers.push(byte[0]);
            }
            assert!(String::from_utf8(headers)
                .unwrap()
                .starts_with("HTTP/1.1 100"));
            socket.write_all(b"[").unwrap();
            clients.push(socket);
        }
        assert_eq!(api.workers.available_permits(), 0);
        let started = Instant::now();
        stop.notify_one();
        let completion = runtime
            .block_on(async { tokio::time::timeout(Duration::from_secs(6), &mut server).await });
        let waited_for_mutation = completion.is_err();
        let mut closed_clients = 0;
        for socket in &mut clients {
            socket
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut response = Vec::new();
            match socket.read_to_end(&mut response) {
                Ok(_) => closed_clients += 1,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
                    ) =>
                {
                    closed_clients += 1
                }
                Err(_) => {}
            }
        }
        let mutation_still_admitted = api.lifecycle.has_active_mutation();
        let original_height = held.chain().ledger().height();
        let original_locked = NativeNode::open(dir.path()).is_err();
        let remaining_permits = api.workers.available_permits();
        // Always release the injected delay and fixture references before any
        // outcome assertion, so RED cannot strand the real mutation barrier.
        drop(clients);
        drop(held);
        drop(api);
        if let Ok(result) = completion {
            result.unwrap();
        } else {
            runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(5), &mut server)
                    .await
                    .expect("admitted block completes after release")
                    .unwrap();
            });
        }
        let mut reopened = NativeNode::open(dir.path()).unwrap();
        assert!(waited_for_mutation && mutation_still_admitted && original_locked);
        assert_eq!(original_height, 0);
        assert_eq!(closed_clients, 8);
        assert_eq!(
            remaining_permits, 7,
            "only the admitted block keeps its request slot"
        );
        assert_eq!(reopened.chain().head_hash(), block.hash().unwrap());
        assert_eq!(reopened.chain().ledger().height(), 1);
        assert_eq!(
            reopened.chain().ledger().issued(),
            50_000 * boole_core::native_network::NATIVE_COIN_UNIT
        );
        assert_eq!(reopened.resource_usage().unwrap().history_blocks, 1);
        assert!(
            !reopened.submit_block(block).unwrap(),
            "same block cannot issue twice"
        );
        assert_eq!(reopened.chain().ledger().height(), 1);
        eprintln!("native-http-shutdown-mutation elapsedMs={} closedClients={closed_clients} admittedBlockPreserved=true", started.elapsed().as_millis());
    }

    #[test]
    fn process_diagnostics_remain_available_while_all_state_requests_wait_on_the_ledger() {
        let dir = crate::durability::PrivateTempDir::new_in(
            &std::env::temp_dir(),
            "boole-native-http-independent-diagnostics",
        )
        .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let api = Api {
            node: Arc::new(Mutex::new(NativeNode::open(dir.path()).unwrap())),
            workers: Arc::new(Semaphore::new(MAX_NATIVE_REQUESTS)),
            diagnostic_workers: Arc::new(Semaphore::new(MAX_NATIVE_DIAGNOSTICS)),
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
        let held = api.node.lock().unwrap();
        let callers: Vec<_> = (0..MAX_NATIVE_REQUESTS)
            .map(|_| std::thread::spawn(move || request(address, "/native/info", None)))
            .collect();
        let deadline = Instant::now() + Duration::from_secs(3);
        while api.workers.available_permits() != 0 {
            assert!(
                Instant::now() < deadline,
                "state requests were not admitted"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let before = Instant::now();
        let diagnostic = request(address, "/native/diagnostics", None);
        let elapsed = before.elapsed();
        // Always release real waiting work and stop the server, including RED.
        drop(held);
        for caller in callers {
            assert!(caller.join().unwrap().starts_with("HTTP/1.1 200"));
        }
        stop.notify_one();
        runtime.block_on(server).unwrap();
        api.lifecycle.stop();
        assert!(diagnostic.starts_with("HTTP/1.1 200"), "{diagnostic}");
        assert!(
            elapsed < Duration::from_secs(1),
            "diagnostic waited behind ledger: {elapsed:?}"
        );
        let value: Value =
            serde_json::from_str(diagnostic.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(value["schema"], "boole.native.diagnostics.v1");
        assert_eq!(value["ledgerReadiness"], "not_checked");
        assert_eq!(value["rpc"]["activeRequests"], MAX_NATIVE_REQUESTS);
        assert_eq!(value["rpc"]["activeDiagnostics"], 1);
        assert_eq!(value["peers"]["enabled"], false);
        assert!(value.get("ready").is_none());
        assert!(value.get("headHash").is_none());
        assert!(value.get("balance").is_none());
    }

    #[test]
    fn diagnostics_do_not_confer_readiness_or_bypass_their_own_limit_and_shutdown_boundary() {
        let dir = crate::durability::PrivateTempDir::new_in(
            &std::env::temp_dir(),
            "boole-native-http-diagnostic-boundaries",
        )
        .unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let api = Api {
            node: Arc::new(Mutex::new(NativeNode::open(dir.path()).unwrap())),
            workers: Arc::new(Semaphore::new(MAX_NATIVE_REQUESTS)),
            diagnostic_workers: Arc::new(Semaphore::new(MAX_NATIVE_DIAGNOSTICS)),
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
        // Inject occupancy at the real bounded observation budget. A refused
        // diagnostic must not consume or disable ordinary state admission.
        let occupied = api
            .diagnostic_workers
            .clone()
            .try_acquire_many_owned(MAX_NATIVE_DIAGNOSTICS as u32)
            .unwrap();
        let excess = request(address, "/native/diagnostics", None);
        assert!(excess.starts_with("HTTP/1.1 429"));
        assert!(excess.contains("native_diagnostic_limit"));
        assert!(request(address, "/ready", None).starts_with("HTTP/1.1 200"));
        drop(occupied);
        for headers in [
            "Host: unrelated.invalid\r\n".to_string(),
            format!("Host: {address}\r\nOrigin: https://untrusted.invalid\r\n"),
            format!("Host: {address}\r\nSec-Fetch-Site: cross-site\r\n"),
        ] {
            let response = request_with_headers(address, "/native/diagnostics", None, &headers);
            assert!(response.starts_with("HTTP/1.1 403"), "{response}");
            assert_eq!(
                api.diagnostic_workers.available_permits(),
                MAX_NATIVE_DIAGNOSTICS
            );
        }
        let history = dir.path().join(crate::native_node::NATIVE_BLOCKS_FILE);
        let preserved = dir.path().join("preserved-native-history.ndjson");
        let original = std::fs::read(&history).unwrap();
        std::fs::rename(&history, &preserved).unwrap();
        assert!(request(address, "/ready", None).starts_with("HTTP/1.1 503"));
        assert!(request(address, "/native/peers", None).starts_with("HTTP/1.1 503"));
        let template = serde_json::json!({
            "producerPk": "00".repeat(32), "rewardPk": "00".repeat(32), "timestampMs": 60000
        })
        .to_string();
        assert!(request(address, "/native/template", Some(&template)).starts_with("HTTP/1.1 503"));
        let diagnostic = request(address, "/native/diagnostics", None);
        assert!(diagnostic.starts_with("HTTP/1.1 200"), "{diagnostic}");
        assert!(diagnostic.contains("\"ledgerReadiness\":\"not_checked\""));
        assert!(!diagnostic.contains("\"ready\""));
        assert!(!diagnostic.contains(dir.path().to_str().unwrap()));
        assert!(!history.exists());
        assert_eq!(std::fs::read(&preserved).unwrap(), original);
        api.lifecycle.request_stop();
        let diagnostic = request(address, "/native/diagnostics", None);
        assert!(diagnostic.starts_with("HTTP/1.1 200"));
        assert!(diagnostic.contains("\"stopping\":true"));
        assert!(request(address, "/ready", None).starts_with("HTTP/1.1 503"));
        stop.notify_one();
        runtime.block_on(server).unwrap();
        api.lifecycle.stop();
        assert_eq!(api.workers.available_permits(), MAX_NATIVE_REQUESTS);
        assert_eq!(
            api.diagnostic_workers.available_permits(),
            MAX_NATIVE_DIAGNOSTICS
        );
        drop(api);
        std::fs::rename(&preserved, &history).unwrap();
        assert_eq!(
            NativeNode::open(dir.path())
                .unwrap()
                .chain()
                .ledger()
                .height(),
            0
        );
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
            diagnostic_workers: Arc::new(Semaphore::new(MAX_NATIVE_DIAGNOSTICS)),
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
