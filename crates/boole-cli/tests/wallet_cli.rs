//! P2.9 — `boole wallet` subcommand façade.
//!
//! The signing primitives that produce and operate on the encrypted
//! vault live in `boole-wallet-agent` (P1.10e/f/g). This slice exposes
//! them on the umbrella `boole` CLI so a wallet operator no longer has
//! to know about a second binary name:
//!
//!   boole wallet init    --vault <path>            (stdin: passphrase)
//!   boole wallet address --vault <path>            (stdin: passphrase)
//!   boole wallet sign    --vault <path> --message <hex>
//!                                                  (stdin: passphrase)
//!   boole wallet migrate --vault <path>            (stdin: passphrase\nseed-hex)
//!
//! The wallet state file is independent of miner state — `--vault` is
//! a free-form path, not anchored to the miner home — so a wallet can
//! sign on behalf of multiple miner instances or live on a removable
//! volume entirely separate from the miner config.
//!
//! These tests pin three contracts:
//!   1. `init` then `address` round-trips: the address printed at vault
//!      creation matches what reads back from the same vault. A drift
//!      would mean either the CLI shells out to the wrong subcommand,
//!      passes the passphrase incorrectly, or strips/munges the agent's
//!      stdout.
//!   2. `sign` produces a signature that verifies against the address.
//!      End-to-end proof the façade preserves the ed25519 contract.
//!   3. Every leaf wallet command, when invoked with `--json`, emits
//!      the unified P2.5 envelope (`{"ok":true,"version":"v1",
//!      "command":"wallet.<verb>","result":{...}}`) — so downstream
//!      tools (scripts, the boole-mcp proxy) can parse wallet output
//!      with the same schema as every other JSON CLI command.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use boole_testkit::rand_suffix;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::Value;

const PASSPHRASE: &str = "correct-horse-battery-staple";

fn cli_bin() -> &'static str {
    env!("CARGO_BIN_EXE_boole-cli")
}

/// Resolve the `boole-wallet-agent` binary via the same sibling-of-
/// boole-cli convention `resolve_wallet_agent_binary()` uses inside the
/// CLI itself (and `node_block.rs` uses for `boole-node`). Cargo only
/// exposes `CARGO_BIN_EXE_*` within the bin's own crate, so cross-crate
/// integration tests must walk to `target/<profile>/`.
fn wallet_agent_bin() -> PathBuf {
    let cli = Path::new(cli_bin());
    let sibling = cli
        .parent()
        .expect("cli has parent dir")
        .join("boole-wallet-agent");
    if !sibling.exists() {
        // Force-build the bin so the test does not silently fall through
        // to a stale or missing path. `cargo test -p boole-cli --test
        // wallet_cli` does not transitively build sibling crates'
        // binaries, but the boole-wallet-agent dev-dep declaration in
        // Cargo.toml ensures the artifact is reachable here.
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "-p", "boole-wallet-agent"])
            .status()
            .expect("invoke cargo to build boole-wallet-agent");
        assert!(
            status.success(),
            "cargo build -p boole-wallet-agent exited non-zero"
        );
    }
    assert!(
        sibling.exists(),
        "expected boole-wallet-agent at {} after cargo build; check Cargo.toml dev-dep",
        sibling.display()
    );
    sibling
}

fn tmp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-cli-wallet-{label}-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmp dir");
    dir
}

fn run_cli(args: &[&str], stdin_input: &str) -> (i32, String, String) {
    let mut child = Command::new(cli_bin())
        .args(args)
        .env("BOOLE_WALLET_AGENT_BIN", wallet_agent_bin())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn boole-cli");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(stdin_input.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait boole-cli");
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    let code = output.status.code().unwrap_or(-1);
    (code, stdout, stderr)
}

fn parse_envelope(stdout: &str) -> Value {
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("CLI stdout not JSON: {e}: {stdout:?}"))
}

fn assert_ok_envelope(parsed: &Value, command: &str) -> Value {
    assert_eq!(
        parsed.get("ok"),
        Some(&Value::Bool(true)),
        "envelope ok must be true; got {parsed}"
    );
    assert_eq!(
        parsed.get("version").and_then(Value::as_str),
        Some("v1"),
        "envelope version must be v1; got {parsed}"
    );
    assert_eq!(
        parsed.get("command").and_then(Value::as_str),
        Some(command),
        "envelope command must be {command:?}; got {parsed}"
    );
    parsed
        .get("result")
        .cloned()
        .unwrap_or_else(|| panic!("envelope missing result field; got {parsed}"))
}

