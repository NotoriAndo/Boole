use boole_core::{
    replay_blocks, AdmissionDecision, BountyProofVerifier, BuildSelectionResult, CalibrationReport,
    Hex32,
};
use boole_lean_runner::{LeanRunner, LeanRunnerConfig};
use boole_node::FileBlockStore;
use boole_node::LeanBountyVerifier;
use boole_node::{run_runtime_smoke, run_runtime_smoke_scenario_file, RuntimeSmokeInput};
use boole_node::{serve_local_node, LocalNodeConfig};
use boole_node::{LeanProofBridge, LeanProofBridgePolicy, ProofSubmissionTemplate};
use boole_node::{RuntimeAdmissionState, RuntimeConfig};
use clap::{ArgGroup, Args, Parser, Subcommand};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// boole-node — typed argv layer (P0.4 / L2 contract).
///
/// `--help` and `--version` are derived by clap from these doc comments
/// and Cargo metadata so release notes can quote them verbatim without
/// drifting from runtime behavior.
#[derive(Parser)]
#[command(name = "boole-node", version, about = "Boole node CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Replay a runtime-smoke fixture or scenario into a fresh block store.
    RuntimeSmoke(RuntimeSmokeArgs),
    /// Run the local HTTP node. Flags override the matching env vars.
    RunLocal(RunLocalArgs),
    /// Submit a Lean proof to the deterministic verifier and (optionally)
    /// commit a block on success.
    SubmitLean(SubmitLeanArgs),
    /// Emit a deterministic agent-proof candidate for downstream submit-lean.
    AgentProof(AgentProofArgs),
}

#[derive(Args)]
#[command(group(
    ArgGroup::new("runtime_smoke_input")
        .args(["fixture", "scenario"])
        .required(true)
))]
struct RuntimeSmokeArgs {
    #[arg(long)]
    fixture: Option<PathBuf>,
    #[arg(long)]
    scenario: Option<PathBuf>,
    #[arg(long = "block-store")]
    block_store: PathBuf,
}

#[derive(Args)]
struct RunLocalArgs {
    #[arg(long)]
    addr: Option<String>,
    #[arg(long, env = "PORT")]
    port: Option<String>,
    #[arg(long, default_value = "fixtures/protocol/runtime-smoke/v1.json")]
    scenario: String,
    #[arg(long = "block-store", env = "BLOCKSTORE_PATH")]
    block_store: Option<String>,
    #[arg(long = "reward-store", env = "REWARDLEDGER_PATH")]
    reward_store: Option<String>,
    #[arg(long = "work-manifests", env = "WORK_MANIFESTS_PATH")]
    work_manifests: Option<PathBuf>,
    #[arg(long, env = "BOUNTIES_PATH")]
    bounties: Option<PathBuf>,
    #[arg(long = "bounty-events", env = "BOUNTY_EVENT_LEDGER_PATH")]
    bounty_events: Option<PathBuf>,
    #[arg(long = "family-manifests", env = "FAMILY_MANIFESTS_DIR")]
    family_manifests: Option<PathBuf>,
    #[arg(long = "operator-signer-pks", env = "OPERATOR_SIGNER_PKS")]
    operator_signer_pks: Option<String>,
    #[arg(long = "session-registry", env = "BOOLE_SESSION_REGISTRY_PATH")]
    session_registry: Option<PathBuf>,
    #[arg(long = "submit-nonce-ledger", env = "BOOLE_SUBMIT_NONCE_LEDGER_PATH")]
    submit_nonce_ledger: Option<PathBuf>,
    #[arg(
        long = "submit-receipt-ledger",
        env = "BOOLE_SUBMIT_RECEIPT_LEDGER_PATH"
    )]
    submit_receipt_ledger: Option<PathBuf>,
    #[arg(
        long = "receipt-commitment-ledger",
        env = "BOOLE_RECEIPT_COMMITMENT_LEDGER_PATH"
    )]
    receipt_commitment_ledger: Option<PathBuf>,
    #[arg(long = "lean-checker-dir", env = "LEAN_CHECKER_DIR")]
    lean_checker_dir: Option<PathBuf>,
    /// P2.6 b — Explicit operator acknowledgement that proofs arriving
    /// at this node will not be Lean-verified (testnet only). Without
    /// either `--lean-checker-dir` or this flag, `/ready` returns 503
    /// (`reason: "lean_checker_not_configured"`). The two flags are
    /// mutually exclusive; supplying both is a misconfiguration.
    #[arg(
        long = "lean-checker-disabled",
        default_value_t = false,
        conflicts_with = "lean_checker_dir"
    )]
    lean_checker_disabled: bool,
    #[arg(long = "max-requests")]
    max_requests: Option<usize>,
    #[arg(long, env = "GENESIS_C")]
    genesis: Option<String>,
    /// L7 state directory (P1.1). Opt-in. When set, the runtime acquires
    /// an exclusive `flock` on `<dir>/state.lock` before opening any
    /// ledger and writes/verifies `<dir>/state.manifest.json`. A second
    /// `boole-node run-local` against the same `--state-dir` exits with
    /// a typed `state-dir-locked` envelope (exit 74) before binding a
    /// port.
    #[arg(long = "state-dir", env = "BOOLE_STATE_DIR")]
    state_dir: Option<PathBuf>,
    /// Network identifier pinned into `state.manifest.json` on first
    /// boot of `--state-dir`. Default `boole-mvp`. Subsequent boots
    /// that pass a different value are rejected by the manifest
    /// contract. Ignored when `--state-dir` is unset.
    #[arg(long = "network-id", env = "BOOLE_NETWORK_ID")]
    network_id: Option<String>,
}

