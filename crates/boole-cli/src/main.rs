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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Version { json }) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({ "ok": true, "name": "boole", "version": env!("CARGO_PKG_VERSION") })
                );
            } else {
                println!("boole {}", env!("CARGO_PKG_VERSION"));
            }
        }
        None => {
            println!("boole {}", env!("CARGO_PKG_VERSION"));
        }
    }
    Ok(())
}
