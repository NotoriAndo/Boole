use boole_core::{replay_blocks, AdmissionDecision, CalibrationReport};
use boole_lean_runner::{LeanRunner, LeanRunnerConfig};
use boole_node::block_store::FileBlockStore;
use boole_node::local_node::{serve_local_node, LocalNodeConfig};
use boole_node::proof_bridge::{LeanProofBridge, ProofSubmissionTemplate};
use boole_node::runtime::{RuntimeAdmissionState, RuntimeConfig};
use boole_node::runtime_smoke::{
    run_runtime_smoke, run_runtime_smoke_scenario_file, RuntimeSmokeInput,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::net::TcpListener;
use std::path::{Path, PathBuf};

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
    let addr = take_optional_flag_value(&mut args, "--addr")?
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let scenario_path = take_optional_flag_value(&mut args, "--scenario")?
        .unwrap_or_else(|| "fixtures/protocol/runtime-smoke/v1.json".to_string());
    let block_path = take_optional_flag_value(&mut args, "--block-store")?
        .unwrap_or_else(|| "/tmp/boole-node-local.ndjson".to_string());
    let max_requests = take_optional_flag_value(&mut args, "--max-requests")?
        .map(|value| value.parse::<usize>())
        .transpose()?;
    if !args.is_empty() {
        anyhow::bail!("unexpected args: {}", args.join(" "));
    }
    let listener = TcpListener::bind(&addr)?;
    let bound = listener.local_addr()?;
    eprintln!("boole-node local listening on http://{bound}");
    eprintln!("boole-node local blockStore={block_path}");
    serve_local_node(
        listener,
        LocalNodeConfig {
            scenario_path: scenario_path.into(),
            block_path: block_path.into(),
            max_requests,
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
    let timeout_ms = take_optional_flag_value(&mut args, "--timeout-ms")?
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(10_000);
    let memory_limit_mb = take_optional_flag_value(&mut args, "--memory-limit-mb")?
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(512);
    let ip_override = take_optional_flag_value(&mut args, "--ip")?;
    let ts = take_optional_flag_value(&mut args, "--ts")?
        .map(|value| value.parse::<u64>())
        .transpose()?
        .unwrap_or(1_800_000_000_123);
    if !args.is_empty() {
        anyhow::bail!("unexpected args: {}", args.join(" "));
    }

    let fixture = submit_lean_fixture(&fixture_path)?;
    let bridge = LeanProofBridge::new(LeanRunner::new(
        LeanRunnerConfig::new(verifier_hash)
            .with_package_dir(checker_dir)
            .with_timeout_ms(timeout_ms)
            .with_memory_limit_mb(memory_limit_mb),
    ));
    let template = ProofSubmissionTemplate {
        c: fixture.constants.c.clone(),
        pk: fixture.constants.pk.clone(),
        n: fixture.constants.n.clone(),
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
    runtime.set_current_c(fixture.constants.c.clone());
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
            "canonTag": bridged.canon_tag,
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
        "boole-node\n\ncommands:\n  runtime-smoke --scenario <path>|--fixture <path> --block-store <path>\n  run-local [--addr 127.0.0.1:8080] [--scenario <path>] [--block-store <path>] [--max-requests <n>]\n  submit-lean --proof <path> --block-store <path> [--checker-dir <path>] [--fixture <path>] [--verifier-hash <hash>]\n  agent-proof --backend fixture-valid|fixture-invalid --out-dir <path>"
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

fn submit_lean_fixture(path: &Path) -> anyhow::Result<SubmitLeanFixture> {
    let raw = std::fs::read_to_string(path)?;
    let mut fixture: SubmitLeanFixture = serde_json::from_str(&raw)?;
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
    fixture.cfg.MinShareScoreMultiplier = 1.0;
    fixture.cfg.K_max = 4;
    fixture.cfg.perIpRateLimitPer60s = 10;
    Ok(fixture)
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
