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
// State file location precedence (P2.3):
//   $BOOLE_MINER_HOME/state.json               (per-subsystem override)
//   $BOOLE_HOME/miner/state.json               (workspace-wide root,
//                                               matches boole-cli's
//                                               keys/sessions/signer-nonces
//                                               layout under BOOLE_HOME)
//   $HOME/.boole/miner/state.json              (canonical post-P2.3 default
//                                               when no env override is set)
//   $XDG_CONFIG_HOME/boole-miner/state.json    (legacy XDG-style location,
//                                               kept as a fallback so
//                                               operators on the pre-
//                                               BOOLE_HOME layout don't
//                                               see their miner state move)
//   $HOME/.config/boole-miner/state.json       (final fallback)
//
// The migration helper `try_migrate_legacy_state_with` detects state
// at one of the two legacy locations, copies it atomically to the
// canonical modern path, and returns a typed outcome so the caller can
// print a one-line stderr notice exactly once per migration.
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

/// Process-level snapshot of the env vars that pin the miner state
/// path. Built once at CLI startup so the resolver and the migration
/// helper see a consistent view, and so tests can construct a
/// deterministic `StateEnv` instead of mutating process-global env
/// vars (which races under cargo's parallel test runner).
#[derive(Debug, Clone)]
pub struct StateEnv {
    pub boole_miner_home: Option<PathBuf>,
    pub boole_home: Option<PathBuf>,
    pub xdg_config_home: Option<PathBuf>,
    pub home: PathBuf,
}

impl StateEnv {
    /// Snapshot from the current process env. Returns `HomeUnset` if
    /// `$HOME` is not exported.
    pub fn from_process() -> Result<Self, StateError> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or(StateError::HomeUnset)?;
        Ok(Self {
            boole_miner_home: std::env::var_os("BOOLE_MINER_HOME").map(PathBuf::from),
            boole_home: std::env::var_os("BOOLE_HOME").map(PathBuf::from),
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            home,
        })
    }
}

/// Outcome of a single legacy-state migration check. The caller maps
/// each variant to a stderr line so the migration notice appears
/// exactly once per migration (`Migrated`), and a softer warning
/// surfaces when an operator left the old file lying around
/// (`BothPresent`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyMigration {
    Migrated { from: PathBuf, to: PathBuf },
    BothPresent { legacy: PathBuf, modern: PathBuf },
}

/// Canonical modern state path: `$BOOLE_MINER_HOME` if set, else
/// `$BOOLE_HOME/miner`, else `$HOME/.boole/miner`. Crucially this
/// never falls back to the legacy XDG/`.config` paths — those belong
/// to `legacy_candidates_with`.
pub fn canonical_state_path_with(env: &StateEnv) -> Result<PathBuf, StateError> {
    if let Some(p) = &env.boole_miner_home {
        return Ok(p.join("state.json"));
    }
    if let Some(p) = &env.boole_home {
        return Ok(p.join("miner").join("state.json"));
    }
    Ok(env.home.join(".boole").join("miner").join("state.json"))
}

/// Ordered legacy state paths to probe when migrating. XDG-style
/// location first (if `$XDG_CONFIG_HOME` is set), then the
/// `$HOME/.config/boole-miner` fallback.
pub fn legacy_candidates_with(env: &StateEnv) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(xdg) = &env.xdg_config_home {
        out.push(xdg.join("boole-miner").join("state.json"));
    }
    out.push(
        env.home
            .join(".config")
            .join("boole-miner")
            .join("state.json"),
    );
    out
}

/// Probe the legacy state locations and copy to the canonical modern
/// path when only the legacy file is present. Returns:
///
///   * `Ok(None)` — neither legacy nor modern present (clean install),
///     or legacy absent and modern present (post-migration steady state).
///   * `Ok(Some(Migrated))` — legacy was present and modern was empty;
///     bytes were copied atomically. Caller prints the one-time notice.
///   * `Ok(Some(BothPresent))` — both files exist; modern wins and is
///     not touched. Caller prints a "operator can remove legacy" warning.
///
/// The legacy file is left in place after a successful migration so the
/// operator can verify the copy by hand before deleting the old path.
pub fn try_migrate_legacy_state_with(
    env: &StateEnv,
) -> Result<Option<LegacyMigration>, StateError> {
    let modern = canonical_state_path_with(env)?;
    let legacy_present = legacy_candidates_with(env)
        .into_iter()
        .find(|p| p != &modern && p.exists());

    if modern.exists() {
        if let Some(legacy) = legacy_present {
            return Ok(Some(LegacyMigration::BothPresent { legacy, modern }));
        }
        return Ok(None);
    }

    let Some(legacy) = legacy_present else {
        return Ok(None);
    };

    if let Some(parent) = modern.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Atomic copy: read legacy → write tmp at mode 0600 → fsync → rename.
    let bytes = std::fs::read(&legacy)?;
    let tmp = modern.with_extension(format!("json.tmp.{}", std::process::id()));
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, &modern) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }

    Ok(Some(LegacyMigration::Migrated {
        from: legacy,
        to: modern,
    }))
}

/// Resolve the path of the miner state file. Honours the legacy
/// fallbacks for read-side compatibility; writers should call
/// `canonical_state_path_with(&StateEnv::from_process()?)` after the
/// CLI has driven any one-shot legacy migration.
pub fn default_state_path() -> Result<PathBuf, StateError> {
    let env = StateEnv::from_process()?;
    let canonical = canonical_state_path_with(&env)?;
    if canonical.exists() {
        return Ok(canonical);
    }
    for legacy in legacy_candidates_with(&env) {
        if legacy.exists() {
            return Ok(legacy);
        }
    }
    Ok(canonical)
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
