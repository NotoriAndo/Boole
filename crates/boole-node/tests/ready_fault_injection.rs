//! P2.6 closure — `/ready` readiness fault-injection matrix.
//!
//! The individual `ready_returns_503_when_*.rs` files each inject ONE
//! fault and assert the 503 + reason. What none of them pin is the
//! `ready_handler` PRECEDENCE: when several preconditions fail at once,
//! the handler must surface a deterministic first-failing reason, and
//! the `checks` object must still report every precondition's individual
//! boolean so an operator sees the full picture, not just the winner.
//!
//! `ready_handler` (crates/boole-node/src/local_node.rs) checks the
//! preconditions in a fixed order and returns the first failure:
//! `replay_matches_runtime` (`replay_runtime_mismatch`), then
//! `state_dir_lock_held` (`state_dir_lock_lost`), then
//! `lean_checker_configured` (`lean_checker_not_configured`), then
//! `ledgers_loaded` (`ledgers_not_loaded`); if all pass it returns 200
//! `{ ok: true, probe: "ready" }`.
//!
//! This file pins:
//!   * the HAPPY PATH (all preconditions satisfied → 200, ok=true, and
//!     every `checks.*` boolean true) — no existing test asserts the
//!     all-green readiness envelope shape; and
//!   * the lean-vs-ledger PRECEDENCE: a node that is BOTH missing the
//!     lean checker AND missing a required ledger must report
//!     `lean_checker_not_configured` (the earlier gate), with
//!     `checks.lean_checker_configured=false` AND
//!     `checks.ledgers_loaded=false` both surfaced. This is the cell the
//!     single-fault tests cannot cover because each injects only one
//!     fault.
//!
//! Together with the per-fault files this completes the §6.5 P2.6
//! readiness matrix: every reason row has a 503 test, plus the ordering
//! and the all-green envelope are now regression-pinned.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use boole_node::{serve_local_node, serve_local_node_with_disk_full_sentinel, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::Value;

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn http_get(addr: SocketAddr, path: &str) -> (u16, Value) {
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("read timeout");
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf).to_string();
    let status: u16 = raw
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = raw
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .unwrap_or_default();
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    (status, parsed)
}

struct Boot {
    addr: SocketAddr,
    handle: thread::JoinHandle<anyhow::Result<()>>,
    dir: PathBuf,
}

/// Boot a node with the given config knobs. Always embedding mode
/// (`state_dir: None`) unless `state_dir` is set, so the readiness
/// preconditions are isolated to the ones each test wants to exercise.
fn boot(
    label: &str,
    lean_disabled: bool,
    state_dir: Option<PathBuf>,
    with_one_ledger: bool,
) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-p2-6-ready-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");

    // When state_dir is set, all four agent-wallet ledgers must be
    // present for `ledgers_loaded` to be true. `with_one_ledger=false`
    // deliberately leaves them None so ledgers_loaded fails.
    let (
        session_registry_path,
        submit_nonce_ledger_path,
        submit_receipt_ledger_path,
        receipt_commitment_ledger_path,
    ) = if state_dir.is_some() && with_one_ledger {
        (
            Some(dir.join("sessions.ndjson")),
            Some(dir.join("submit-nonces.ndjson")),
            Some(dir.join("submit-receipts.ndjson")),
            Some(dir.join("receipt-commitments.ndjson")),
        )
    } else {
        (None, None, None, None)
    };

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();

    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: Some(1),
                operator_signer_pks: vec![],
                session_registry_path,
                submit_nonce_ledger_path,
                signed_nonce_ledger_path: None,
                submit_receipt_ledger_path,
                receipt_commitment_ledger_path,
                genesis_override: None,
                state_dir,
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: lean_disabled,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: true,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Boot { addr, handle, dir }
}

fn finish(b: Boot) {
    b.handle.join().expect("server thread").expect("server ok");
    let _ = std::fs::remove_dir_all(&b.dir);
}

