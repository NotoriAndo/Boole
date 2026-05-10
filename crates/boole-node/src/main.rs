use boole_core::{
    replay_blocks, AdmissionDecision, BountyProofVerifier, BuildSelectionResult, CalibrationReport,
};
use boole_lean_runner::{LeanRunner, LeanRunnerConfig};
use boole_node::block_store::FileBlockStore;
use boole_node::lean_bounty_verifier::LeanBountyVerifier;
use boole_node::local_node::{serve_local_node, LocalNodeConfig};
use boole_node::proof_bridge::{LeanProofBridge, LeanProofBridgePolicy, ProofSubmissionTemplate};
use boole_node::runtime::{RuntimeAdmissionState, RuntimeConfig};
use boole_node::runtime_smoke::{
    run_runtime_smoke, run_runtime_smoke_scenario_file, RuntimeSmokeInput,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn main() -> anyhow::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("runtime-smoke") => run_runtime_smoke_command(args),
        Some("run-local") => run_local_command(args),
        Some("submit-lean") => run_submit_lean_command(args),
        Some("agent-proof") => run_agent_proof_command(args),
        Some("--help") | Some("-h") | None => {
            print_help();
            Ok(())
        }
        Some(other) => anyhow::bail!("unknown command {other}"),
    }
}

fn run_runtime_smoke_command(mut args: Vec<String>) -> anyhow::Result<()> {
    args.remove(0);
    let fixture_path = take_optional_flag_value(&mut args, "--fixture")?;
    let scenario_path = take_optional_flag_value(&mut args, "--scenario")?;
    let block_path = take_flag_value(&mut args, "--block-store")?;
    if fixture_path.is_some() == scenario_path.is_some() {
        anyhow::bail!("provide exactly one of --fixture or --scenario");
    }
    if !args.is_empty() {
        anyhow::bail!("unexpected args: {}", args.join(" "));
    }
    let output = if let Some(scenario_path) = scenario_path {
        run_runtime_smoke_scenario_file(scenario_path.into(), block_path.into())?
    } else {
        run_runtime_smoke(RuntimeSmokeInput {
            fixture_path: fixture_path.expect("checked fixture path").into(),
            block_path: block_path.into(),
        })?
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn run_local_command(mut args: Vec<String>) -> anyhow::Result<()> {
    args.remove(0);
    // The pof boole-cli wrapper drives the local node through env vars
    // (PORT, BLOCKSTORE_PATH, GENESIS_C). Mirror that fan-out so that the
    // Rust `boole node start` wrapper can keep the same shape — flags win
    // over env vars so explicit invocations remain debuggable.
    let port_env = std::env::var("PORT").ok();
    let block_env = std::env::var("BLOCKSTORE_PATH").ok();
    let reward_env = std::env::var("REWARDLEDGER_PATH").ok();
    let work_env = std::env::var("WORK_MANIFESTS_PATH").ok();
    let bounties_env = std::env::var("BOUNTIES_PATH").ok();
    let bounty_events_env = std::env::var("BOUNTY_EVENT_LEDGER_PATH").ok();
    let family_manifests_env = std::env::var("FAMILY_MANIFESTS_DIR").ok();
    let operator_signer_pks_env = std::env::var("OPERATOR_SIGNER_PKS").ok();
    let lean_checker_env = std::env::var("LEAN_CHECKER_DIR").ok();
    let genesis_env = std::env::var("GENESIS_C").ok();
    let addr_flag = take_optional_flag_value(&mut args, "--addr")?;
    let port_flag = take_optional_flag_value(&mut args, "--port")?;
    let scenario_path = take_optional_flag_value(&mut args, "--scenario")?
        .unwrap_or_else(|| "fixtures/protocol/runtime-smoke/v1.json".to_string());
    let block_flag = take_optional_flag_value(&mut args, "--block-store")?;
    let reward_flag = take_optional_flag_value(&mut args, "--reward-store")?;
    let work_flag = take_optional_flag_value(&mut args, "--work-manifests")?;
    let bounties_flag = take_optional_flag_value(&mut args, "--bounties")?;
    let bounty_events_flag = take_optional_flag_value(&mut args, "--bounty-events")?;
    let family_manifests_flag = take_optional_flag_value(&mut args, "--family-manifests")?;
    let operator_signer_pks_flag = take_optional_flag_value(&mut args, "--operator-signer-pks")?;
    let lean_checker_flag = take_optional_flag_value(&mut args, "--lean-checker-dir")?;
    let max_requests = take_optional_flag_value(&mut args, "--max-requests")?
        .map(|value| value.parse::<usize>())
        .transpose()?;
    let genesis_flag = take_optional_flag_value(&mut args, "--genesis")?;
    if !args.is_empty() {
        anyhow::bail!("unexpected args: {}", args.join(" "));
    }
    let addr = if let Some(addr) = addr_flag {
        addr
    } else if let Some(port) = port_flag.or(port_env) {
        format!("127.0.0.1:{port}")
    } else {
        "127.0.0.1:8080".to_string()
    };
    let block_path = block_flag
        .or(block_env)
        .unwrap_or_else(|| "/tmp/boole-node-local.ndjson".to_string());
    let reward_ledger_path = reward_flag
        .or(reward_env)
        .unwrap_or_else(|| "/tmp/boole-node-rewards.ndjson".to_string());
    let work_manifests_path: Option<PathBuf> = work_flag.or(work_env).map(PathBuf::from);
    let bounties_path: Option<PathBuf> = bounties_flag.or(bounties_env).map(PathBuf::from);
    let bounty_event_ledger_path: Option<PathBuf> =
        bounty_events_flag.or(bounty_events_env).map(PathBuf::from);
    let family_manifests_dir: Option<PathBuf> = family_manifests_flag
        .or(family_manifests_env)
        .map(PathBuf::from);
    let operator_signer_pks: Vec<String> = operator_signer_pks_flag
        .or(operator_signer_pks_env)
        .map(|raw| {
            raw.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    for pk in &operator_signer_pks {
        if pk.len() != 64 || !pk.bytes().all(|b| b.is_ascii_hexdigit()) {
            anyhow::bail!(
                "--operator-signer-pks entry {pk:?} is not 64 hex chars"
            );
        }
    }
    let lean_checker_dir: Option<PathBuf> =
        lean_checker_flag.or(lean_checker_env).map(PathBuf::from);
    let genesis_override = genesis_flag.or(genesis_env);
    let listener = TcpListener::bind(&addr)?;
    let bound = listener.local_addr()?;
    eprintln!("boole-node local listening on http://{bound}");
    eprintln!("boole-node local blockStore={block_path}");
    eprintln!("boole-node local rewardLedger={reward_ledger_path}");
    if let Some(path) = work_manifests_path.as_ref() {
        eprintln!("boole-node local workManifests={}", path.display());
    } else {
        eprintln!("boole-node local workManifests=<none>");
    }
    if let Some(path) = bounties_path.as_ref() {
        eprintln!("boole-node local bounties={}", path.display());
    } else {
        eprintln!("boole-node local bounties=<none>");
    }
    if let Some(path) = bounty_event_ledger_path.as_ref() {
        eprintln!("boole-node local bountyEvents={}", path.display());
    } else {
        eprintln!("boole-node local bountyEvents=<none>");
    }
    if let Some(path) = family_manifests_dir.as_ref() {
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
        lean_checker_dir.as_ref().map(|dir| {
            eprintln!("boole-node local leanCheckerDir={}", dir.display());
            let mut m: HashMap<String, Arc<dyn BountyProofVerifier>> = HashMap::new();
            m.insert(
                "lean".to_string(),
                Arc::new(LeanBountyVerifier::new(dir.clone())),
            );
            m
        });
    serve_local_node(
        listener,
        LocalNodeConfig {
            scenario_path: scenario_path.into(),
            block_path: block_path.into(),
            reward_ledger_path: Some(reward_ledger_path.into()),
            work_manifests_path,
            bounties_path,
            bounty_event_ledger_path,
            bounty_verifiers,
            family_manifests_dir,
            max_requests,
            operator_signer_pks,
            genesis_override,
        },
    )
}

fn run_submit_lean_command(mut args: Vec<String>) -> anyhow::Result<()> {
    args.remove(0);
    let proof_path = PathBuf::from(take_flag_value(&mut args, "--proof")?);
    let checker_dir = PathBuf::from(
        take_optional_flag_value(&mut args, "--checker-dir")?.unwrap_or_else(|| ".".to_string()),
    );
    let fixture_path = PathBuf::from(
        take_optional_flag_value(&mut args, "--fixture")?
            .unwrap_or_else(|| "fixtures/protocol/admission/v1.json".to_string()),
    );
    let block_path = PathBuf::from(take_flag_value(&mut args, "--block-store")?);
    let verifier_hash = take_optional_flag_value(&mut args, "--verifier-hash")?
        .unwrap_or_else(|| "boole-submit-lean-v0".to_string());
    let required_checker_artifact_hash =
        match take_optional_flag_value(&mut args, "--require-checker-artifact-hash")? {
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
    let timeout_ms = take_optional_flag_value(&mut args, "--timeout-ms")?
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(10_000);
    let memory_limit_mb = take_optional_flag_value(&mut args, "--memory-limit-mb")?
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(8192);
    let ip_override = take_optional_flag_value(&mut args, "--ip")?;
    let head_c_override = take_optional_flag_value(&mut args, "--head-c")?;
    let admission_nonce_override = take_optional_flag_value(&mut args, "--admission-nonce")?;
    let difficulty_mode = take_optional_flag_value(&mut args, "--difficulty-mode")?
        .unwrap_or_else(|| "fixture".to_string());
    if !matches!(difficulty_mode.as_str(), "fixture" | "preflight-easy") {
        anyhow::bail!(
            "unsupported --difficulty-mode {difficulty_mode}; expected fixture or preflight-easy"
        );
    }
    let ts = take_optional_flag_value(&mut args, "--ts")?
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(1_800_000_000_123);
    if !args.is_empty() {
        anyhow::bail!("unexpected args: {}", args.join(" "));
    }
    // Validate `--admission-nonce` shape *before* fixture parse + Lean spawn so
    // a malformed value fails fast and never pays for `lake exec boole_check`.
    // Reason kebab parallels S9's `malformed-pk` so downstream tooling can
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

fn run_agent_proof_command(mut args: Vec<String>) -> anyhow::Result<()> {
    args.remove(0);
    let backend = take_flag_value(&mut args, "--backend")?;
    let out_dir = PathBuf::from(take_flag_value(&mut args, "--out-dir")?);
    if !args.is_empty() {
        anyhow::bail!("unexpected args: {}", args.join(" "));
    }

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

fn print_help() {
    println!(
        "boole-node\n\ncommands:\n  runtime-smoke --scenario <path>|--fixture <path> --block-store <path>\n  run-local [--addr 127.0.0.1:8080] [--port <n>] [--scenario <path>] [--block-store <path>] [--reward-store <path>] [--work-manifests <path>] [--bounties <path>] [--bounty-events <path>] [--family-manifests <dir>] [--operator-signer-pks <hex,hex,...>] [--lean-checker-dir <path>] [--genesis <64-hex>] [--max-requests <n>]\n  submit-lean --proof <path> --block-store <path> [--checker-dir <path>] [--fixture <path>] [--verifier-hash <hash>] [--head-c <64-hex>] [--admission-nonce <64-hex>] [--difficulty-mode fixture|preflight-easy]\n  agent-proof --backend fixture-valid|fixture-invalid --out-dir <path>\n\nenvironment (mirrors pof booleCli wrapper):\n  PORT                  default port for run-local (overridden by --port/--addr)\n  BLOCKSTORE_PATH       default block store path (overridden by --block-store)\n  REWARDLEDGER_PATH     default reward ledger path (overridden by --reward-store)\n  WORK_MANIFESTS_PATH   optional work-manifest catalog path (overridden by --work-manifests)\n  BOUNTIES_PATH         optional bounty catalog path (overridden by --bounties)\n  BOUNTY_EVENT_LEDGER_PATH  optional bounty audit log path (overridden by --bounty-events)
  FAMILY_MANIFESTS_DIR  optional directory of FamilyManifest *.json files (overridden by --family-manifests)\n  OPERATOR_SIGNER_PKS   comma-separated hex32 pks trusted to sign FamilyManifests; empty disables promotion (overridden by --operator-signer-pks)\n  LEAN_CHECKER_DIR      lake/lean checker directory; enables `lean` verifier (overridden by --lean-checker-dir)\n  GENESIS_C             scenario genesis_c override (overridden by --genesis)"
    );
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
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn take_optional_flag_value(args: &mut Vec<String>, flag: &str) -> anyhow::Result<Option<String>> {
    let Some(index) = args.iter().position(|arg| arg == flag) else {
        return Ok(None);
    };
    args.remove(index);
    if index >= args.len() {
        anyhow::bail!("missing value for flag {flag}");
    }
    Ok(Some(args.remove(index)))
}

fn take_flag_value(args: &mut Vec<String>, flag: &str) -> anyhow::Result<String> {
    let Some(index) = args.iter().position(|arg| arg == flag) else {
        anyhow::bail!("missing required flag {flag}");
    };
    args.remove(index);
    if index >= args.len() {
        anyhow::bail!("missing value for flag {flag}");
    }
    Ok(args.remove(index))
}
