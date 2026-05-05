use boole_node::local_node::{serve_local_node, LocalNodeConfig};
use boole_node::runtime_smoke::{
    run_runtime_smoke, run_runtime_smoke_scenario_file, RuntimeSmokeInput,
};
use std::net::TcpListener;

fn main() -> anyhow::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("runtime-smoke") => run_runtime_smoke_command(args),
        Some("run-local") => run_local_command(args),
        Some("--help") | Some("-h") | None => {
            print_help();
            Ok(())
        }
        Some(other) => anyhow::bail!("unknown command {other}"),
    }
}

fn run_runtime_smoke_command(mut args: Vec<String>) -> anyhow::Result<()> {
    args.remove(0);
    let fixture_path = take_optional_flag_value(&mut args, "--fixture")?;
    let scenario_path = take_optional_flag_value(&mut args, "--scenario")?;
    let block_path = take_flag_value(&mut args, "--block-store")?;
    if fixture_path.is_some() == scenario_path.is_some() {
        anyhow::bail!("provide exactly one of --fixture or --scenario");
    }
    if !args.is_empty() {
        anyhow::bail!("unexpected args: {}", args.join(" "));
    }
    let output = if let Some(scenario_path) = scenario_path {
        run_runtime_smoke_scenario_file(scenario_path.into(), block_path.into())?
    } else {
        run_runtime_smoke(RuntimeSmokeInput {
            fixture_path: fixture_path.expect("checked fixture path").into(),
            block_path: block_path.into(),
        })?
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn run_local_command(mut args: Vec<String>) -> anyhow::Result<()> {
    args.remove(0);
    let addr = take_optional_flag_value(&mut args, "--addr")?
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let scenario_path = take_optional_flag_value(&mut args, "--scenario")?
        .unwrap_or_else(|| "fixtures/protocol/runtime-smoke/v1.json".to_string());
    let block_path = take_optional_flag_value(&mut args, "--block-store")?
        .unwrap_or_else(|| "/tmp/boole-node-local.ndjson".to_string());
    let max_requests = take_optional_flag_value(&mut args, "--max-requests")?
        .map(|value| value.parse::<usize>())
        .transpose()?;
    if !args.is_empty() {
        anyhow::bail!("unexpected args: {}", args.join(" "));
    }
    let listener = TcpListener::bind(&addr)?;
    let bound = listener.local_addr()?;
    eprintln!("boole-node local listening on http://{bound}");
    eprintln!("boole-node local blockStore={block_path}");
    serve_local_node(
        listener,
        LocalNodeConfig {
            scenario_path: scenario_path.into(),
            block_path: block_path.into(),
            max_requests,
        },
    )
}

fn print_help() {
    println!(
        "boole-node\n\ncommands:\n  runtime-smoke --scenario <path>|--fixture <path> --block-store <path>\n  run-local [--addr 127.0.0.1:8080] [--scenario <path>] [--block-store <path>] [--max-requests <n>]"
    );
}

fn take_optional_flag_value(args: &mut Vec<String>, flag: &str) -> anyhow::Result<Option<String>> {
    let Some(index) = args.iter().position(|arg| arg == flag) else {
        return Ok(None);
    };
    args.remove(index);
    if index >= args.len() {
        anyhow::bail!("missing value for flag {flag}");
    }
    Ok(Some(args.remove(index)))
}

fn take_flag_value(args: &mut Vec<String>, flag: &str) -> anyhow::Result<String> {
    let Some(index) = args.iter().position(|arg| arg == flag) else {
        anyhow::bail!("missing required flag {flag}");
    };
    args.remove(index);
    if index >= args.len() {
        anyhow::bail!("missing value for flag {flag}");
    }
    Ok(args.remove(index))
}
