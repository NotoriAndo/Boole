//! P1.10f — `boole-wallet-agent` integration tests. Boots the compiled
//! binary, drives subcommands via stdin+args, verifies on-disk vault
//! shape and round-trippable signatures.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::Value;

const PASSPHRASE: &str = "correct-horse-battery-staple";

fn tmp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "boole-wallet-agent-{}-{}-{}",
        label,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("tmp dir");
    dir
}

fn run_agent(args: &[&str], stdin_input: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_boole-wallet-agent"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn wallet-agent");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(stdin_input.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait wallet-agent");
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    let code = output.status.code().unwrap_or(-1);
    (code, stdout, stderr)
}

fn init_vault(vault_path: &Path) -> String {
    let (code, stdout, stderr) = run_agent(
        &["init", "--vault", vault_path.to_str().expect("path")],
        &format!("{PASSPHRASE}\n"),
    );
    assert_eq!(code, 0, "init failed: stderr={stderr}");
    stdout.trim().to_string()
}

#[test]
fn init_creates_vault_file_and_prints_pubkey_hex() {
    let dir = tmp_dir("init");
    let vault = dir.join("wallet.vault.json");

    let pubkey_hex = init_vault(&vault);

    assert!(vault.exists(), "vault file must exist");
    let meta = fs::metadata(&vault).expect("metadata");
    let mode = std::os::unix::fs::PermissionsExt::mode(&meta.permissions());
    assert_eq!(mode & 0o777, 0o600, "vault must be 0600");

    let bytes = fs::read(&vault).expect("read vault");
    let json: Value = serde_json::from_slice(&bytes).expect("parse vault json");
    assert_eq!(json["version"], 1);
    assert_eq!(json["kdf"]["algo"], "argon2id");
    assert_eq!(json["aead"]["algo"], "chacha20poly1305");

    assert_eq!(pubkey_hex.len(), 64, "pubkey hex must be 32 bytes");
    hex::decode(&pubkey_hex).expect("pubkey must be valid hex");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn init_refuses_to_overwrite_existing_vault() {
    let dir = tmp_dir("init-refuse-overwrite");
    let vault = dir.join("wallet.vault.json");
    let _first_pubkey = init_vault(&vault);

    let (code, _stdout, stderr) = run_agent(
        &["init", "--vault", vault.to_str().expect("path")],
        &format!("{PASSPHRASE}\n"),
    );
    assert_ne!(code, 0, "second init must fail");
    assert!(
        stderr.contains("already exists"),
        "stderr must mention overwrite refusal: {stderr}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pubkey_matches_init_pubkey_after_correct_passphrase() {
    let dir = tmp_dir("pubkey-matches");
    let vault = dir.join("wallet.vault.json");
    let expected = init_vault(&vault);

    let (code, stdout, stderr) = run_agent(
        &["pubkey", "--vault", vault.to_str().expect("path")],
        &format!("{PASSPHRASE}\n"),
    );
    assert_eq!(code, 0, "pubkey failed: stderr={stderr}");
    assert_eq!(stdout.trim(), expected);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn pubkey_with_wrong_passphrase_exits_nonzero_with_decryption_failure() {
    let dir = tmp_dir("pubkey-wrong-pass");
    let vault = dir.join("wallet.vault.json");
    let _ = init_vault(&vault);

    let (code, _stdout, stderr) = run_agent(
        &["pubkey", "--vault", vault.to_str().expect("path")],
        "wrong-passphrase\n",
    );
    assert_ne!(code, 0, "wrong passphrase must fail");
    assert!(
        stderr.contains("decryption failed") || stderr.contains("open vault"),
        "stderr must surface decryption failure: {stderr}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn sign_produces_ed25519_signature_verifiable_against_init_pubkey() {
    let dir = tmp_dir("sign-roundtrip");
    let vault = dir.join("wallet.vault.json");
    let pubkey_hex = init_vault(&vault);

    let message = b"boole-wallet-agent test payload";
    let message_hex = hex::encode(message);

    let (code, stdout, stderr) = run_agent(
        &[
            "sign",
            "--vault",
            vault.to_str().expect("path"),
            "--message",
            &message_hex,
        ],
        &format!("{PASSPHRASE}\n"),
    );
    assert_eq!(code, 0, "sign failed: stderr={stderr}");
    let signature_hex = stdout.trim();
    assert_eq!(signature_hex.len(), 128, "ed25519 sig must be 64 bytes hex");

    let pubkey_bytes: [u8; 32] = hex::decode(&pubkey_hex)
        .expect("decode pubkey hex")
        .try_into()
        .expect("32-byte pubkey");
    let verifying_key = VerifyingKey::from_bytes(&pubkey_bytes).expect("valid pubkey");
    let sig_bytes: [u8; 64] = hex::decode(signature_hex)
        .expect("decode sig hex")
        .try_into()
        .expect("64-byte sig");
    let signature = Signature::from_bytes(&sig_bytes);
    verifying_key
        .verify(message, &signature)
        .expect("signature must verify under reported pubkey");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn sign_with_empty_passphrase_exits_nonzero_before_touching_vault() {
    let dir = tmp_dir("sign-empty-pass");
    let vault = dir.join("wallet.vault.json");
    let _ = init_vault(&vault);

    let (code, _stdout, stderr) = run_agent(
        &[
            "sign",
            "--vault",
            vault.to_str().expect("path"),
            "--message",
            "deadbeef",
        ],
        "\n",
    );
    assert_ne!(code, 0, "empty passphrase must fail");
    assert!(
        stderr.contains("passphrase must not be empty"),
        "stderr must mention empty passphrase: {stderr}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn sign_rejects_non_hex_message_with_typed_error() {
    let dir = tmp_dir("sign-bad-hex");
    let vault = dir.join("wallet.vault.json");
    let _ = init_vault(&vault);

    let (code, _stdout, stderr) = run_agent(
        &[
            "sign",
            "--vault",
            vault.to_str().expect("path"),
            "--message",
            "not-a-hex-string",
        ],
        &format!("{PASSPHRASE}\n"),
    );
    assert_ne!(code, 0, "non-hex message must fail");
    assert!(
        stderr.contains("hex-encoded bytes") || stderr.contains("Invalid character"),
        "stderr must mention hex decode failure: {stderr}"
    );

    let _ = fs::remove_dir_all(&dir);
}
