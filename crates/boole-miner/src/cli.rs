// boole-miner CLI subtree.
//
// Exposed as a library entry point so both `boole-miner` (the standalone
// binary at `src/bin/boole-miner.rs`) and `boole-cli` (via `mine start /
// mine bounty`) can drive the same code paths without duplicating clap
// argument parsing.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use clap::{Args, Subcommand, ValueEnum};

use crate::bounty_client::{BountyClient, BountyProofInputs, BountyProofResult};
use crate::canonicalizer::StructuralCanonicalizer;
use crate::chain_head::HttpChainHeadFetcher;
use crate::llm_driver::{
    create_driver, AgentCliDriver, ClaudeCliDriver, LLMBackend, LLMDriverConfig, MockDriver,
    MockResponse, ProverDriver,
};
use crate::local_verify::Verifier;
// P1.9 — `AcceptingVerifier` import is feature-gated together with the
// `--mock-verify-accept` flag below; the no-feature build never
// references the bypass.
#[cfg(feature = "dev-tools")]
use crate::local_verify::AcceptingVerifier;
use crate::mining_loop::{
    run_mining_loop, MiningEvent, MiningLoopDeps, MiningLoopOptions, MiningLoopSummary,
    MiningRunContext, MiningRunDriverMode, MiningRunTargetMode, MiningRunVerifierMode,
};
use crate::state::{
    default_state_path, generate_miner_state, load_state, save_state, signing_key_from_state,
    state_exists, update_config, ConfigPatch, DispatcherConfig, LlmConfig, MinerStateConfig,
};
use crate::submit_client::{SubmitClient, Submitter};
use crate::target_emitter::{
    FamilyV1LengthBoundTargetEmitter, FixedSeedTargetEmitter, StubTargetEmitter, TargetEmitter,
};
use boole_core::Hex32;

#[derive(Debug, Subcommand)]
pub enum MineCommand {
    /// Generate ed25519 keypair, derive address, save state file.
    Init(InitArgs),
    /// Print the miner's address (= public key hex).
    Address(StateArgs),
    /// Read or write persistent miner config.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Run the mining loop until stop conditions are met.
    Start(StartArgs),
    /// Submit a single bounty proof to a dispatcher.
    Bounty(BountyArgs),
}

#[derive(Debug, Args)]
pub struct StateArgs {
    /// Override the state file path (default: $BOOLE_MINER_HOME / XDG / ~/.config).
    #[arg(long)]
    pub state: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    #[command(flatten)]
    pub state_args: StateArgs,
    /// Dispatcher base URL.
    #[arg(long = "dispatcher-url", default_value = "http://127.0.0.1:8080")]
    pub dispatcher_url: String,
    /// LLM backend: mock | claude_cli | agent_cli | openai_compat | anthropic | openai | google.
    #[arg(long = "llm-backend", default_value = "mock")]
    pub llm_backend: String,
    /// Override the model id (forwarded to the backend if applicable).
    #[arg(long = "llm-model")]
    pub llm_model: Option<String>,
    /// Base URL override. Required for openai_compat (e.g. http://localhost:11434).
    /// Optional for anthropic / openai / google (Azure proxy / Vertex / etc.).
    #[arg(long = "llm-base-url")]
    pub llm_base_url: Option<String>,
    /// `agent_cli` executable (e.g. hermes, openclaw, opencode).
    #[arg(long = "agent-command")]
    pub agent_command: Option<String>,
    /// `agent_cli` argv prefix as JSON string array.
    #[arg(long = "agent-args")]
    pub agent_args: Option<String>,
    /// Overwrite an existing state file.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print config (or a single dotted key).
    Get {
        #[command(flatten)]
        state_args: StateArgs,
        /// Optional dotted key (`dispatcher.url`, `llm.backend`, ...).
        key: Option<String>,
        /// Reveal secret values (e.g. `llm.apiKey`) in plaintext.
        #[arg(long)]
        reveal: bool,
    },
    /// Update a single dotted config key.
    Set {
        #[command(flatten)]
        state_args: StateArgs,
        key: String,
        value: String,
    },
}

#[derive(Debug, Args)]
pub struct StartArgs {
    #[command(flatten)]
    pub state_args: StateArgs,
    /// Profile for active family-derived targets.
    #[arg(long, default_value = "v1-lenbound")]
    pub profile: String,
    /// Difficulty parameter D.
    #[arg(short = 'D', long, default_value_t = 1)]
    pub difficulty: u32,
    /// Structural N parameter (v03+).
    #[arg(short = 'N', long)]
    pub n: Option<u32>,
    /// Stop after N accepted shares.
    #[arg(long = "max-shares")]
    pub max_shares: Option<u64>,
    /// Stop after N ticket cycles.
    #[arg(long = "max-cycles")]
    pub max_cycles: Option<u64>,
    /// HTTP timeout for `GET /head`.
    #[arg(long = "head-timeout-ms", default_value_t = 10_000)]
    pub head_timeout_ms: u64,
    /// Override `--llm-backend mock` with this canned response.
    #[arg(long = "mock-llm-response")]
    pub mock_llm_response: Option<String>,
    /// Bypass Lean verification and accept any generated proof source.
    ///
    /// P1.9 — feature-gated so a release build does not expose the
    /// bypass on the CLI surface; tests opt in via
    /// `cargo test --features boole-miner/dev-tools`.
    #[cfg(feature = "dev-tools")]
    #[arg(long = "mock-verify-accept")]
    pub mock_verify_accept: bool,
    /// Use a fixed seed for the target (smoke-test reproducibility).
    #[arg(long = "fixed-target-seed-hex")]
    pub fixed_target_seed_hex: Option<String>,
    /// Render text paired with `--fixed-target-seed-hex`.
    #[arg(long = "fixed-target-render")]
    pub fixed_target_render: Option<String>,
    /// Use deterministic `CounterNonce` instead of `OsRngNonce` for grinders
    /// (test-only knob — production runs with OS randomness).
    #[arg(long = "deterministic-nonces")]
    pub deterministic_nonces: bool,
    /// Per-grind attempt cap (applies to ticket / share / submit grinders).
    #[arg(long = "grind-max-attempts")]
    pub grind_max_attempts: Option<u64>,
    /// Lean checker project root (e.g. `lean/checker`). Required by the
    /// real Lean verifier when `--mock-verify-accept` is not passed.
    /// Falls back to the `BOOLE_LEAN_DIR` env var.
    #[arg(long = "lean-dir")]
    pub lean_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BountyNetworkPreset {
    Testnet,
    Dev,
    Mainnet,
}

impl BountyNetworkPreset {
    pub fn network_id(self) -> &'static str {
        match self {
            BountyNetworkPreset::Testnet => "boole-testnet",
            BountyNetworkPreset::Dev => "boole-dev",
            BountyNetworkPreset::Mainnet => "boole-mainnet",
        }
    }
}

#[derive(Debug, Args)]
pub struct BountyArgs {
    /// Dispatcher base URL.
    #[arg(long)]
    pub node: String,
    /// P2.10 — network preset that scopes the produced
    /// `boole.signed.v1` envelope. Required: there is no safe default
    /// across testnet/dev/mainnet, and a misrouted signature can be
    /// replayed across networks if the binding is missing.
    #[arg(long, value_enum)]
    pub network: BountyNetworkPreset,
    /// Bounty id.
    #[arg(long)]
    pub id: String,
    /// Prover ed25519 public key (32-byte lowercase hex). Must match
    /// the pk derived from `--prover-sk-hex`.
    #[arg(long)]
    pub prover: String,
    /// Prover ed25519 signing-key seed (32-byte lowercase hex). The
    /// node requires `boole.signed.v1` envelopes on POST
    /// `/bounties/{id}/proof`, so this is required for any real submit.
    #[arg(long = "prover-sk-hex")]
    pub prover_sk_hex: String,
    /// Path to a file holding the canonical envelope bytes (default: empty).
    #[arg(long = "envelope-path")]
    pub envelope_path: Option<PathBuf>,
    /// HTTP timeout in milliseconds.
    #[arg(long = "timeout-ms", default_value_t = 30_000)]
    pub timeout_ms: u64,
}