#[test]
fn ready_returns_200_with_all_checks_true_on_clean_embedding_boot() {
    // Embedding mode, lean explicitly disabled, no state-dir → every
    // readiness precondition is satisfied. No existing test pins the
    // all-green 200 envelope; this is the matrix's happy-path row.
    let booted = boot("happy", true, None, false);
    let (status, body) = http_get(booted.addr, "/ready");
    assert_eq!(status, 200, "clean boot must be ready: {body}");
    assert_eq!(body.get("ok"), Some(&Value::Bool(true)), "ok=true: {body}");
    assert_eq!(
        body.get("probe").and_then(Value::as_str),
        Some("ready"),
        "probe=ready: {body}"
    );
    for check in [
        "replay_matches_runtime",
        "state_dir_lock_held",
        "lean_checker_configured",
        "ledgers_loaded",
        "disk_space_ok",
    ] {
        assert_eq!(
            body.pointer(&format!("/checks/{check}")),
            Some(&Value::Bool(true)),
            "all-green /ready must report checks.{check}=true: {body}"
        );
    }
    finish(booted);
}

/// Boot a clean embedding node (lean disabled, no state-dir) but inject a
/// disk-full sentinel set to `true`, so every readiness precondition except
/// disk space is satisfied.
fn boot_disk_full(label: &str) -> Boot {
    let dir = std::env::temp_dir().join(format!(
        "boole-p2-6-ready-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let disk_full = Arc::new(AtomicBool::new(true));

    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node_with_disk_full_sentinel(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: Some(1),
                operator_signer_pks: vec![],
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                signed_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                genesis_override: None,
                state_dir: None,
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
                allow_anonymous_submit: true,
            },
            disk_full,
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    Boot { addr, handle, dir }
}

#[test]
fn ready_returns_503_when_disk_full_sentinel_is_set() {
    // Clean embedding (lean disabled, no state-dir) so every other
    // precondition passes; only the injected disk-full sentinel trips, and
    // it is the last-checked reason, so it surfaces only when alone.
    let booted = boot_disk_full("disk-full");
    let (status, body) = http_get(booted.addr, "/ready");
    assert_eq!(status, 503, "disk-full must be 503: {body}");
    assert_eq!(
        body.get("reason").and_then(Value::as_str),
        Some("disk_full_sentinel"),
        "reason must be disk_full_sentinel: {body}"
    );
    assert_eq!(
        body.pointer("/checks/disk_space_ok"),
        Some(&Value::Bool(false)),
        "checks.disk_space_ok must be false: {body}"
    );
    for check in [
        "replay_matches_runtime",
        "state_dir_lock_held",
        "lean_checker_configured",
        "ledgers_loaded",
    ] {
        assert_eq!(
            body.pointer(&format!("/checks/{check}")),
            Some(&Value::Bool(true)),
            "only disk-full should fail; checks.{check} must be true: {body}"
        );
    }
    finish(booted);
}

#[test]
fn ready_precedence_lean_before_ledgers_when_both_fail() {
    // state_dir set (production mode) with NO ledgers AND no lean checker
    // and lean NOT disabled: both `lean_checker_configured` and
    // `ledgers_loaded` are false. The handler checks lean before ledgers,
    // so the surfaced reason must be `lean_checker_not_configured`, while
    // the `checks` object exposes BOTH failures.
    let dir = std::env::temp_dir().join(format!(
        "boole-p2-6-ready-precedence-statedir-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let state_dir = dir.join("state");
    std::fs::create_dir_all(&state_dir).expect("state dir");

    let booted = boot("precedence", false, Some(state_dir), false);
    let (status, body) = http_get(booted.addr, "/ready");
    assert_eq!(
        status, 503,
        "node missing both lean checker and ledgers must be 503: {body}"
    );
    assert_eq!(
        body.get("reason").and_then(Value::as_str),
        Some("lean_checker_not_configured"),
        "precedence: lean gate is checked before the ledger gate, so the \
         surfaced reason must be lean_checker_not_configured even though \
         ledgers are also missing: {body}"
    );
    assert_eq!(
        body.pointer("/checks/lean_checker_configured"),
        Some(&Value::Bool(false)),
        "checks must expose the lean failure: {body}"
    );
    assert_eq!(
        body.pointer("/checks/ledgers_loaded"),
        Some(&Value::Bool(false)),
        "checks must ALSO expose the simultaneous ledger failure, not \
         just the winning reason: {body}"
    );
    finish(booted);
    let _ = std::fs::remove_dir_all(&dir);
}
