use anyhow::{Context, Result};
use boole_devnet_driver::{
    driver::{Driver, StallKind, StepOutcome},
    engine::{Engine, RpcEngine},
    l1::RpcL1,
};
use clap::Parser;
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(about = "Finalized-only loopback Boole devnet derivation")]
struct Args {
    #[arg(long)]
    l1_rpc: String,
    #[arg(long)]
    engine_rpc: String,
    #[arg(long)]
    node_rpc: String,
    #[arg(long)]
    jwt_secret: PathBuf,
    #[arg(long)]
    until: u64,
    #[arg(long)]
    evidence: Option<PathBuf>,
    #[arg(long)]
    m2_journal: Option<PathBuf>,
    #[arg(long)]
    m2_config: Option<PathBuf>,
    #[arg(long)]
    m3_gates: Option<PathBuf>,
    #[arg(long)]
    m3_forks: Option<PathBuf>,
    #[arg(long)]
    gate_cache: Option<PathBuf>,
    #[arg(long)]
    gate_artifacts: Option<PathBuf>,
    #[arg(long)]
    zstd: Option<PathBuf>,
    #[arg(long,default_value="verify",value_parser=["verify","follow"])]
    mode: String,
    #[arg(long)]
    follow_from: Option<u64>,
    #[arg(long)]
    follow_through: Option<u64>,
    #[arg(long)]
    follow_canonical: Option<PathBuf>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    let config: boole_derivation::m2::Config = match args.m2_config {
        Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
        None => boole_derivation::m2::Config::devnet(),
    };
    eprintln!(
        "slot_table_digest={}",
        boole_derivation::attributes::hex_string(&config.slot_table_digest())
    );
    let l1 = RpcL1::new_m2(&args.l1_rpc)?;
    let mut engine = RpcEngine::new(&args.engine_rpc, &args.node_rpc, &args.jwt_secret)?;
    let l2_gas_limit = engine.head()?.gas_limit;
    let mut driver = if config.m3.is_some() {
        use boole_gate_supervisor::{
            codec::Codec,
            process::{Envelope, Worker},
            supervisor::{Host, Mode, Supervisor},
        };
        let forks = match args.m3_forks {
            Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
            None => boole_derivation::m3::ForkTable::genesis(&config)?,
        };
        let mode = if args.mode == "follow" {
            Mode::Follow {
                from: args
                    .follow_from
                    .context("follow mode requires --follow-from")?,
                through: args
                    .follow_through
                    .context("follow mode requires --follow-through")?,
            }
        } else {
            Mode::Verify
        };
        let gates: Vec<Worker> = match args.m3_gates {
            Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
            None => vec![],
        };
        let cache = args.gate_cache.context("M3 requires --gate-cache")?;
        let artifacts = args
            .gate_artifacts
            .unwrap_or_else(|| cache.join("artifacts"));
        let codec = match args.zstd {
            Some(path) => Codec::new(path)?,
            None => Codec::portable_stub(),
        };
        let supervisor = Supervisor::new(
            &config,
            &forks,
            mode,
            gates,
            Some(codec),
            &cache,
            &cache,
            &artifacts,
            Host::local()?,
            Envelope::default(),
        )?;
        let mut driver = Driver::with_m3(l1, engine, config, l2_gas_limit, supervisor, forks)?;
        if let Some(path) = args.follow_canonical {
            let rows: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
            for row in rows
                .as_array()
                .context("canonical follow blocks must be an array")?
            {
                let l1 = serde_json::from_value(row["l1"].clone())?;
                let txs = serde_json::from_value::<Vec<String>>(row["transactions"].clone())?;
                driver.provide_follow_block(&l1, &txs)?;
            }
        }
        driver
    } else {
        anyhow::ensure!(
            args.mode == "verify"
                && args.m3_gates.is_none()
                && args.m3_forks.is_none()
                && args.follow_canonical.is_none(),
            "M3 options require an M3 profile"
        );
        Driver::with_config(l1, engine, config, l2_gas_limit)?
    };
    if let Some(path) = args.m2_journal {
        driver.set_journal(&path)?;
    }
    if let Some(path) = args.evidence {
        driver.set_evidence(&path)?;
    }
    let mut h = driver
        .engine
        .head()?
        .number
        .checked_add(1)
        .context("head height overflow")?;
    while h <= args.until {
        let outcome = match driver.step(h) {
            Ok(outcome) => outcome,
            Err(stall) if matches!(stall.kind, StallKind::VerdictPending { .. }) => {
                eprintln!("{stall}; retrying the same height");
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            Err(stall) => return Err(stall.into()),
        };
        match outcome {
            StepOutcome::WaitingForFinality => {
                eprintln!("waiting for finality at {h}");
                std::thread::sleep(Duration::from_millis(500));
            }
            StepOutcome::Derived(_) => {
                println!("derived height {h}");
                h = h.checked_add(1).context("height overflow")?;
            }
        }
    }
    Ok(())
}