pub const SECRET_KEYS: &[&str] = &["llm.apiKey", "llm.api_key"];
const REDACTED: &str = "***";

fn iso_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let days = secs.div_euclid(86_400);
    let seconds_of_day = secs.rem_euclid(86_400);
    let h = (seconds_of_day / 3600) as u32;
    let m = ((seconds_of_day % 3600) / 60) as u32;
    let s = (seconds_of_day % 60) as u32;

    let z = days + 719_468;
    let era = if z >= 0 {
        z / 146_097
    } else {
        (z - 146_096) / 146_097
    };
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    if mo <= 2 {
        y += 1;
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y, mo, d, h, m, s, millis
    )
}

fn resolve_state_path(state_args: &StateArgs) -> Result<PathBuf, anyhow::Error> {
    if let Some(p) = state_args.state.clone() {
        return Ok(p);
    }
    Ok(default_state_path()?)
}

fn ensure_backend(s: &str) -> anyhow::Result<LLMBackend> {
    LLMBackend::parse(s).ok_or_else(|| anyhow::anyhow!("unknown llm backend: {s}"))
}

/// P2.4 — env var that an operator must set to opt in to spending real
/// money on a paid LLM backend. The name is deliberately verbose so a
/// stray export in a parent shell does not silently re-enable billing.
pub const PAID_LLM_ALLOW_ENV: &str = "BOOLE_ALLOW_PAID_LLM";

/// P1.10d — the only ingress for the LLM API key. The legacy
/// `--llm-api-key` argv flag was removed so the secret cannot leak via
/// `/proc/<pid>/cmdline`; callers must set this env var instead.
pub const LLM_API_KEY_ENV: &str = "BOOLE_LLM_API_KEY";

fn read_llm_api_key_from_env() -> Option<String> {
    std::env::var(LLM_API_KEY_ENV).ok()
}

/// P2.4 — backends that bill against an upstream paid API. ClaudeCli
/// and AgentCli shell out to a locally configured CLI whose billing
/// terms the operator already accepted by installing it. OpenAiCompat
/// is the local-proxy slot (Ollama, vLLM, LM Studio); operators that
/// point it at a paid hosted endpoint are responsible for that
/// disclosure themselves. The three named here are the unambiguous
/// hosted paid APIs.
pub fn is_paid_llm_backend(backend: LLMBackend) -> bool {
    matches!(
        backend,
        LLMBackend::Anthropic | LLMBackend::OpenAi | LLMBackend::Google
    )
}

/// P2.4 — pure paid-LLM gate. Splits the env read out of the policy so
/// tests can cover every (backend, env) combination without racing on
/// the global process environment. The wrapper
/// `enforce_paid_llm_gate_from_env` is the thin call site used by
/// `run_init` and `run_start`.
pub fn enforce_paid_llm_gate(
    backend: LLMBackend,
    allow_env_value: Option<&str>,
) -> anyhow::Result<()> {
    if !is_paid_llm_backend(backend) {
        return Ok(());
    }
    let allowed = matches!(
        allow_env_value.map(str::trim),
        Some("1") | Some("true") | Some("TRUE") | Some("True")
    );
    if allowed {
        return Ok(());
    }
    anyhow::bail!(
        "refusing to run paid LLM backend `{}` without explicit opt-in. \
         Set {}=1 to acknowledge that this backend bills against a real \
         hosted API key.",
        backend.as_str(),
        PAID_LLM_ALLOW_ENV
    )
}

fn enforce_paid_llm_gate_from_env(backend: LLMBackend) -> anyhow::Result<()> {
    let raw = std::env::var(PAID_LLM_ALLOW_ENV).ok();
    enforce_paid_llm_gate(backend, raw.as_deref())
}

/// P2.4 (slice 43) — exit code returned to the shell when the miner
/// refuses to launch a paid backend without an opt-in. Documented as
/// `3 (policy-refused)` in §6.5 P2.4 so operators and CI systems can
/// programmatically distinguish "we declined to spend money" from a
/// generic configuration error (`1`) or a panic (`>=101`).
pub const EXIT_CODE_POLICY_REFUSED: i32 = 3;

/// P2.4 (slice 43) — outcome of [`evaluate_paid_api_policy`].
///
/// Non-error variants tell the caller exactly which branch fired so it
/// can record a metric (`AllowedByEnv` increments the opt-in counter)
/// or run an interactive prompt (`RequiresInteractiveConfirm` — slice
/// 44 wires the prompt). `NotPaid` is the fast path for local /
/// agent-driven backends that do not bill against a hosted API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaidApiPolicyOutcome {
    /// Backend does not bill against a hosted paid API — no gate fires.
    NotPaid,
    /// Operator exported the opt-in env var with a truthy value.
    AllowedByEnv,
    /// Paid backend, no opt-in, but stdin is a TTY — the caller must
    /// prompt the operator (`y/N`). Slice 43 returns this verbatim so
    /// the caller decides; slice 44 wires the prompt into `run_start`.
    RequiresInteractiveConfirm,
}

/// P2.4 (slice 43/44) — typed refusal carrying everything the binary
/// needs to terminate cleanly: the documented exit code and the unified
/// CLI JSON envelope (`ok=false`, `version=v1`, `command=<caller>`,
/// `error.reason=paid-api-not-opted-in`). The binary catches this via
/// `err.downcast_ref::<PaidApiPolicyError>()`, prints the envelope to
/// stderr, and calls `std::process::exit(refusal.exit_code)`.
#[derive(Debug, Clone, thiserror::Error)]
#[error("paid API gate refused (exit_code={exit_code})")]
pub struct PaidApiPolicyError {
    pub exit_code: i32,
    pub envelope: serde_json::Value,
}

/// P2.4 (slice 43) — pure policy decision for the paid-API gate.
///
/// Inputs are explicit (no env read, no TTY syscall) so the function is
/// trivially testable across every (backend, opt-in env value, TTY)
/// combination without racing on the global process environment.
///
/// Semantics:
///   * Non-paid backend → `Ok(NotPaid)` — the gate does not fire.
///   * Truthy opt-in env (matches `enforce_paid_llm_gate` semantics,
///     i.e. trimmed value in `{"1","true","TRUE","True"}`) →
///     `Ok(AllowedByEnv)`.
///   * Paid backend, no opt-in, TTY → `Ok(RequiresInteractiveConfirm)`.
///   * Paid backend, no opt-in, no TTY → `Err(PaidApiPolicyError)` with
///     `exit_code = EXIT_CODE_POLICY_REFUSED` and the documented
///     envelope shape.
pub fn evaluate_paid_api_policy(
    backend: LLMBackend,
    allow_env_value: Option<&str>,
    is_tty: bool,
    command: &str,
) -> Result<PaidApiPolicyOutcome, PaidApiPolicyError> {
    if !is_paid_llm_backend(backend) {
        return Ok(PaidApiPolicyOutcome::NotPaid);
    }
    let allowed = matches!(
        allow_env_value.map(str::trim),
        Some("1") | Some("true") | Some("TRUE") | Some("True")
    );
    if allowed {
        return Ok(PaidApiPolicyOutcome::AllowedByEnv);
    }
    if is_tty {
        return Ok(PaidApiPolicyOutcome::RequiresInteractiveConfirm);
    }
    Err(PaidApiPolicyError {
        exit_code: EXIT_CODE_POLICY_REFUSED,
        envelope: serde_json::json!({
            "ok": false,
            "version": "v1",
            "command": command,
            "error": {
                "reason": "paid-api-not-opted-in",
                "backend": backend.as_str(),
                "allowEnv": PAID_LLM_ALLOW_ENV,
            },
        }),
    })
}

