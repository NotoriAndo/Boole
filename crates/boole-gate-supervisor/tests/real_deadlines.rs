mod support;
use anyhow::Result;
use boole_gate_supervisor::{
    codec::{digest, Codec, Scratch},
    process::{Envelope, Worker},
    real_supervisor::{install_devnet_artifacts, Host, Mode, Supervisor},
    stub::Descriptor,
};
#[test]
fn expired_real_preparation_is_a_counted_transient_attempt() -> Result<()> {
    let root = Scratch::new(&std::env::temp_dir())?;
    let binary = std::path::PathBuf::from(env!("CARGO_BIN_EXE_boole-gate-supervisor"));
    let pin = digest(&std::fs::read(&binary)?);
    let descriptor = Descriptor::pinned(pin);
    let gate = descriptor.digest();
    let mut c = support::scenario::m3_cfg();
    c.m3.as_mut().unwrap().genesis_gate_rule_digest = gate;
    let (c, s, _, mut history) = support::fixture_with(c, Some(0));
    let forks = boole_derivation::m3::ForkTable::genesis(&c)?;
    let artifacts = root.0.join("artifacts");
    install_devnet_artifacts(&c, &artifacts)?;
    let mut supervisor = Supervisor::new(
        &c,
        &forks,
        Mode::Verify,
        vec![Worker {
            descriptor,
            binary,
            binary_sha256: pin,
        }],
        Some(Codec::portable_stub()),
        &root.0.join("cache"),
        &root.0,
        &artifacts,
        Host::local()?,
        Envelope::default(),
    )?;
    let worker: boole_gate_supervisor::real_worker::Worker = serde_json::from_value(
        serde_json::json!({"descriptor":"missing","gate_rule_digest":gate,"python":"missing","python_sha256":vec![2;32],"scripts":"missing","tools_root":"missing","counter_driver":"missing","resolver":{"root":"missing","manifests":{}},"public_counter":false,"canary":null}),
    )?;
    supervisor.real_workers.insert(gate, worker);
    supervisor.envelope.wall_ms = 0;
    assert!(supervisor
        .collect_at(&s, &c, s.height() + 1, &forks, &mut history, 10)?
        .is_empty());
    assert_eq!(
        supervisor.real_retries.len(),
        1,
        "did not reach real preparation"
    );
    let retry = supervisor.real_retries.values().next().unwrap();
    assert_eq!(
        (retry.attempts, retry.next_at),
        (1, 70),
        "{:?}",
        supervisor.alerts
    );
    assert!(supervisor.alerts.iter().any(|a| a.starts_with("DEFER(T)")));
    supervisor.envelope.wall_ms = 1000;
    assert!(supervisor
        .collect_at(&s, &c, s.height() + 1, &forks, &mut history, 80)?
        .is_empty());
    let retry = supervisor.real_retries.values().next().unwrap();
    assert_eq!(
        retry.attempts, 1,
        "missing node release must stay uncounted"
    );
    assert!(retry.next_at >= 140);
    Ok(())
}
