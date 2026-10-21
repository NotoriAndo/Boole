use anyhow::{Context, Result};
use boole_devnet_driver::{
    driver::{Driver, StepOutcome},
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
}
fn main() -> Result<()> {
    let args = Args::parse();
    let l1 = RpcL1::new(&args.l1_rpc)?;
    let engine = RpcEngine::new(&args.engine_rpc, &args.node_rpc, &args.jwt_secret)?;
    let mut driver = Driver::new(l1, engine);
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
        match driver.step(h)? {
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