fn parse_agent_args(s: &str) -> anyhow::Result<Vec<String>> {
    let parsed: serde_json::Value = serde_json::from_str(s)
        .map_err(|_| anyhow::anyhow!("--agent-args must be a JSON string array"))?;
    let arr = parsed
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("--agent-args must be a JSON string array"))?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        let s = v
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("--agent-args items must be strings"))?;
        out.push(s.to_string());
    }
    Ok(out)
}

pub fn run_init(args: InitArgs) -> anyhow::Result<()> {
    let path = resolve_state_path(&args.state_args)?;
    if state_exists(&path) && !args.force {
        anyhow::bail!(
            "state already exists at {}; pass --force to overwrite",
            path.display()
        );
    }
    let backend = ensure_backend(&args.llm_backend)?;
    // P2.4 — gate before any state is materialized so a paid backend
    // cannot be persisted onto disk and then trivially started later.
    enforce_paid_llm_gate_from_env(backend)?;
    if backend == LLMBackend::AgentCli && args.agent_command.is_none() {
        anyhow::bail!("--agent-command required when --llm-backend=agent_cli");
    }
    if backend == LLMBackend::OpenAiCompat {
        if args.llm_base_url.is_none() {
            anyhow::bail!(
                "--llm-base-url required when --llm-backend=openai_compat \
                 (e.g. http://localhost:11434 for Ollama)"
            );
        }
        if args.llm_model.is_none() {
            anyhow::bail!(
                "--llm-model required when --llm-backend=openai_compat \
                 (e.g. \"gemma3:27b\")"
            );
        }
    }
    // P1.10d — the API key only enters through the env var; the legacy
    // argv flag was removed to keep the secret out of /proc/<pid>/cmdline.
    let resolved_api_key = read_llm_api_key_from_env();
    if matches!(
        backend,
        LLMBackend::Anthropic | LLMBackend::OpenAi | LLMBackend::Google
    ) {
        if resolved_api_key.is_none() {
            anyhow::bail!(
                "{} env var required when --llm-backend={}",
                LLM_API_KEY_ENV,
                backend.as_str()
            );
        }
        if args.llm_model.is_none() {
            anyhow::bail!(
                "--llm-model required when --llm-backend={} \
                 (e.g. \"claude-opus-4-7\" / \"gpt-5\" / \"gemini-2.5-pro\")",
                backend.as_str()
            );
        }
    }
    let llm = LlmConfig {
        backend: backend.as_str().to_string(),
        api_key: resolved_api_key,
        model: args.llm_model,
        base_url: args.llm_base_url,
        agent_command: args.agent_command,
        agent_args: args
            .agent_args
            .as_deref()
            .map(parse_agent_args)
            .transpose()?,
    };
    let cfg = MinerStateConfig {
        dispatcher: DispatcherConfig {
            url: args.dispatcher_url,
        },
        llm,
    };
    let now = iso_now();
    let state = generate_miner_state(cfg, &now);
    save_state(&state, &path)?;
    println!("address: {}", state.address);
    println!("state:   {}", path.display());
    Ok(())
}

pub fn run_address(args: StateArgs) -> anyhow::Result<()> {
    let path = resolve_state_path(&args)?;
    let state = load_state(&path)?;
    println!("{}", state.address);
    Ok(())
}

pub fn run_config_get(
    state_args: StateArgs,
    key: Option<String>,
    reveal: bool,
) -> anyhow::Result<()> {
    let path = resolve_state_path(&state_args)?;
    let state = load_state(&path)?;
    if let Some(k) = key {
        let v = lookup_dotted(&state.config, &k)?;
        let display = if reveal || !is_secret(&k) {
            v.unwrap_or_default()
        } else if v.is_some() {
            REDACTED.to_string()
        } else {
            String::new()
        };
        println!("{}", display);
        return Ok(());
    }
    let mut redacted = state.config.clone();
    if !reveal && redacted.llm.api_key.is_some() {
        redacted.llm.api_key = Some(REDACTED.to_string());
    }
    println!("{}", serde_json::to_string_pretty(&redacted)?);
    Ok(())
}

pub fn run_config_set(state_args: StateArgs, key: String, value: String) -> anyhow::Result<()> {
    let path = resolve_state_path(&state_args)?;
    let state = load_state(&path)?;
    let mut llm = state.config.llm.clone();
    let mut patch = ConfigPatch::default();
    let normalized = key
        .replace("api_key", "apiKey")
        .replace("base_url", "baseUrl")
        .replace("agent_command", "agentCommand")
        .replace("agent_args", "agentArgs");
    match normalized.as_str() {
        "dispatcher.url" => patch.dispatcher_url = Some(value.clone()),
        "llm.backend" => {
            ensure_backend(&value)?;
            llm.backend = value.clone();
            patch.llm = Some(llm);
        }
        "llm.apiKey" => {
            llm.api_key = Some(value.clone());
            patch.llm = Some(llm);
        }
        "llm.model" => {
            llm.model = Some(value.clone());
            patch.llm = Some(llm);
        }
        "llm.baseUrl" => {
            llm.base_url = Some(value.clone());
            patch.llm = Some(llm);
        }
        "llm.agentCommand" => {
            llm.agent_command = Some(value.clone());
            patch.llm = Some(llm);
        }
        "llm.agentArgs" => {
            llm.agent_args = Some(parse_agent_args(&value)?);
            patch.llm = Some(llm);
        }
        other => anyhow::bail!("unknown config key: {other}"),
    }
    update_config(patch, &path)?;
    let display = if is_secret(&key) {
        "<redacted>".to_string()
    } else {
        value
    };
    println!("set {}={}", key, display);
    Ok(())
}

fn is_secret(key: &str) -> bool {
    SECRET_KEYS.contains(&key)
}

fn lookup_dotted(cfg: &MinerStateConfig, key: &str) -> anyhow::Result<Option<String>> {
    Ok(match key {
        "dispatcher.url" => Some(cfg.dispatcher.url.clone()),
        "llm.backend" => Some(cfg.llm.backend.clone()),
        "llm.model" => cfg.llm.model.clone(),
        "llm.apiKey" | "llm.api_key" => cfg.llm.api_key.clone(),
        "llm.baseUrl" | "llm.base_url" => cfg.llm.base_url.clone(),
        "llm.agentCommand" | "llm.agent_command" => cfg.llm.agent_command.clone(),
        "llm.agentArgs" | "llm.agent_args" => cfg
            .llm
            .agent_args
            .as_ref()
            .map(|a| serde_json::to_string(a).unwrap_or_default()),
        other => anyhow::bail!("unknown config key: {other}"),
    })
}

