mod support;
use boole_gate_supervisor::{
    codec::Scratch,
    real_descriptor::Descriptor,
    real_reply::{Caps, Context, Reply},
    real_retry::Retry,
    real_supervisor::defer_worker_stop,
};
#[test]
fn genuine_worker_input_integrity_stop_remains_local_without_a_proof_verdict() -> anyhow::Result<()>
{
    let mut c = support::scenario::m3_cfg();
    let extra = [7; 32];
    c.env_allowed = [vec![], vec![extra]].into();
    c.pkg_table = vec![extra];
    c.k_max = 1;
    let (c, s, key, mut history) = support::fixture_with_open_packages(c, Some(0), 777, &[extra]);
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let input = boole_gate_supervisor::real_rebuild::rebuild(
        &s,
        &c,
        key,
        &mut history,
        Some(&boole_gate_supervisor::codec::Codec::portable_stub()),
        &scratch.0,
    )?
    .input;
    assert_ne!(
        input.env_id,
        boole_derivation::m2::env_id(c.schema_id, c.base_root, &[])
    );
    let context = Context::from_input(&input);
    let d = Descriptor::decode(include_bytes!(
        "../../../fixtures/m3-real-gate/descriptor-v1.json"
    ))?;
    let vectors: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/m3-real-gate/reply-cases.json"
    ))?;
    let case = vectors["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "input_integrity")
        .unwrap();
    let mut raw: serde_json::Value =
        serde_json::from_slice(&hex::decode(case["raw_hex"].as_str().unwrap())?)?;
    raw["input_digest"] = hex::encode(context.digest()?).into();
    // Preserve the canonical field order in the typed reply.
    let reply: Reply = serde_json::from_value(raw)?;
    let bytes = boole_gate_supervisor::real_descriptor::ascii(&reply)?;
    assert_eq!(
        Reply::decode(
            &bytes,
            &d,
            &context,
            &Caps {
                lean: 0,
                nanoda: 0,
                statement: "a".repeat(64)
            },
            false
        )?
        .outcome,
        "STOP"
    );
    let mut retry = Retry::default();
    let mut alerts = vec![];
    assert!(defer_worker_stop(&mut retry, &mut alerts, 10)?.is_none());
    assert_eq!(retry.attempts, 0);
    assert!(retry.next_at >= 70);
    assert!(!alerts.is_empty());
    Ok(())
}

#[test]
fn amended_first_failure_agrees_with_python_on_every_shared_vector() -> anyhow::Result<()> {
    let v: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/m3-real-gate/reconstruction-cases.json"
    ))?;
    let rows = v["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 183);
    for case in rows {
        let d = Descriptor::decode(&hex::decode(case["descriptor_hex"].as_str().unwrap())?)?;
        let c: Context = serde_json::from_value(case["context"].clone())?;
        let (p, local) = boole_gate_supervisor::real_admission::authenticate(
            &hex::decode(case["package_hex"].as_str().unwrap())?,
            &hex::decode(case["inventory_hex"].as_str().unwrap())?,
            &d,
            &c,
            case["public_counter"].as_bool().unwrap(),
            case["fork_authorized"].as_bool().unwrap(),
        )?;
        let output = case["bundle_hex"].as_str().map(hex::decode).transpose()?;
        let status = case["decoder_status_hex"]
            .as_str()
            .map(hex::decode)
            .transpose()?;
        let decoder = status.as_ref().map(|s| {
            (
                s.as_slice(),
                boole_gate_supervisor::real_descriptor::hash(case["decoder_pin"].as_str().unwrap())
                    .unwrap(),
                boole_gate_supervisor::real_descriptor::hash(
                    case["transport_digest"].as_str().unwrap(),
                )
                .unwrap(),
            )
        });
        let actual = if let Some(raw) = local {
            raw
        } else {
            boole_gate_supervisor::real_admission::transport(
                &d,
                &c,
                &p,
                output.as_deref(),
                decoder,
            )?
            .unwrap_or_else(|| panic!("case did not reach a decision: {}", case["name"]))
        };
        assert_eq!(
            hex::encode(actual),
            case["reply_hex"],
            "{} {}",
            case["name"],
            case["phase"]
        );
    }
    Ok(())
}

#[test]
fn reachable_alternate_allowed_environment_open_gets_0106_instead_of_an_uncounted_retry(
) -> anyhow::Result<()> {
    use boole_gate_supervisor::{codec::digest, real_descriptor::hash};
    let mut c = support::scenario::m3_cfg();
    let extra = [7; 32];
    c.env_allowed = [vec![], vec![extra]].into();
    c.pkg_table = vec![extra];
    c.k_max = 1;
    let expected = hex::encode(boole_derivation::m2::env_id(c.schema_id, c.base_root, &[]));
    let inventory =
        format!("{{\"version\":1,\"environment\":\"{expected}\",\"base\":{{}},\"library\":{{}}}}")
            .into_bytes();
    let package=format!("{{\"version\":1,\"mode\":\"full\",\"problem\":\"fixture\",\"env_id\":\"{expected}\",\"target\":[[\"str\",\"Fixture\"]],\"statement\":\"{}\",\"reference_sha256\":\"{}\",\"inventory_sha256\":\"{}\",\"fuel_cap\":null}}","a".repeat(64),"b".repeat(64),hex::encode(digest(&inventory))).into_bytes();
    let mut raw = b"BOOLE-M3/EXPORT-BUNDLE/v1\0".to_vec();
    raw.extend(1u64.to_be_bytes());
    raw.push(b'x');
    let (c, s, key, mut history) =
        support::fixture_with_content(c, Some(0), 777, &[extra], Some(package), Some(raw.clone()));
    let scratch = Scratch::new(&std::env::temp_dir())?;
    let rebuilt =
        boole_gate_supervisor::real_rebuild::rebuild_real(&s, &c, key, &mut history, &scratch.0)?;
    assert_eq!(
        rebuilt.input.early, None,
        "real reconstruction must not choose a transport verdict first"
    );
    assert_ne!(rebuilt.input.env_id, hash(&expected)?);
    let context = Context::from_input(&rebuilt.input);
    let mut d = Descriptor::decode(include_bytes!(
        "../../../fixtures/m3-real-gate/descriptor-v1.json"
    ))?;
    d.artifact_table_sha256 = hex::encode(digest(&inventory));
    let (p, local) = boole_gate_supervisor::real_admission::authenticate(
        &rebuilt.input.package,
        &inventory,
        &d,
        &context,
        true,
        false,
    )?;
    assert!(local.is_none());
    let bytes =
        boole_gate_supervisor::real_admission::transport(&d, &context, &p, Some(&raw), None)?
            .unwrap();
    let r = Reply::decode_with_statement(&bytes, &d, &context, &p.caps, false, &p.authenticated)?;
    assert_eq!(
        (r.outcome.as_str(), r.reason, r.row.as_str(), r.fuel),
        ("REJECT", 0x0106, "front_invalid", Some(0))
    );
    assert_eq!(r.kernels.lean.state, "not-run");
    assert_eq!(r.kernels.nanoda.state, "not-run");
    Ok(())
}
