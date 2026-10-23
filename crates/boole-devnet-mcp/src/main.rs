use anyhow::{ensure, Context, Result};
use boole_derivation::m2::Config;
use boole_devnet_driver::{
    engine::Rpc,
    l1::{JournalSource, RpcL1},
};
use boole_devnet_mcp::{
    client::{fixed, l2_gas_limit, Client, ClientOptions, RpcL2, RpcPoster},
    codec::Codec,
    hook::Precheck,
    protocol,
};
use clap::Parser;
use std::{
    io::{self, BufReader, Write},
    path::PathBuf,
    sync::mpsc,
    thread,
    time::Duration,
};

#[derive(Parser)]
#[command(about = "Closed-local devnet miner MCP over stdio; uses unlocked development accounts")]
struct Args {
    #[arg(long)]
    devnet: bool,
    #[arg(long)]
    l1_rpc: String,
    #[arg(long)]
    node_rpc: String,
    #[arg(long)]
    account: String,
    #[arg(long)]
    state_dir: PathBuf,
    #[arg(long)]
    repair_journals: bool,
    #[arg(long)]
    m2_config: Option<PathBuf>,
    #[arg(long)]
    m3_forks: Option<PathBuf>,
    #[arg(long, requires_all=["m3_gates","gate_artifacts"])]
    verify_own: bool,
    #[arg(long, requires = "verify_own")]
    m3_gates: Option<PathBuf>,
    #[arg(long, requires = "verify_own")]
    gate_artifacts: Option<PathBuf>,
    #[arg(long)]
    zstd: Option<PathBuf>,
    #[arg(long, requires = "precheck_sha256")]
    precheck: Option<PathBuf>,
    #[arg(long, requires = "precheck")]
    precheck_sha256: Option<String>,
    #[arg(long, requires = "precheck")]
    precheck_gate_rule_digest: Option<String>,
}
fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        args.devnet,
        "--devnet is required; no production posting mode"
    );
    let c: Config = match args.m2_config {
        Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
        None => Config::devnet(),
    };
    let account = fixed(&args.account)?;
    let codec = args.zstd.map(Codec::new).transpose()?;
    let hook = args
        .precheck
        .map(|executable| -> Result<_> {
            Ok(Precheck {
                executable,
                gate_rule_digest: args
                    .precheck_gate_rule_digest
                    .as_deref()
                    .map(fixed)
                    .transpose()?,
                sha256: fixed(
                    args.precheck_sha256
                        .as_deref()
                        .context("pre-check pin missing")?,
                )?,
                timeout: Duration::from_secs(120),
            })
        })
        .transpose()?;
    let forks: Option<boole_derivation::m3::ForkTable> = args
        .m3_forks
        .map(|p| -> Result<_> { Ok(serde_json::from_slice(&std::fs::read(p)?)?) })
        .transpose()?;
    let gas = l2_gas_limit(&args.node_rpc)?;
    let genesis = Rpc::new(&args.node_rpc)?
        .call("eth_getBlockByNumber", serde_json::json!(["0x0", false]))?;
    let network = boole_devnet_archive::journal::NetworkIdentity::new(
        &c,
        forks.as_ref(),
        genesis["hash"].as_str().context("L2 genesis hash")?.into(),
        gas,
    )?;
    let source = JournalSource::open(
        RpcL1::new_m2(&args.l1_rpc)?,
        &args.state_dir,
        network,
        args.repair_journals,
    )?;
    let own = if args.verify_own {
        let workers: Vec<boole_gate_supervisor::process::Worker> = serde_json::from_slice(
            &std::fs::read(args.m3_gates.context("--verify-own requires --m3-gates")?)?,
        )?;
        let pins: Vec<_> = workers.iter().map(|w| w.binary_sha256).collect();
        source.journal.borrow().cleanup(&pins, None)?;
        Some(boole_devnet_mcp::own::OwnVerification {
            gates: workers
                .into_iter()
                .map(|worker| {
                    Box::new(boole_devnet_mcp::own::PinnedOwnGate {
                        worker,
                        envelope: boole_gate_supervisor::process::Envelope::default(),
                    }) as Box<dyn boole_devnet_mcp::own::OwnGate>
                })
                .collect(),
            artifacts: args
                .gate_artifacts
                .context("--verify-own requires --gate-artifacts")?,
        })
    } else {
        source.journal.borrow().cleanup(&[], None)?;
        None
    };
    let mut client = Client::new(
        source,
        RpcPoster::new(&args.l1_rpc, account)?,
        c,
        account,
        args.state_dir,
        ClientOptions {
            l2: Some(Box::new(RpcL2::new(&args.node_rpc)?)),
            l2_gas_limit: gas,
            codec,
            hook,
            forks,
            own,
        },
    )?;
    let (sender, receiver) = mpsc::sync_channel(8);
    thread::spawn(move || {
        let mut reader = BufReader::new(io::stdin());
        loop {
            match protocol::read_frame(&mut reader) {
                Ok(Some(message)) => {
                    if sender.send(Ok(message)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(_) => {
                    let _ = sender.send(Err(()));
                    break;
                }
            }
        }
    });
    let mut stdout = io::stdout().lock();
    loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(message)) => {
                if let Some(response) =
                    protocol::dispatch(&message, &mut |name, args| client.call(name, args))
                {
                    writeln!(stdout, "{response}")?;
                    stdout.flush()?;
                }
            }
            Ok(Err(())) => anyhow::bail!("invalid stdio frame"),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        // A polling worker remains active even when the AI is not sending calls.
        // RPC write errors are retained as sanitized per-submission progress.
        if client.tick().is_err() {
            eprintln!("devnet progress paused; query status/submission or verify the local node");
        }
    }
    Ok(())
}
