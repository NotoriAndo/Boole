//! P1.10f / P1.10g — `boole-wallet-agent` signing isolation binary and
//! plaintext-key migration tool.
//!
//! Owns one encrypted wallet vault (P1.10e `EncryptedVault`) on disk and
//! exposes a minimal subcommand surface:
//!
//! - `init   --vault <path>` — generate a fresh ed25519 keypair, seal
//!   the seed into a new vault file, print the public key (hex).
//! - `pubkey --vault <path>` — open the vault and print the public key.
//! - `sign   --vault <path> --message <hex>` — open the vault, sign the
//!   message bytes (raw ed25519), print the signature (hex).
//! - `migrate-from-hex --vault <path>` — read a passphrase + 32-byte
//!   hex seed from stdin (two lines) and seal the seed into a new
//!   vault. Converts the existing plaintext miner `state.json` /
//!   `~/.boole/keys/<id>.json` / `~/.boole/sessions/<id>.json` files
//!   when piped through `jq -r .sk`. Never accepts secrets on argv.
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

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{anyhow, bail, Context, Result};
use boole_core::vault::{EncryptedVault, VaultParams};
use boole_wallet_agent::VAULT_AAD;
use clap::{Parser, Subcommand};
use ed25519_dalek::{Signer, SigningKey, SECRET_KEY_LENGTH};
use rand_core::{OsRng, RngCore};

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
    MigrateFromHex {
        #[arg(long)]
        vault: PathBuf,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Init { vault } => cmd_init(&vault),
        Command::Pubkey { vault } => cmd_pubkey(&vault),
        Command::Sign { vault, message } => cmd_sign(&vault, &message),
        Command::MigrateFromHex { vault } => cmd_migrate_from_hex(&vault),
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
    let line = read_stdin_line().context("read passphrase from stdin")?;
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
    write_new_file_atomic_0600(vault_path, &bytes)?;
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
    let signature = signing_key.sign(&message); // P2.10-exempt: raw ed25519, not a SignedEnvelope constructor (ADR-0003 §42-46)
    println!("{}", hex::encode(signature.to_bytes()));
    Ok(())
}

fn cmd_migrate_from_hex(vault_path: &Path) -> Result<()> {
    if vault_path.exists() {
        bail!(
            "vault already exists at {}; refusing to overwrite",
            vault_path.display()
        );
    }
    let passphrase = read_passphrase()?;
    let seed_line = read_stdin_line().context("read seed hex line from stdin")?;
    let seed_hex = seed_line
        .trim_end_matches('\n')
        .trim_end_matches('\r')
        .trim();
    let seed_bytes = hex::decode(seed_hex).context("seed must be hex-encoded bytes")?;
    if seed_bytes.len() != SECRET_KEY_LENGTH {
        bail!(
            "seed length {} bytes; expected {SECRET_KEY_LENGTH}-byte ed25519 seed",
            seed_bytes.len()
        );
    }
    let seed_array: [u8; SECRET_KEY_LENGTH] = seed_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("seed slice -> array conversion"))?;
    let signing_key = SigningKey::from_bytes(&seed_array);
    let pubkey_hex = hex::encode(signing_key.verifying_key().to_bytes());
    let vault = EncryptedVault::seal(&passphrase, &seed_array, VAULT_AAD, VaultParams::default())
        .map_err(|e| anyhow!("seal vault: {e}"))?;
    let bytes = vault
        .to_json_bytes()
        .map_err(|e| anyhow!("serialize vault: {e}"))?;
    write_new_file_atomic_0600(vault_path, &bytes)?;
    println!("{pubkey_hex}");
    Ok(())
}

fn read_stdin_line() -> Result<String> {
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line)
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

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn sync_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

/// Create a new vault without ever replacing an existing final path. The hard
/// link is the create-if-absent commit: two writers may stage safely, but only
/// one can link its staged inode to `path`.
fn write_new_file_atomic_0600(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create parent dir {}", parent.display()))?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("vault path has no UTF-8 filename")?;
    let mut staged = None;
    for _ in 0..64 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let tmp = parent.join(format!(
            ".{filename}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
        {
            Ok(file) => {
                staged = Some((tmp, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).with_context(|| format!("create {}", tmp.display())),
        }
    }
    let (tmp, mut file) = staged.context("create unique temporary vault file")?;
    let write_result = (|| -> io::Result<()> {
        file.write_all(bytes)?;
        file.sync_all()
    })();
    drop(file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(error).with_context(|| format!("write staged vault {}", tmp.display()));
    }
    if let Err(error) = fs::hard_link(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(error).with_context(|| format!("create vault {}", path.display()));
    }
    if let Err(error) = sync_directory(parent) {
        let _ = fs::remove_file(&tmp);
        return Err(error).with_context(|| format!("sync vault directory {}", parent.display()));
    }
    fs::remove_file(&tmp).with_context(|| format!("remove staged vault {}", tmp.display()))?;
    sync_directory(parent).with_context(|| format!("sync vault directory {}", parent.display()))?;
    Ok(())
}
