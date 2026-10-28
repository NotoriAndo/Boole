//! Optional retained-artifact Mac fixture, also invoked by the bounded B1 job.
mod support;
use boole_gate_supervisor::real_worker::Worker;
#[test]
fn real_worker_refuses_unavailable_profiles_before_reading_any_sdk() {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return;
    }
    let w = Worker {
        descriptor: "missing".into(),
        gate_rule_digest: [1; 32],
        python: "missing".into(),
        python_sha256: [2; 32],
        scripts: "missing".into(),
        tools_root: "missing".into(),
        counter_driver: "missing".into(),
        resolver: boole_gate_supervisor::real_artifact::Resolver {
            root: "missing".into(),
            manifests: Default::default(),
        },
        public_counter: false,
        canary: None,
        decoder: None,
    };
    assert!(w
        .validate()
        .unwrap_err()
        .is::<boole_gate_supervisor::real_linux::StartupRefusal>());
}
#[test]
#[cfg(target_os = "macos")]
fn real_install_native_mac_startup_and_runtime_artifact_failures() -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use boole_gate_supervisor::{
        codec::{Codec, Scratch},
        process::Envelope,
        real_descriptor::hash,
        real_supervisor::{Host, Mode, Release, Supervisor},
    };
    let Some(path) = std::env::var_os("BOOLE_M3_NATIVE_WORKER_CFG") else {
        eprintln!("Native real-install fixture not supplied; pure profile controls still run");
        return Ok(());
    };
    let worker: Worker = serde_json::from_slice(&std::fs::read(path)?)?;
    let root = Scratch::new(&std::env::temp_dir())?;
    let descriptor = worker.descriptor()?;
    hash(&descriptor.artifact_table_sha256)?;
    let mut c = support::scenario::m3_cfg();
    let base = c.base_root;
    c.env_allowed = [vec![]].into();
    c.m3.as_mut().unwrap().genesis_gate_rule_digest = worker.gate_rule_digest;
    let forks = boole_derivation::m3::ForkTable::genesis(&c)?;
    let envelope = Envelope {
        memory_mib: 8000,
        wall_ms: 600000,
        output_cap: 65536,
    };
    let start = |w: Worker, name: &str| {
        Supervisor::new_releases(
            &c,
            &forks,
            Mode::Verify,
            vec![Release::Real(Box::new(w))],
            Some(Codec::portable_stub()),
            &root.0.join(name),
            &root.0,
            &root.0,
            Host::local()?,
            envelope.clone(),
        )
    };
    if worker.public_counter {
        ensure!(
            worker.canary(&envelope, &root.0),
            "native public qualification canary failed"
        );
        ensure!(
            start(worker.clone(), "public-counter").is_err(),
            "qualification flag entered verify mode"
        );
        eprintln!("REAL-INSTALL public_counter_refused=true canary=true; not a named verify descriptor mode");
        return Ok(());
    }
    let mut supervisor = start(worker.clone(), "good")?;
    let mut missing = worker.clone();
    missing.resolver.manifests.remove(&base);
    ensure!(
        start(missing, "missing").is_err(),
        "S16 admitted a missing artifact"
    );
    let (c, state, key, mut history) = support::fixture_with(c.clone(), Some(0));
    for mismatch in [false, true] {
        let w = supervisor
            .real_workers
            .get_mut(&worker.gate_rule_digest)
            .context("real dispatch")?;
        w.resolver = worker.resolver.clone();
        if mismatch {
            let bad = [9; 32];
            std::fs::write(
                w.resolver.root.join("objects").join(hex::encode(bad)),
                b"mismatched manifest",
            )?;
            w.resolver.manifests.insert(base, bad);
        } else {
            w.resolver.manifests.remove(&base);
        }
        let records =
            supervisor.collect_at(&state, &c, state.height() + 1, &forks, &mut history, 0)?;
        assert!(
            records.is_empty(),
            "artifact failure fabricated REJECT or VOID"
        );
        assert!(
            supervisor.real_retries.values().all(|r| r.attempts == 0),
            "D-node counted as submitted attempt"
        );
        assert_eq!(
            state.candidates[&key].status(),
            boole_derivation::m2::Status::Checking
        );
    }
    assert!(supervisor.alerts.iter().any(|a| a.starts_with("d-node:")));
    eprintln!("REAL-INSTALL native=true startup=true missing_refused=true runtime_missing_and_mismatch=DEFER(D-node) attempts=0 verdicts=0");
    Ok(())
}

