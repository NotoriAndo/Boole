//! P1.10f / P1.10g — `boole-wallet-agent` integration tests. Boots the
//! compiled binary, drives subcommands via stdin+args, verifies on-disk
//! vault shape, round-trippable signatures, and plaintext-key migration.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use ed25519_dalek::{Signature, SigningKey, Verifier, VerifyingKey};
use rand_core::{OsRng, RngCore};
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

// ---- P1.10g migrate-from-hex tests ----------------------------------------

fn random_seed_hex() -> String {
    let mut seed = [0_u8; 32];
    OsRng.fill_bytes(&mut seed);
    hex::encode(seed)
}

fn migrate_input(passphrase: &str, seed_hex: &str) -> String {
    format!("{passphrase}\n{seed_hex}\n")
}

#[test]
fn migrate_from_hex_seals_provided_seed_and_pubkey_matches_native_derivation() {
    let dir = tmp_dir("migrate-seal");
    let vault = dir.join("migrated.vault.json");
    let seed_hex = random_seed_hex();
    let seed_bytes: [u8; 32] = hex::decode(&seed_hex)
        .expect("decode seed")
        .try_into()
        .expect("32 bytes");
    let expected_pubkey = hex::encode(
        SigningKey::from_bytes(&seed_bytes)
            .verifying_key()
            .to_bytes(),
    );

    let (code, stdout, stderr) = run_agent(
        &["migrate-from-hex", "--vault", vault.to_str().expect("path")],
        &migrate_input(PASSPHRASE, &seed_hex),
    );
    assert_eq!(code, 0, "migrate-from-hex failed: stderr={stderr}");
    assert_eq!(stdout.trim(), expected_pubkey);

    assert!(vault.exists(), "vault file must exist after migrate");
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &fs::metadata(&vault).expect("metadata").permissions(),
    );
    assert_eq!(mode & 0o777, 0o600, "vault must be 0600");
    let json: Value = serde_json::from_slice(&fs::read(&vault).expect("read")).expect("json");
    assert_eq!(json["version"], 1);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn migrate_from_hex_then_sign_round_trips_signature_under_migrated_pubkey() {
    let dir = tmp_dir("migrate-then-sign");
    let vault = dir.join("migrated.vault.json");
    let seed_hex = random_seed_hex();

    let (init_code, pubkey_line, init_stderr) = run_agent(
        &["migrate-from-hex", "--vault", vault.to_str().expect("path")],
        &migrate_input(PASSPHRASE, &seed_hex),
    );
    assert_eq!(init_code, 0, "migrate failed: stderr={init_stderr}");
    let pubkey_hex = pubkey_line.trim().to_string();

    let message = b"post-migration payload";
    let (sign_code, sign_stdout, sign_stderr) = run_agent(
        &[
            "sign",
            "--vault",
            vault.to_str().expect("path"),
            "--message",
            &hex::encode(message),
        ],
        &format!("{PASSPHRASE}\n"),
    );
    assert_eq!(
        sign_code, 0,
        "sign after migrate failed: stderr={sign_stderr}"
    );

    let pubkey_bytes: [u8; 32] = hex::decode(&pubkey_hex)
        .expect("pubkey hex")
        .try_into()
        .expect("32 bytes");
    let sig_bytes: [u8; 64] = hex::decode(sign_stdout.trim())
        .expect("sig hex")
        .try_into()
        .expect("64 bytes");
    VerifyingKey::from_bytes(&pubkey_bytes)
        .expect("valid pubkey")
        .verify(message, &Signature::from_bytes(&sig_bytes))
        .expect("signature must verify under migrated pubkey");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn migrate_from_hex_refuses_to_overwrite_existing_vault() {
    let dir = tmp_dir("migrate-no-overwrite");
    let vault = dir.join("existing.vault.json");
    let _ = init_vault(&vault);

    let seed_hex = random_seed_hex();
    let (code, _stdout, stderr) = run_agent(
        &["migrate-from-hex", "--vault", vault.to_str().expect("path")],
        &migrate_input(PASSPHRASE, &seed_hex),
    );
    assert_ne!(code, 0, "migrate over existing vault must fail");
    assert!(
        stderr.contains("already exists"),
        "stderr must mention overwrite refusal: {stderr}"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn migrate_from_hex_rejects_short_seed_with_typed_error_before_writing_vault() {
    let dir = tmp_dir("migrate-short-seed");
    let vault = dir.join("rejected.vault.json");
    let short = "deadbeef";

    let (code, _stdout, stderr) = run_agent(
        &["migrate-from-hex", "--vault", vault.to_str().expect("path")],
        &migrate_input(PASSPHRASE, short),
    );
    assert_ne!(code, 0, "short seed must fail");
    assert!(
        stderr.contains("32-byte") || stderr.contains("seed length"),
        "stderr must mention seed length: {stderr}"
    );
    assert!(
        !vault.exists(),
        "vault file must not be written on rejected input"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn migrate_from_hex_rejects_non_hex_seed_with_typed_error() {
    let dir = tmp_dir("migrate-non-hex");
    let vault = dir.join("rejected.vault.json");

    let (code, _stdout, stderr) = run_agent(
        &["migrate-from-hex", "--vault", vault.to_str().expect("path")],
        &migrate_input(
            PASSPHRASE,
            "not-hex-but-64-characters-long-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    );
    assert_ne!(code, 0, "non-hex seed must fail");
    assert!(
        stderr.contains("hex") || stderr.contains("Invalid character"),
        "stderr must mention hex failure: {stderr}"
    );
    assert!(!vault.exists(), "no vault on rejection");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn migrate_from_hex_rejects_empty_passphrase_before_consuming_seed_line() {
    let dir = tmp_dir("migrate-empty-pass");
    let vault = dir.join("rejected.vault.json");
    let seed_hex = random_seed_hex();

    let (code, _stdout, stderr) = run_agent(
        &["migrate-from-hex", "--vault", vault.to_str().expect("path")],
        &format!("\n{seed_hex}\n"),
    );
    assert_ne!(code, 0, "empty passphrase must fail");
    assert!(
        stderr.contains("passphrase must not be empty"),
        "stderr must mention empty passphrase: {stderr}"
    );
    assert!(!vault.exists(), "no vault on rejection");

    let _ = fs::remove_dir_all(&dir);
}
