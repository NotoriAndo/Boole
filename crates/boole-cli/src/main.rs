use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "boole")]
#[command(about = "Boole native CLI migration spike")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print CLI version information.
    Version {
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Chain inspection commands.
    Chain {
        #[command(subcommand)]
        command: ChainCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ChainCommand {
    /// Replay a protocol fixture or block log and print final state.
    Replay {
        /// Path to replay fixture JSON.
        #[arg(long)]
        fixture: std::path::PathBuf,
        /// Emit JSON output.
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = run(cli);
    if let Err(err) = result {
        eprintln!(
            "{}",
            serde_json::json!({
                "ok": false,
                "error": "runtime",
                "message": err.to_string()
            })
        );
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Some(Command::Version { json }) => print_version(json),
        Some(Command::Chain { command }) => match command {
            ChainCommand::Replay { fixture, json } => replay_fixture(&fixture, json),
        },
        None => print_version(false),
    }
}

fn print_version(json: bool) -> anyhow::Result<()> {
    if json {
        println!(
            "{}",
            serde_json::json!({ "ok": true, "name": "boole", "version": env!("CARGO_PKG_VERSION") })
        );
    } else {
        println!("boole {}", env!("CARGO_PKG_VERSION"));
    }
    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct ReplayFixture {
    blocks: Vec<boole_core::PersistedBlock>,
}

fn replay_fixture(path: &std::path::Path, json: bool) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(path)?;
    let fixture: ReplayFixture = serde_json::from_str(&raw)?;
    let replay = boole_core::replay_blocks(&fixture.blocks)?;
    if json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "latestC": replay.latest_c,
                "height": replay.height,
                "balances": replay.balances.into_iter().map(|(pk, amount)| (pk, amount.to_string())).collect::<std::collections::BTreeMap<_, _>>()
            })
        );
    } else {
        println!("latestC={} height={}", replay.latest_c, replay.height);
    }
    Ok(())
}
