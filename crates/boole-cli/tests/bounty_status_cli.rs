//! S14 — `boole bounty status ...`.
//!
//! Boots a real local node (mock-bounty fixture) and drives the CLI binary
//! against it. Default output is the bare `<newStatus>` word; `--json`
//! returns the full server envelope; 4xx/5xx forward typed errors to
//! stderr with exit 1.

use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{Bounty, BountyProofVerifier};
use boole_node::local_node::{serve_local_node, LocalNodeConfig};
use serde_json::{json, Value};

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
}

fn scenario_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/runtime-smoke/v1.json")
        .canonicalize()
        .expect("scenario path")
}

fn mock_bounty_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/protocol/bounties/v1-mock.json")
        .canonicalize()
        .expect("mock bounty fixture path")
}

struct MockAccept;
impl BountyProofVerifier for MockAccept {
    fn verify(&self, _bounty: &Bounty, _envelope: &Value) -> Result<bool, String> {
        Ok(true)
    }
}

fn mock_verifiers() -> HashMap<String, Arc<dyn BountyProofVerifier>> {
    let mut m: HashMap<String, Arc<dyn BountyProofVerifier>> = HashMap::new();
    m.insert("mock-accept".to_string(), Arc::new(MockAccept));
    m
}

fn fresh_tmp(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-s14-status-cli-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir
}

fn boot(
    max_requests: usize,
    bounty_event_path: PathBuf,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>) {
    let dir = bounty_event_path.parent().expect("parent").to_path_buf();
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let verifiers = mock_verifiers();
    let bounties_path = mock_bounty_fixture_path();
    let handle = thread::spawn(move || {
        tx.send(()).expect("ready");
        serve_local_node(
            listener,
            LocalNodeConfig {
                scenario_path: scenario,
                block_path,
                reward_ledger_path: None,
                work_manifests_path: None,
                bounties_path: Some(bounties_path),
                bounty_event_ledger_path: Some(bounty_event_path),
                bounty_verifiers: Some(verifiers),
                family_manifests_dir: None,
                max_requests: Some(max_requests),
                operator_signer_pks: vec![],
                genesis_override: None,
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    (addr, handle)
}

fn make_dev_key(dir: &Path, id: &str) {
    let out = cli()
        .env("BOOLE_KEYS_DIR", dir)
        .args(["keys", "new", "--id", id, "--dev"])
        .output()
        .expect("keys new");
    assert!(
        out.status.success(),
        "keys new must succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn status_args(
    id: &str,
    new_status: &str,
    signing_key: &str,
    addr: SocketAddr,
    json_flag: bool,
) -> Vec<String> {
    let mut args = vec![
        "bounty".to_string(),
        "status".to_string(),
        "--id".to_string(),
        id.to_string(),
        "--new-status".to_string(),
        new_status.to_string(),
        "--signing-key".to_string(),
        signing_key.to_string(),
        "--node".to_string(),
        format!("http://{addr}"),
    ];
    if json_flag {
        args.push("--json".to_string());
    }
    args
}

#[test]
fn successful_status_change_emits_bare_new_status_on_stdout() {
    let dir = fresh_tmp("happy");
    let keys_dir = dir.join("keys");
    let event_path = dir.join("bounty-events.ndjson");
    make_dev_key(&keys_dir, "operator");
    let (addr, handle) = boot(1, event_path);

    let args = status_args("gamma-1", "withdrawn", "operator", addr, false);
    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys_dir)
        .args(&args)
        .output()
        .expect("run status");
    assert!(
        out.status.success(),
        "expected success: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(
        stdout, "withdrawn",
        "default prints bare newStatus: {stdout:?}"
    );
    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn json_flag_emits_full_server_envelope() {
    let dir = fresh_tmp("json");
    let keys_dir = dir.join("keys");
    let event_path = dir.join("bounty-events.ndjson");
    make_dev_key(&keys_dir, "operator");
    let (addr, handle) = boot(1, event_path);

    let args = status_args("gamma-1", "withdrawn", "operator", addr, true);
    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys_dir)
        .args(&args)
        .output()
        .expect("run status --json");
    assert!(
        out.status.success(),
        "expected success: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: Value = serde_json::from_slice(&out.stdout).expect("stdout json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["bounty"]["id"], "gamma-1");
    assert_eq!(parsed["bounty"]["status"], "withdrawn");
    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn v1_key_is_refused_with_legacy_v1_key_typed_envelope() {
    let dir = fresh_tmp("legacy");
    let keys_dir = dir.join("keys");
    let event_path = dir.join("bounty-events.ndjson");
    std::fs::create_dir_all(&keys_dir).expect("mkdir keys");
    let v1 = json!({
        "schema": "boole.keys.v1",
        "id": "old-op",
        "pk": "00".repeat(32),
        "createdAt": "2025-01-01T00:00:00Z",
    });
    std::fs::write(keys_dir.join("old-op.json"), v1.to_string()).expect("write v1");
    let (addr, handle) = boot(1, event_path);

    let args = status_args("gamma-1", "withdrawn", "old-op", addr, false);
    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys_dir)
        .args(&args)
        .output()
        .expect("run status");
    assert!(!out.status.success(), "v1 keys cannot sign");
    assert_eq!(out.status.code(), Some(3), "refused operation exits 3");
    let envelope: Value = serde_json::from_slice(&out.stderr).expect("stderr typed envelope");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["reason"], "legacy_v1_key");
    assert_eq!(envelope["id"], "old-op");
    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn server_invalid_transition_is_forwarded_to_stderr_with_exit_1() {
    // epsilon-1 boots in status=withdrawn (terminal). Trying to move it back
    // to open must surface the server's typed bounty_terminal envelope on
    // stderr with exit 1.
    let dir = fresh_tmp("terminal");
    let keys_dir = dir.join("keys");
    let event_path = dir.join("bounty-events.ndjson");
    make_dev_key(&keys_dir, "operator");
    let (addr, handle) = boot(1, event_path);

    let args = status_args("epsilon-1", "open", "operator", addr, false);
    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys_dir)
        .args(&args)
        .output()
        .expect("run status");
    assert!(!out.status.success(), "terminal transition must exit 1");
    assert!(
        out.stdout.is_empty(),
        "stdout empty on rejection: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let envelope: Value = serde_json::from_slice(&out.stderr).expect("stderr typed envelope");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["reason"], "bounty_terminal");
    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);
}
