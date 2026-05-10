//! S9 — `boole account balance --pk <hex32>` CLI surface.
//!
//! Boots a real local node with a pre-seeded reward ledger, then drives
//! the CLI binary with `--node http://<addr> --json` and asserts the
//! response shape. Mirrors the live-node testing pattern of
//! `tests/node_block.rs` rather than mocking transport.

use std::io::Write;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use boole_core::{PersistedBlock, PersistedRewardEvent};
use boole_node::block_store::FileBlockStore;
use boole_node::local_node::{serve_local_node, LocalNodeConfig};
use boole_node::reward_store::FileRewardLedger;
use serde::Deserialize;
use serde_json::Value;

const PK_2: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const PK_UNKNOWN: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[derive(Debug, Deserialize)]
struct ReplayFixture {
    blocks: Vec<PersistedBlock>,
    #[serde(rename = "rewardEvents")]
    reward_events: Vec<PersistedRewardEvent>,
}

fn replay_fixture() -> ReplayFixture {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/protocol/replay/v1.json");
    let text = std::fs::read_to_string(&path).expect("read replay fixture");
    serde_json::from_str(&text).expect("fixture parses")
}

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
    let fix = replay_fixture();
    for block in &fix.blocks {
        FileBlockStore::append(&block_path, block).expect("append block");
    }
    for event in &fix.reward_events {
        FileRewardLedger::append(&reward_path, event).expect("append reward");
    }

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
fn account_balance_cli_prints_json_envelope_with_flag() {
    let (addr, handle, dir) = boot_node_with_seeded_ledger(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "account",
            "balance",
            "--pk",
            PK_2,
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
    assert_eq!(parsed["pk"], PK_2);
    assert_eq!(parsed["balance"], "3");
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn account_balance_cli_prints_bare_balance_without_json_flag() {
    let (addr, handle, dir) = boot_node_with_seeded_ledger(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["account", "balance", "--pk", PK_2, "--node", &cli_url(addr)])
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
        "3",
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
    // No node hit at all — clap-level validation prefers exit 2 (user error).
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "account",
            "balance",
            "--pk",
            "tooshort",
            "--node",
            "http://127.0.0.1:1",
            "--json",
        ])
        .output()
        .expect("run cli");
    assert!(!output.status.success(), "must exit non-zero");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = writeln!(std::io::stderr(), "stderr was: {stderr}");
    let parsed: Value = serde_json::from_slice(&output.stderr).expect("stderr json");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["reason"], "malformed-pk");
}