fn minimal_worker() -> Worker {
    serde_json::from_value(serde_json::json!({"descriptor":"missing","gate_rule_digest":vec![1;32],"python":"missing","python_sha256":vec![2;32],"scripts":"missing","tools_root":"missing","counter_driver":"missing","resolver":{"root":"missing","manifests":{}},"public_counter":false,"canary":null})).unwrap()
}
#[test]
fn public_counter_qualification_switch_is_not_a_verify_release() {
    let c = support::scenario::m3_cfg();
    let mut w = minimal_worker();
    assert!(w.validate_config(&c, true).is_ok());
    w.public_counter = true;
    assert!(w.validate_config(&c, true).is_err());
    assert!(w.validate_config(&c, false).is_ok());
}
#[test]
fn real_s16_refuses_config_package_limit_above_worker_admission() {
    let w = minimal_worker();
    let mut c = support::scenario::m3_cfg();
    c.statement_raw_max = 1048577;
    assert!(w.validate_config(&c, true).is_err());
}
#[test]
fn public_counter_changes_the_authenticated_input_binding() -> anyhow::Result<()> {
    let (c, s, key, mut h) = support::fixture();
    let root = boole_gate_supervisor::codec::Scratch::new(&std::env::temp_dir())?;
    let i = boole_gate_supervisor::real_rebuild::rebuild(
        &s,
        &c,
        key,
        &mut h,
        Some(&boole_gate_supervisor::codec::Codec::portable_stub()),
        &root.0,
    )?
    .input;
    assert_ne!(
        boole_gate_supervisor::real_worker::input_binding(&i, &[], false, false)?,
        boole_gate_supervisor::real_worker::input_binding(&i, &[], false, true)?
    );
    Ok(())
}

#[test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn native_preparation_does_not_fetch_a_reference_before_a_submission_front_failure(
) -> anyhow::Result<()> {
    use boole_gate_supervisor::{
        codec::Scratch,
        real_deadline::Deadline,
        real_reply::{Context, Reply},
    };
    let Some(root) = std::env::var_os("BOOLE_M3_FRONT_ORDER_FIXTURE") else {
        return Ok(());
    };
    let root = std::path::Path::new(&root);
    let worker: Worker = serde_json::from_slice(&std::fs::read(root.join("worker.json"))?)?;
    let c: Context = serde_json::from_slice(&std::fs::read(root.join("context.json"))?)?;
    let input = c.input(
        std::fs::read(root.join("bundle.bin"))?,
        std::fs::read(root.join("package.json"))?,
    )?;
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let p = worker.prepare_until(&input, &[], false, &scratch.0, Deadline::after(60000))?;
    let raw = worker.execute(
        &input,
        &p,
        &boole_gate_supervisor::process::Envelope::default(),
    )?;
    let reply = Reply::decode_with_statement(
        &raw,
        &p.descriptor,
        &p.context,
        &p.package.caps,
        false,
        &p.package.authenticated,
    )?;
    assert_eq!(
        (reply.outcome.as_str(), reply.reason, reply.row.as_str()),
        ("REJECT", 0x0106, "front_invalid")
    );
    assert_eq!(raw, std::fs::read(root.join("EXPECTED.json"))?);
    println!("REAL-FRONT missing-reference=true submission-first=true kernels=not-run");
    Ok(())
}

#[test]
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn committed_export_mismatch_produces_a_terminal_real_transport_record() -> anyhow::Result<()> {
    use boole_gate_supervisor::{
        codec::{Codec, Scratch},
        process::Envelope,
        real_descriptor::hash,
        real_reply::Context,
        real_supervisor::Supervisor,
    };
    let (Some(fixture), Some(decoder)) = (
        std::env::var_os("BOOLE_M3_FRONT_ORDER_FIXTURE"),
        std::env::var_os("BOOLE_M3_REAL_DECODER"),
    ) else {
        return Ok(());
    };
    let fixture = std::path::Path::new(&fixture);
    let mut worker: Worker = serde_json::from_slice(&std::fs::read(fixture.join("worker.json"))?)?;
    let config: boole_gate_supervisor::real_decoder::Config =
        serde_json::from_slice(&std::fs::read(decoder)?)?;
    worker.decoder = Some(config.clone());
    let mut context: Context =
        serde_json::from_slice(&std::fs::read(fixture.join("context.json"))?)?;
    context.export_digest = hex::encode(boole_gate_supervisor::codec::digest(
        b"a different miner commitment",
    ));
    let raw = std::fs::read(fixture.join("bundle.bin"))?;
    let package = std::fs::read(fixture.join("package.json"))?;
    let root = Scratch::new(&std::env::temp_dir())?;
    let frame = Codec::new(config.executable)?.compress(&raw, &root.0)?;
    let job = boole_derivation::m3::Job {
        queue_index: 0,
        key: (hash(&context.problem_id)?, hash(&context.commit_hash)?),
        kind: boole_derivation::m3::RecordKind::Verdict,
        avail: 1,
        epoch_override: Some(0),
    };
    let (bytes, record) = Supervisor::qualify_real_transport(
        &worker,
        &context,
        package,
        &frame,
        raw.len() as u64,
        &[],
        &job,
        0,
        &Envelope {
            wall_ms: 60000,
            ..Envelope::default()
        },
        &root.0,
    )?;
    let record =
        record.ok_or_else(|| anyhow::anyhow!("digest mismatch became an uncounted local stop"))?;
    assert_eq!(
        (record.outcome, record.reason, record.fuel),
        (boole_derivation::m3::Outcome::Reject, 0x0106, 0)
    );
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(value["row"], "front_invalid");
    assert_eq!(value["kernels"]["lean"]["state"], "not-run");
    println!("REAL-DIGEST terminal-record=true reason=0106 missing-reference-not-read=true");
    Ok(())
}
