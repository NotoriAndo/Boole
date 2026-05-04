use boole_core::{replay_blocks, AdmissionDecision, CalibrationReport};
use boole_node::block_store::FileBlockStore;
use boole_node::runtime::RuntimeAdmissionState;
use boole_node::runtime::RuntimeConfig;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeSmokeOutput {
    ok: bool,
    accepted: bool,
    height: u64,
    prev_c: String,
    c: String,
    replay_height: u64,
    replay_latest_c: String,
    runtime_head: String,
    dropped_stale_shares: usize,
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some("runtime-smoke") {
        println!("boole-node migration spike");
        return Ok(());
    }
    args.remove(0);
    let fixture_path = take_flag_value(&mut args, "--fixture")?;
    let block_path = take_flag_value(&mut args, "--block-store")?;
    if !args.is_empty() {
        anyhow::bail!("unexpected args: {}", args.join(" "));
    }
    let output = run_runtime_smoke(fixture_path.into(), block_path.into())?;
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn run_runtime_smoke(
    fixture_path: PathBuf,
    block_path: PathBuf,
) -> anyhow::Result<RuntimeSmokeOutput> {
    let raw = std::fs::read_to_string(fixture_path)?;
    let mut fixture: Fixture = serde_json::from_str(&raw)?;
    fixture.constants.c =
        "0000000000000000000000000000000000000000000000000000000000000000".to_string();
    fixture.cfg.T_share =
        "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string();
    fixture.cfg.T_block =
        "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe".to_string();
    fixture.cfg.MinShareScoreMultiplier = 1.0;
    fixture.cfg.K_max = 4;

    let config = RuntimeConfig::from_calibration_report(fixture.cfg, 60_000)
        .map_err(|err| anyhow::anyhow!(err))?;
    let mut runtime = RuntimeAdmissionState::new(config);
    runtime.set_current_c(fixture.constants.c.clone());

    let valid_op = fixture
        .operations
        .iter()
        .find(|op| op.name == "valid_after_bad_not_rate_limited")
        .ok_or_else(|| anyhow::anyhow!("admission fixture missing valid operation"))?;
    let body = body_for(&fixture.constants, &valid_op.body_patch);
    runtime
        .observe_ticket_from_body(&body)
        .map_err(|err| anyhow::anyhow!(err))?;
    let decision =
        runtime.admit_body_with_canon_tag(1_800_000_000_000, &fixture.constants.ip, &body, 0);
    let accepted = matches!(decision, AdmissionDecision::Accepted { .. });
    if !accepted {
        anyhow::bail!("runtime smoke admission was rejected: {decision:?}");
    }

    let accepted_tags = BTreeSet::from([0]);
    let committed =
        runtime.commit_next_block_for_current_c(&block_path, 1_800_000_000_123, &accepted_tags)?;
    let recovered = FileBlockStore::recover(&block_path)?;
    let replay = replay_blocks(recovered.blocks())?;
    let runtime_head = runtime
        .current_c()
        .ok_or_else(|| anyhow::anyhow!("runtime head is not set after commit"))?
        .to_string();

    Ok(RuntimeSmokeOutput {
        ok: true,
        accepted,
        height: committed.block.height,
        prev_c: committed.block.prev_c,
        c: committed.block.c,
        replay_height: replay.height,
        replay_latest_c: replay.latest_c,
        runtime_head,
        dropped_stale_shares: committed.dropped_stale_shares,
    })
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
