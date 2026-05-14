// Miner state — keypair generation and persistent config.
//
// The boole-miner CLI runs on the miner's own machine and holds the miner's
// own ed25519 secret key in plaintext on disk (chmod 0600 on POSIX). This
// is the non-custodial path.
//
// Boole's address convention is pk-as-address: the 32-byte ed25519 public
// key (hex) is itself the on-chain address. This deviates from pof
// (bech32("boole", sha256(pk)[:20])) but matches every other Boole crate
// and avoids pulling in a bech32 dependency.
//
// State file location precedence:
//   $BOOLE_MINER_HOME/state.json
//   $XDG_CONFIG_HOME/boole-miner/state.json
//   $HOME/.config/boole-miner/state.json
use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use ed25519_dalek::{SigningKey, VerifyingKey};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const SCHEMA_VERSION: u32 = 1;
const ED25519_SK_BYTES: usize = 32;
const ED25519_PK_BYTES: usize = 32;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmConfig {
    pub backend: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent_args: Option<Vec<String>>,
}

// P0.8: hand-written `Debug` so api_key never reaches logs/panic messages.
// The presence of a key is still observable as `Some("<redacted>")` vs
// `None`, so missing-config diagnostics remain useful.
impl fmt::Debug for LlmConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlmConfig")
            .field("backend", &self.backend)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("agent_command", &self.agent_command)
            .field("agent_args", &self.agent_args)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DispatcherConfig {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MinerStateConfig {
    pub dispatcher: DispatcherConfig,
    pub llm: LlmConfig,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MinerState {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    /// ed25519 secret seed, hex (32 bytes).
    pub sk: String,
    /// ed25519 public key, hex (32 bytes).
    pub pk: String,
    /// Address used on-chain. Boole convention: address == pk hex.
    pub address: String,
    /// ISO 8601 UTC creation timestamp.
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub config: MinerStateConfig,
}

// P0.8: hand-written `Debug` so the ed25519 secret seed `sk` never reaches
// logs/panic messages. `pk` and `address` are public-by-design.
impl fmt::Debug for MinerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MinerState")
            .field("schema_version", &self.schema_version)
            .field("sk", &"<redacted>")
            .field("pk", &self.pk)
            .field("address", &self.address)
            .field("created_at", &self.created_at)
            .field("config", &self.config)
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum StateError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported state schemaVersion {0}")]
    UnsupportedSchema(u32),
    #[error("HOME env var not set")]
    HomeUnset,
    #[error("ed25519 secret key must be {ED25519_SK_BYTES} bytes, got {0}")]
    BadSkLength(usize),
    #[error("ed25519 public key must be {ED25519_PK_BYTES} bytes, got {0}")]
    BadPkLength(usize),
    #[error("hex decode failed for {field}: {detail}")]
    BadHex { field: &'static str, detail: String },
}

/// Boole convention: the address IS the ed25519 public key, hex-encoded.
pub fn pubkey_to_address(pk: &[u8; ED25519_PK_BYTES]) -> String {
    hex::encode(pk)
}

/// Resolve the path of the miner state file.
///
/// Precedence: `$BOOLE_MINER_HOME` > `$XDG_CONFIG_HOME/boole-miner` >
/// `$HOME/.config/boole-miner`.
pub fn default_state_path() -> Result<PathBuf, StateError> {
    if let Ok(p) = std::env::var("BOOLE_MINER_HOME") {
        return Ok(PathBuf::from(p).join("state.json"));
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(xdg).join("boole-miner").join("state.json"));
    }
    let home = std::env::var("HOME").map_err(|_| StateError::HomeUnset)?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("boole-miner")
        .join("state.json"))
}

/// Generate a fresh keypair + state envelope. Caller persists with `save_state`.
pub fn generate_miner_state(config: MinerStateConfig, now_iso8601_utc: &str) -> MinerState {
    let mut sk_bytes = [0u8; ED25519_SK_BYTES];
    OsRng.fill_bytes(&mut sk_bytes);
    let signing = SigningKey::from_bytes(&sk_bytes);
    let pk_bytes = signing.verifying_key().to_bytes();
    MinerState {
        schema_version: SCHEMA_VERSION,
        sk: hex::encode(sk_bytes),
        pk: hex::encode(pk_bytes),
        address: pubkey_to_address(&pk_bytes),
        created_at: now_iso8601_utc.to_string(),
        config,
    }
}

pub fn state_exists(path: &Path) -> bool {
    path.exists()
}

pub fn load_state(path: &Path) -> Result<MinerState, StateError> {
    let raw = std::fs::read(path)?;
    let state: MinerState = serde_json::from_slice(&raw)?;
    if state.schema_version != SCHEMA_VERSION {
        return Err(StateError::UnsupportedSchema(state.schema_version));
    }
    Ok(state)
}

/// Atomic write at mode 0600: tmp file in same directory → fsync → rename.
/// Mirrors `crates/boole-cli/src/main.rs::atomic_write_0600`. The mode is
/// set at open time so plaintext sk is never world-readable, even
/// transiently.
pub fn save_state(state: &MinerState, path: &Path) -> Result<(), StateError> {
    if state.schema_version != SCHEMA_VERSION {
        return Err(StateError::UnsupportedSchema(state.schema_version));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(state)?;
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err.into());
    }
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct ConfigPatch {
    pub dispatcher_url: Option<String>,
    pub llm: Option<LlmConfig>,
}

/// Update persistent config without regenerating the keypair.
pub fn update_config(patch: ConfigPatch, path: &Path) -> Result<MinerState, StateError> {
    let mut state = load_state(path)?;
    if let Some(url) = patch.dispatcher_url {
        state.config.dispatcher.url = url;
    }
    if let Some(llm) = patch.llm {
        state.config.llm = llm;
    }
    save_state(&state, path)?;
    Ok(state)
}

/// Decode the persisted secret key into a usable signing key.
pub fn signing_key_from_state(state: &MinerState) -> Result<SigningKey, StateError> {
    let bytes = hex::decode(&state.sk).map_err(|e| StateError::BadHex {
        field: "sk",
        detail: e.to_string(),
    })?;
    if bytes.len() != ED25519_SK_BYTES {
        return Err(StateError::BadSkLength(bytes.len()));
    }
    let mut sk = [0u8; ED25519_SK_BYTES];
    sk.copy_from_slice(&bytes);
    Ok(SigningKey::from_bytes(&sk))
}

pub fn verifying_key_from_state(state: &MinerState) -> Result<VerifyingKey, StateError> {
    let signing = signing_key_from_state(state)?;
    Ok(signing.verifying_key())
}
