use anyhow::{ensure, Context, Result};
use boole_derivation::{m2::Config, m3::ForkTable};
use boole_devnet_archive::{
    durability::PrivateTempDir,
    journal::{read_bound, NetworkIdentity, ReadOnlyJournal},
};
use boole_devnet_audit::{rpc::RpcNode, ArchiveInputs, ExpectedHeads, Inputs, JournalInputs, Node};
use boole_gate_supervisor::{
    codec::Codec,
    process::{Envelope, Worker},
    supervisor::{Host, Mode, Supervisor},
};
use clap::{ArgGroup, Parser};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
#[derive(Parser)]
#[command(about="Unsigned read-only devnet audit; replay uses a fresh private Engine",group(ArgGroup::new("source").required(true).args(["journal","archive"])))]
struct Args {
    #[arg(long)]
    journal: Option<PathBuf>,
    #[arg(long, requires = "l1_rpc")]
    archive: Vec<String>,
    #[arg(long)]
    l1_rpc: Option<String>,
    #[arg(long)]
    node_rpc: String,
    #[arg(long)]
    m2_config: PathBuf,
    #[arg(long)]
    m3_forks: Option<PathBuf>,
    #[arg(long)]
    m3_gates: Option<PathBuf>,
    #[arg(long)]
    gate_artifacts: Option<PathBuf>,
    #[arg(long)]
    zstd: Option<PathBuf>,
    #[arg(long)]
    expected_l1_number: u64,
    #[arg(long)]
    expected_l1_hash: String,
    #[arg(long, requires = "expected_l2_hash")]
    expected_l2_number: Option<u64>,
    #[arg(long, requires = "expected_l2_number")]
    expected_l2_hash: Option<String>,
    #[arg(long,default_value="pure",value_parser=["pure","replay"])]
    mode: String,
    #[arg(long)]
    op_reth: Option<PathBuf>,
    #[arg(long)]
    l2_genesis: Option<PathBuf>,
}
fn json_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_slice(
        &read_bound(path, boole_devnet_archive::input::ROW_MAX)?.0,
    )?)
}
fn supervisor(
    args: &Args,
    c: &Config,
    forks: Option<&ForkTable>,
    root: &Path,
) -> Result<Option<Supervisor>> {
    if c.m3.is_none() {
        return Ok(None);
    }
    let workers: Vec<Worker> = json_file(
        args.m3_gates
            .as_deref()
            .context("M3 requires pinned gates")?,
    )?;
    let codec = if let Some(path) = &args.zstd {
        Codec::new(path.clone())?
    } else {
        Codec::portable_stub()
    };
    Ok(Some(Supervisor::new(
        c,
        forks.context("M3 forks")?,
        Mode::Verify,
        workers,
        Some(codec),
        &root.join("cache"),
        root,
        args.gate_artifacts
            .as_deref()
            .context("M3 requires public artifacts")?,
        Host::local()?,
        Envelope::default(),
    )?))
}
fn prepare(
    args: &Args,
    c: &Config,
    forks: Option<&ForkTable>,
    node: &mut RpcNode,
    root: &Path,
) -> Result<Box<dyn Inputs>> {
    let genesis = node.header(0)?;
    let network = NetworkIdentity::new(c, forks, genesis.hash, genesis.gas_limit)?;
    if let Some(path) = &args.journal {
        return Ok(Box::new(JournalInputs(ReadOnlyJournal::open(
            path, &network,
        )?)));
    }
    let source = boole_devnet_driver::l1::JournalSource::open_archive(
        boole_devnet_driver::l1::RpcL1::new_m2(
            args.l1_rpc.as_deref().context("own L1 tip endpoint")?,
        )?,
        &root.join("inputs"),
        network.clone(),
        false,
        boole_devnet_archive::client::ArchiveClient::http(args.archive.clone())?,
    )?;
    drop(source);
    Ok(Box::new(ArchiveInputs(ReadOnlyJournal::open(
        &root.join("inputs"),
        &network,
    )?)))
}
fn run(args: Args) -> Result<Value> {
    let config: Config = json_file(&args.m2_config)?;
    let forks = if config.m3.is_some() {
        Some(match args.m3_forks.as_deref() {
            Some(path) => json_file(path)?,
            None => ForkTable::genesis(&config)?,
        })
    } else {
        ensure!(
            args.m3_forks.is_none() && args.m3_gates.is_none(),
            "M3 options without M3"
        );
        None
    };
    ensure!(
        boole_derivation::attributes::hex_bytes(&args.expected_l1_hash, "independent L1 head")?
            .len()
            == 32,
        "L1 head width"
    );
    let expected = ExpectedHeads {
        l1_number: args.expected_l1_number,
        l1_hash: args.expected_l1_hash.clone(),
        l2: args.expected_l2_number.zip(args.expected_l2_hash.clone()),
    };
    if let Some(source) = args.journal.as_ref() {
        ensure!(
            !std::env::temp_dir()
                .canonicalize()?
                .starts_with(source.canonicalize()?),
            "audit TMPDIR must be outside the audited source"
        );
    }
    let work = PrivateTempDir::new("devnet-audit")?;
    let mut node = match RpcNode::new(&args.node_rpc) {
        Ok(node) => node,
        Err(error) => {
            if let Some(stop) = error.downcast_ref::<boole_devnet_audit::resource::ResourceStop>() {
                let mut receipt =
                    boole_devnet_audit::resource_receipt(&config, forks.as_ref(), 0, "node", *stop);
                receipt["mode"] = json!(args.mode);
                return Ok(receipt);
            } else {
                return Err(error);
            }
        }
    };
    let mut inputs = match prepare(&args, &config, forks.as_ref(), &mut node, work.path()) {
        Ok(input) => input,
        Err(error) => {
            if let Some(stop) = error.downcast_ref::<boole_devnet_audit::resource::ResourceStop>() {
                let mut receipt = boole_devnet_audit::resource_receipt(
                    &config,
                    forks.as_ref(),
                    0,
                    "input",
                    *stop,
                );
                receipt["mode"] = json!(args.mode);
                return Ok(receipt);
            }
            let height = match error.downcast_ref::<boole_devnet_archive::error::Error>() {
                Some(
                    boole_devnet_archive::error::Error::Availability { height, .. }
                    | boole_devnet_archive::error::Error::Integrity { height, .. },
                ) => *height,
                _ => 0,
            };
            let mut r =
                boole_devnet_audit::failed_receipt(&config, forks.as_ref(), height, "input");
            r["mode"] = json!(args.mode);
            return Ok(r);
        }
    };
    let pure_work = PrivateTempDir::new_in(work.path(), "pure")?;
    let mut gate = match supervisor(&args, &config, forks.as_ref(), pure_work.path()) {
        Ok(gate) => gate,
        Err(_) => {
            let mut r = boole_devnet_audit::failed_receipt(&config, forks.as_ref(), 0, "verdict");
            r["mode"] = json!(args.mode);
            return Ok(r);
        }
    };
    let mut receipt = boole_devnet_audit::audit_in(
        inputs.as_mut(),
        &mut node,
        &config,
        forks.as_ref(),
        gate.as_mut(),
        &expected,
        pure_work.path(),
    );
    receipt["mode"] = json!(args.mode);
    if args.mode == "replay" && boole_devnet_audit::succeeded(&receipt) {
        receipt["replayed_blocks"] = json!("0");
        let started = (|| -> Result<_> {
            let replay_work = PrivateTempDir::new_in(work.path(), "replay")?;
            let replay_gate = supervisor(&args, &config, forks.as_ref(), replay_work.path())?;
            let private = boole_devnet_audit::replay::PrivateEngine::start(
                args.op_reth
                    .as_deref()
                    .context("replay requires --op-reth")?,
                args.l2_genesis
                    .as_deref()
                    .context("replay requires --l2-genesis")?,
                replay_work.path(),
            )?;
            Ok((private, replay_gate, replay_work))
        })();
        match started {
            Ok((mut private, replay_gate, replay_work)) => {
                if let Err(error) = boole_devnet_audit::replay::replay(
                    inputs.as_mut(),
                    &mut node,
                    config.clone(),
                    forks.clone(),
                    replay_gate,
                    &private,
                    &mut receipt,
                ) {
                    boole_devnet_audit::replay::report_failure(&mut receipt, &error, "unavailable");
                }
                let stopped = private.stop().is_ok();
                let issues = private.log_issues().unwrap_or(1);
                boole_devnet_audit::replay::report_cleanup(&mut receipt, stopped, issues);
                drop(private);
                drop(replay_work);
            }
            Err(error) => {
                boole_devnet_audit::replay::report_failure(&mut receipt, &error, "unavailable")
            }
        }
    }
    boole_devnet_audit::recheck_inputs(&mut receipt, inputs.as_ref());
    Ok(receipt)
}
fn main() {
    match run(Args::parse()) {
        Ok(receipt) => {
            boole_devnet_audit::emit_receipt(&receipt);
        }
        Err(_) => {
            eprintln!("audit setup unavailable; sources preserved");
            std::process::exit(2)
        }
    }
}
