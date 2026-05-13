//! S10 — `boole work list` and `boole work get --id <id>` CLI surfaces.
//!
//! Boots a real local node with a static work-manifest catalog wired
//! via `LocalNodeConfig.work_manifests_path`, then drives the CLI
//! binary against it. Mirrors the live-node testing pattern of
//! `tests/account_balance_cli.rs` rather than mocking transport.

use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::{serve_local_node, LocalNodeConfig};
use serde_json::Value;

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn scenario_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn work_fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/work/v1.json")
        .canonicalize()
        .expect("work fixture path")
}

fn boot_node_with_work(
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-s10-work-cli-{}-{}",
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
    let block_path_for_thread = block_path.clone();
    let work_path = work_fixture_path();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path: block_path_for_thread,
                reward_ledger_path: None,
                work_manifests_path: Some(work_path),
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                session_registry_path: None,
                submit_nonce_ledger_path: None,
                submit_receipt_ledger_path: None,
                receipt_commitment_ledger_path: None,
                genesis_override: None,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    (addr, handle, dir)
}

fn cli_url(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

#[test]
fn work_list_default_prints_one_line_per_manifest() {
    let (addr, handle, dir) = boot_node_with_work(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["work", "list", "--node", &cli_url(addr)])
        .output()
        .expect("run cli");
    assert!(
        output.status.success(),
        "expected success: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "expected one line per manifest: {stdout:?}");
    assert!(
        lines[0].starts_with("lean-bounty-1\t"),
        "first line workId: {stdout:?}"
    );
    assert!(
        lines[0].contains("\tlean.protocol-invariant\t"),
        "first line familyId: {stdout:?}"
    );
    assert!(
        lines[0].ends_with("\topen"),
        "first line status: {stdout:?}"
    );
    assert!(
        lines[1].starts_with("smart-contract-invariant-v01-direct\t"),
        "second line workId: {stdout:?}"
    );
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn work_list_json_prints_full_envelope() {
    let (addr, handle, dir) = boot_node_with_work(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["work", "list", "--node", &cli_url(addr), "--json"])
        .output()
        .expect("run cli");
    assert!(output.status.success());
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("stdout json");
    assert_eq!(parsed["ok"], true);
    let work = parsed["work"]
        .as_array()
        .unwrap_or_else(|| panic!("work array: {parsed}"));
    assert_eq!(work.len(), 2);
    assert_eq!(work[0]["workId"], "lean-bounty-1");
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn work_get_default_prints_verifier_hash() {
    let (addr, handle, dir) = boot_node_with_work(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "work",
            "get",
            "--id",
            "lean-bounty-1",
            "--node",
            &cli_url(addr),
        ])
        .output()
        .expect("run cli");
    assert!(
        output.status.success(),
        "expected success: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.trim(),
        "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
        "default prints verifierHash: {stdout:?}"
    );
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn work_get_json_prints_full_envelope() {
    let (addr, handle, dir) = boot_node_with_work(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "work",
            "get",
            "--id",
            "lean-bounty-1",
            "--node",
            &cli_url(addr),
            "--json",
        ])
        .output()
        .expect("run cli");
    assert!(output.status.success());
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("stdout json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["work"]["workId"], "lean-bounty-1");
    assert_eq!(
        parsed["work"]["verifier"]["metadata"]["verifierHash"],
        "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"
    );
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn work_get_unknown_forwards_typed_error_exit_1() {
    let (addr, handle, dir) = boot_node_with_work(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "work",
            "get",
            "--id",
            "no-such-work",
            "--node",
            &cli_url(addr),
            "--json",
        ])
        .output()
        .expect("run cli");
    assert!(!output.status.success(), "unknown id must exit non-zero");
    assert!(
        output.stdout.is_empty(),
        "stdout empty on rejection: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let parsed: Value = serde_json::from_slice(&output.stderr).expect("stderr json");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["reason"], "work_not_found");
    assert_eq!(parsed["id"], "no-such-work");
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}