#[test]
fn wallet_init_then_address_round_trips_with_json_envelope() {
    let dir = tmp_dir("init-address");
    let vault = dir.join("wallet.vault.json");

    let vault_str = vault.to_str().expect("vault path utf8");
    let (code_init, stdout_init, stderr_init) = run_cli(
        &["wallet", "init", "--vault", vault_str, "--json"],
        &format!("{PASSPHRASE}\n"),
    );
    assert_eq!(
        code_init, 0,
        "wallet init exit code must be 0; stderr={stderr_init}"
    );
    let init_env = parse_envelope(&stdout_init);
    let init_result = assert_ok_envelope(&init_env, "wallet.init");
    let init_address = init_result
        .get("address")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("wallet init envelope missing address; got {init_env}"))
        .to_string();
    assert_eq!(
        init_address.len(),
        64,
        "wallet address must be 32-byte ed25519 pubkey in hex; got {init_address:?}"
    );
    hex::decode(&init_address).expect("address must be valid hex");
    assert!(
        vault.exists(),
        "wallet init must persist the vault file at --vault path"
    );
    assert_metadata_0600(&vault);

    let (code_addr, stdout_addr, stderr_addr) = run_cli(
        &["wallet", "address", "--vault", vault_str, "--json"],
        &format!("{PASSPHRASE}\n"),
    );
    assert_eq!(
        code_addr, 0,
        "wallet address exit code must be 0; stderr={stderr_addr}"
    );
    let addr_env = parse_envelope(&stdout_addr);
    let addr_result = assert_ok_envelope(&addr_env, "wallet.address");
    let addr_value = addr_result
        .get("address")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("wallet address envelope missing address; got {addr_env}"));
    assert_eq!(
        addr_value, init_address,
        "wallet address must read back exactly what wallet init produced; \
         drift would indicate the façade passes the passphrase or vault \
         path inconsistently between subcommands"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wallet_sign_produces_signature_that_verifies_against_address() {
    let dir = tmp_dir("sign");
    let vault = dir.join("wallet.vault.json");
    let vault_str = vault.to_str().expect("vault path utf8");

    let (code_init, stdout_init, stderr_init) = run_cli(
        &["wallet", "init", "--vault", vault_str, "--json"],
        &format!("{PASSPHRASE}\n"),
    );
    assert_eq!(code_init, 0, "wallet init failed: stderr={stderr_init}");
    let init_env = parse_envelope(&stdout_init);
    let address_hex = assert_ok_envelope(&init_env, "wallet.init")
        .get("address")
        .and_then(Value::as_str)
        .expect("address")
        .to_string();

    let message_bytes: &[u8] = b"P2.9 wallet sign roundtrip";
    let message_hex = hex::encode(message_bytes);
    let (code_sign, stdout_sign, stderr_sign) = run_cli(
        &[
            "wallet",
            "sign",
            "--vault",
            vault_str,
            "--message",
            &message_hex,
            "--json",
        ],
        &format!("{PASSPHRASE}\n"),
    );
    assert_eq!(
        code_sign, 0,
        "wallet sign exit code must be 0; stderr={stderr_sign}"
    );
    let sign_env = parse_envelope(&stdout_sign);
    let sign_result = assert_ok_envelope(&sign_env, "wallet.sign");
    let signature_hex = sign_result
        .get("signature")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("wallet sign envelope missing signature; got {sign_env}"));
    assert_eq!(
        signature_hex.len(),
        128,
        "ed25519 signature hex must be 64 bytes; got {signature_hex:?}"
    );

    let pubkey_bytes = hex::decode(&address_hex).expect("address hex");
    let pubkey_array: [u8; 32] = pubkey_bytes
        .as_slice()
        .try_into()
        .expect("address must be 32 bytes");
    let verifying_key = VerifyingKey::from_bytes(&pubkey_array).expect("ed25519 pubkey");
    let signature_bytes = hex::decode(signature_hex).expect("signature hex");
    let signature_array: [u8; 64] = signature_bytes
        .as_slice()
        .try_into()
        .expect("signature must be 64 bytes");
    let signature = Signature::from_bytes(&signature_array);
    verifying_key
        .verify(message_bytes, &signature)
        .expect("CLI-produced signature must verify against CLI-produced address");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn wallet_migrate_ingests_existing_hex_seed_into_new_vault() {
    let dir = tmp_dir("migrate");
    let vault = dir.join("wallet.vault.json");
    let vault_str = vault.to_str().expect("vault path utf8");

    let seed_hex = "1111111122222222333333334444444455555555666666667777777788888888";
    let stdin_input = format!("{PASSPHRASE}\n{seed_hex}\n");
    let (code, stdout, stderr) = run_cli(
        &["wallet", "migrate", "--vault", vault_str, "--json"],
        &stdin_input,
    );
    assert_eq!(
        code, 0,
        "wallet migrate exit code must be 0; stderr={stderr}"
    );
    let env = parse_envelope(&stdout);
    let result = assert_ok_envelope(&env, "wallet.migrate");
    let address_hex = result
        .get("address")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("wallet migrate envelope missing address; got {env}"));

    let seed_bytes = hex::decode(seed_hex).expect("seed hex");
    let seed_array: [u8; 32] = seed_bytes.as_slice().try_into().expect("32-byte seed");
    let expected_signing = ed25519_dalek::SigningKey::from_bytes(&seed_array);
    let expected_pubkey = hex::encode(expected_signing.verifying_key().to_bytes());
    assert_eq!(
        address_hex, expected_pubkey,
        "wallet migrate must derive the same ed25519 pubkey from the \
         supplied seed as boole-wallet-agent would; drift means the CLI \
         is feeding the seed-hex line to the agent on the wrong stdin \
         position or stripping the passphrase"
    );

    assert!(vault.exists(), "migrate must persist the vault file");
    assert_metadata_0600(&vault);

    let _ = std::fs::remove_dir_all(&dir);
}

fn assert_metadata_0600(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).expect("vault metadata");
    let mode = meta.permissions().mode();
    assert_eq!(
        mode & 0o777,
        0o600,
        "wallet vault file must be 0600 (operator-only); got {:o}",
        mode & 0o777
    );
}
