#![cfg(feature = "w0-fixture")]
use boole_gate_supervisor::{codec::Scratch, real_w0::finish_evidence};
use std::fs;
#[test]
fn evidence_destination_is_reserved_before_a_case_can_start() {
    let base = Scratch::new(&std::env::temp_dir()).unwrap();
    let scratch = Scratch::new(&base.0).unwrap();
    let destination = base.0.join("reserved");
    let _guard = boole_gate_supervisor::real_w0::EvidenceGuard::new(scratch, &destination).unwrap();
    assert!(
        destination.is_dir(),
        "receipt destination was not checked/reserved before execution"
    );
}
#[test]
fn a_failed_case_keeps_private_channel_and_diagnostics_after_scratch_drop() {
    let base = Scratch::new(&std::env::temp_dir()).unwrap();
    let evidence = base.0.join("case");
    {
        let scratch = Scratch::new(&base.0).unwrap();
        fs::write(scratch.0.join("receipt.json"), b"failed-receipt").unwrap();
        fs::write(scratch.0.join("step.private.ndjson"), b"private-channel").unwrap();
        fs::write(scratch.0.join("step.stderr"), b"diagnostic").unwrap();
        assert!(
            finish_evidence(&scratch.0, &evidence, Err(anyhow::anyhow!("failed case"))).is_err()
        );
    }
    assert_eq!(
        fs::read(evidence.join("step.private.ndjson")).unwrap(),
        b"private-channel"
    );
    assert_eq!(
        fs::read(evidence.join("step.stderr")).unwrap(),
        b"diagnostic"
    );
    let hashes: serde_json::Value =
        serde_json::from_slice(&fs::read(evidence.join("hashes.json")).unwrap()).unwrap();
    assert_eq!(hashes.as_object().unwrap().len(), 3);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(evidence.join("receipt.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o400
    );
    assert!(finish_evidence(&base.0, &evidence, Ok(())).is_err());
}

#[test]
fn evidence_is_retained_on_unwind_before_scratch_drop() {
    let base = Scratch::new(&std::env::temp_dir()).unwrap();
    let destination = base.0.join("unwound");
    assert!(std::panic::catch_unwind(|| {
        let scratch = Scratch::new(&base.0).unwrap();
        let guard =
            boole_gate_supervisor::real_w0::EvidenceGuard::new(scratch, &destination).unwrap();
        fs::write(guard.source().join("diagnostic"), b"panic-evidence").unwrap();
        panic!("case failed");
    })
    .is_err());
    assert_eq!(
        fs::read(destination.join("diagnostic")).unwrap(),
        b"panic-evidence"
    );
}

#[test]
fn oversized_and_nonregular_evidence_keeps_hash_list_and_original_scratch() {
    let base = Scratch::new(&std::env::temp_dir()).unwrap();
    let scratch = Scratch::new(&base.0).unwrap();
    let original = scratch.0.clone();
    fs::write(original.join("large.stderr"), vec![7u8; 1048577]).unwrap();
    fs::create_dir(original.join("bad.stdout")).unwrap();
    let destination = base.0.join("oversize");
    let guard = boole_gate_supervisor::real_w0::EvidenceGuard::new(scratch, &destination).unwrap();
    assert!(guard.finish(Ok(())).is_err());
    let hashes: serde_json::Value =
        serde_json::from_slice(&fs::read(destination.join("hashes.json")).unwrap()).unwrap();
    assert_eq!(hashes["large.stderr"]["truncated"], true);
    assert!(hashes["large.stderr"]["error"].is_string());
    assert!(hashes["bad.stdout"]["error"].is_string());
    assert_eq!(
        fs::metadata(destination.join("large.stderr"))
            .unwrap()
            .len(),
        1048576
    );
    assert_eq!(
        fs::metadata(original.join("large.stderr")).unwrap().len(),
        1048577
    );
}
