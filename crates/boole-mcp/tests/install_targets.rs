//! P2.2 closure (slice 40) — `boole-mcp install --target <ide>` writes
//! an `mcpServers.boole` entry into the IDE's settings file. The
//! contract under test:
//!
//!   * For each of `claude | codex | cursor | opencode`, a fresh
//!     `$HOME` ends up with a settings file at the canonical IDE path
//!     containing `mcpServers.boole.command` pointing at the running
//!     binary, plus an `args` array suitable for launching `serve`.
//!   * Re-running install is idempotent: the second invocation does not
//!     mutate the settings bytes (no duplication, no key churn).
//!   * Pre-existing unrelated keys (other `mcpServers.*` entries and
//!     top-level settings) are preserved.
//!   * `--dry-run` writes nothing and emits a unified envelope on stdout
//!     with `result.dry_run = true` and a `planned_content` field.
//!   * Unknown `--target` is rejected by clap (non-zero exit).
//!
//! The success/failure stdout/stderr shape is the unified
//! `{ok,version,command,result|error}` envelope so the install flow is
//! parseable by the same tooling as the rest of the CLI surface.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

fn bin_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("target");
    p.push(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    p.push("boole-mcp");
    p
}

fn temp_home() -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let p = env::temp_dir().join(format!(
        "boole-mcp-install-test-{}-{}",
        std::process::id(),
        seq
    ));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).expect("temp home");
    p
}

fn settings_path(home: &Path, target: &str) -> PathBuf {
    match target {
        "claude" => home.join(".claude").join("settings.json"),
        "codex" => home.join(".codex").join("config.json"),
        "cursor" => home.join(".cursor").join("mcp.json"),
        "opencode" => home.join(".config").join("opencode").join("config.json"),
        other => panic!("unknown target {other}"),
    }
}

fn run_install(home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin_path())
        .env("HOME", home)
        .arg("install")
        .args(args)
        .output()
        .expect("spawn boole-mcp install")
}

fn parse_envelope(bytes: &[u8]) -> Value {
    let s = std::str::from_utf8(bytes).expect("utf8");
    serde_json::from_str(s.trim()).unwrap_or_else(|e| panic!("unified envelope parse: {e}: {s:?}"))
}

#[test]
fn install_each_ide_writes_canonical_settings_entry() {
    for target in ["claude", "codex", "cursor", "opencode"] {
        let home = temp_home();
        let out = run_install(&home, &["--target", target]);
        assert!(
            out.status.success(),
            "{target}: stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        let env = parse_envelope(&out.stdout);
        assert_eq!(env["ok"], true, "{target}");
        assert_eq!(env["version"], "v1", "{target}");
        assert_eq!(env["command"], "install", "{target}");
        assert_eq!(env["result"]["target"], target, "{target}");
        assert_eq!(env["result"]["dry_run"], false, "{target}");

        let s = settings_path(&home, target);
        assert!(s.exists(), "{target}: settings file at {s:?} should exist");
        let txt = fs::read_to_string(&s).expect("settings");
        let v: Value = serde_json::from_str(&txt).expect("settings json");
        let entry = &v["mcpServers"]["boole"];
        assert!(
            entry["command"].is_string(),
            "{target}: mcpServers.boole.command should be a string"
        );
        let cmd = entry["command"].as_str().unwrap();
        assert!(
            cmd.ends_with("boole-mcp") || cmd.ends_with("boole-mcp.exe"),
            "{target}: command should be the boole-mcp binary path; got {cmd}"
        );
        assert!(
            entry["args"].is_array(),
            "{target}: mcpServers.boole.args should be an array"
        );
        let _ = fs::remove_dir_all(&home);
    }
}

#[test]
fn install_is_idempotent_on_second_run() {
    let home = temp_home();
    let _ = run_install(&home, &["--target", "claude"]);
    let s = settings_path(&home, "claude");
    let first = fs::read_to_string(&s).expect("settings after first install");
    let _ = run_install(&home, &["--target", "claude"]);
    let second = fs::read_to_string(&s).expect("settings after second install");
    assert_eq!(
        first, second,
        "second install must not mutate settings bytes (idempotent merge)"
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn install_preserves_unrelated_keys() {
    let home = temp_home();
    let s = settings_path(&home, "claude");
    fs::create_dir_all(s.parent().unwrap()).expect("mkdirs");
    fs::write(
        &s,
        r#"{"theme":"dark","mcpServers":{"other":{"command":"x"}}}"#,
    )
    .expect("seed settings");
    let _ = run_install(&home, &["--target", "claude"]);
    let txt = fs::read_to_string(&s).expect("settings");
    let v: Value = serde_json::from_str(&txt).expect("settings json");
    assert_eq!(v["theme"], "dark", "unrelated top-level key preserved");
    assert_eq!(
        v["mcpServers"]["other"]["command"], "x",
        "unrelated sibling mcp server preserved"
    );
    assert!(
        v["mcpServers"]["boole"]["command"].is_string(),
        "boole entry inserted alongside sibling"
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn install_dry_run_does_not_write() {
    let home = temp_home();
    let out = run_install(&home, &["--target", "claude", "--dry-run"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = settings_path(&home, "claude");
    assert!(!s.exists(), "dry-run must not write the settings file");
    let env = parse_envelope(&out.stdout);
    assert_eq!(env["ok"], true);
    assert_eq!(env["version"], "v1");
    assert_eq!(env["command"], "install");
    assert_eq!(env["result"]["dry_run"], true);
    assert_eq!(env["result"]["target"], "claude");
    assert!(
        env["result"]["planned_content"]["mcpServers"]["boole"]["command"].is_string(),
        "dry-run envelope must include the planned content"
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn install_unknown_target_rejected_by_clap() {
    let home = temp_home();
    let out = run_install(&home, &["--target", "vim"]);
    assert!(
        !out.status.success(),
        "unknown target should be rejected by clap value_enum"
    );
    let _ = fs::remove_dir_all(&home);
}
