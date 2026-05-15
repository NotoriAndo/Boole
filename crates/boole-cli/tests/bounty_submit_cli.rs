//! S12 — `boole bounty submit --id <id> --proof-hash <hex32> --prover <hex32>
//! --envelope <path|inline> [--node URL] [--json]`.
//!
//! Boots a real local node with the mock-verifier fixture
//! (`fixtures/protocol/bounties/v1-mock.json`) and a mock verifier registry,
//! then drives the CLI binary against it. Default output is the bare bounty
//! status word (`solved` / `open` / `duplicate`); `--json` returns the full
//! server envelope; 4xx/5xx forward typed errors to stderr with exit 1.

use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{Bounty, BountyProofVerifier};
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::{json, Value};

const PROOF_HASH_A: &str = "aaaa000000000000000000000000000000000000000000000000000000000000";
const PROOF_HASH_B: &str = "bbbb000000000000000000000000000000000000000000000000000000000000";
const PROVER_X: &str = "1100000000000000000000000000000000000000000000000000000000000000";

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
    fn verify(&self, _b: &Bounty, _e: &Value) -> Result<bool, String> {
        Ok(true)
    }
}
struct MockReject;
impl BountyProofVerifier for MockReject {
    fn verify(&self, _b: &Bounty, _e: &Value) -> Result<bool, String> {
        Ok(false)
    }
}

fn boot_with_mock(
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-s12-bounty-submit-cli-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let block_path = dir.join("blocks.ndjson");
    let bounty_event_path = dir.join("bounty-events.ndjson");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let scenario = scenario_path();
    let block_path_for_thread = block_path.clone();
    let bounties_path = mock_bounty_fixture_path();
    let mut verifiers: HashMap<String, Arc<dyn BountyProofVerifier>> = HashMap::new();
    verifiers.insert("mock-accept".to_string(), Arc::new(MockAccept));
    verifiers.insert("mock-reject".to_string(), Arc::new(MockReject));
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
                bounty_event_ledger_path: Some(bounty_event_path),
                bounty_verifiers: Some(verifiers),
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

fn run_submit(
    addr: SocketAddr,
    id: &str,
    proof_hash: &str,
    prover: &str,
    envelope: &str,
    json_flag: bool,
) -> std::process::Output {
    let mut args = vec![
        "bounty".to_string(),
        "submit".to_string(),
        "--id".to_string(),
        id.to_string(),
        "--proof-hash".to_string(),
        proof_hash.to_string(),
        "--prover".to_string(),
        prover.to_string(),
        "--envelope".to_string(),
        envelope.to_string(),
        "--node".to_string(),
        cli_url(addr),
    ];
    if json_flag {
        args.push("--json".to_string());
    }
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(&args)
        .output()
        .expect("run cli")
}

#[test]
fn submit_default_accept_prints_bare_status_solved() {
    let (addr, handle, dir) = boot_with_mock(1);
    let out = run_submit(addr, "gamma-1", PROOF_HASH_A, PROVER_X, "{}", false);
    assert!(
        out.status.success(),
        "expected success: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(stdout, "solved", "default prints bare status: {stdout:?}");
    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn submit_json_accept_prints_full_envelope() {
    let (addr, handle, dir) = boot_with_mock(1);
    let out = run_submit(addr, "gamma-1", PROOF_HASH_A, PROVER_X, "{}", true);
    assert!(out.status.success());
    let parsed: Value = serde_json::from_slice(&out.stdout).expect("stdout json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["accepted"], true);
    assert_eq!(parsed["duplicate"], false);
    assert_eq!(parsed["bounty"]["status"], "solved");
    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn submit_default_reject_prints_bare_status_open() {
    let (addr, handle, dir) = boot_with_mock(1);
    let out = run_submit(addr, "delta-1", PROOF_HASH_A, PROVER_X, "{}", false);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(
        stdout, "open",
        "rejected proof keeps bounty open: {stdout:?}"
    );
    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn submit_default_dedup_prints_duplicate() {
    let (addr, handle, dir) = boot_with_mock(2);
    let out1 = run_submit(addr, "gamma-1", PROOF_HASH_B, PROVER_X, "{}", false);
    assert!(out1.status.success());
    let out2 = run_submit(addr, "gamma-1", PROOF_HASH_B, PROVER_X, "{}", false);
    assert!(out2.status.success());
    let stdout = String::from_utf8_lossy(&out2.stdout).trim().to_string();
    assert_eq!(
        stdout, "duplicate",
        "second post on same proofHash prints duplicate: {stdout:?}"
    );
    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn submit_unknown_bounty_forwards_typed_error_exit_1() {
    let (addr, handle, dir) = boot_with_mock(1);
    let out = run_submit(addr, "no-such", PROOF_HASH_A, PROVER_X, "{}", false);
    assert!(!out.status.success(), "unknown bounty must exit non-zero");
    assert!(
        out.stdout.is_empty(),
        "stdout empty on rejection: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let parsed: Value = serde_json::from_slice(&out.stderr).expect("stderr json");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["reason"], "bounty_not_found");
    assert_eq!(parsed["id"], "no-such");
    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);

    // Suppress unused-import warning on the json! macro path used elsewhere.
    let _ = json!({});
}
