use anyhow::{ensure, Context, Result};
use boole_derivation::m2::Config;
use boole_devnet_driver::l1::RpcL1;
use boole_devnet_mcp::{
    client::{fixed, Client, RpcPoster},
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
    account: String,
    #[arg(long)]
    state_dir: PathBuf,
    #[arg(long)]
    m2_config: Option<PathBuf>,
    #[arg(long)]
    zstd: Option<PathBuf>,
    #[arg(long, requires = "precheck_sha256")]
    precheck: Option<PathBuf>,
    #[arg(long, requires = "precheck")]
    precheck_sha256: Option<String>,
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
                sha256: fixed(
                    args.precheck_sha256
                        .as_deref()
                        .context("pre-check pin missing")?,
                )?,
                timeout: Duration::from_secs(120),
            })
        })
        .transpose()?;
    let mut client = Client::new(
        RpcL1::new_m2(&args.l1_rpc)?,
        RpcPoster::new(&args.l1_rpc, account)?,
        c,
        account,
        args.state_dir,
        codec,
        hook,
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
