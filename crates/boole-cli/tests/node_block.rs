use boole_node::{serve_local_node, LocalNodeConfig};
use boole_testkit::rand_suffix;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn scenario_path() -> PathBuf {
    repo_root().join("fixtures/protocol/runtime-smoke/v1.json")
}

fn boot_node(max_requests: usize) -> (SocketAddr, thread::JoinHandle<anyhow::Result<()>>, PathBuf) {
    let tmp = std::env::temp_dir().join(format!(
        "boole-cli-node-block-{}-{}.ndjson",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_file(&tmp);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    let block_path = tmp.clone();
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
            },
        )
    });
    rx.recv().expect("server ready");
    thread::sleep(Duration::from_millis(50));
    (addr, handle, tmp)
}

fn submit_step_zero(addr: SocketAddr) {
    let scenario_text = std::fs::read_to_string(scenario_path()).expect("scenario");
    let scenario: Value = serde_json::from_str(&scenario_text).expect("scenario json");
    let body = &scenario["steps"][0]["body"];
    let payload = serde_json::json!({"body": body, "canonTag": 0});
    let payload_str = serde_json::to_string(&payload).expect("submit json");
    let request = format!(
        "POST /submit HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload_str.len(),
        payload_str
    );
    let mut stream = TcpStream::connect(addr).expect("connect");
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf).to_string();
    let (_, body_text) = raw
        .split_once("\r\n\r\n")
        .unwrap_or_else(|| panic!("submit response: {raw}"));
    let parsed: Value = serde_json::from_str(body_text).expect("submit body json");
    assert_eq!(
        parsed["accepted"], true,
        "submit must accept step 0 to drive block tests; got {parsed}"
    );
}

fn cli_url(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

#[test]
fn block_latest_against_empty_chain_prints_genesis_envelope_to_stdout() {
    let (addr, handle, tmp) = boot_node(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["block", "latest", "--node", &cli_url(addr), "--json"])
        .output()
        .expect("run cli");
    assert!(
        output.status.success(),
        "expected success: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr should be empty on success: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("stdout json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["block"], Value::Null);
    assert_eq!(parsed["height"], Value::Null);
    assert_eq!(parsed["c"], "0".repeat(64));
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn block_latest_after_one_commit_prints_block_envelope_to_stdout() {
    let (addr, handle, tmp) = boot_node(2);
    submit_step_zero(addr);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(["block", "latest", "--node", &cli_url(addr), "--json"])
        .output()
        .expect("run cli");
    assert!(
        output.status.success(),
        "expected success: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("stdout json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["height"], 0);
    assert!(
        parsed["block"].is_object(),
        "block must be object: {parsed}"
    );
    let c = parsed["c"].as_str().expect("c hex string");
    assert_eq!(c.len(), 64, "c must be 64 hex: {c}");
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn block_get_height_zero_after_commit_prints_block_envelope_to_stdout() {
    let (addr, handle, tmp) = boot_node(2);
    submit_step_zero(addr);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "block",
            "get",
            "--height",
            "0",
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
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("stdout json");
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["height"], 0);
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn block_get_height_bad_request_prints_typed_error_to_stderr() {
    let (addr, handle, tmp) = boot_node(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "block",
            "get",
            "--height",
            "notanumber",
            "--node",
            &cli_url(addr),
            "--json",
        ])
        .output()
        .expect("run cli");
    assert!(!output.status.success(), "must exit non-zero on 400");
    assert!(
        output.stdout.is_empty(),
        "stdout must be empty on error: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let parsed: Value = serde_json::from_slice(&output.stderr).expect("stderr json");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["reason"], "bad_request");
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn block_get_height_not_found_prints_typed_error_to_stderr() {
    let (addr, handle, tmp) = boot_node(1);
    let output = Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args([
            "block",
            "get",
            "--height",
            "9999",
            "--node",
            &cli_url(addr),
            "--json",
        ])
        .output()
        .expect("run cli");
    assert!(!output.status.success(), "must exit non-zero on 404");
    assert!(
        output.stdout.is_empty(),
        "stdout must be empty on error: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let parsed: Value = serde_json::from_slice(&output.stderr).expect("stderr json");
    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["reason"], "not_found");
    handle.join().expect("server thread").expect("server exits");
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn node_start_spawns_daemon_serving_health() {
    let port_listener = TcpListener::bind("127.0.0.1:0").expect("free port probe");
    let port = port_listener.local_addr().expect("addr").port();
    drop(port_listener);

    let data_dir = std::env::temp_dir().join(format!(
        "boole-cli-node-start-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    std::fs::create_dir_all(&data_dir).expect("data dir");

    let cli_bin = env!("CARGO_BIN_EXE_boole-cli");
    let node_bin = Path::new(cli_bin)
        .parent()
        .expect("cli parent")
        .join("boole-node");

    let scenario = scenario_path();
    let mut child = Command::new(cli_bin)
        .args([
            "node",
            "start",
            "--port",
            &port.to_string(),
            "--data-dir",
            data_dir.to_str().expect("utf8 data-dir"),
            "--scenario",
            scenario.to_str().expect("utf8 scenario"),
            // The readiness probe below uses raw TCP connect; that counts
            // as one served request slot before the actual GET /health
            // request arrives. Cap at 2 so the server stays alive long
            // enough to answer the GET deterministically.
            "--max-requests",
            "2",
        ])
        .env("BOOLE_NODE_BIN", &node_bin)
        .spawn()
        .expect("spawn node start");

    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().expect("addr");
    let mut connected = false;
    for _ in 0..200 {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok() {
            connected = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(connected, "boole-node did not start listening on {addr}");

    let mut stream = TcpStream::connect(addr).expect("connect health");
    stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("write health");
    let mut buf = Vec::new();
    let _ = stream.read_to_end(&mut buf);
    let raw = String::from_utf8_lossy(&buf).to_string();
    assert!(
        raw.starts_with("HTTP/1.1 200"),
        "health must return 200: {raw}"
    );
    let (_, body_text) = raw.split_once("\r\n\r\n").expect("body");
    let parsed: Value = serde_json::from_str(body_text).expect("health json");
    assert_eq!(parsed["status"], "ok");

    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&data_dir);
}
