//! S19 — `boole mine bounty --node URL --id <id> --prover <hex32>
//! [--envelope-path <path>]`. Boots a local node with the mock-verifier
//! fixture and drives the CLI binary against it.

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
use serde_json::Value;

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

fn boot_with_mock(
    max_requests: usize,
) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "boole-s19-mine-bounty-cli-{}-{}",
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
                state_dir: None,
                network_id: None,
                lean_checker_dir: None,
                lean_checker_disabled: true,
                http_rate_limit_per_60s: None,
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
fn mine_bounty_submits_envelope_and_prints_ok_envelope() {
    let (addr, handle, dir) = boot_with_mock(1);

    let envelope = dir.join("envelope.bin");
    std::fs::write(&envelope, b"{}").expect("write envelope");

    let out = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "mine",
            "bounty",
            "--node",
            cli_url(addr).as_str(),
            "--id",
            "gamma-1",
            "--prover",
            PROVER_X,
            "--envelope-path",
            envelope.to_str().unwrap(),
        ])
        .output()
        .expect("run cli");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: Value = serde_json::from_str(stdout.trim()).expect("stdout json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["accepted"], true);
    assert_eq!(parsed["duplicate"], false);
    assert_eq!(parsed["bounty"]["status"], "solved");

    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mine_bounty_rejects_malformed_prover_locally() {
    let out = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "mine",
            "bounty",
            "--node",
            "http://127.0.0.1:1",
            "--id",
            "gamma-1",
            "--prover",
            "not-hex",
        ])
        .output()
        .expect("run cli");
    assert!(!out.status.success(), "should fail on malformed prover");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("bad_prover"), "stderr: {stderr}");
}
