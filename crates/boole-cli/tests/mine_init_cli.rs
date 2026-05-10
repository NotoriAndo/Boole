//! S19 — `boole mine init` / `boole mine address` / `boole mine config`
//! smoke tests. Drives the CLI binary against an isolated state path.

use std::path::PathBuf;
use std::process::Command;

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn fresh_state_path(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-s19-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir.join("state.json")
}

fn run_cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_boole-cli"))
        .args(args)
        .output()
        .expect("run cli")
}

#[test]
fn mine_init_creates_state_and_prints_address() {
    let state_path = fresh_state_path("init");
    let out = run_cli(&[
        "mine",
        "init",
        "--state",
        state_path.to_str().unwrap(),
        "--dispatcher-url",
        "http://example.invalid",
        "--llm-backend",
        "mock",
    ]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(state_path.exists(), "state file should exist");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("address: "), "stdout: {stdout}");
}

#[test]
fn mine_address_prints_pk_hex() {
    let state_path = fresh_state_path("addr");
    let out = run_cli(&[
        "mine",
        "init",
        "--state",
        state_path.to_str().unwrap(),
        "--dispatcher-url",
        "http://example.invalid",
        "--llm-backend",
        "mock",
    ]);
    assert!(out.status.success());

    let out = run_cli(&["mine", "address", "--state", state_path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(stdout.len(), 64, "address should be 64 hex chars: {stdout}");
    assert!(
        stdout.bytes().all(|b| b.is_ascii_hexdigit()),
        "address should be hex: {stdout}"
    );
}

#[test]
fn mine_init_refuses_to_overwrite_without_force() {
    let state_path = fresh_state_path("noforce");
    let _ = run_cli(&[
        "mine",
        "init",
        "--state",
        state_path.to_str().unwrap(),
        "--dispatcher-url",
        "http://example.invalid",
        "--llm-backend",
        "mock",
    ]);
    let out = run_cli(&[
        "mine",
        "init",
        "--state",
        state_path.to_str().unwrap(),
        "--dispatcher-url",
        "http://example.invalid",
        "--llm-backend",
        "mock",
    ]);
    assert!(
        !out.status.success(),
        "should reject overwrite without --force"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("already exists"), "stderr: {stderr}");
}

#[test]
fn mine_config_get_set_round_trips() {
    let state_path = fresh_state_path("config");
    let _ = run_cli(&[
        "mine",
        "init",
        "--state",
        state_path.to_str().unwrap(),
        "--dispatcher-url",
        "http://example.invalid",
        "--llm-backend",
        "mock",
    ]);
    let out = run_cli(&[
        "mine",
        "config",
        "set",
        "--state",
        state_path.to_str().unwrap(),
        "dispatcher.url",
        "http://updated.invalid",
    ]);
    assert!(out.status.success());

    let out = run_cli(&[
        "mine",
        "config",
        "get",
        "--state",
        state_path.to_str().unwrap(),
        "dispatcher.url",
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(stdout, "http://updated.invalid");
}

#[test]
fn mine_config_get_redacts_secret_by_default() {
    let state_path = fresh_state_path("redact");
    let _ = run_cli(&[
        "mine",
        "init",
        "--state",
        state_path.to_str().unwrap(),
        "--dispatcher-url",
        "http://example.invalid",
        "--llm-backend",
        "mock",
        "--llm-api-key",
        "shh-secret-token",
    ]);
    let out = run_cli(&[
        "mine",
        "config",
        "get",
        "--state",
        state_path.to_str().unwrap(),
        "llm.apiKey",
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(stdout, "***");

    let out = run_cli(&[
        "mine",
        "config",
        "get",
        "--state",
        state_path.to_str().unwrap(),
        "llm.apiKey",
        "--reveal",
    ]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(stdout, "shh-secret-token");
}
