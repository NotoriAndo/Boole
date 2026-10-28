#[path = "support/controller_pins.rs"]
mod pin_fixture;
use boole_gate_supervisor::{
    real_artifact::local::Profile,
    real_controller::*,
    real_deadline::Deadline,
    real_descriptor::{ascii, Descriptor},
    real_reply::Context,
};
fn context() -> Context {
    let v: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/m3-real-gate/reply-cases.json"
    ))
    .unwrap();
    serde_json::from_value(v["context"].clone()).unwrap()
}
fn request(d: &[u8], c: &Context, epoch: u32, decoder: [u8; 32], transport: [u8; 32]) -> Request {
    Request::new_with_pins(
        Phase::Front,
        Tool::Front,
        Limits {
            memory_mib: 512,
            wall_ms: 30000,
            output_bytes: 65536,
        },
        Invocation {
            descriptor: d,
            context: c,
            epoch,
            decoder,
            transport,
            profile: Profile::LinuxAarch64,
        },
        vec![],
        Deadline::after(30000),
        &pin_fixture::Pins {
            decoder: Some(decoder),
            transport: Some(transport),
        },
    )
    .unwrap()
}
fn status(r: &Request, observation: &str) -> Vec<u8> {
    format!("{{\"version\":1,\"request_sha256\":\"{}\",\"phase\":\"front\",\"tool\":\"front\",\"observation\":{observation}}}",hex::encode(r.binding())).into_bytes()
}
const DESCRIPTOR: &[u8] = include_bytes!("../../../fixtures/m3-real-gate/descriptor-v1.json");
#[test]
fn request_refuses_unknown_decoder_and_transport_without_a_pin_provider() {
    let c = context();
    let result = Request::new(
        Phase::Front,
        Tool::Front,
        Limits {
            memory_mib: 512,
            wall_ms: 30000,
            output_bytes: 65536,
        },
        Invocation {
            descriptor: DESCRIPTOR,
            context: &c,
            epoch: 0,
            decoder: [7; 32],
            transport: [8; 32],
            profile: Profile::LinuxAarch64,
        },
        vec![],
        Deadline::after(30000),
    );
    assert!(
        result.is_err(),
        "unknown decoder/transport hashes were merely hashed instead of refused"
    );
}
#[test]
fn evidence_integrity_precedes_capture_limits_with_trusted_capture_twins() -> anyhow::Result<()> {
    let c = context();
    let r = request(DESCRIPTOR, &c, 0, [7; 32], [8; 32]);
    let foreign = request(DESCRIPTOR, &c, 0, [7; 32], [8; 32]);
    for bad in [
        vec![],
        b"{".to_vec(),
        status(&foreign, r#"{"kind":"finished","exit":0}"#),
    ] {
        let error = Execution::from_channels(&r, &bad, vec![0; 65537], vec![])
            .err()
            .expect("invalid status accepted");
        assert_eq!(
            error.class(),
            DeferClass::DNode,
            "oversized stdout masked untrusted status"
        );
    }
    let valid = status(&r, r#"{"kind":"finished","exit":0}"#);
    assert_eq!(
        Execution::from_channels(&r, &valid, vec![0; 65537], vec![])
            .err()
            .unwrap()
            .class(),
        DeferClass::T
    );
    let trusted_capture = status(&r, r#"{"kind":"physical","stop":"capture"}"#);
    assert_eq!(
        Execution::from_channels(&r, &trusted_capture, vec![], vec![])
            .err()
            .unwrap()
            .class(),
        DeferClass::T
    );
    let missing_producer = status(&r, r#"{"kind":"native-claim","cause":"memory"}"#);
    assert_eq!(
        Execution::from_channels(&r, &missing_producer, vec![0; 65537], vec![])
            .err()
            .unwrap()
            .class(),
        DeferClass::DNode
    );
    Ok(())
}
#[test]
fn b2_cause_private_channel_requires_every_request_identity_and_bounded_canonical_bytes(
) -> anyhow::Result<()> {
    let c = context();
    let r = request(DESCRIPTOR, &c, 0, [7; 32], [8; 32]);
    let fresh = request(DESCRIPTOR, &c, 0, [7; 32], [8; 32]);
    assert_eq!(r.input_binding(), fresh.input_binding());
    assert_ne!(r.binding(), fresh.binding());
    let done = status(&r, r#"{"kind":"finished","exit":0}"#);
    assert_eq!(
        Evidence::decode(&done, &r)?.disposition(),
        Disposition::Completed
    );
    assert!(
        Evidence::decode(&done, &fresh).is_err(),
        "foreign invocation channel accepted"
    );
    let mut contexts = vec![];
    for field in [
        "chain_id",
        "problem_id",
        "commit_hash",
        "solver",
        "env_id",
        "export_digest",
        "package_sha256",
        "leaf_kind",
        "lane",
        "slot_fuel",
    ] {
        let mut v = serde_json::to_value(&c)?;
        v[field] = match field {
            "chain_id" => serde_json::json!(c.chain_id + 1),
            "leaf_kind" => serde_json::json!(2),
            "lane" => serde_json::json!(2),
            "slot_fuel" => serde_json::json!(c.slot_fuel + 1),
            "solver" => serde_json::json!("ab".repeat(20)),
            _ => serde_json::json!("ab".repeat(32)),
        };
        contexts.push(serde_json::from_value::<Context>(v)?);
    }
    for c in contexts {
        let other = request(DESCRIPTOR, &c, 0, [7; 32], [8; 32]);
        assert_ne!(r.input_binding(), other.input_binding());
        assert!(Evidence::decode(&done, &other).is_err());
    }
    for (epoch, decoder, transport) in [
        (1, [7; 32], [8; 32]),
        (0, [9; 32], [8; 32]),
        (0, [7; 32], [9; 32]),
    ] {
        assert_ne!(
            r.input_binding(),
            request(DESCRIPTOR, &c, epoch, decoder, transport).input_binding()
        );
    }
    for field in ["source_sha256", "artifact_table_sha256"] {
        let mut d = Descriptor::decode(DESCRIPTOR)?;
        if field == "source_sha256" {
            d.source_sha256 = "ab".repeat(32)
        } else {
            d.artifact_table_sha256 = "ab".repeat(32)
        };
        let bytes = ascii(&d)?;
        assert_ne!(
            r.input_binding(),
            request(&bytes, &c, 0, [7; 32], [8; 32]).input_binding()
        );
    }
    for bad in [
        done.iter().copied().chain([b' ']).collect(),
        String::from_utf8(done.clone())?
            .replace("\"version\":1", "\"version\":2")
            .into_bytes(),
        String::from_utf8(done.clone())?
            .replace("\"tool\":\"front\"", "\"tool\":\"decoder\"")
            .into_bytes(),
        String::from_utf8(done.clone())?
            .replace("\"phase\":\"front\"", "\"phase\":\"kernel\"")
            .into_bytes(),
        String::from_utf8(done.clone())?
            .replace("\"exit\":0", "\"exit\":0,\"stderr\":\"cap\"")
            .into_bytes(),
        vec![b' '; 16385],
    ] {
        let e = Evidence::decode(&bad, &r)
            .err()
            .expect("bad channel admitted");
        assert_eq!(e.class(), DeferClass::DNode);
    }
    let e = Execution::from_channels(&r, &done, vec![0; 65537], vec![])
        .err()
        .expect("unbounded capture");
    assert_eq!(e.class(), DeferClass::T);
    let e = boole_gate_supervisor::real_linux::execute(&r)
        .err()
        .expect("pending backend cannot execute");
    assert_eq!(e.class(), DeferClass::DNode);
    Ok(())
}
#[test]
fn b2_cause_unproduced_native_claims_and_exit_codes_never_fabricate_terminal_verdicts(
) -> anyhow::Result<()> {
    let c = context();
    let r = request(DESCRIPTOR, &c, 0, [7; 32], [8; 32]);
    for cause in [
        "depth",
        "kernel-panic",
        "cap-origin",
        "pre-kernel-cap-origin",
        "structured-selection",
        "memory",
    ] {
        let raw = status(
            &r,
            &format!(r#"{{"kind":"native-claim","cause":"{cause}"}}"#),
        );
        let e = Evidence::decode(&raw, &r)?;
        assert_eq!(e.disposition(), Disposition::Defer(DeferClass::DNode));
        assert_eq!(e.fault().unwrap().class(), DeferClass::DNode);
    }
    for exit in [1, 2, 3, 4, 70, -9, 255] {
        let raw = status(&r, &format!(r#"{{"kind":"finished","exit":{exit}}}"#));
        assert_eq!(
            Evidence::decode(&raw, &r)?.disposition(),
            Disposition::Defer(DeferClass::DNode)
        );
    }
    for stop in ["wall", "memory", "tasks", "capture"] {
        let raw = status(&r, &format!(r#"{{"kind":"physical","stop":"{stop}"}}"#));
        assert_eq!(
            Evidence::decode(&raw, &r)?.disposition(),
            Disposition::Defer(DeferClass::T)
        );
    }
    for unsupported in [
        r#"{"kind":"native-claim","cause":"kernel-panic","role":"submission"}"#,
        r#"{"kind":"reject","reason":774}"#,
        r#"{"kind":"native-claim","cause":"new-cause"}"#,
    ] {
        assert!(Evidence::decode(&status(&r, unsupported), &r).is_err());
    }
    Ok(())
}

#[test]
fn independent_pin_provider_checks_unknowns_mismatches_and_descriptor_profiles(
) -> anyhow::Result<()> {
    let c = context();
    let make = |pins: &dyn InvocationPins, d: &[u8]| {
        Request::new_with_pins(
            Phase::Front,
            Tool::Front,
            Limits {
                memory_mib: 512,
                wall_ms: 30000,
                output_bytes: 65536,
            },
            Invocation {
                descriptor: d,
                context: &c,
                epoch: 0,
                decoder: [7; 32],
                transport: [8; 32],
                profile: Profile::LinuxAarch64,
            },
            vec![],
            Deadline::after(30000),
            pins,
        )
    };
    for (decoder, transport, expected) in [
        (None, Some([8; 32]), PinError::UnknownDecoder),
        (Some([7; 32]), None, PinError::UnknownTransport),
        (Some([9; 32]), Some([8; 32]), PinError::DecoderMismatch),
        (Some([7; 32]), Some([9; 32]), PinError::TransportMismatch),
        (Some([0; 32]), Some([8; 32]), PinError::UnknownDecoder),
    ] {
        let error = make(&pin_fixture::Pins { decoder, transport }, DESCRIPTOR)
            .err()
            .expect("unknown/mismatched pin admitted");
        assert_eq!(error.downcast_ref::<PinError>(), Some(&expected));
    }
    assert!(make(
        &pin_fixture::Pins {
            decoder: Some([7; 32]),
            transport: Some([8; 32])
        },
        DESCRIPTOR
    )
    .is_ok());
    let mut d = Descriptor::decode(DESCRIPTOR)?;
    d.profiles.remove("Linux-aarch64");
    assert!(make(
        &pin_fixture::Pins {
            decoder: Some([7; 32]),
            transport: Some([8; 32])
        },
        &ascii(&d)?
    )
    .is_err());
    Ok(())
}
