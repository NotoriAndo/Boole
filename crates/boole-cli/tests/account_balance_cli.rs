//! S9 — `boole account balance --pk <hex32>` CLI surface.
//!
//! Boots a real local node with a pre-seeded reward ledger, then drives
//! the CLI binary with `--node http://<addr> --json` and asserts the
//! response shape. Mirrors the live-node testing pattern of
//! `tests/node_block.rs` rather than mocking transport.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::Value;
use std::net::TcpStream;

const PK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PK_UNKNOWN: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn scenario_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn boot_node_with_seeded_ledger(
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-s9-account-cli-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let reward_path = dir.join("rewards.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path_for_thread = block_path.clone();
    let reward_path_for_thread = reward_path.clone();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                proof_dedup_ledger_path: None,
                scenario_path: scenario,
                block_path: block_path_for_thread,
                reward_ledger_path: Some(reward_path_for_thread),
                work_manifests_path: None,
                bounties_path: None,
                bounty_event_ledger_path: None,
                bounty_verifiers: None,
                family_manifests_dir: None,
                max_requests: Some(max_requests),
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
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    (addr, handle, dir)
}

// SC.5 — the node boots under the strict genesis-aware replay, so the
// legacy pre-evidence golden chain can no longer be seeded from disk;
// the tests commit a real block through the live node instead (the
// smoke step-0 share: mined AND proposed by PK_B, so balance "2").
fn commit_smoke_block(addr: SocketAddr) {
    let raw = std::fs::read_to_string(scenario_path()).expect("read scenario");
    let scenario: Value = serde_json::from_str(&raw).expect("scenario json");
    let body = serde_json::json!({"body": scenario["steps"][0]["body"], "canonTag": 0});
    let body_str = serde_json::to_string(&body).expect("body json");
    let request = format!(
        "POST /submit HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
        body_str.len()
    );
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf);
    assert!(
        raw.lines().next().unwrap_or_default().contains("200"),
        "smoke share must commit block 0: {raw}"
    );
}

fn cli_url(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

#[test]
fn account_balance_cli_prints_json_envelope_with_flag() {
    let (addr, handle, dir) = boot_node_with_seeded_ledger(2);
    commit_smoke_block(addr);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "account",
            "balance",
            "--pk",
            PK_B,
            "--node",
            &cli_url(addr),
            "--json",
        ])
        .output()
        .expect("run cli");
    assert!(
        output.status.success(),
        "expected success: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr empty on success: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("stdout json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["pk"], PK_B);
    assert_eq!(parsed["balance"], "2");
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn account_balance_cli_prints_bare_balance_without_json_flag() {
    let (addr, handle, dir) = boot_node_with_seeded_ledger(2);
    commit_smoke_block(addr);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["account", "balance", "--pk", PK_B, "--node", &cli_url(addr)])
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
        "2",
        "non-json prints bare balance only: {stdout:?}"
    );
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn account_balance_cli_returns_zero_for_unknown_pk() {
    let (addr, handle, dir) = boot_node_with_seeded_ledger(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "account",
            "balance",
            "--pk",
            PK_UNKNOWN,
            "--node",
            &cli_url(addr),
            "--json",
        ])
        .output()
        .expect("run cli");
    assert!(output.status.success(), "unknown pk → 0, not error");
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("stdout json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["balance"], "0");
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn account_balance_cli_rejects_malformed_pk_locally() {
    // No node hit at all — local validation prefers exit 2 (user error),
    // including uppercase noncanonical hex that `Hex32::from_hex` rejects.
    for pk in ["tooshort", &"A".repeat(64)] {
        let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
            .args([
                "account",
                "balance",
                "--pk",
                pk,
                "--node",
                "http://127.0.0.1:1",
                "--json",
            ])
            .output()
            .expect("run cli");
        assert!(!output.status.success(), "must exit non-zero for {pk}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = writeln!(std::io::stderr(), "stderr was: {stderr}");
        let parsed: Value = serde_json::from_slice(&output.stderr).expect("stderr json");
        assert_eq!(parsed["ok"], false);
        assert_eq!(parsed["reason"], "malformed_pk");
    }
}
