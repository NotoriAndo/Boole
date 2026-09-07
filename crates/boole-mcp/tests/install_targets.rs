//! P2.2 closure (slice 40) — `boole-mcp install --target <ide>` writes
//! the target IDE's canonical MCP entry into its settings file. The contract
//! under test:
//!
//!   * For each of `claude | codex | cursor | opencode`, a fresh
//!     `$HOME` ends up with a settings file at the canonical IDE path
//!     containing its native MCP shape pointing at the running binary: Claude
//!     and Cursor use `mcpServers.boole`, Codex uses `[mcp_servers.boole]`,
//!     and OpenCode uses `mcp.boole`. Each launches `stdio` (real MCP
//!     stdio transport) with `--node-url http://127.0.0.1:8080` so
//!     the legacy proxy tools (bounty.list / receipt.get) keep working, and a
//!     separate `--native-shadow-url http://127.0.0.1:8082` for native verdicts.
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
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_boole-mcp"))
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
        "claude" => home.join(".claude.json"),
        "codex" => home.join(".codex").join("config.toml"),
        "cursor" => home.join(".cursor").join("mcp.json"),
        "opencode" => home.join(".config").join("opencode").join("opencode.json"),
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
fn install_claude_writes_a_user_scope_stdio_server() {
    let home = temp_home();
    let out = run_install(&home, &["--target", "claude"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let settings = home.join(".claude.json");
    let value: Value = serde_json::from_str(&fs::read_to_string(&settings).expect("Claude config"))
        .expect("Claude config JSON");
    let entry = &value["mcpServers"]["boole"];
    assert_eq!(entry["type"], "stdio");
    assert!(entry["command"].is_string());
    assert_eq!(entry["args"][0], "stdio");
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn install_opencode_writes_a_local_command_array() {
    let home = temp_home();
    let out = run_install(&home, &["--target", "opencode"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let settings = home.join(".config").join("opencode").join("opencode.json");
    let value: Value =
        serde_json::from_str(&fs::read_to_string(&settings).expect("OpenCode config"))
            .expect("OpenCode config JSON");
    let entry = &value["mcp"]["boole"];
    assert_eq!(entry["type"], "local");
    let command = entry["command"].as_array().expect("local command array");
    assert!(command.len() > 1, "command includes binary and stdio args");
    assert_eq!(command[1], "stdio");
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn install_opencode_preserves_other_configuration_and_mcp_servers() {
    let home = temp_home();
    let settings = settings_path(&home, "opencode");
    fs::create_dir_all(settings.parent().expect("settings parent")).expect("mkdir config");
    fs::write(
        &settings,
        r#"{"theme":"dark","mcp":{"other":{"type":"local","command":["other"]}}}"#,
    )
    .expect("seed OpenCode config");

    let out = run_install(&home, &["--target", "opencode"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value =
        serde_json::from_str(&fs::read_to_string(&settings).expect("OpenCode config"))
            .expect("OpenCode JSON");
    assert_eq!(value["theme"], "dark");
    assert_eq!(value["mcp"]["other"]["command"][0], "other");
    assert_eq!(value["mcp"]["boole"]["type"], "local");
    let _ = fs::remove_dir_all(&home);
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
        if target == "codex" {
            assert!(txt.contains("[mcp_servers.boole]"), "{target}: {txt}");
            assert!(txt.contains("command = "), "{target}: {txt}");
            assert!(txt.contains("\"stdio\""), "{target}: {txt}");
            assert!(txt.contains("\"--native-shadow-url\""), "{target}: {txt}");
            let _ = fs::remove_dir_all(&home);
            continue;
        }
        let v: Value = serde_json::from_str(&txt).expect("settings json");
        if target == "opencode" {
            let entry = &v["mcp"]["boole"];
            assert_eq!(entry["type"], "local", "{target}: {entry}");
            let command = entry["command"]
                .as_array()
                .expect("OpenCode local command array");
            assert!(command.len() > 1, "{target}: command={command:?}");
            assert_eq!(command[1], "stdio", "{target}: command={command:?}");
            let _ = fs::remove_dir_all(&home);
            continue;
        }
        let entry = &v["mcpServers"]["boole"];
        assert!(
            entry["command"].is_string(),
            "{target}: mcpServers.boole.command should be a string"
        );
        if target == "claude" {
            assert_eq!(entry["type"], "stdio", "{target}: {entry}");
        }
        let cmd = entry["command"].as_str().unwrap();
        assert!(
            cmd.ends_with("boole-mcp") || cmd.ends_with("boole-mcp.exe"),
            "{target}: command should be the boole-mcp binary path; got {cmd}"
        );
        assert!(
            entry["args"].is_array(),
            "{target}: mcpServers.boole.args should be an array"
        );
        let args: Vec<&str> = entry["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            args[0], "stdio",
            "{target}: args[0] must be 'stdio' (MCP stdio transport); got {args:?}"
        );
        assert!(
            args.contains(&"--node-url"),
            "{target}: args must include --node-url for proxy tools; got {args:?}"
        );
        assert!(
            args.windows(2)
                .any(|pair| { pair == ["--native-shadow-url", "http://127.0.0.1:8082"] }),
            "{target}: native verifier must use its separate loopback URL; got {args:?}"
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
fn install_codex_preserves_other_toml_tables_and_comments() {
    let home = temp_home();
    let s = settings_path(&home, "codex");
    fs::create_dir_all(s.parent().unwrap()).expect("mkdirs");
    fs::write(
        &s,
        "# Keep this user setting\nmodel = \"gpt-5.6\"\n\n[mcp_servers.other]\ncommand = \"other-mcp\"\n",
    )
    .expect("seed config");

    let out = run_install(&home, &["--target", "codex"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = fs::read_to_string(&s).expect("config");
    assert!(text.contains("# Keep this user setting"));
    assert!(text.contains("model = \"gpt-5.6\""));
    assert!(text.contains("[mcp_servers.other]"));
    assert!(text.contains("command = \"other-mcp\""));
    assert!(text.contains("[mcp_servers.boole]"));
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn install_codex_switches_http_to_stdio_without_transport_conflicts() {
    let home = temp_home();
    let s = settings_path(&home, "codex");
    fs::create_dir_all(s.parent().unwrap()).expect("mkdirs");
    fs::write(
        &s,
        r#"
[mcp_servers.boole]
url = "http://127.0.0.1:9/mcp"
bearer_token_env_var = "SYNTHETIC_TOKEN"
http_headers = { X-Test = "synthetic" }
env_http_headers = { X-Test-Env = "SYNTHETIC_HEADER" }
auth = "oauth"
http_headers_helper = ["echo", "{}"]
oauth = { client_id = "synthetic" }
enabled = false
tool_timeout_sec = 180
disabled_tools = ["boole.mine"]

[mcp_servers.other]
url = "http://127.0.0.1:8/mcp"
"#,
    )
    .expect("seed HTTP config");
    let out = run_install(&home, &["--target", "codex"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let first = fs::read_to_string(&s).expect("installed config");
    let parsed = first.parse::<toml_edit::DocumentMut>().expect("TOML");
    let entry = parsed["mcp_servers"]["boole"].as_table().unwrap();
    for key in [
        "url",
        "bearer_token_env_var",
        "http_headers",
        "env_http_headers",
        "auth",
        "http_headers_helper",
        "oauth",
    ] {
        assert!(
            !entry.contains_key(key),
            "HTTP-only {key} survived stdio switch"
        );
    }
    assert_eq!(entry["args"][0].as_str(), Some("stdio"));
    assert_eq!(entry["enabled"].as_bool(), Some(false));
    assert_eq!(entry["tool_timeout_sec"].as_integer(), Some(180));
    assert_eq!(entry["disabled_tools"][0].as_str(), Some("boole.mine"));
    assert_eq!(
        parsed["mcp_servers"]["other"]["url"].as_str(),
        Some("http://127.0.0.1:8/mcp")
    );
    assert!(run_install(&home, &["--target", "codex"]).status.success());
    assert_eq!(fs::read_to_string(&s).unwrap(), first);
    let _ = fs::remove_dir_all(&home);
}

#[test]
#[ignore = "requires an installed Codex CLI; run explicitly with BOOLE_TEST_CODEX_BIN"]
fn install_codex_http_to_stdio_loads_in_actual_codex_consumer() {
    let codex = env::var_os("BOOLE_TEST_CODEX_BIN").expect("BOOLE_TEST_CODEX_BIN");
    let home = temp_home();
    let s = settings_path(&home, "codex");
    fs::create_dir_all(s.parent().unwrap()).unwrap();
    fs::write(&s, "[mcp_servers.boole]\nurl = \"http://127.0.0.1:9/mcp\"\nbearer_token_env_var = \"SYNTHETIC_TOKEN\"\nhttp_headers = { X-Test = \"synthetic\" }\nenv_http_headers = { X-Test-Env = \"SYNTHETIC_HEADER\" }\n").unwrap();
    for expected in ["streamable_http", "stdio"] {
        let out = Command::new(&codex)
            .args(["mcp", "list", "--json"])
            .env("HOME", &home)
            .env("CODEX_HOME", home.join(".codex"))
            .current_dir(&home)
            .output()
            .expect("Codex config consumer");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let servers: Value = serde_json::from_slice(&out.stdout).expect("Codex list JSON");
        let boole = servers
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == "boole")
            .unwrap();
        assert_eq!(boole["transport"]["type"], expected);
        if expected == "streamable_http" {
            assert!(run_install(&home, &["--target", "codex"]).status.success());
        }
    }
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn install_codex_handles_quoted_and_inline_server_tables_without_losing_boole_env() {
    let home = temp_home();
    let s = settings_path(&home, "codex");
    fs::create_dir_all(s.parent().unwrap()).expect("mkdirs");
    fs::write(
        &s,
        "# keep user context\n[mcp_servers.\"other.server\"]\ncommand = \"other\"\n\n[mcp_servers.boole]\ncommand = \"old-boole\"\nargs = [\"old\"]\n\n[mcp_servers.boole.env]\nBOOLE_TOKEN = \"keep-me\"\n",
    )
    .expect("seed config");

    let out = run_install(&home, &["--target", "codex"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = fs::read_to_string(&s).expect("config");
    assert!(text.contains("# keep user context"));
    let parsed = text
        .parse::<toml_edit::DocumentMut>()
        .expect("valid TOML after merge");
    assert_eq!(
        parsed["mcp_servers"]["other.server"]["command"].as_str(),
        Some("other")
    );
    assert_eq!(
        parsed["mcp_servers"]["boole"]["env"]["BOOLE_TOKEN"].as_str(),
        Some("keep-me")
    );
    assert_ne!(
        parsed["mcp_servers"]["boole"]["command"].as_str(),
        Some("old-boole")
    );
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn install_codex_converts_inline_mcp_servers_to_a_valid_server_table() {
    let home = temp_home();
    let s = settings_path(&home, "codex");
    fs::create_dir_all(s.parent().unwrap()).expect("mkdirs");
    fs::write(
        &s,
        "mcp_servers = { other = { command = \"other\" }, boole = { command = \"old\" } }\n",
    )
    .expect("seed config");

    let out = run_install(&home, &["--target", "codex"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = fs::read_to_string(&s).expect("config");
    let parsed = text
        .parse::<toml_edit::DocumentMut>()
        .expect("valid TOML after merge");
    assert_eq!(
        parsed["mcp_servers"]["other"]["command"].as_str(),
        Some("other")
    );
    assert!(parsed["mcp_servers"]["boole"]["args"].is_array());
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn install_codex_rejects_invalid_toml_without_echoing_its_contents() {
    let home = temp_home();
    let s = settings_path(&home, "codex");
    fs::create_dir_all(s.parent().unwrap()).expect("mkdirs");
    let original = "token = \"secret-value-must-not-be-echoed\"\nthis is not TOML\n";
    fs::write(&s, original).expect("seed config");

    let out = run_install(&home, &["--target", "codex"]);
    assert!(!out.status.success(), "invalid TOML must fail");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("secret-value-must-not-be-echoed"));
    assert_eq!(
        fs::read_to_string(&s).expect("config left untouched"),
        original
    );
    let _ = fs::remove_dir_all(&home);
}

#[cfg(unix)]
#[test]
fn install_codex_keeps_existing_private_mode_and_leaves_no_temp_config() {
    let home = temp_home();
    let s = settings_path(&home, "codex");
    fs::create_dir_all(s.parent().unwrap()).expect("mkdirs");
    fs::write(&s, "[mcp_servers.private]\ncommand = \"private\"\n").expect("seed config");
    fs::set_permissions(&s, fs::Permissions::from_mode(0o600)).expect("restrict config");

    let out = run_install(&home, &["--target", "codex"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::metadata(&s).expect("mode").permissions().mode() & 0o777,
        0o600
    );
    assert!(fs::read_to_string(&s)
        .expect("config")
        .contains("[mcp_servers.private]"));
    let entries: Vec<_> = fs::read_dir(s.parent().unwrap())
        .expect("config directory")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(entries, vec![std::ffi::OsString::from("config.toml")]);
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
    let args = env["result"]["planned_content"]["mcpServers"]["boole"]["args"]
        .as_array()
        .expect("dry-run planned_content args must be an array");
    assert_eq!(
        args[0].as_str().unwrap_or(""),
        "stdio",
        "dry-run: args[0] must be 'stdio'; got {args:?}"
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
