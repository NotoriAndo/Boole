use boole_gate_supervisor::{stub::*, GateInput, GateReply};
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
#[test]
fn t43_stub_returns_typed_deterministic_outcomes() {
    let d = Descriptor::default();
    let i = input();
    let GateReply::Result { value } = evaluate(&d, &i) else {
        panic!("typed ACCEPT missing")
    };
    assert_eq!((value.outcome, value.reason, value.fuel), (1, 0, 777));
    assert_ne!(value.statement_id, [0; 32]);
    assert_eq!(evaluate(&d, &i), evaluate(&d, &i));
}
#[test]
fn t40_s16_stub_is_refused_outside_devnet_and_on_unknown_hosts() {
    let mut d = Descriptor::default();
    d.binaries = d.profiles.iter().map(|p| (p.clone(), [1; 32])).collect();
    d.validate(720926, "darwin-aarch64").unwrap();
    assert!(d.validate(720927, "linux-aarch64").is_err());
    assert!(d.validate(720926, "unknown").is_err());
}

#[test]
fn portable_raw_frames_are_canonical_and_checksum_bound() {
    use boole_gate_supervisor::{codec::Codec, raw_frame};
    for n in [1, 255, 256, 131072, 131073] {
        let raw = (0..n).map(|i| (i % 251) as u8).collect::<Vec<_>>();
        let frame = raw_frame::encode(&raw).unwrap();
        let codec = Codec::portable_stub();
        assert_eq!(
            codec
                .decode(&frame, n as u64, &std::env::temp_dir())
                .unwrap(),
            raw
        );
        let mut bad = frame;
        *bad.last_mut().unwrap() ^= 1;
        assert!(codec.decode(&bad, n as u64, &std::env::temp_dir()).is_err());
    }
}

#[test]
fn t43_closed_outcomes_and_malformed_adapter_results_are_checked() {
    use boole_derivation::m3::{Job, RecordKind};
    use boole_gate_supervisor::supervisor::checked_record;
    let d = Descriptor::default();
    let mut i = input();
    let job = Job {
        queue_index: 0,
        key: (i.problem_id, i.commit_hash),
        kind: RecordKind::Verdict,
        avail: 1,
        epoch_override: None,
    };
    for (tag, outcome, reason) in [
        (0, 1, 0),
        (1, 2, 0x0301),
        (2, 2, 0x0306),
        (3, 3, 0x0801),
        (4, 3, 0x0803),
        (5, 3, 0x0804),
        (10, 2, 0x0306),
    ] {
        i.raw = export(tag, 777);
        let GateReply::Result { value } = evaluate(&d, &i) else {
            panic!("typed outcome missing")
        };
        assert_eq!((value.outcome, value.reason), (outcome, reason));
        checked_record(&i, &d, &job, 0, &value).unwrap();
    }
    i.raw = export(0, 777);
    let GateReply::Result { value } = evaluate(&d, &i) else {
        unreachable!()
    };
    let mut bad = value.clone();
    bad.statement_id = [0; 32];
    assert!(checked_record(&i, &d, &job, 0, &bad).is_err());
    let mut bad = value.clone();
    bad.fuel = 10001;
    assert!(checked_record(&i, &d, &job, 0, &bad).is_err());
    let mut bad = value;
    bad.reason = 0x0199;
    assert!(checked_record(&i, &d, &job, 0, &bad).is_err());
}