#[derive(Args)]
struct SubmitLeanArgs {
    #[arg(long)]
    proof: PathBuf,
    #[arg(long = "checker-dir", default_value = ".")]
    checker_dir: PathBuf,
    #[arg(long, default_value = "fixtures/protocol/admission/v1.json")]
    fixture: PathBuf,
    #[arg(long = "block-store")]
    block_store: PathBuf,
    #[arg(long = "verifier-hash", default_value = "boole-submit-lean-v0")]
    verifier_hash: String,
    /// Required to enforce checker-artifact-hash policy. Missing value
    /// returns a structured JSON error on stderr (kept Optional so we can
    /// emit that JSON instead of clap's default error format).
    #[arg(long = "require-checker-artifact-hash")]
    require_checker_artifact_hash: Option<String>,
    #[arg(long = "timeout-ms", default_value_t = 10_000)]
    timeout_ms: u64,
    #[arg(long = "memory-limit-mb", default_value_t = 8192)]
    memory_limit_mb: u64,
    #[arg(long)]
    ip: Option<String>,
    #[arg(long = "head-c")]
    head_c: Option<String>,
    #[arg(long = "admission-nonce")]
    admission_nonce: Option<String>,
    #[arg(long = "difficulty-mode", default_value = "fixture")]
    difficulty_mode: String,
    #[arg(long, default_value_t = 1_800_000_000_123)]
    ts: u64,
}

#[derive(Args)]
struct AgentProofArgs {
    #[arg(long)]
    backend: String,
    #[arg(long = "out-dir")]
    out_dir: PathBuf,
}

fn main() -> anyhow::Result<()> {
    boole_core::telemetry::init(boole_core::telemetry::BinaryName::Node);
    let cli = Cli::parse();
    match cli.command {
        Command::RuntimeSmoke(args) => run_runtime_smoke_command(args),
        Command::RunLocal(args) => run_local_command(args),
        Command::SubmitLean(args) => run_submit_lean_command(args),
        Command::AgentProof(args) => run_agent_proof_command(args),
    }
}

