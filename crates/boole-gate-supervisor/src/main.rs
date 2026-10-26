use anyhow::{ensure, Result};
fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref() == Some("worker") {
        return boole_gate_supervisor::process::worker_main();
    }
    if matches!(
        std::env::args().nth(1).as_deref(),
        Some("real-job-v1" | "real-file-job-v1" | "real-transport-job-v2")
    ) {
        return real_job();
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
    let _supervisor = boole_gate_supervisor::real_supervisor::Supervisor::new_releases(
        &c,
        &forks,
        boole_gate_supervisor::real_supervisor::Mode::Verify,
        gates,
        Some(boole_gate_supervisor::codec::Codec::portable_stub()),
        &args[3],
        &args[3],
        &args[4],
        boole_gate_supervisor::real_supervisor::Host::local()?,
        boole_gate_supervisor::process::Envelope::default(),
    )?;
    println!(
        "{}",
        serde_json::json!({"ok":true,"config_digest":boole_derivation::attributes::hex_string(&c.digest()),"slot_table_digest":boole_derivation::attributes::hex_string(&c.slot_table_digest())})
    );
    Ok(())
}

fn real_job() -> Result<()> {
    use boole_gate_supervisor::{
        codec::read_bounded, process::Envelope, real_descriptor::hash, real_reply::Context,
        real_supervisor::Supervisor,
    };
    let args: Vec<_> = std::env::args()
        .skip(2)
        .map(std::path::PathBuf::from)
        .collect();
    ensure!(
        args.len() == 7,
        "real-job-v1 WORKER CONTEXT BUNDLE PACKAGE OUTPUT SCRATCH ENVELOPE"
    );
    let worker = serde_json::from_slice(&read_bounded(&args[0], 16 * 1024 * 1024)?)?;
    let c: Context = serde_json::from_slice(&read_bounded(&args[1], 65536)?)?;
    c.validate()?;
    let package = read_bounded(&args[3], 1048576)?;
    ensure!(
        boole_gate_supervisor::codec::digest(&package) == hash(&c.package_sha256)?,
        "real package input address"
    );
    let job = boole_derivation::m3::Job {
        queue_index: 0,
        key: (hash(&c.problem_id)?, hash(&c.commit_hash)?),
        kind: boole_derivation::m3::RecordKind::Verdict,
        avail: 1,
        epoch_override: Some(0),
    };
    let bounds: serde_json::Value = serde_json::from_slice(&read_bounded(&args[6], 4096)?)?;
    let envelope = Envelope {
        memory_mib: bounds["memory_mib"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("memory envelope"))?,
        wall_ms: bounds["wall_ms"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("wall envelope"))?,
        output_cap: 65536,
    };
    let components = if bounds["components"].is_null() {
        vec![]
    } else {
        serde_json::from_value(bounds["components"].clone())?
    };
    let (reply, record) = if std::env::args().nth(1).as_deref() == Some("real-file-job-v1") {
        ensure!(
            std::fs::metadata(&args[2])?.len() <= 256 * 1024 * 1024
                && boole_gate_supervisor::real_worker::file_hash(&args[2])?
                    == hash(&c.export_digest)?,
            "file bundle binding/cap"
        );
        Supervisor::qualify_real_files(
            &worker,
            &c,
            package,
            &components,
            &job,
            0,
            &envelope,
            &args[5],
        )?
    } else if std::env::args().nth(1).as_deref() == Some("real-transport-job-v2") {
        let transport = read_bounded(&args[2], boole_derivation::m2::Z_CAP)?;
        let length = bounds["raw_bytes"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("transport raw length"))?;
        Supervisor::qualify_real_transport(
            &worker,
            &c,
            package,
            &transport,
            length,
            &components,
            &job,
            0,
            &envelope,
            &args[5],
        )?
    } else {
        let input = c.input(
            read_bounded(&args[2], boole_derivation::m2::RAW_CAP as usize)?,
            package,
        )?;
        Supervisor::qualify_real_job(&worker, &input, &components, &job, 0, &envelope, &args[5])?
    };
    boole_gate_supervisor::codec::private_file(&args[4], &reply)?;
    println!(
        "{}",
        serde_json::json!({"descriptor":"boole-m3-real/v1","record":record.map(|r|r.encode()).transpose()?.map(|r|boole_derivation::attributes::hex_string(&r)),"reply_sha256":boole_derivation::attributes::hex_string(&boole_gate_supervisor::codec::digest(&reply)),"activation":false})
    );
    Ok(())
}
