use boole_node::runtime_smoke::{run_runtime_smoke, RuntimeSmokeInput};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some("runtime-smoke") {
        println!("boole-node migration spike");
        return Ok(());
    }
    args.remove(0);
    let fixture_path = take_flag_value(&mut args, "--fixture")?;
    let block_path = take_flag_value(&mut args, "--block-store")?;
    if !args.is_empty() {
        anyhow::bail!("unexpected args: {}", args.join(" "));
    }
    let output = run_runtime_smoke(RuntimeSmokeInput {
        fixture_path: fixture_path.into(),
        block_path: block_path.into(),
    })?;
    println!("{}", serde_json::to_string(&output)?);
    Ok(())
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