fn run_runtime_smoke_command(args: RuntimeSmokeArgs) -> anyhow::Result<()> {
    let output = if let Some(scenario_path) = args.scenario {
        run_runtime_smoke_scenario_file(scenario_path, args.block_store)?
    } else {
        run_runtime_smoke(RuntimeSmokeInput {
            fixture_path: args
                .fixture
                .expect("clap group guarantees fixture or scenario"),
            block_path: args.block_store,
        })?
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn run_local_command(args: RunLocalArgs) -> anyhow::Result<()> {
    // Flags win over env vars (clap `env` attribute already implements
    // that fallback per arg), so this layer only resolves the remaining
    // address/path defaults and validates structural invariants.
    let addr = if let Some(addr) = args.addr {
        addr
    } else if let Some(port) = args.port {
        format!("127.0.0.1:{port}")
    } else {
        "127.0.0.1:8080".to_string()
    };
    let block_path = args
        .block_store
        .unwrap_or_else(|| "/tmp/boole-node-local.ndjson".to_string());
    let reward_ledger_path = args
        .reward_store
        .unwrap_or_else(|| "/tmp/boole-node-rewards.ndjson".to_string());
    let operator_signer_pks: Vec<String> = args
        .operator_signer_pks
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    for pk in &operator_signer_pks {
        if Hex32::from_hex(pk).is_err() {
            anyhow::bail!("--operator-signer-pks entry {pk:?} is not 64 lowercase hex chars");
        }
    }
    let listener = TcpListener::bind(&addr)?;
    let bound = listener.local_addr()?;
    eprintln!("boole-node local listening on http://{bound}");
    eprintln!("boole-node local blockStore={block_path}");
    eprintln!("boole-node local rewardLedger={reward_ledger_path}");
    if let Some(path) = args.work_manifests.as_ref() {
        eprintln!("boole-node local workManifests={}", path.display());
    } else {
        eprintln!("boole-node local workManifests=<none>");
    }
    if let Some(path) = args.bounties.as_ref() {
        eprintln!("boole-node local bounties={}", path.display());
    } else {
        eprintln!("boole-node local bounties=<none>");
    }
    if let Some(path) = args.bounty_events.as_ref() {
        eprintln!("boole-node local bountyEvents={}", path.display());
    } else {
        eprintln!("boole-node local bountyEvents=<none>");
    }
    if let Some(path) = args.family_manifests.as_ref() {
        eprintln!("boole-node local familyManifestsDir={}", path.display());
    } else {
        eprintln!("boole-node local familyManifestsDir=<none>");
    }
    eprintln!(
        "boole-node local operatorSignerPks={}",
        if operator_signer_pks.is_empty() {
            "<none>".to_string()
        } else {
            format!("{} pk(s)", operator_signer_pks.len())
        }
    );
    let bounty_verifiers: Option<HashMap<String, Arc<dyn BountyProofVerifier>>> =
        args.lean_checker_dir.as_ref().map(|dir| {
            eprintln!("boole-node local leanCheckerDir={}", dir.display());
            let mut m: HashMap<String, Arc<dyn BountyProofVerifier>> = HashMap::new();
            m.insert(
                "lean".to_string(),
                Arc::new(LeanBountyVerifier::new(dir.clone())),
            );
            m
        });
    if args.lean_checker_dir.is_none() {
        eprintln!(
            "boole-node local leanCheckerDir=<none> leanCheckerDisabled={}",
            args.lean_checker_disabled
        );
    }
    if let Some(dir) = args.state_dir.as_ref() {
        eprintln!("boole-node local stateDir={}", dir.display());
    }
    let result = serve_local_node(
        listener,
        LocalNodeConfig {
            scenario_path: args.scenario.into(),
            block_path: block_path.into(),
            reward_ledger_path: Some(reward_ledger_path.into()),
            work_manifests_path: args.work_manifests,
            bounties_path: args.bounties,
            bounty_event_ledger_path: args.bounty_events,
            bounty_verifiers,
            family_manifests_dir: args.family_manifests,
            max_requests: args.max_requests,
            operator_signer_pks,
            session_registry_path: args.session_registry,
            submit_nonce_ledger_path: args.submit_nonce_ledger,
            submit_receipt_ledger_path: args.submit_receipt_ledger,
            receipt_commitment_ledger_path: args.receipt_commitment_ledger,
            genesis_override: args.genesis,
            state_dir: args.state_dir,
            network_id: args.network_id,
            lean_checker_dir: args.lean_checker_dir,
            lean_checker_disabled: args.lean_checker_disabled,
            http_rate_limit_per_60s: None,
        },
    );
    match result {
        Ok(()) => Ok(()),
        Err(err) => {
            if let Some(state_err) = err.downcast_ref::<boole_node::StateDirError>() {
                if let boole_node::StateDirError::Locked(dir) = state_err {
                    eprintln!(
                        "{}",
                        serde_json::to_string(&json!({
                            "ok": false,
                            "command": "run-local",
                            "error": "state-dir-locked",
                            "stateDir": dir.display().to_string(),
                            "message": state_err.to_string(),
                        }))?,
                    );
                    std::process::exit(74);
                }
            }
            Err(err)
        }
    }
}

fn run_submit_lean_command(args: SubmitLeanArgs) -> anyhow::Result<()> {
    let proof_path = args.proof;
    let checker_dir = args.checker_dir;
    let fixture_path = args.fixture;
    let block_path = args.block_store;
    let verifier_hash = args.verifier_hash;
    // Kept Optional in clap so we can emit a structured JSON error on
    // stderr (downstream tooling pattern-matches `error` codes) instead
    // of clap's default human-readable "missing required argument" line.
    let required_checker_artifact_hash = match args.require_checker_artifact_hash {
        Some(value) => value,
        None => {
            eprintln!(
                "{}",
                serde_json::to_string(&json!({
                    "ok": false,
                    "command": "submit-lean",
                    "accepted": false,
                    "error": "missing_checker_artifact_policy",
                    "shareAccepted": false,
                    "blockProduced": false,
                    "invalidAccepted": 0,
                }))?
            );
            std::process::exit(1);
        }
    };
    let timeout_ms = args.timeout_ms;
    let memory_limit_mb = args.memory_limit_mb;
    let ip_override = args.ip;
    let head_c_override = args.head_c;
    let admission_nonce_override = args.admission_nonce;
    let difficulty_mode = args.difficulty_mode;
    if !matches!(difficulty_mode.as_str(), "fixture" | "preflight-easy") {
        anyhow::bail!(
            "unsupported --difficulty-mode {difficulty_mode}; expected fixture or preflight-easy"
        );
    }
    let ts = args.ts;
    // Validate `--admission-nonce` shape *before* fixture parse + Lean spawn so
    // a malformed value fails fast and never pays for `lake exec boole_check`.
    // The reason code mirrors account/session malformed public-key rejections so downstream tooling can
    // pattern-match across both surfaces.
    if let Some(value) = admission_nonce_override.as_deref() {
        if !is_well_formed_hex32(value) {
            eprintln!(
                "{}",
                serde_json::to_string(&json!({
                    "ok": false,
                    "command": "submit-lean",
                    "accepted": false,
                    "error": "malformed-admission-nonce",
                    "shareAccepted": false,
                    "blockProduced": false,
                    "invalidAccepted": 0,
                }))?
            );
            std::process::exit(1);
        }
    }

    let fixture = submit_lean_fixture(&fixture_path, &difficulty_mode)?;
    let bridge_policy = LeanProofBridgePolicy::new()
        .require_verifier_hash(verifier_hash.clone())
        .allow_checker_artifact_hash(required_checker_artifact_hash);
    let bridge = LeanProofBridge::new_with_policy(
        LeanRunner::new(
            LeanRunnerConfig::new(verifier_hash)
                .with_package_dir(checker_dir)
                .with_timeout_ms(timeout_ms)
                .with_memory_limit_mb(memory_limit_mb),
        ),
        bridge_policy,
    );
    let template = ProofSubmissionTemplate {
        c: head_c_override.unwrap_or_else(|| fixture.constants.c.clone()),
        pk: fixture.constants.pk.clone(),
        n: admission_nonce_override
            .clone()
            .unwrap_or_else(|| fixture.constants.n.clone()),
        j: fixture.constants.j.clone(),
        nonce_s: fixture.constants.nonce_s.clone(),
    };
    let bridged = match bridge.build_submission_body(&proof_path, &template) {
        Ok(bridged) => bridged,
        Err(err) => {
            eprintln!(
                "{}",
                serde_json::to_string(&json!({
                    "ok": false,
                    "command": "submit-lean",
                    "accepted": false,
                    "error": err.kind(),
                    "lean": err.lean(),
                    "shareAccepted": false,
                    "blockProduced": false,
                    "invalidAccepted": 0,
                }))?
            );
            std::process::exit(1);
        }
    };

    let config = RuntimeConfig::from_calibration_report(fixture.cfg, 60_000)
        .map_err(|err| anyhow::anyhow!(err))?;
    let mut runtime = RuntimeAdmissionState::new(config);
    runtime.set_current_c(template.c.clone());
    runtime
        .observe_ticket_from_body(&bridged.body)
        .map_err(|err| anyhow::anyhow!(err))?;
    let ip = ip_override.unwrap_or(fixture.constants.ip);
    let decision =
        runtime.admit_body_with_canon_tag(ts as i64, &ip, &bridged.body, bridged.canon_tag);
    let AdmissionDecision::Accepted { share_hash } = decision else {
        eprintln!(
            "{}",
            serde_json::to_string(&json!({
                "ok": false,
                "command": "submit-lean",
                "accepted": false,
                "error": "admission_rejected",
                "decision": format!("{decision:?}"),
                "lean": bridged.lean,
                "shareAccepted": false,
                "blockProduced": false,
                "invalidAccepted": 0,
            }))?
        );
        std::process::exit(1);
    };

    let accepted_tags = BTreeSet::from([bridged.canon_tag]);
    let selection = runtime.build_block_selection_for_current_c(&accepted_tags)?;
    if !matches!(selection, BuildSelectionResult::Ok(_)) {
        let runtime_head = runtime
            .current_c()
            .ok_or_else(|| anyhow::anyhow!("runtime head is not set after submit-lean"))?
            .to_string();
        println!(
            "{}",
            serde_json::to_string(&json!({
                "ok": true,
                "command": "submit-lean",
                "accepted": true,
                "lean": bridged.lean,
                "shareAccepted": true,
                "shareHash": share_hash.to_hex(),
                "packageBytes": hex::encode(&bridged.package_bytes),
                "submissionBody": bridged.body,
                "canonTag": bridged.canon_tag,
                "blockProduced": false,
                "block": null,
                "blockSelection": format!("{selection:?}"),
                "replayHeight": 0,
                "replayLatestC": runtime_head,
                "runtimeHead": runtime_head,
                "replayMatchesRuntime": true,
                "blockStorePath": block_path.to_string_lossy(),
                "difficultyMode": difficulty_mode,
                "invalidAccepted": 0,
            }))?
        );
        return Ok(());
    }

    let committed = runtime.commit_next_block_for_current_c(&block_path, ts, &accepted_tags)?;
    let recovered = FileBlockStore::recover(&block_path)?;
    let replay = replay_blocks(recovered.blocks())?;
    let runtime_head = runtime
        .current_c()
        .ok_or_else(|| anyhow::anyhow!("runtime head is not set after submit-lean"))?
        .to_string();
    println!(
        "{}",
        serde_json::to_string(&json!({
            "ok": true,
            "command": "submit-lean",
            "accepted": true,
            "lean": bridged.lean,
            "shareAccepted": true,
            "shareHash": share_hash.to_hex(),
            "packageBytes": hex::encode(&bridged.package_bytes),
            "submissionBody": bridged.body,
            "canonTag": bridged.canon_tag,
            "blockProduced": true,
            "block": {
                "height": committed.block.height,
                "prevC": committed.block.prev_c,
                "c": committed.block.c,
                "selectedShares": committed.block.selected_share_hashes.len(),
                "difficultyEpoch": committed.block.difficulty_epoch,
                "tBlock": committed.block.t_block,
                "tShare": committed.block.t_share,
                "difficultyWeight": committed.block.difficulty_weight,
            },
            "replayHeight": replay.height,
            "replayLatestC": replay.latest_c,
            "runtimeHead": runtime_head,
            "replayMatchesRuntime": replay.latest_c == runtime_head,
            "blockStorePath": block_path.to_string_lossy(),
            "difficultyMode": difficulty_mode,
            "invalidAccepted": 0,
        }))?
    );
    Ok(())
}

fn run_agent_proof_command(args: AgentProofArgs) -> anyhow::Result<()> {
    let backend = args.backend;
    let out_dir = args.out_dir;

    let (file_name, proof_source, backend_description) = match backend.as_str() {
        "fixture-valid" => (
            "Proof.lean",
            "theorem boole_agent_fixture_valid : 2 + 2 = 4 := by\n  decide\n",
            "deterministic valid Lean fixture backend",
        ),
        "fixture-invalid" => (
            "Proof.lean",
            "theorem boole_agent_fixture_invalid : 2 + 2 = 5 := by\n  decide\n",
            "deterministic invalid Lean fixture backend",
        ),
        other => anyhow::bail!("unsupported agent-proof backend {other}"),
    };

    std::fs::create_dir_all(&out_dir)?;
    let proof_path = out_dir.join(file_name);
    std::fs::write(&proof_path, proof_source)?;
    let source_hash = blake3::hash(proof_source.as_bytes()).to_hex().to_string();

    println!(
        "{}",
        serde_json::to_string(&json!({
            "ok": true,
            "command": "agent-proof",
            "backend": backend,
            "backendDescription": backend_description,
            "agentProofCandidate": true,
            "trusted": false,
            "consensusAccepted": false,
            "proofFormat": "lean",
            "proofPath": proof_path.to_string_lossy(),
            "sourceHash": source_hash,
            "safety": {
                "agentOutputTrusted": false,
                "requiresDeterministicVerifier": true,
                "consensusBoundary": "boole-node submit-lean / LeanRunner / canonical package / replay",
            }
        }))?
    );
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmitLeanFixture {
    constants: SubmitLeanConstants,
    cfg: CalibrationReport,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubmitLeanConstants {
    c: String,
    pk: String,
    n: String,
    j: String,
    nonce_s: String,
    ip: String,
}

fn submit_lean_fixture(path: &Path, difficulty_mode: &str) -> anyhow::Result<SubmitLeanFixture> {
    let raw = std::fs::read_to_string(path)?;
    let mut fixture: SubmitLeanFixture = serde_json::from_str(&raw)?;
    if difficulty_mode == "preflight-easy" {
        fixture.constants.c =
            "0000000000000000000000000000000000000000000000000000000000000000".to_string();
        fixture.cfg.T_submit =
            "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
        fixture.cfg.T_ticket =
            "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
        fixture.cfg.T_share =
            "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
        fixture.cfg.T_block =
            "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();
        fixture.cfg.MinShareScoreMultiplier = serde_json::Number::from(1);
        fixture.cfg.K_max = 4;
        fixture.cfg.perIpRateLimitPer60s = 10;
    }
    Ok(fixture)
}

fn is_well_formed_hex32(s: &str) -> bool {
    boole_core::Hex32::from_hex(s).is_ok()
}
