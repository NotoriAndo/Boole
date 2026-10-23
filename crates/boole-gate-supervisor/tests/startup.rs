use boole_derivation::{m2::Config, m3::*};
use boole_gate_supervisor::{
    codec::{digest, Codec, Scratch},
    process::{Envelope, Worker},
    stub::Descriptor,
    supervisor::*,
};
#[test]
fn s16_verify_checks_epochs_artifacts_profiles_resources_and_source_release() {
    let root = Scratch::new(&std::env::temp_dir()).unwrap();
    let artifacts = root.0.join("artifacts");
    let binary = std::path::PathBuf::from(env!("CARGO_BIN_EXE_boole-gate-supervisor"));
    let hash = digest(&std::fs::read(&binary).unwrap());
    let descriptor = Descriptor::pinned(hash);
    let gate = Worker {
        descriptor: descriptor.clone(),
        binary,
        binary_sha256: hash,
    };
    let mut c = Config::devnet();
    c.m3 = Some(M3Config::devnet(c.slot_table.len()));
    c.m3.as_mut().unwrap().genesis_gate_rule_digest = descriptor.digest();
    let forks = ForkTable::genesis(&c).unwrap();
    install_devnet_artifacts(&c, &artifacts).unwrap();
    let host = Host::local().unwrap();
    let mut attempt = 0;
    let mut start =
        |config: &Config, table: &ForkTable, mode: Mode, gates: Vec<Worker>, host: Host| {
            attempt += 1;
            Supervisor::new(
                config,
                table,
                mode,
                gates,
                Some(Codec::portable_stub()),
                &root.0.join(format!("cache{attempt}")),
                &root.0,
                &artifacts,
                host,
                Envelope::default(),
            )
        };
    assert!(start(&c, &forks, Mode::Verify, vec![gate.clone()], host.clone()).is_ok());
    let mut wrong = host.clone();
    wrong.profile = "unlisted".into();
    assert!(start(&c, &forks, Mode::Verify, vec![gate.clone()], wrong).is_err());
    let mut wrong = host.clone();
    wrong.ram_mib = 1023;
    assert!(start(&c, &forks, Mode::Verify, vec![gate.clone()], wrong).is_err());
    let mut future = forks.clone();
    future.rows.push(ForkRow {
        epoch: 1,
        height: 100,
        gate_rule_digest: [8; 32],
        kind: ForkKind::Policy,
        list: vec![],
    });
    assert!(start(&c, &future, Mode::Verify, vec![gate.clone()], host.clone()).is_err());
    let mut wrong = gate.clone();
    wrong.descriptor.implementation_sha256[0] ^= 1;
    let mut different = c.clone();
    different.m3.as_mut().unwrap().genesis_gate_rule_digest = wrong.descriptor.digest();
    let table = ForkTable::genesis(&different).unwrap();
    assert!(start(&different, &table, Mode::Verify, vec![wrong], host.clone()).is_err());
    std::fs::remove_file(artifacts.join(boole_derivation::attributes::hex_string(&c.base_root)))
        .unwrap();
    assert!(start(&c, &forks, Mode::Verify, vec![gate], host.clone()).is_err());
    assert!(start(
        &c,
        &future,
        Mode::Follow {
            from: 1,
            through: 100
        },
        vec![],
        host
    )
    .is_ok());
}
