use crate::block_store::FileBlockStore;
use crate::runtime::{RuntimeAdmissionState, RuntimeConfig};
use boole_core::{replay_blocks, AdmissionDecision, CalibrationReport, DifficultyRetargetPolicy};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct RuntimeSmokeInput {
    pub fixture_path: PathBuf,
    pub block_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RuntimeSmokeScenario {
    pub config: RuntimeConfig,
    pub genesis_c: String,
    pub body: Map<String, Value>,
    pub ip: String,
    pub canon_tag: u8,
    pub block_path: PathBuf,
    pub ts: u64,
}

#[derive(Debug, Clone)]
pub struct RuntimeSmokeStep {
    pub body: Map<String, Value>,
    pub c_from_runtime_head: bool,
    pub expected_prev_c: Option<String>,
    pub restart_from_store: bool,
    pub ip: String,
    pub canon_tag: u8,
    pub ts: u64,
}

#[derive(Debug, Clone)]
pub struct RuntimeSmokeMultiScenario {
    pub config: RuntimeConfig,
    pub genesis_c: String,
    pub steps: Vec<RuntimeSmokeStep>,
    pub block_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSmokeBlockOutput {
    pub height: u64,
    pub prev_c: String,
    pub c: String,
    pub proposer_pk: String,
    pub difficulty_epoch: u64,
    pub t_block: String,
    pub t_share: String,
    pub difficulty_weight: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSmokeOutput {
    pub ok: bool,
    pub accepted: bool,
    pub height: u64,
    pub prev_c: String,
    pub c: String,
    pub replay_height: u64,
    pub replay_latest_c: String,
    pub runtime_head: String,
    pub dropped_stale_shares: usize,
    pub store_size: usize,
    pub latest_matches_runtime: bool,
    pub replay_matches_runtime: bool,
    pub block_store_path: String,
    pub blocks: Vec<RuntimeSmokeBlockOutput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSmokeScenarioJson {
    cfg: CalibrationReport,
    difficulty_retarget: Option<DifficultyRetargetPolicy>,
    genesis_c: String,
    body: Option<Map<String, Value>>,
    ip: Option<String>,
    canon_tag: Option<u8>,
    ts: Option<u64>,
    steps: Option<Vec<RuntimeSmokeStepJson>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSmokeStepJson {
    body: Map<String, Value>,
    #[serde(default)]
    c_from_runtime_head: bool,
    expected_prev_c: Option<String>,
    #[serde(default)]
    restart_from_store: bool,
    ip: String,
    canon_tag: u8,
    ts: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    constants: Constants,
    cfg: CalibrationReport,
    operations: Vec<Operation>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Constants {
    c: String,
    pk: String,
    n: String,
    j: String,
    nonce_s: String,
    ip: String,
    valid_bytes_hex: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Operation {
    name: String,
    #[serde(default)]
    body_patch: Map<String, Value>,
}

pub fn run_runtime_smoke(input: RuntimeSmokeInput) -> anyhow::Result<RuntimeSmokeOutput> {
    let raw = std::fs::read_to_string(input.fixture_path)?;
    let mut fixture: Fixture = serde_json::from_str(&raw)?;
    fixture.constants.c =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    fixture.cfg.T_share =
        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    fixture.cfg.T_block =
        "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();
    fixture.cfg.MinShareScoreMultiplier = serde_json::Number::from(1);
    fixture.cfg.K_max = 4;

    let config = RuntimeConfig::from_calibration_report(fixture.cfg, 60_000)
        .map_err(|err| anyhow::anyhow!(err))?;
    let valid_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .ok_or_else(|| anyhow::anyhow!("admission fixture missing valid operation"))?;
    let body = body_for(&fixture.constants, &valid_op.body_patch);

    run_runtime_smoke_scenario(RuntimeSmokeScenario {
        config,
        genesis_c: fixture.constants.c,
        body,
        ip: fixture.constants.ip,
        canon_tag: 0,
        block_path: input.block_path,
        ts: 1_800_000_000_123,
    })
}

pub fn run_runtime_smoke_scenario_file(
    scenario_path: PathBuf,
    block_path: PathBuf,
) -> anyhow::Result<RuntimeSmokeOutput> {
    let raw = std::fs::read_to_string(scenario_path)?;
    let scenario: RuntimeSmokeScenarioJson = serde_json::from_str(&raw)?;
    let mut config = RuntimeConfig::from_calibration_report(scenario.cfg, 60_000)
        .map_err(|err| anyhow::anyhow!(err))?;
    if let Some(policy) = scenario.difficulty_retarget {
        config = config
            .with_difficulty_retarget(policy)
            .map_err(|err| anyhow::anyhow!(err))?;
    }
    if let Some(steps) = scenario.steps {
        return run_runtime_smoke_multi_scenario(RuntimeSmokeMultiScenario {
            config,
            genesis_c: scenario.genesis_c,
            steps: steps
                .into_iter()
                .map(|step| RuntimeSmokeStep {
                    body: step.body,
                    c_from_runtime_head: step.c_from_runtime_head,
                    expected_prev_c: step.expected_prev_c,
                    restart_from_store: step.restart_from_store,
                    ip: step.ip,
                    canon_tag: step.canon_tag,
                    ts: step.ts,
                })
                .collect(),
            block_path,
        });
    }
    run_runtime_smoke_scenario(RuntimeSmokeScenario {
        config,
        genesis_c: scenario.genesis_c,
        body: scenario
            .body
            .ok_or_else(|| anyhow::anyhow!("scenario missing body"))?,
        ip: scenario
            .ip
            .ok_or_else(|| anyhow::anyhow!("scenario missing ip"))?,
        canon_tag: scenario
            .canon_tag
            .ok_or_else(|| anyhow::anyhow!("scenario missing canonTag"))?,
        block_path,
        ts: scenario
            .ts
            .ok_or_else(|| anyhow::anyhow!("scenario missing ts"))?,
    })
}

pub fn run_runtime_smoke_scenario(
    scenario: RuntimeSmokeScenario,
) -> anyhow::Result<RuntimeSmokeOutput> {
    run_runtime_smoke_multi_scenario(RuntimeSmokeMultiScenario {
        config: scenario.config,
        genesis_c: scenario.genesis_c,
        steps: vec![RuntimeSmokeStep {
            body: scenario.body,
            c_from_runtime_head: false,
            expected_prev_c: None,
            restart_from_store: false,
            ip: scenario.ip,
            canon_tag: scenario.canon_tag,
            ts: scenario.ts,
        }],
        block_path: scenario.block_path,
    })
}

pub fn run_runtime_smoke_multi_scenario(
    scenario: RuntimeSmokeMultiScenario,
) -> anyhow::Result<RuntimeSmokeOutput> {
    if scenario.steps.is_empty() {
        anyhow::bail!("runtime smoke scenario must contain at least one step");
    }
    let config = scenario.config;
    let mut runtime = RuntimeAdmissionState::new(config.clone());
    runtime.set_current_c(scenario.genesis_c);
    let mut accepted = true;
    let mut total_dropped_stale_shares = 0usize;
    let mut blocks = Vec::new();

    for step in scenario.steps {
        if step.restart_from_store {
            runtime =
                RuntimeAdmissionState::boot_from_store(config.clone(), &scenario.block_path, None)?;
        }
        if let Some(expected_prev_c) = &step.expected_prev_c {
            let runtime_head = runtime.current_c().ok_or_else(|| {
                anyhow::anyhow!("runtime head is not set before expectedPrevC check")
            })?;
            if runtime_head != expected_prev_c {
                anyhow::bail!(
                    "expectedPrevC {} does not match runtime head {}",
                    expected_prev_c,
                    runtime_head
                );
            }
        }
        let mut body = step.body;
        if step.c_from_runtime_head {
            let runtime_head = runtime
                .current_c()
                .ok_or_else(|| anyhow::anyhow!("runtime head is not set before step"))?
                .to_string();
            body.insert("c".to_string(), Value::String(runtime_head));
        }
        runtime
            .observe_ticket_from_body(&body)
            .map_err(|err| anyhow::anyhow!(err))?;
        let decision =
            runtime.admit_body_with_canon_tag(1_800_000_000_000, &step.ip, &body, step.canon_tag);
        let step_accepted = matches!(decision, AdmissionDecision::Accepted { .. });
        accepted &= step_accepted;
        if !step_accepted {
            anyhow::bail!("runtime smoke admission was rejected: {decision:?}");
        }

        let accepted_tags = BTreeSet::from([step.canon_tag]);
        let committed = runtime.commit_next_block_for_current_c(
            &scenario.block_path,
            step.ts,
            &accepted_tags,
        )?;
        total_dropped_stale_shares += committed.dropped_stale_shares;
        blocks.push(RuntimeSmokeBlockOutput {
            height: committed.block.height,
            prev_c: committed.block.prev_c,
            c: committed.block.c,
            proposer_pk: committed.block.proposer_pk,
            difficulty_epoch: committed.block.difficulty_epoch,
            t_block: committed.block.t_block,
            t_share: committed.block.t_share,
            difficulty_weight: committed.block.difficulty_weight,
        });
        assert_runtime_store_replay_consistency(&scenario.block_path, runtime.current_c())?;
    }

    let recovered = FileBlockStore::recover(&scenario.block_path)?;
    let replay = replay_blocks(recovered.blocks())?;
    let runtime_head = runtime
        .current_c()
        .ok_or_else(|| anyhow::anyhow!("runtime head is not set after commit"))?
        .to_string();
    let store_size = recovered.size();
    let latest_matches_runtime = recovered
        .latest()
        .map(|block| block.c == runtime_head)
        .unwrap_or(false);
    let replay_matches_runtime = replay.latest_c == runtime_head;
    let block_store_path = scenario.block_path.to_string_lossy().to_string();
    let latest_block = blocks
        .last()
        .ok_or_else(|| anyhow::anyhow!("runtime smoke scenario did not produce a block"))?;

    Ok(RuntimeSmokeOutput {
        ok: true,
        accepted,
        height: latest_block.height,
        prev_c: latest_block.prev_c.clone(),
        c: latest_block.c.clone(),
        replay_height: replay.height,
        replay_latest_c: replay.latest_c,
        runtime_head,
        dropped_stale_shares: total_dropped_stale_shares,
        store_size,
        latest_matches_runtime,
        replay_matches_runtime,
        block_store_path,
        blocks,
    })
}

fn assert_runtime_store_replay_consistency(
    block_path: &PathBuf,
    runtime_head: Option<&str>,
) -> anyhow::Result<()> {
    let runtime_head = runtime_head
        .ok_or_else(|| anyhow::anyhow!("runtime head is not set during consistency check"))?;
    let recovered = FileBlockStore::recover(block_path)?;
    let replay = replay_blocks(recovered.blocks())?;
    let latest = recovered
        .latest()
        .ok_or_else(|| anyhow::anyhow!("block store is empty during consistency check"))?;
    if latest.c != runtime_head {
        anyhow::bail!(
            "runtime/store divergence: latest block {} != runtime head {}",
            latest.c,
            runtime_head
        );
    }
    if replay.latest_c != runtime_head {
        anyhow::bail!(
            "runtime/replay divergence: replay head {} != runtime head {}",
            replay.latest_c,
            runtime_head
        );
    }
    Ok(())
}

fn body_for(constants: &Constants, patch: &Map<String, Value>) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("c".to_string(), Value::String(constants.c.clone()));
    body.insert("pk".to_string(), Value::String(constants.pk.clone()));
    body.insert("n".to_string(), Value::String(constants.n.clone()));
    body.insert("j".to_string(), Value::String(constants.j.clone()));
    body.insert(
        "nonceS".to_string(),
        Value::String(constants.nonce_s.clone()),
    );
    body.insert(
        "bytes".to_string(),
        Value::String(constants.valid_bytes_hex.clone()),
    );
    for (key, value) in patch {
        if value.is_null() {
            body.remove(key);
        } else {
            body.insert(key.clone(), value.clone());
        }
    }
    body
}
