// Standalone boole-miner binary. Thin wrapper around `boole_miner::cli` —
// the real CLI surface lives in the library so `boole-cli mine ...` can
// drive the same code paths without reparsing arguments.
use clap::Parser;

use boole_miner::cli::{run_mine, MineCommand};
use boole_miner::PaidApiPolicyError;

#[derive(Parser)]
#[command(name = "boole-miner", about = "Boole-v3.1.1 standalone miner")]
struct Cli {
    #[command(subcommand)]
    command: MineCommand,
}

fn main() {
    // P0.5 slice 65 — install the telemetry subscriber before any work.
    // Default-silent unless RUST_LOG opts in, so the miner's stdout/stderr
    // envelope contract is unchanged.
    boole_core::telemetry::init(boole_core::telemetry::BinaryName::Miner);
    let cli = Cli::parse();
    if let Err(err) = run_mine(cli.command) {
        // P2.4 — typed paid-API refusal carries its own exit code and a
        // unified-envelope JSON payload. Print the envelope verbatim to
        // stderr and use the documented exit code so automation can
        // distinguish "we declined to spend money" (3) from a generic
        // configuration error (1).
        if let Some(refusal) = err.downcast_ref::<PaidApiPolicyError>() {
            eprintln!("{}", refusal.envelope);
            std::process::exit(refusal.exit_code);
        }
        eprintln!("{err}");
        std::process::exit(1);
    }
}
