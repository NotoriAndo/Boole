//! P1.10f — `boole-wallet-agent` signing isolation binary.
//!
//! Owns one encrypted wallet vault (P1.10e `EncryptedVault`) on disk and
//! exposes a minimal subcommand surface:
//!
//! - `init   --vault <path>` — generate a fresh ed25519 keypair, seal
//!   the seed into a new vault file, print the public key (hex).
//! - `pubkey --vault <path>` — open the vault and print the public key.
//! - `sign   --vault <path> --message <hex>` — open the vault, sign the
//!   message bytes (raw ed25519), print the signature (hex).
//!
//! Passphrase input: first line of stdin. This keeps the binary
//! invocation pattern identical for interactive shells (`read -s` +
//! pipe) and for test fixtures (`echo "$PASS" | ...`), and avoids
//! pulling in an interactive TTY dependency at this slice. A future
//! slice may add an `rpassword` prompt when isatty().
//!
//! AAD binding: `boole-wallet-agent.v1`. Any other vault (different
//! consumer) must bind a different AAD; mixing vault files across
//! consumers will fail at open() with `DecryptionFailed`.

use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use boole_core::vault::{EncryptedVault, VaultParams};
use clap::{Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey, SECRET_KEY_LENGTH};
use rand_core::{OsRng, RngCore};

const VAULT_AAD: &[u8] = b"boole-wallet-agent.v1";

#[derive(Parser)]
#[command(name = "boole-wallet-agent", about = "Boole wallet signing agent")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init {
        #[arg(long)]
        vault: PathBuf,
    },
    Pubkey {
        #[arg(long)]
        vault: PathBuf,
    },
    Sign {
        #[arg(long)]
        vault: PathBuf,
        #[arg(long)]
        message: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Init { vault } => cmd_init(&vault),
        Command::Pubkey { vault } => cmd_pubkey(&vault),
        Command::Sign { vault, message } => cmd_sign(&vault, &message),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let _ = writeln!(io::stderr(), "boole-wallet-agent: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn read_passphrase() -> Result<Vec<u8>> {
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .context("read passphrase from stdin")?;
    let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
    if trimmed.is_empty() {
        bail!("passphrase must not be empty");
    }
    Ok(trimmed.as_bytes().to_vec())
}

fn cmd_init(vault_path: &Path) -> Result<()> {
    if vault_path.exists() {
        bail!(
            "vault already exists at {}; refusing to overwrite",
            vault_path.display()
        );
    }
    let passphrase = read_passphrase()?;
    let mut seed = [0_u8; SECRET_KEY_LENGTH];
    OsRng.fill_bytes(&mut seed);
    let signing_key = SigningKey::from_bytes(&seed);
    let pubkey_hex = hex::encode(signing_key.verifying_key().to_bytes());
    let vault = EncryptedVault::seal(&passphrase, &seed, VAULT_AAD, VaultParams::default())
        .map_err(|e| anyhow!("seal vault: {e}"))?;
    let bytes = vault
        .to_json_bytes()
        .map_err(|e| anyhow!("serialize vault: {e}"))?;
    write_file_atomic_0600(vault_path, &bytes)?;
    println!("{pubkey_hex}");
    Ok(())
}

fn cmd_pubkey(vault_path: &Path) -> Result<()> {
    let signing_key = open_signing_key(vault_path)?;
    println!("{}", hex::encode(signing_key.verifying_key().to_bytes()));
    Ok(())
}

fn cmd_sign(vault_path: &Path, message_hex: &str) -> Result<()> {
    let message = hex::decode(message_hex).context("--message must be hex-encoded bytes")?;
    let signing_key = open_signing_key(vault_path)?;
    let signature = signing_key.sign(&message);
    println!("{}", hex::encode(signature.to_bytes()));
    Ok(())
}

fn open_signing_key(vault_path: &Path) -> Result<SigningKey> {
    let passphrase = read_passphrase()?;
    let bytes = fs::read(vault_path)
        .with_context(|| format!("read vault file {}", vault_path.display()))?;
    let vault = EncryptedVault::from_json_bytes(&bytes)
        .map_err(|e| anyhow!("parse vault envelope: {e}"))?;
    let seed = vault
        .open(&passphrase, VAULT_AAD)
        .map_err(|e| anyhow!("open vault: {e}"))?;
    let seed_array: [u8; SECRET_KEY_LENGTH] = seed
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("vault plaintext is not a {SECRET_KEY_LENGTH}-byte ed25519 seed"))?;
    Ok(SigningKey::from_bytes(&seed_array))
}

fn write_file_atomic_0600(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create parent dir {}", parent.display()))?;
    let tmp = path.with_extension("vault.tmp");
    let _ = fs::remove_file(&tmp);
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .with_context(|| format!("create temp vault file {}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("rename to {}", path.display()))?;
    Ok(())
}
