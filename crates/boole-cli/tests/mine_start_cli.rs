//! S19 — `boole mine start` smoke tests. Uses a closed port and a tight
//! `--head-timeout-ms` so the loop exits after a single failing head fetch.
//! Full E2E pipeline coverage lives in `boole-miner/tests/mining_loop.rs`,
//! which exercises the loop with stub collaborators; here we only verify
//! that the CLI binary parses arguments, runs the loop, and prints the
//! summary envelope.

use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn fresh_state_path(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-s19-mine-start-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir.join("state.json")
}

/// Bind a TCP socket to a free port and immediately drop it. Returns the
/// `host:port` of a port that was free (and is very likely still closed when
/// the caller dials it). Slightly racy in theory but reliable in practice.
fn closed_port_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(listener);
    format!("http://{}", addr)
}

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(args)
        .output()
        .expect("run cli")
}

fn extract_summary(stdout: &str) -> Value {
    let marker = "summary:";
    let idx = stdout
        .find(marker)
        .expect("summary marker present in stdout");
    let tail = &stdout[idx + marker.len()..];
    serde_json::from_str(tail.trim()).expect("summary parses as JSON")
}

#[test]
fn mine_start_exits_after_max_cycles_when_head_fetch_fails() {
    let state_path = fresh_state_path("nohead");
    let init = run_cli(&[
        "mine",
        "init",
        "--state",
        state_path.to_str().unwrap(),
        "--dispatcher-url",
        &closed_port_url(),
        "--llm-backend",
        "mock",
    ]);
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let out = run_cli(&[
        "mine",
        "start",
        "--state",
        state_path.to_str().unwrap(),
        "--max-cycles",
        "1",
        "--head-timeout-ms",
        "100",
        "--mock-verify-accept",
    ]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let summary = extract_summary(&stdout);
    assert_eq!(summary["protocol"]["cyclesRun"], 1, "summary={summary}");
    assert!(
        summary["protocol"]["networkErrors"].as_u64().unwrap_or(0) >= 1,
        "expected at least one protocol.networkError: {summary}"
    );
    assert_eq!(summary["protocol"]["sharesAccepted"], 0);
    assert_eq!(summary["protocol"]["ticketsFound"], 0);
    assert_eq!(summary["agent"]["driverCalls"], 0);
    assert_eq!(summary["cyclesRun"], 1, "flat summary={summary}");
    assert!(
        summary["networkErrors"].as_u64().unwrap_or(0) >= 1,
        "expected at least one flat networkError: {summary}"
    );
    assert_eq!(summary["sharesAccepted"], 0);
    assert_eq!(summary["ticketsFound"], 0);
}

#[test]
fn mine_start_rejects_unpaired_fixed_target_flags() {
    let state_path = fresh_state_path("badpair");
    let init = run_cli(&[
        "mine",
        "init",
        "--state",
        state_path.to_str().unwrap(),
        "--dispatcher-url",
        "http://example.invalid",
        "--llm-backend",
        "mock",
    ]);
    assert!(init.status.success());

    let out = run_cli(&[
        "mine",
        "start",
        "--state",
        state_path.to_str().unwrap(),
        "--fixed-target-seed-hex",
        "deadbeef",
        // intentionally omit --fixed-target-render
        "--max-cycles",
        "0",
    ]);
    assert!(
        !out.status.success(),
        "should reject unpaired fixed-target flags"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("must be provided together"),
        "stderr: {stderr}"
    );
}
