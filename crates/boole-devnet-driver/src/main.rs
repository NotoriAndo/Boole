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
    /// Pinned loopback archive sources, tried in order under FETCH.
    #[arg(long)]
    archive: Vec<String>,
    /// Serve this verify node's public finalized archive (no pruning).
    #[arg(long)]
    archive_dir: Option<PathBuf>,
    #[arg(long, requires = "archive_dir", default_value = "127.0.0.1:0")]
    archive_bind: std::net::SocketAddr,
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
    boole_devnet_archive::fault::validate_environment()?;
    let args = Args::parse();
    if args.archive_dir.is_some() && !args.archive_bind.ip().is_loopback() {
        return Err(boole_devnet_archive::error::startup(
            21,
            "archive bind must be loopback",
        ));
    }
    let config: boole_derivation::m2::Config = match args.m2_config {
        Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
        None => boole_derivation::m2::Config::devnet(),
    };
    eprintln!(
        "slot_table_digest={}",
        boole_derivation::attributes::hex_string(&config.slot_table_digest())
    );
    anyhow::ensure!(
        (args.archive.is_empty() && args.archive_dir.is_none()) || args.mode == "verify",
        "M4b1 archive requires verify mode"
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
    let l1 = if args.archive.is_empty() {
        JournalSource::open(l1, &root, network.clone(), args.repair_journals)?
    } else {
        JournalSource::open_archive(
            l1,
            &root,
            network.clone(),
            args.repair_journals,
            boole_devnet_archive::client::ArchiveClient::http(args.archive)?,
        )?
    };
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
    let mut capture_paths = None;
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
        capture_paths = Some(boole_devnet_driver::driver::archive::CapturePaths {
            cache: cache.clone(),
            artifacts: artifacts.clone(),
            scratch: cache.clone(),
        });
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
    let mut archive_sink = None;
    let mut archive_server = None;
    if let Some(path) = args.archive_dir {
        use boole_devnet_archive::{
            archive::{ArchiveIdentity, ArchiveProfile, ArchiveStore},
            http::ArchiveServer,
        };
        use std::sync::{Arc, Mutex};
        let anchor = driver.l1.journal.borrow().row(0)?.block_hash;
        let store = Arc::new(Mutex::new(ArchiveStore::open(
            &path,
            ArchiveProfile {
                identity: ArchiveIdentity {
                    network,
                    l1_anchor_number: 0,
                    l1_anchor_hash: anchor,
                },
                bind: args.archive_bind,
                prune_before: None,
            },
            args.repair_journals,
        )?));
        let server = ArchiveServer::start(Arc::clone(&store), args.archive_bind)?;
        eprintln!("archive_url={}", server.url());
        let mut sink = boole_devnet_driver::driver::archive::ArchiveSink::new(
            &mut driver,
            store,
            capture_paths,
        )?;
        sync_archive(&mut sink, &mut driver)?;
        archive_sink = Some(sink);
        archive_server = Some(server);
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
                if let Some(sink) = &mut archive_sink {
                    sync_archive(sink, &mut driver)?;
                }
                println!("derived height {h}");
                h = h.checked_add(1).context("height overflow")?;
            }
        }
    }
    // An archive node remains available after the requested finalized range.
    if archive_server.is_some() {
        loop {
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    Ok(())
}
fn sync_archive(
    sink: &mut boole_devnet_driver::driver::archive::ArchiveSink,
    driver: &mut Driver<JournalSource<RpcL1>, RpcEngine>,
) -> Result<()> {
    loop {
        match sink.synchronize(driver) {
            Ok(()) => return Ok(()),
            Err(error)
                if matches!(
                    error.downcast_ref::<boole_devnet_archive::error::Error>(),
                    Some(boole_devnet_archive::error::Error::Availability { .. })
                ) =>
            {
                eprintln!("{error}; retrying archive publication only");
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(error) => return Err(error),
        }
    }
}
