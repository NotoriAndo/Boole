//! S13b — `boole bounty announce ...`.
//!
//! Boots a real local node (mock-verifier fixture) and drives the CLI binary
//! against it. Default output is the bare bounty id; `--json` returns the
//! full server envelope; 4xx/5xx forward typed errors to stderr with exit 1.
//! Local validation (e.g. malformed `--problem-hash`) exits 2 without a
//! network round-trip.

use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use boole_core::{Bounty, BountyProofVerifier};
use boole_node::{serve_local_node, LocalNodeConfig};
use serde_json::{json, Value};

const PROBLEM_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const VERIFIER_HASH_META: &str =
    r#"{"verifierHash":"2222222222222222222222222222222222222222222222222222222222222222"}"#;

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
        "boole-s13b-announce-cli-{label}-{}-{}",
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

fn announce_args(
    id: &str,
    problem_hash: &str,
    verifier_metadata: &str,
    signing_key: &str,
    addr: SocketAddr,
    json_flag: bool,
) -> Vec<String> {
    let mut args = vec![
        "bounty".to_string(),
        "announce".to_string(),
        "--id".to_string(),
        id.to_string(),
        "--domain".to_string(),
        "code.spec-template".to_string(),
        "--problem-hash".to_string(),
        problem_hash.to_string(),
        "--verifier-kind".to_string(),
        "mock-accept".to_string(),
        "--verifier-metadata".to_string(),
        verifier_metadata.to_string(),
        "--reward".to_string(),
        "100".to_string(),
        "--deadline".to_string(),
        "1900000000000".to_string(),
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
fn successful_announce_emits_bare_bounty_id_on_stdout() {
    let dir = fresh_tmp("happy");
    let keys_dir = dir.join("keys");
    let event_path = dir.join("bounty-events.ndjson");
    make_dev_key(&keys_dir, "announcer");
    let (addr, handle) = boot(1, event_path);

    let args = announce_args(
        "cli-bounty-1",
        PROBLEM_HASH,
        VERIFIER_HASH_META,
        "announcer",
        addr,
        false,
    );
    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys_dir)
        .args(&args)
        .output()
        .expect("run announce");
    assert!(
        out.status.success(),
        "expected success: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(stdout, "cli-bounty-1", "default prints bare id: {stdout:?}");
    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn json_flag_emits_full_server_envelope() {
    let dir = fresh_tmp("json");
    let keys_dir = dir.join("keys");
    let event_path = dir.join("bounty-events.ndjson");
    make_dev_key(&keys_dir, "announcer");
    let (addr, handle) = boot(1, event_path);

    let args = announce_args(
        "cli-bounty-2",
        PROBLEM_HASH,
        VERIFIER_HASH_META,
        "announcer",
        addr,
        true,
    );
    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys_dir)
        .args(&args)
        .output()
        .expect("run announce --json");
    assert!(
        out.status.success(),
        "expected success: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: Value = serde_json::from_slice(&out.stdout).expect("stdout json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["bounty"]["id"], "cli-bounty-2");
    assert_eq!(parsed["bounty"]["status"], "open");
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
        "id": "old-bob",
        "pk": "00".repeat(32),
        "createdAt": "2025-01-01T00:00:00Z",
    });
    std::fs::write(keys_dir.join("old-bob.json"), v1.to_string()).expect("write v1");
    let (addr, handle) = boot(1, event_path);

    let args = announce_args(
        "cli-bounty-legacy",
        PROBLEM_HASH,
        VERIFIER_HASH_META,
        "old-bob",
        addr,
        false,
    );
    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys_dir)
        .args(&args)
        .output()
        .expect("run announce");
    assert!(!out.status.success(), "v1 keys cannot sign");
    assert_eq!(out.status.code(), Some(3), "refused operation exits 3");
    let envelope: Value = serde_json::from_slice(&out.stderr).expect("stderr typed envelope");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["reason"], "legacy_v1_key");
    assert_eq!(envelope["id"], "old-bob");
    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn malformed_problem_hash_exits_2_without_network_call() {
    // Local validation must reject before any TCP connect, so we deliberately
    // do not boot a node — if the CLI tried to call us we'd see ECONNREFUSED
    // turn into a non-typed error.
    let dir = fresh_tmp("malformed");
    let keys_dir = dir.join("keys");
    make_dev_key(&keys_dir, "announcer");

    let args = announce_args(
        "cli-bounty-3",
        "deadbeef", // wrong length
        VERIFIER_HASH_META,
        "announcer",
        SocketAddr::from(([127, 0, 0, 1], 1)), // unused
        false,
    );
    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys_dir)
        .args(&args)
        .output()
        .expect("run announce");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2), "bad usage exits 2");
    let envelope: Value = serde_json::from_slice(&out.stderr).expect("stderr typed envelope");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["reason"], "malformed-problem-hash");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn server_duplicate_is_forwarded_to_stderr_with_exit_1() {
    let dir = fresh_tmp("dup");
    let keys_dir = dir.join("keys");
    let event_path = dir.join("bounty-events.ndjson");
    make_dev_key(&keys_dir, "announcer");
    let (addr, handle) = boot(2, event_path);

    let args = announce_args(
        "cli-bounty-dup",
        PROBLEM_HASH,
        VERIFIER_HASH_META,
        "announcer",
        addr,
        false,
    );
    let first = cli()
        .env("BOOLE_KEYS_DIR", &keys_dir)
        .args(&args)
        .output()
        .expect("first announce");
    assert!(first.status.success(), "first call must succeed");

    let second = cli()
        .env("BOOLE_KEYS_DIR", &keys_dir)
        .args(&args)
        .output()
        .expect("second announce");
    assert!(!second.status.success(), "duplicate id must exit 1");
    assert!(
        second.stdout.is_empty(),
        "stdout empty on rejection: {}",
        String::from_utf8_lossy(&second.stdout)
    );
    let envelope: Value = serde_json::from_slice(&second.stderr).expect("stderr typed envelope");
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["reason"], "bounty_already_exists");
    assert_eq!(envelope["id"], "cli-bounty-dup");
    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verifier_metadata_accepts_a_file_path() {
    let dir = fresh_tmp("file");
    let keys_dir = dir.join("keys");
    let event_path = dir.join("bounty-events.ndjson");
    make_dev_key(&keys_dir, "announcer");
    let (addr, handle) = boot(1, event_path);

    let metadata_path = dir.join("verifier.json");
    std::fs::write(&metadata_path, VERIFIER_HASH_META).expect("write metadata");

    let args = announce_args(
        "cli-bounty-file",
        PROBLEM_HASH,
        metadata_path.to_str().expect("utf8"),
        "announcer",
        addr,
        false,
    );
    let out = cli()
        .env("BOOLE_KEYS_DIR", &keys_dir)
        .args(&args)
        .output()
        .expect("run announce");
    assert!(
        out.status.success(),
        "expected success: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(stdout, "cli-bounty-file");
    handle.join().expect("server").expect("server ok");
    let _ = std::fs::remove_dir_all(&dir);
}
