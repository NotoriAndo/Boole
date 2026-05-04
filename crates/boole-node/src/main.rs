use boole_node::runtime_smoke::{
    run_runtime_smoke, run_runtime_smoke_scenario_file, RuntimeSmokeInput,
};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) != Some("runtime-smoke") {
        println!("boole-node migration spike");
        return Ok(());
    }
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
