use boole_gate_supervisor::{codec::digest, process::*, stub::*, GateInput, GateReply};
fn input() -> GateInput {
    GateInput {
        chain_id: 720926,
        problem_id: [1; 32],
        commit_hash: [2; 32],
        solver: [3; 20],
        env_id: [4; 32],
        export_digest: [5; 32],
        lane: 1,
        leaf_kind: 1,
        instance_seed: [6; 32],
        fuel_slot: 10000,
        raw: export(0, 777),
        package: package(10000),
        artifacts: vec![],
        early: None,
    }
}
fn worker() -> Worker {
    let path = std::path::PathBuf::from(env!("CARGO_BIN_EXE_boole-gate-supervisor"));
    Worker {
        descriptor: Descriptor::pinned(digest(&std::fs::read(&path).unwrap())),
        binary_sha256: digest(&std::fs::read(&path).unwrap()),
        binary: path,
    }
}
#[test]
fn t43_real_isolated_process_returns_typed_result_independent_of_envelope() {
    let w = worker();
    let a = w.run(&input(), &Envelope::default(), &std::env::temp_dir());
    let GateReply::Result { value } = a.clone() else {
        panic!("isolated ACCEPT missing: {a:?}")
    };
    assert_eq!((value.outcome, value.reason, value.fuel), (1, 0, 777));
    let b = w.run(
        &input(),
        &Envelope {
            memory_mib: 1536,
            wall_ms: 15000,
            output_cap: 32768,
        },
        &std::env::temp_dir(),
    );
    assert_eq!(a, b);
}
#[test]
fn t40_s16_binary_tampering_is_refused() {
    let mut w = worker();
    w.binary_sha256[0] ^= 1;
    assert!(w.validate().is_err());
}

#[test]
fn t43_generic_exit_and_unknown_final_marker_are_defer() {
    let w = worker();
    for tag in [8, 9] {
        let mut i = input();
        i.raw = export(tag, 0);
        assert!(matches!(
            w.run(&i, &Envelope::default(), &std::env::temp_dir()),
            GateReply::Defer { .. }
        ));
    }
}

#[test]
fn pinned_full_decoder_agrees_with_portable_frames_and_rejects_bad_checksums() {
    let Some(path) = std::env::var_os("BOOLE_DEVNET_ZSTD") else {
        eprintln!("full decoder qualification skipped: BOOLE_DEVNET_ZSTD absent");
        return;
    };
    let codec = boole_gate_supervisor::codec::Codec::new(path.into()).unwrap();
    let root = std::env::temp_dir();
    let raw = vec![b'z'; 150000];
    let compressed = codec.compress(&raw, &root).unwrap();
    assert_eq!(
        codec.decode(&compressed, raw.len() as u64, &root).unwrap(),
        raw
    );
    let mut corrupt = compressed.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    assert!(codec
        .decode(&corrupt, raw.len() as u64, &root)
        .unwrap_err()
        .is::<boole_gate_supervisor::codec::InvalidData>());
    let portable = boole_gate_supervisor::raw_frame::encode(&raw).unwrap();
    assert_eq!(
        codec.decode(&portable, raw.len() as u64, &root).unwrap(),
        raw
    );
}

#[test]
fn typed_package_and_fuel_faults_precede_the_unknown_exit_probe() {
    let w = worker();
    let mut i = input();
    i.raw = export(8, 777);
    i.package = b"bad package".to_vec();
    assert_eq!(
        w.run(&i, &Envelope::default(), &std::env::temp_dir()),
        evaluate(&w.descriptor, &i)
    );
    let mut i = input();
    i.raw = export(8, 10001);
    assert_eq!(
        w.run(&i, &Envelope::default(), &std::env::temp_dir()),
        evaluate(&w.descriptor, &i)
    );
}