pub fn run_bounty(args: BountyArgs) -> anyhow::Result<()> {
    // P2.5: `mine bounty` is the only always-JSON `mine` subcommand;
    // every exit path emits the unified CLI envelope ({"ok","version",
    // "command","result"|"error"}) with kebab-case `reason` tokens.
    // The envelope shape mirrors `boole_cli::cli_envelope::encode_ok`
    // / `encode_err`; we inline the construction here because boole-cli
    // depends on boole-miner (so the helper crate is not in scope) and
    // the COMMAND_INVENTORY drift test already enforces inventory-side
    // consistency for `mine.bounty`.
    if !is_well_formed_hex32(&args.prover) {
        bounty_emit_err(
            "bad-prover",
            serde_json::json!({ "detail": "expected 32-byte lowercase hex" }),
        );
    }
    let envelope_bytes: Vec<u8> = match args.envelope_path.as_deref() {
        Some(p) => match std::fs::read(p) {
            Ok(bytes) => bytes,
            Err(err) => bounty_emit_err(
                "envelope-unreadable",
                serde_json::json!({
                    "path": p.to_string_lossy(),
                    "detail": err.to_string(),
                }),
            ),
        },
        None => Vec::new(),
    };
    let signing_key = match boole_core::SigningKeyV2::from_seed_hex(&args.prover_sk_hex) {
        Ok(k) => k,
        Err(err) => bounty_emit_err("bad-prover-sk-hex", serde_json::json!({ "detail": err })),
    };
    if signing_key.pk_hex() != args.prover {
        bounty_emit_err(
            "prover-sk-pk-mismatch",
            serde_json::json!({
                "detail": "--prover-sk-hex derives a pk that does not match --prover",
            }),
        );
    }
    let client =
        BountyClient::with_timeout(args.node.clone(), Duration::from_millis(args.timeout_ms));
    let result = client.submit_proof(BountyProofInputs {
        bounty_id: &args.id,
        signing_key: &signing_key,
        envelope: serde_json::json!({"bytes": hex::encode(&envelope_bytes)}),
        envelope_bytes: &envelope_bytes,
        network_id: args.network.network_id(),
    });
    match result {
        BountyProofResult::Ok {
            accepted,
            duplicate,
            bounty,
        } => {
            let envelope = serde_json::json!({
                "ok": true,
                "version": "v1",
                "command": "mine.bounty",
                "result": {
                    "accepted": accepted,
                    "duplicate": duplicate,
                    "bounty": bounty,
                },
            });
            println!("{envelope}");
            Ok(())
        }
        BountyProofResult::NotFound { id } => {
            bounty_emit_err("not-found", serde_json::json!({ "id": id }))
        }
        BountyProofResult::Terminal { status } => {
            bounty_emit_err("terminal", serde_json::json!({ "status": status }))
        }
        BountyProofResult::NoVerifier { verifier_kind } => bounty_emit_err(
            "no-verifier",
            serde_json::json!({ "verifierKind": verifier_kind }),
        ),
        BountyProofResult::BadRequest { error, detail } => bounty_emit_err(
            "bad-request",
            serde_json::json!({ "error": error, "detail": detail }),
        ),
        BountyProofResult::NetworkError { cause } => {
            bounty_emit_err("network-error", serde_json::json!({ "error": cause }))
        }
    }
}

/// Emit a `mine.bounty` unified-envelope error to stderr and exit 1.
/// Inlined here for the same reason as the envelope construction in
/// [`run_bounty`]: boole-miner cannot depend on boole-cli.
fn bounty_emit_err(reason: &str, extras: serde_json::Value) -> ! {
    let mut error = serde_json::Map::new();
    error.insert(
        "reason".to_string(),
        serde_json::Value::String(reason.to_string()),
    );
    if let serde_json::Value::Object(map) = extras {
        for (k, v) in map {
            if k == "reason" {
                continue;
            }
            error.insert(k, v);
        }
    }
    let envelope = serde_json::json!({
        "ok": false,
        "version": "v1",
        "command": "mine.bounty",
        "error": serde_json::Value::Object(error),
    });
    eprintln!("{envelope}");
    std::process::exit(1);
}

fn is_well_formed_hex32(s: &str) -> bool {
    Hex32::from_hex(s).is_ok()
}

#[derive(Debug, Clone, Copy)]
enum FamilyTargetProfile {
    V1Lenbound,
}

fn parse_family_target_profile(profile: &str) -> Option<FamilyTargetProfile> {
    match profile {
        "v1-lenbound" => Some(FamilyTargetProfile::V1Lenbound),
        _ => None,
    }
}

/// P1.9 — split-bodies verifier builder so the no-feature build does
/// not even reference `AcceptingVerifier`. With `dev-tools`,
/// `mock_verify_accept` may select the bypass; without it the only
/// way to verify is the real Lean verifier and the parameter is
/// always `false`.
#[cfg(feature = "dev-tools")]
fn build_proof_verifier(
    mock_verify_accept: bool,
    lean_dir: Option<PathBuf>,
    profile: String,
) -> anyhow::Result<Box<dyn Verifier>> {
    if mock_verify_accept {
        return Ok(Box::new(AcceptingVerifier));
    }
    build_real_lean_verifier(lean_dir, profile)
}

#[cfg(not(feature = "dev-tools"))]
fn build_proof_verifier(
    _mock_verify_accept: bool,
    lean_dir: Option<PathBuf>,
    profile: String,
) -> anyhow::Result<Box<dyn Verifier>> {
    build_real_lean_verifier(lean_dir, profile)
}

fn build_real_lean_verifier(
    lean_dir: Option<PathBuf>,
    profile: String,
) -> anyhow::Result<Box<dyn Verifier>> {
    let Some(dir) = lean_dir.or_else(|| std::env::var_os("BOOLE_LEAN_DIR").map(PathBuf::from))
    else {
        anyhow::bail!(
            "real Lean verification requires --lean-dir <PATH> \
             (or BOOLE_LEAN_DIR env var); enable the boole-miner \
             `dev-tools` feature and pass --mock-verify-accept to \
             bypass for smoke tests"
        );
    };
    Ok(Box::new(crate::local_verify::LeanVerifier::new(
        dir, profile,
    )))
}

fn family_target_emitter(
    profile: FamilyTargetProfile,
    pinned_seed: Option<String>,
) -> Box<dyn TargetEmitter> {
    match profile {
        FamilyTargetProfile::V1Lenbound => {
            let emitter = FamilyV1LengthBoundTargetEmitter::new();
            match pinned_seed {
                Some(seed) => Box::new(emitter.with_pinned_seed(seed)),
                None => Box::new(emitter),
            }
        }
    }
}

pub fn run_start(args: StartArgs) -> anyhow::Result<MiningLoopSummary> {
    let allow_env = std::env::var(PAID_LLM_ALLOW_ENV).ok();
    let is_tty = std::io::IsTerminal::is_terminal(&std::io::stdin());
    let prompt: PaidApiConfirmPrompt = Box::new(real_paid_api_confirm_prompt);
    let hooks = PaidPolicyHooks {
        prompt: Some(prompt),
        optin_counter: Some(global_paid_api_optin_counter()),
    };
    run_start_with_paid_policy_hooks(args, allow_env.as_deref(), is_tty, hooks)
}

/// P2.4 (slice 46) — typed counter handle for the
/// `boole_paid_api_optin_total` metric. `Arc<AtomicU64>` so the policy
/// code, the binary's `run_start`, and an eventual metrics endpoint can
/// all share the same monotonic stream, and so tests can inject their
/// own counter via [`PaidPolicyHooks::optin_counter`] to avoid races on
/// the process-global counter under cargo's parallel runner.
pub type PaidApiOptInCounter = Arc<AtomicU64>;

/// Process-global `boole_paid_api_optin_total` counter. Lazily
/// initialised so the static is zero-cost when no paid-LLM run ever
/// fires. The binary's `run_start` passes a clone into the hook bundle
/// so every `AllowedByEnv` decision is reflected here.
fn global_paid_api_optin_counter() -> PaidApiOptInCounter {
    static GLOBAL: std::sync::OnceLock<PaidApiOptInCounter> = std::sync::OnceLock::new();
    Arc::clone(GLOBAL.get_or_init(|| Arc::new(AtomicU64::new(0))))
}

