// Standalone boole-miner binary. Thin wrapper around `boole_miner::cli` —
// the real CLI surface lives in the library so `boole-cli mine ...` can
// drive the same code paths without reparsing arguments.
use clap::Parser;

use boole_miner::cli::{run_mine, MineCommand};

#[derive(Parser)]
#[command(name = "boole-miner", about = "Boole-v3.1.1 standalone miner")]
struct Cli {
    #[command(subcommand)]
    command: MineCommand,
}

fn main() {
    let cli = Cli::parse();
    if let Err(err) = run_mine(cli.command) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
