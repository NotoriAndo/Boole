use anyhow::{ensure, Result};
fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("worker") {
        return boole_gate_supervisor::process::worker_main();
    }
    ensure!(
        std::env::args().nth(1).as_deref() == Some("check-install"),
        "use worker or check-install CONFIG GATES FORKS CACHE ARTIFACTS"
    );
    let args: Vec<_> = std::env::args()
        .skip(2)
        .map(std::path::PathBuf::from)
        .collect();
    ensure!(
        args.len() == 5,
        "check-install CONFIG GATES FORKS CACHE ARTIFACTS"
    );
    let c: boole_derivation::m2::Config = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let gates = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let forks = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let _supervisor = boole_gate_supervisor::supervisor::Supervisor::new(
        &c,
        &forks,
        boole_gate_supervisor::supervisor::Mode::Verify,
        gates,
        Some(boole_gate_supervisor::codec::Codec::portable_stub()),
        &args[3],
        &args[3],
        &args[4],
        boole_gate_supervisor::supervisor::Host::local()?,
        boole_gate_supervisor::process::Envelope::default(),
    )?;
    println!(
        "{}",
        serde_json::json!({"ok":true,"config_digest":boole_derivation::attributes::hex_string(&c.digest()),"slot_table_digest":boole_derivation::attributes::hex_string(&c.slot_table_digest())})
    );
    Ok(())
}
