mod support;
use boole_gate_supervisor::{
    codec::{Codec, Scratch},
    process::Envelope,
    real_linux::{StartupRefusal, PENDING},
    real_supervisor::{Host, Mode, Release, Supervisor},
    real_worker::Worker,
};

#[test]
fn b2_disp_linux_startup_reports_all_pending_seams_before_any_artifact_read() -> anyhow::Result<()>
{
    let mut c = support::scenario::m3_cfg();
    c.m3.as_mut().unwrap().genesis_gate_rule_digest = [1; 32];
    let forks = boole_derivation::m3::ForkTable::genesis(&c)?;
    let w: Worker = serde_json::from_value(
        serde_json::json!({"descriptor":"missing","gate_rule_digest":vec![1;32],"python":"missing","python_sha256":vec![2;32],"scripts":"missing","tools_root":"missing","counter_driver":"missing","resolver":{"root":"missing","manifests":{}},"public_counter":false,"canary":null}),
    )?;
    let root = Scratch::new(&std::env::temp_dir())?;
    let error = Supervisor::new_releases(
        &c,
        &forks,
        Mode::Verify,
        vec![Release::Real(Box::new(w))],
        Some(Codec::portable_stub()),
        &root.0,
        &root.0,
        &root.0,
        Host {
            profile: "linux-aarch64".into(),
            ram_mib: 32768,
            cpus: 4,
        },
        Envelope::default(),
    )
    .err()
    .expect("Linux cannot start before W0/release prerequisites");
    let refusal = error
        .downcast_ref::<StartupRefusal>()
        .unwrap_or_else(|| panic!("untyped Linux startup refusal: {error:#}"));
    assert_eq!(refusal.profile, "Linux-aarch64");
    assert_eq!(refusal.pending, PENDING);
    Ok(())
}

#[test]
fn b2_disp_unknown_architectures_never_fall_back_to_mac_or_partial_linux() {
    use boole_gate_supervisor::real_linux::{select, Dispatch};
    assert_eq!(select("darwin-aarch64").unwrap(), Dispatch::MacDevelopment);
    for name in [
        "Linux-x86_64",
        "linux-x86_64",
        "darwin-x86_64",
        "Linux-arm64",
        "",
        "Linux-aarch64",
        "Darwin-arm64",
    ] {
        assert!(select(name).is_err(), "profile admitted: {name}");
    }
    assert_eq!(select("linux-aarch64").unwrap_err().pending, PENDING);
}