/// P2.4 (slice 46) — read the process-global opt-in counter. Returns
/// the cumulative number of `AllowedByEnv` decisions taken since the
/// binary started. Wired by `run_start`'s default hooks; tests that use
/// the hook-aware seam can read their injected counter directly.
pub fn paid_api_optin_total() -> u64 {
    global_paid_api_optin_counter().load(Ordering::SeqCst)
}

/// P2.4 (slice 46) — bundle of test-friendly hooks for
/// [`run_start_with_paid_policy_hooks`]. Default = no prompt + no
/// counter, which preserves slice-44 semantics (TTY-no-opt-in refuses
/// without consulting any callback, AllowedByEnv proceeds silently).
#[derive(Default)]
pub struct PaidPolicyHooks {
    /// Interactive y/N confirm callback fired iff
    /// `evaluate_paid_api_policy` returns `RequiresInteractiveConfirm`.
    pub prompt: Option<PaidApiConfirmPrompt>,
    /// `boole_paid_api_optin_total` counter. Incremented by exactly
    /// 1 on every `AllowedByEnv` decision; untouched on every other
    /// outcome (NotPaid, RequiresInteractiveConfirm with Proceed,
    /// refusal). Tests inject their own counter so they do not race
    /// on the process-global counter.
    pub optin_counter: Option<PaidApiOptInCounter>,
}

/// P2.4 (slice 45) — operator's response to the paid-API interactive
/// confirm prompt. The prompt callback returns this so the policy code
/// path stays agnostic of stdin shape (real terminal, test closure,
/// canned automation feed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaidApiConfirmDecision {
    /// Operator typed `y`/`yes` (case-insensitive). Gate lets the run
    /// proceed exactly as if `BOOLE_ALLOW_PAID_LLM=1` were set.
    Proceed,
    /// Anything else (empty line, `n`, `no`, junk, EOF on a closed pipe
    /// even though `is_tty=true` claimed otherwise). Gate refuses.
    Refuse,
}

/// P2.4 (slice 45) — callback signature for the interactive confirm.
/// Boxed `FnOnce` so callers can wire a real stdin reader or a test
/// closure; an `Err(io::Error)` from the closure is treated as Refuse
/// so a closed/broken stdin cannot accidentally consent on the
/// operator's behalf.
pub type PaidApiConfirmPrompt = Box<dyn FnOnce() -> std::io::Result<PaidApiConfirmDecision>>;

/// Default interactive prompt used by the real binary. Writes the
/// question to stderr (so JSON-on-stdout subcommands are not polluted)
/// and reads exactly one line from stdin; anything other than `y`/`yes`
/// (case-insensitive, trimmed) is treated as Refuse.
fn real_paid_api_confirm_prompt() -> std::io::Result<PaidApiConfirmDecision> {
    use std::io::{BufRead, Write};
    let stderr = std::io::stderr();
    let mut h = stderr.lock();
    write!(
        h,
        "boole-miner: about to launch a paid LLM backend. \
         This will bill against your hosted API key. Type `y` to proceed: "
    )?;
    h.flush()?;
    drop(h);

    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    let trimmed = line.trim().to_ascii_lowercase();
    if trimmed == "y" || trimmed == "yes" {
        Ok(PaidApiConfirmDecision::Proceed)
    } else {
        Ok(PaidApiConfirmDecision::Refuse)
    }
}

/// P2.4 (slice 44) — testable seam over `run_start` that takes the
/// paid-API policy inputs (env value + TTY presence) explicitly. Kept
/// as a thin shim over `run_start_with_paid_policy_hooks` with default
/// (empty) hooks so the slice-44 test matrix stays meaningful.
pub fn run_start_with_paid_policy_inputs(
    args: StartArgs,
    allow_env_value: Option<&str>,
    is_tty: bool,
) -> anyhow::Result<MiningLoopSummary> {
    run_start_with_paid_policy_hooks(args, allow_env_value, is_tty, PaidPolicyHooks::default())
}

/// P2.4 (slice 45) — prompt-aware seam preserved as a thin shim over
/// the slice-46 hook bundle so existing slice-45 tests stay green.
pub fn run_start_with_paid_policy_inputs_and_prompt(
    args: StartArgs,
    allow_env_value: Option<&str>,
    is_tty: bool,
    prompt: Option<PaidApiConfirmPrompt>,
) -> anyhow::Result<MiningLoopSummary> {
    run_start_with_paid_policy_hooks(
        args,
        allow_env_value,
        is_tty,
        PaidPolicyHooks {
            prompt,
            optin_counter: None,
        },
    )
}

