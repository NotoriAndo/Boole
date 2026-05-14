// boole-miner CLI subtree.
//
// Exposed as a library entry point so both `boole-miner` (the standalone
// binary at `src/bin/boole-miner.rs`) and `boole-cli` (via `mine start /
// mine bounty`) can drive the same code paths without duplicating clap
// argument parsing.
use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Subcommand};

use crate::bounty_client::{BountyClient, BountyProofInputs, BountyProofResult};
use crate::canonicalizer::StructuralCanonicalizer;
use crate::chain_head::HttpChainHeadFetcher;
use crate::family_v031::Profile as FamilyProfile;
use crate::llm_driver::{
    create_driver, AgentCliDriver, ClaudeCliDriver, LLMBackend, LLMDriverConfig, MockDriver,
    MockResponse, ProverDriver,
};
use crate::local_verify::{AcceptingVerifier, Verifier};
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
    FamilyV031TargetEmitter, FamilyV1LengthBoundTargetEmitter, FixedSeedTargetEmitter,
    StubTargetEmitter, TargetEmitter,
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
    /// API key (required for anthropic / openai / google; optional for openai_compat).
    #[arg(long = "llm-api-key")]
    pub llm_api_key: Option<String>,
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
    /// Profile (v01 | v02 | v03 | v031 | v031-lp | v1-lenbound).
    #[arg(long, default_value = "v01")]
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

#[derive(Debug, Args)]
pub struct BountyArgs {
    /// Dispatcher base URL.
    #[arg(long)]
    pub node: String,
    /// Bounty id.
    #[arg(long)]
    pub id: String,
    /// Prover ed25519 public key (32-byte lowercase hex).
    #[arg(long)]
    pub prover: String,
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
    if matches!(
        backend,
        LLMBackend::Anthropic | LLMBackend::OpenAi | LLMBackend::Google
    ) {
        if args.llm_api_key.is_none() {
            anyhow::bail!(
                "--llm-api-key required when --llm-backend={}",
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
        api_key: args.llm_api_key,
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
    if !is_well_formed_hex32(&args.prover) {
        anyhow::bail!(
            "{}",
            serde_json::json!({
                "ok": false,
                "reason": "bad_prover",
                "detail": "expected 32-byte lowercase hex"
            })
        );
    }
    let envelope_bytes: Vec<u8> = match args.envelope_path.as_deref() {
        Some(p) => std::fs::read(p)?,
        None => Vec::new(),
    };
    let mut prover_pk = [0u8; 32];
    hex::decode_to_slice(&args.prover, &mut prover_pk)
        .map_err(|e| anyhow::anyhow!("decode prover: {e}"))?;
    let client =
        BountyClient::with_timeout(args.node.clone(), Duration::from_millis(args.timeout_ms));
    let result = client.submit_proof(BountyProofInputs {
        bounty_id: &args.id,
        prover_pk: &prover_pk,
        envelope: serde_json::json!({"bytes": hex::encode(&envelope_bytes)}),
        envelope_bytes: &envelope_bytes,
    });
    match result {
        BountyProofResult::Ok {
            accepted,
            duplicate,
            bounty,
        } => {
            println!(
                "{}",
                serde_json::json!({
                    "ok": true,
                    "accepted": accepted,
                    "duplicate": duplicate,
                    "bounty": bounty,
                })
            );
            Ok(())
        }
        BountyProofResult::NotFound { id } => Err(anyhow::anyhow!(
            "{}",
            serde_json::json!({"ok": false, "reason": "not_found", "id": id})
        )),
        BountyProofResult::Terminal { status } => Err(anyhow::anyhow!(
            "{}",
            serde_json::json!({"ok": false, "reason": "terminal", "status": status})
        )),
        BountyProofResult::NoVerifier { verifier_kind } => Err(anyhow::anyhow!(
            "{}",
            serde_json::json!({"ok": false, "reason": "no_verifier", "verifierKind": verifier_kind})
        )),
        BountyProofResult::BadRequest { error, detail } => Err(anyhow::anyhow!(
            "{}",
            serde_json::json!({"ok": false, "reason": "bad_request", "error": error, "detail": detail})
        )),
        BountyProofResult::NetworkError { cause } => Err(anyhow::anyhow!(
            "{}",
            serde_json::json!({"ok": false, "reason": "network_error", "error": cause})
        )),
    }
}

fn is_well_formed_hex32(s: &str) -> bool {
    Hex32::from_hex(s).is_ok()
}

#[derive(Debug, Clone, Copy)]
enum FamilyTargetProfile {
    V031(FamilyProfile),
    V1Lenbound,
}

fn parse_family_target_profile(profile: &str) -> Option<FamilyTargetProfile> {
    match profile {
        "v031-lp" => Some(FamilyTargetProfile::V031(FamilyProfile::V031Lp)),
        "v031" => Some(FamilyTargetProfile::V031(FamilyProfile::V031)),
        "v1-lenbound" => Some(FamilyTargetProfile::V1Lenbound),
        _ => None,
    }
}

fn family_target_emitter(
    profile: FamilyTargetProfile,
    pinned_seed: Option<String>,
) -> Box<dyn TargetEmitter> {
    match profile {
        FamilyTargetProfile::V031(profile) => {
            let emitter = FamilyV031TargetEmitter::new(profile);
            match pinned_seed {
                Some(seed) => Box::new(emitter.with_pinned_seed(seed)),
                None => Box::new(emitter),
            }
        }
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
    let path = resolve_state_path(&args.state_args)?;
    let state = load_state(&path)?;
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
             unless a family profile is selected (v031-lp | v031 | v1-lenbound)"
        ),
        (None, None, None) => Box::new(StubTargetEmitter::new(
            "synthetic target — supply --fixed-target-render or use a family profile",
        )),
    };

    let verifier_mode = if args.mock_verify_accept {
        MiningRunVerifierMode::MockAccept
    } else {
        MiningRunVerifierMode::RealVerifier
    };

    let verifier: Box<dyn Verifier> = if args.mock_verify_accept {
        Box::new(AcceptingVerifier)
    } else if let Some(lean_dir) = args
        .lean_dir
        .clone()
        .or_else(|| std::env::var_os("BOOLE_LEAN_DIR").map(PathBuf::from))
    {
        Box::new(crate::local_verify::LeanVerifier::new(
            lean_dir,
            args.profile.clone(),
        ))
    } else {
        anyhow::bail!(
            "real Lean verification requires --lean-dir <PATH> \
             (or BOOLE_LEAN_DIR env var); pass --mock-verify-accept \
             to bypass for smoke tests"
        );
    };

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
