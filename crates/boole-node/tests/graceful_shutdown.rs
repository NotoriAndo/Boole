//! P2.7 — programmatic graceful shutdown trigger.
//!
//! Orchestrators (systemd, k8s, supervisord) signal "please stop"
//! externally; tests need the same hook without sending real signals
//! to the test process. `serve_local_node_with_shutdown` takes an
//! externally-owned `Arc<Notify>` whose `notify_one()` unblocks
//! `axum::serve`'s graceful-shutdown future and lets in-flight
//! requests drain before the server thread returns.
//!
//! Contract:
//!   * Calling `notify_one()` on the trigger causes `serve_local_node_with_shutdown`
//!     to return `Ok(())` within a bounded time, with no in-flight
//!     request being dropped mid-response.
//!   * If the trigger is never fired, behavior matches `serve_local_node`
//!     (same `max_requests` fallback, same fatal-error semantics).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use boole_node::{serve_local_node_with_shutdown, LocalNodeConfig};
use boole_testkit::rand_suffix;
use tokio::sync::Notify;

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn http_get_status(addr: SocketAddr, path: &str) -> u16 {
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_write_timeout(Some(Duration::from_secs(10)))
        .expect("write timeout");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf).to_string();
    raw.lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

#[test]
fn external_shutdown_trigger_drains_and_returns_within_bounded_time() {
    let dir = std::env::temp_dir().join(format!(
        "boole-graceful-shutdown-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (ready_tx, ready_rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_for_thread = block_path.clone();

    let trigger = Arc::new(Notify::new());
    let trigger_for_thread = trigger.clone();

    let handle = thread::spawn(move || {
        ready_tx.send(()).expect("ready");
        serve_local_node_with_shutdown(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path: block_for_thread,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: None,
                operator_signer_pks: vec![],
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                genesis_override: None,
                state_dir: None,
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
            },
            trigger_for_thread,
        )
    });
    ready_rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(100));

    let probe_status = http_get_status(addr, "/live");
    assert_eq!(probe_status, 200, "/live must respond before shutdown");

    let fire_at = Instant::now();
    trigger.notify_one();

    let join_deadline = Duration::from_secs(5);
    let join_started = Instant::now();
    let join_result = loop {
        if handle.is_finished() {
            break handle.join();
        }
        if join_started.elapsed() > join_deadline {
            panic!(
                "server did not exit within {join_deadline:?} of trigger fire \
                 (elapsed since trigger: {:?})",
                fire_at.elapsed()
            );
        }
        thread::sleep(Duration::from_millis(20));
    };
    let elapsed_since_trigger = fire_at.elapsed();
    assert!(
        elapsed_since_trigger < Duration::from_secs(3),
        "graceful shutdown should complete within 3s of trigger fire, took {elapsed_since_trigger:?}"
    );

    join_result
        .expect("server thread joined cleanly")
        .expect("server exits Ok(()) after external trigger");

    let _ = std::fs::remove_dir_all(&dir);
}