/// P2.4 (slice 46) — full-fat seam: same as
/// `run_start_with_paid_policy_inputs_and_prompt` plus an explicit
/// `optin_counter` that is incremented by exactly 1 when (and only
/// when) the policy returns `AllowedByEnv`. Every other outcome
/// (`NotPaid`, `RequiresInteractiveConfirm` with either Proceed or
/// Refuse, refusal) leaves the counter alone. The TTY-prompt Proceed
/// path is deliberately NOT counted because the master plan scopes
/// `boole_paid_api_optin_total` to the env opt-in path.
pub fn run_start_with_paid_policy_hooks(
    args: StartArgs,
    allow_env_value: Option<&str>,
    is_tty: bool,
    hooks: PaidPolicyHooks,
) -> anyhow::Result<MiningLoopSummary> {
    let path = resolve_state_path(&args.state_args)?;
    let state = load_state(&path)?;
    // P2.4 — re-gate at start time. A state file may have been produced
    // before this binary version learned about the gate, or `init` may
    // have been run under an environment that carried the opt-in but
    // the operator now wants to start the loop without it. Either way,
    // the loop itself must not begin calling a paid backend unless the
    // current process environment still asserts the opt-in.
    let backend = ensure_backend(&state.config.llm.backend)?;
    let typed_refusal = || PaidApiPolicyError {
        exit_code: EXIT_CODE_POLICY_REFUSED,
        envelope: serde_json::json!({
            "ok": false,
            "version": "v1",
            "command": "mine.start",
            "error": {
                "reason": "paid-api-not-opted-in",
                "backend": backend.as_str(),
                "allowEnv": PAID_LLM_ALLOW_ENV,
            },
        }),
    };
    match evaluate_paid_api_policy(backend, allow_env_value, is_tty, "mine.start") {
        Ok(PaidApiPolicyOutcome::NotPaid) => {}
        Ok(PaidApiPolicyOutcome::AllowedByEnv) => {
            if let Some(counter) = hooks.optin_counter.as_ref() {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }
        Ok(PaidApiPolicyOutcome::RequiresInteractiveConfirm) => match hooks.prompt {
            Some(cb) => match cb() {
                Ok(PaidApiConfirmDecision::Proceed) => {}
                Ok(PaidApiConfirmDecision::Refuse) | Err(_) => {
                    return Err(anyhow::Error::new(typed_refusal()));
                }
            },
            None => return Err(anyhow::Error::new(typed_refusal())),
        },
        Err(refusal) => return Err(anyhow::Error::new(refusal)),
    }
    let signing = signing_key_from_state(&state)?;
    let pk_bytes = signing.verifying_key().to_bytes();
    let pk = Hex32::from_bytes(pk_bytes);

    // Validation lives in the emitter match below — `--fixed-target-render`
    // alone is invalid; `--fixed-target-seed-hex` alone is valid only for
    // a family profile.

    let chain_head = HttpChainHeadFetcher::with_timeout(
        state.config.dispatcher.url.clone(),
        Duration::from_millis(args.head_timeout_ms),
        args.difficulty,
        args.profile.clone(),
        args.n,
    );

    let driver_mode = if args.mock_llm_response.is_some()
        || matches!(ensure_backend(&state.config.llm.backend)?, LLMBackend::Mock)
    {
        MiningRunDriverMode::MockLlmResponse
    } else {
        MiningRunDriverMode::RealLlmOrAgent
    };

    let driver: Box<dyn ProverDriver> = if let Some(canned) = args.mock_llm_response.clone() {
        Box::new(MockDriver::new(vec![MockResponse::Text(canned)]))
    } else {
        let backend = ensure_backend(&state.config.llm.backend)?;
        match backend {
            LLMBackend::Mock => Box::new(MockDriver::new(Vec::new())),
            LLMBackend::ClaudeCli => Box::new(
                ClaudeCliDriver::new("claude", Duration::from_secs(120))
                    .with_model(state.config.llm.model.clone()),
            ),
            LLMBackend::AgentCli => {
                let cmd =
                    state.config.llm.agent_command.clone().ok_or_else(|| {
                        anyhow::anyhow!("agent_command not set in state.config.llm")
                    })?;
                let agent_args = state.config.llm.agent_args.clone().unwrap_or_default();
                Box::new(AgentCliDriver::new(
                    cmd,
                    agent_args,
                    Duration::from_secs(300),
                ))
            }
            backend @ (LLMBackend::OpenAiCompat
            | LLMBackend::Anthropic
            | LLMBackend::OpenAi
            | LLMBackend::Google) => create_driver(&LLMDriverConfig {
                backend,
                timeout: Duration::from_secs(300),
                claude_binary: None,
                agent_command: None,
                agent_args: Vec::new(),
                api_key: state.config.llm.api_key.clone(),
                model: state.config.llm.model.clone(),
                base_url: state.config.llm.base_url.clone(),
                max_tokens: None,
            })?,
        }
    };

    let family_profile = parse_family_target_profile(&args.profile);

    let target_mode = match (
        family_profile,
        args.fixed_target_seed_hex.as_ref(),
        args.fixed_target_render.as_ref(),
    ) {
        (_, Some(_), Some(_)) => MiningRunTargetMode::FixedSeed,
        (Some(_), Some(_), None) => MiningRunTargetMode::FixedSeed,
        (Some(_), None, None) => MiningRunTargetMode::ChainDerived,
        (None, None, None) => MiningRunTargetMode::Stub,
        _ => MiningRunTargetMode::Stub,
    };

    let emitter: Box<dyn TargetEmitter> = match (
        family_profile,
        args.fixed_target_seed_hex.clone(),
        args.fixed_target_render.clone(),
    ) {
        (_, None, Some(_)) => {
            anyhow::bail!("--fixed-target-render requires --fixed-target-seed-hex")
        }
        // Legacy v01-style: caller hand-writes both seed and render.
        (_, Some(seed), Some(render)) => Box::new(FixedSeedTargetEmitter {
            seed_hex: seed,
            render,
            d: args.difficulty,
            profile: args.profile.clone(),
            n: args.n,
        }),
        // Family-derived render with a pinned seed (smoke determinism).
        (Some(profile), Some(seed), None) => family_target_emitter(profile, Some(seed)),
        // Production: family-derived seed + render.
        (Some(profile), None, None) => family_target_emitter(profile, None),
        (None, Some(_), None) => anyhow::bail!(
            "--fixed-target-seed-hex and --fixed-target-render must be provided together \
             unless the active family profile is selected (v1-lenbound)"
        ),
        (None, None, None) => Box::new(StubTargetEmitter::new(
            "synthetic target — supply --fixed-target-render or use a family profile",
        )),
    };

    // P1.9 — feature-gated bypass selection. With `dev-tools` the
    // `--mock-verify-accept` flag exists and may short-circuit Lean;
    // without it the flag does not compile and only the real Lean
    // verifier is reachable.
    #[cfg(feature = "dev-tools")]
    let mock_verify_accept = args.mock_verify_accept;
    #[cfg(not(feature = "dev-tools"))]
    let mock_verify_accept = false;

    let verifier_mode = if mock_verify_accept {
        MiningRunVerifierMode::MockAccept
    } else {
        MiningRunVerifierMode::RealVerifier
    };

    let verifier: Box<dyn Verifier> = build_proof_verifier(
        mock_verify_accept,
        args.lean_dir.clone(),
        args.profile.clone(),
    )?;

    let canonicalizer = Box::new(StructuralCanonicalizer);
    let submit_client: Box<dyn Submitter> =
        Box::new(SubmitClient::new(state.config.dispatcher.url.clone()));

    let mut grind_cfg = crate::grinder::GrinderConfig::default();
    if let Some(max) = args.grind_max_attempts {
        grind_cfg.max_attempts = Some(max);
    }

    let opts = MiningLoopOptions {
        max_shares: args.max_shares,
        max_cycles: args.max_cycles,
        ticket_grind: grind_cfg,
        share_grind: grind_cfg,
        submit_grind: grind_cfg,
        llm_retry: Default::default(),
        run_context: MiningRunContext {
            verifier_mode,
            driver_mode,
            target_mode,
        },
        cancel: None,
        deterministic_nonces: args.deterministic_nonces,
    };

    let deps = MiningLoopDeps {
        pk,
        chain_head: Box::new(chain_head),
        emitter,
        driver,
        verifier,
        canonicalizer,
        submit_client,
        prompt_builder: None,
        log: Some(Box::new(|e: &MiningEvent| {
            println!("{}", event_for_log(e));
        })),
        sleeper: None,
    };

    let summary = run_mining_loop(deps, opts);
    println!(
        "\nsummary: {}",
        serde_json::to_string_pretty(&summary_for_log(&summary))?
    );
    Ok(summary)
}

fn summary_for_log(s: &MiningLoopSummary) -> serde_json::Value {
    serde_json::json!({
        "agent": {
            "driverCalls": s.agent.driver_calls,
            "driverAnswered": s.agent.driver_answered,
            "driverRejected": s.agent.driver_rejected,
            "driverErrored": s.agent.driver_errored,
            "proofIntakeAccepted": s.agent.proof_intake_accepted,
            "proofIntakeRejected": s.agent.proof_intake_rejected,
        },
        "protocol": {
            "cyclesRun": s.protocol.cycles_run,
            "ticketsFound": s.protocol.tickets_found,
            "verifyAccepted": s.protocol.verify_accepted,
            "verifyRejected": s.protocol.verify_rejected,
            "sharesAccepted": s.protocol.shares_accepted,
            "sharesRejected": s.protocol.shares_rejected,
            "rateLimited": s.protocol.rate_limited,
            "networkErrors": s.protocol.network_errors,
            "announceRejected": s.protocol.announce_rejected,
            "proposerShares": s.protocol.proposer_shares,
            "loopClass": s.protocol.loop_class,
            "publicScoringEligible": s.protocol.public_scoring_eligible,
            "ineligibilityReasons": s.protocol.ineligibility_reasons,
        },
        // Flat mirror for stdout-line scrapers. The nested `agent`/`protocol`
        // objects remain canonical for new code.
        "cyclesRun": s.protocol.cycles_run,
        "ticketsFound": s.protocol.tickets_found,
        "verifyAccepted": s.protocol.verify_accepted,
        "verifyRejected": s.protocol.verify_rejected,
        "sharesAccepted": s.protocol.shares_accepted,
        "sharesRejected": s.protocol.shares_rejected,
        "rateLimited": s.protocol.rate_limited,
        "networkErrors": s.protocol.network_errors,
        "announceRejected": s.protocol.announce_rejected,
        "proposerShares": s.protocol.proposer_shares,
        "loopClass": s.protocol.loop_class,
        "publicScoringEligible": s.protocol.public_scoring_eligible,
        "ineligibilityReasons": s.protocol.ineligibility_reasons,
        "driverCalls": s.agent.driver_calls,
        "driverAnswered": s.agent.driver_answered,
        "driverRejected": s.agent.driver_rejected,
        "driverErrored": s.agent.driver_errored,
        "proofIntakeAccepted": s.agent.proof_intake_accepted,
        "proofIntakeRejected": s.agent.proof_intake_rejected,
    })
}

fn event_for_log(e: &MiningEvent) -> String {
    serde_json::to_string(&event_to_json(e)).unwrap_or_default()
}

fn event_to_json(e: &MiningEvent) -> serde_json::Value {
    match e {
        MiningEvent::HeadFetched { c_hex, m } => {
            serde_json::json!({"kind":"head_fetched","c":c_hex,"M":m})
        }
        MiningEvent::LoopClassified {
            loop_class,
            public_scoring_eligible,
            ineligibility_reasons,
        } => serde_json::json!({
            "kind":"loop_classified",
            "loopClass":loop_class,
            "publicScoringEligible":public_scoring_eligible,
            "ineligibilityReasons":ineligibility_reasons,
        }),
        MiningEvent::TicketFound {
            n_hex,
            hashes_attempted,
            elapsed_ms,
        } => serde_json::json!({
            "kind":"ticket_found","n":n_hex,
            "hashesAttempted":hashes_attempted,"elapsedMs":elapsed_ms,
        }),
        MiningEvent::TicketAnnounced { result } => {
            serde_json::json!({"kind":"ticket_announced","result":format!("{:?}",result)})
        }
        MiningEvent::TicketExhausted { hashes_attempted } => {
            serde_json::json!({"kind":"ticket_exhausted","hashesAttempted":hashes_attempted})
        }
        MiningEvent::TargetEmitted { j_index, seed_hex } => {
            serde_json::json!({"kind":"target_emitted","j":j_index,"seed":seed_hex})
        }
        MiningEvent::LlmOutcome {
            j_index,
            outcome,
            elapsed_ms,
            reason,
            proof_contract_version,
            canonicalizer_version,
            model_specific_overrides,
        } => serde_json::json!({
            "kind":"llm_outcome","j":j_index,"outcome":outcome.as_str(),
            "elapsedMs":elapsed_ms,"reason":reason,
            "proofContractVersion":proof_contract_version,
            "canonicalizerVersion":canonicalizer_version,
            "modelSpecificOverrides":model_specific_overrides,
        }),
        MiningEvent::VerifyOutcome {
            j_index,
            accepted,
            reason,
            elapsed_ms,
            attempt_artifact_path,
        } => {
            let mut value = serde_json::json!({
                "kind":"verify_outcome","j":j_index,"accepted":accepted,
                "reason":reason,"elapsedMs":elapsed_ms,
            });
            if let Some(path) = attempt_artifact_path {
                value["attemptArtifactPath"] =
                    serde_json::Value::String(path.display().to_string());
            }
            value
        }
        MiningEvent::ShareFound {
            j_hex,
            is_proposer,
            hashes_attempted,
        } => serde_json::json!({
            "kind":"share_found","j":j_hex,"isProposer":is_proposer,
            "hashesAttempted":hashes_attempted,
        }),
        MiningEvent::ShareGrindExhausted {
            j_index,
            hashes_attempted,
        } => serde_json::json!({
            "kind":"share_grind_exhausted","j":j_index,
            "hashesAttempted":hashes_attempted,
        }),
        MiningEvent::SubmitPowFound {
            nonce_s_hex,
            hashes_attempted,
        } => serde_json::json!({
            "kind":"submit_pow_found","nonceS":nonce_s_hex,
            "hashesAttempted":hashes_attempted,
        }),
        MiningEvent::SubmitPowExhausted { hashes_attempted } => {
            serde_json::json!({"kind":"submit_pow_exhausted","hashesAttempted":hashes_attempted})
        }
        MiningEvent::SubmitOutcome { result } => {
            serde_json::json!({"kind":"submit_outcome","result":format!("{:?}",result)})
        }
        MiningEvent::HeadAdvancedMidCycle {
            old_c_hex,
            new_c_hex,
            reason,
        } => serde_json::json!({
            "kind":"head_advanced_mid_cycle",
            "oldC":old_c_hex,
            "newC":new_c_hex,
            "reason":reason.as_str(),
        }),
        MiningEvent::CycleComplete { cycle } => {
            serde_json::json!({"kind":"cycle_complete","cycle":cycle})
        }
        MiningEvent::HeadFetchFailed { error } => {
            serde_json::json!({"kind":"head_fetch_failed","error":error})
        }
    }
}

pub fn run_mine(cmd: MineCommand) -> anyhow::Result<()> {
    // P2.3 — drive the legacy-state migration check once per CLI
    // invocation, before any subcommand reads state. The notice goes to
    // stderr so JSON-on-stdout subcommands are not polluted; failure to
    // probe (e.g. HOME unset) is non-fatal — the resolver itself will
    // surface the same error if it actually needs the path.
    if let Ok(env) = crate::state::StateEnv::from_process() {
        match crate::state::try_migrate_legacy_state_with(&env) {
            Ok(Some(crate::state::LegacyMigration::Migrated { from, to })) => {
                eprintln!(
                    "legacy state path migrated from {} to {}",
                    from.display(),
                    to.display()
                );
            }
            Ok(Some(crate::state::LegacyMigration::BothPresent { legacy, modern })) => {
                eprintln!(
                    "warning: legacy state path {} present alongside modern {}; using modern (remove the legacy file to silence this warning)",
                    legacy.display(),
                    modern.display()
                );
            }
            Ok(None) => {}
            Err(e) => eprintln!("legacy state migration check failed: {e}"),
        }
    }

    match cmd {
        MineCommand::Init(args) => run_init(args),
        MineCommand::Address(args) => run_address(args),
        MineCommand::Config { command } => match command {
            ConfigCommand::Get {
                state_args,
                key,
                reveal,
            } => run_config_get(state_args, key, reveal),
            ConfigCommand::Set {
                state_args,
                key,
                value,
            } => run_config_set(state_args, key, value),
        },
        MineCommand::Start(args) => run_start(args).map(|_| ()),
        MineCommand::Bounty(args) => run_bounty(args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // P2.4 — paid-LLM gate. Splitting the pure function `enforce_paid_llm_gate`
    // from `enforce_paid_llm_gate_from_env` keeps the policy testable without
    // mutating the global process environment, which would race against
    // every other parallel test in the binary.

    #[test]
    fn paid_llm_gate_rejects_anthropic_without_opt_in() {
        let err = enforce_paid_llm_gate(LLMBackend::Anthropic, None).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("BOOLE_ALLOW_PAID_LLM"),
            "error message must name the opt-in env var, got: {msg}"
        );
        assert!(
            msg.contains("anthropic"),
            "error message must name the rejected backend, got: {msg}"
        );
    }

    #[test]
    fn paid_llm_gate_accepts_anthropic_with_explicit_opt_in() {
        enforce_paid_llm_gate(LLMBackend::Anthropic, Some("1")).expect("opt-in 1 allows");
        enforce_paid_llm_gate(LLMBackend::Anthropic, Some("true")).expect("opt-in true allows");
    }

    #[test]
    fn paid_llm_gate_rejects_anthropic_with_unrelated_env_value() {
        enforce_paid_llm_gate(LLMBackend::Anthropic, Some("0")).unwrap_err();
        enforce_paid_llm_gate(LLMBackend::Anthropic, Some("yes")).unwrap_err();
        enforce_paid_llm_gate(LLMBackend::Anthropic, Some("")).unwrap_err();
    }

    #[test]
    fn paid_llm_gate_lets_local_backends_through_unconditionally() {
        for backend in [
            LLMBackend::Mock,
            LLMBackend::ClaudeCli,
            LLMBackend::AgentCli,
            LLMBackend::OpenAiCompat,
        ] {
            enforce_paid_llm_gate(backend, None)
                .unwrap_or_else(|_| panic!("local backend {:?} must not require opt-in", backend));
        }
    }

    #[test]
    fn paid_llm_gate_classifies_paid_backends() {
        assert!(is_paid_llm_backend(LLMBackend::Anthropic));
        assert!(is_paid_llm_backend(LLMBackend::OpenAi));
        assert!(is_paid_llm_backend(LLMBackend::Google));
        assert!(!is_paid_llm_backend(LLMBackend::Mock));
        assert!(!is_paid_llm_backend(LLMBackend::ClaudeCli));
        assert!(!is_paid_llm_backend(LLMBackend::AgentCli));
        assert!(!is_paid_llm_backend(LLMBackend::OpenAiCompat));
    }

    #[test]
    fn summary_for_log_emits_nested_agent_and_protocol_reports() {
        let summary = MiningLoopSummary {
            agent: crate::mining_loop::AgentRuntimeReport {
                driver_calls: 3,
                driver_answered: 2,
                driver_rejected: 1,
                driver_errored: 0,
                proof_intake_accepted: 2,
                proof_intake_rejected: 0,
            },
            protocol: crate::mining_loop::ProtocolReport {
                cycles_run: 4,
                tickets_found: 5,
                verify_accepted: 6,
                verify_rejected: 7,
                shares_accepted: 8,
                shares_rejected: 9,
                rate_limited: 10,
                network_errors: 11,
                announce_rejected: 12,
                proposer_shares: 13,
                loop_class: "smoke".to_string(),
                public_scoring_eligible: false,
                ineligibility_reasons: vec!["open_thresholds".to_string()],
            },
        };

        let json = summary_for_log(&summary);

        assert_eq!(json["agent"]["driverCalls"], 3);
        assert_eq!(json["agent"]["driverAnswered"], 2);
        assert_eq!(json["agent"]["proofIntakeAccepted"], 2);
        assert_eq!(json["agent"]["proofIntakeRejected"], 0);
        assert!(json["agent"].get("llmSolved").is_none());
        assert_eq!(json["protocol"]["verifyAccepted"], 6);
        assert_eq!(json["protocol"]["sharesAccepted"], 8);
        assert_eq!(json["protocol"]["publicScoringEligible"], false);
        assert_eq!(
            json["protocol"]["ineligibilityReasons"][0],
            "open_thresholds"
        );
        assert_eq!(json["driverCalls"], 3);
        assert_eq!(json["driverAnswered"], 2);
        assert_eq!(json["proofIntakeAccepted"], 2);
        assert_eq!(json["proofIntakeRejected"], 0);
        assert!(json.get("llmSolved").is_none());
        assert_eq!(json["cyclesRun"], 4);
        assert_eq!(json["verifyAccepted"], 6);
        assert_eq!(json["sharesAccepted"], 8);
        assert_eq!(json["publicScoringEligible"], false);
        assert_eq!(json["ineligibilityReasons"][0], "open_thresholds");
    }

    #[test]
    fn mining_report_summary_matches_v1_artifact_contract_fixture() {
        let summary = MiningLoopSummary {
            agent: crate::mining_loop::AgentRuntimeReport {
                driver_calls: 4,
                driver_answered: 3,
                driver_rejected: 1,
                driver_errored: 0,
                proof_intake_accepted: 2,
                proof_intake_rejected: 1,
            },
            protocol: crate::mining_loop::ProtocolReport {
                cycles_run: 4,
                tickets_found: 4,
                verify_accepted: 1,
                verify_rejected: 1,
                shares_accepted: 1,
                shares_rejected: 0,
                rate_limited: 0,
                network_errors: 0,
                announce_rejected: 0,
                proposer_shares: 1,
                loop_class: "smoke".to_string(),
                public_scoring_eligible: false,
                ineligibility_reasons: vec!["controlled_local_smoke".to_string()],
            },
        };
        let actual = summary_for_log(&summary);
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/protocol/mining-report/v1-summary.json"
        ))
        .expect("mining report fixture is valid JSON");

        assert_eq!(actual, expected);
        assert!(actual.get("llmSolved").is_none());
        assert_ne!(actual["driverAnswered"], actual["verifyAccepted"]);
        assert_ne!(actual["proofIntakeAccepted"], actual["sharesAccepted"]);
        assert_eq!(actual["publicScoringEligible"], false);
    }

    #[test]
    fn verify_outcome_ndjson_emits_attempt_artifact_path_only() {
        let event = MiningEvent::VerifyOutcome {
            j_index: 3,
            accepted: false,
            reason: "elaborate_failed".to_string(),
            elapsed_ms: 12,
            attempt_artifact_path: Some(PathBuf::from("/tmp/boole-attempt-artifact")),
        };

        let json = event_to_json(&event);

        assert_eq!(json["kind"], "verify_outcome");
        assert_eq!(json["attemptArtifactPath"], "/tmp/boole-attempt-artifact");
        assert!(json.get("proofSource").is_none());
        assert!(json.get("generatedModule").is_none());
        assert!(json.get("leanStdout").is_none());
        assert!(json.get("leanStderr").is_none());
    }

    #[test]
    fn llm_outcome_ndjson_emits_proof_intake_policy_metadata() {
        let event = MiningEvent::LlmOutcome {
            j_index: 7,
            outcome: crate::mining_loop::LlmOutcomeKind::Answered,
            elapsed_ms: 42,
            reason: None,
            proof_contract_version: crate::proof_intake::PROOF_BODY_CONTRACT_VERSION,
            canonicalizer_version: crate::proof_intake::PROOF_CANONICALIZER_VERSION,
            model_specific_overrides: false,
        };

        let json = event_to_json(&event);

        assert_eq!(json["kind"], "llm_outcome");
        assert_eq!(json["outcome"], "answered");
        assert_ne!(json["outcome"], "solved");
        assert_eq!(json["proofContractVersion"], "boole-proof-body-v1");
        assert_eq!(json["canonicalizerVersion"], "boole-proof-canonicalizer-v1");
        assert_eq!(json["modelSpecificOverrides"], false);
        assert!(json.get("proofSource").is_none());
    }

    #[test]
    fn mining_report_llm_outcome_events_match_v1_artifact_contract_fixture() {
        let events = serde_json::Value::Array(vec![
            event_to_json(&MiningEvent::LlmOutcome {
                j_index: 0,
                outcome: crate::mining_loop::LlmOutcomeKind::Answered,
                elapsed_ms: 42,
                reason: None,
                proof_contract_version: crate::proof_intake::PROOF_BODY_CONTRACT_VERSION,
                canonicalizer_version: crate::proof_intake::PROOF_CANONICALIZER_VERSION,
                model_specific_overrides: false,
            }),
            event_to_json(&MiningEvent::LlmOutcome {
                j_index: 1,
                outcome: crate::mining_loop::LlmOutcomeKind::IntakeRejected,
                elapsed_ms: 7,
                reason: Some("expected theorem body only".to_string()),
                proof_contract_version: crate::proof_intake::PROOF_BODY_CONTRACT_VERSION,
                canonicalizer_version: crate::proof_intake::PROOF_CANONICALIZER_VERSION,
                model_specific_overrides: false,
            }),
        ]);
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/protocol/mining-report/v1-llm-outcomes.json"
        ))
        .expect("mining report event fixture is valid JSON");

        assert_eq!(events, expected);
        assert!(events.to_string().contains("answered"));
        assert!(events.to_string().contains("intake_rejected"));
        assert!(!events.to_string().contains("solved"));
    }
}
