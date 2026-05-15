//! S11 — `boole bounty list` and `boole bounty get --id <id>` CLI surfaces.
//!
//! Boots a real local node with a static bounty catalog wired via
//! `LocalNodeConfig.bounties_path`, then drives the CLI binary against
//! it. Mirrors the live-node testing pattern of `tests/work_cli.rs`
//! rather than mocking transport.

use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::Value;

fn scenario_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn bounty_fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/bounties/v1.json")
        .canonicalize()
        .expect("bounty fixture path")
}

fn boot_node_with_bounties(
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-s11-bounty-cli-{}-{}",
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
    let bounties_path = bounty_fixture_path();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path: block_path_for_thread,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: Some(bounties_path),
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
fn bounty_list_default_prints_one_line_per_bounty() {
    let (addr, handle, dir) = boot_node_with_bounties(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["bounty", "list", "--node", &cli_url(addr)])
        .output()
        .expect("run cli");
    assert!(
        output.status.success(),
        "expected success: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "expected one line per bounty: {stdout:?}");
    // 4-column tab-separated: id\tdomain\tstatus\treward
    assert_eq!(
        lines[0], "alpha-1\tlean.protocol-invariant\topen\t42",
        "first row layout: {stdout:?}"
    );
    assert_eq!(
        lines[1], "beta-1\tcode.spec-template\tsolved\t11",
        "second row layout: {stdout:?}"
    );
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bounty_list_json_prints_full_envelope() {
    let (addr, handle, dir) = boot_node_with_bounties(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["bounty", "list", "--node", &cli_url(addr), "--json"])
        .output()
        .expect("run cli");
    assert!(output.status.success());
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("stdout json");
    assert_eq!(parsed["ok"], true);
    let bounties = parsed["bounties"]
        .as_array()
        .unwrap_or_else(|| panic!("bounties array: {parsed}"));
    assert_eq!(bounties.len(), 2);
    assert_eq!(bounties[0]["id"], "alpha-1");
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bounty_get_default_prints_verifier_hash() {
    let (addr, handle, dir) = boot_node_with_bounties(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["bounty", "get", "--id", "alpha-1", "--node", &cli_url(addr)])
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
fn bounty_get_json_prints_full_envelope() {
    let (addr, handle, dir) = boot_node_with_bounties(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "bounty",
            "get",
            "--id",
            "alpha-1",
            "--node",
            &cli_url(addr),
            "--json",
        ])
        .output()
        .expect("run cli");
    assert!(output.status.success());
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("stdout json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["bounty"]["id"], "alpha-1");
    assert_eq!(parsed["bounty"]["domain"], "lean.protocol-invariant");
    assert_eq!(
        parsed["bounty"]["verifier"]["metadata"]["verifierHash"],
        "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"
    );
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bounty_get_unknown_forwards_typed_error_exit_1() {
    let (addr, handle, dir) = boot_node_with_bounties(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "bounty",
            "get",
            "--id",
            "no-such-bounty",
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
    assert_eq!(parsed["reason"], "bounty_not_found");
    assert_eq!(parsed["id"], "no-such-bounty");
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}
