use anyhow::{Context, Result};
use boole_devnet_driver::{
    driver::{Driver, StallKind, StepOutcome},
    engine::{Engine, RpcEngine},
    l1::{JournalSource, RpcL1},
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
    state_dir: Option<PathBuf>,
    #[arg(long)]
    /// Request an operator-approved checkpoint move (currently refuses immutable evidence).
    recover_engine_tail: bool,
    #[arg(long)]
    repair_journals: bool,
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
    let forks: Option<boole_derivation::m3::ForkTable> = if config.m3.is_some() {
        Some(match args.m3_forks.as_ref() {
            Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
            None => boole_derivation::m3::ForkTable::genesis(&config)?,
        })
    } else {
        None
    };
    let root = args
        .state_dir
        .context("M4 requires --state-dir for the locked input journal")?;
    let network = boole_devnet_archive::journal::NetworkIdentity::new(
        &config,
        forks.as_ref(),
        engine.header(0)?.hash,
        l2_gas_limit,
    )?;
    let l1 = JournalSource::open(l1, &root, network, args.repair_journals)?;
    let root = l1.journal.borrow().root().to_path_buf();
    let checkpoint = args
        .m2_journal
        .unwrap_or_else(|| root.join("checkpoint.json"));
    let evidence = args.evidence.unwrap_or_else(|| root.join("evidence.jsonl"));
    for path in [&checkpoint, &evidence] {
        anyhow::ensure!(
            path.parent()
                .map(std::fs::canonicalize)
                .transpose()?
                .as_ref()
                == Some(&root),
            "M4 output must be inside --state-dir"
        );
    }
    let mut cache_guard = None;
    let mut driver = if config.m3.is_some() {
        use boole_gate_supervisor::{
            codec::Codec,
            process::{Envelope, Worker},
            supervisor::{Host, Mode, Supervisor},
        };
        let forks = forks.context("M3 forks")?;
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
        let cache = args.gate_cache.unwrap_or_else(|| root.join("gate-cache"));
        anyhow::ensure!(
            cache == root.join("gate-cache"),
            "--gate-cache must be state-dir/gate-cache"
        );
        let pins: Vec<_> = gates.iter().map(|w| w.binary_sha256).collect();
        l1.journal.borrow().cleanup(&pins, Some(&checkpoint))?;
        let guard = boole_devnet_archive::state_dir::acquire(&cache)?;
        boole_devnet_archive::boot::cleanup(&cache, &guard, &pins, None)?;
        cache_guard = Some(guard);
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
        l1.journal.borrow().cleanup(&[], Some(&checkpoint))?;
        Driver::with_config(l1, engine, config, l2_gas_limit)?
    };
    let _cache_guard = cache_guard;
    driver.set_journal(&checkpoint)?;
    driver.set_evidence_repair(&evidence, args.repair_journals)?;
    driver.set_recover_engine_tail(args.recover_engine_tail);
    driver.resume()?;
    let mut h = driver
        .engine
        .head()?
        .number
        .checked_add(1)
        .context("head height overflow")?;
    while h <= args.until {
        let outcome = match driver.step(h) {
            Ok(outcome) => outcome,
            Err(stall)
                if matches!(
                    stall.kind,
                    StallKind::VerdictPending { .. } | StallKind::Availability { .. }
                ) =>
            {
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
