use boole_gate_supervisor::{codec::digest, real_descriptor::Descriptor, real_package::Package};
use serde_json::{json, Value};
fn sha(v: &Value) -> String {
    hex::encode(digest(&serde_json::to_vec(v).unwrap()))
}
fn canonical_package(v: &Value) -> anyhow::Result<Vec<u8>> {
    let keys = [
        "version",
        "mode",
        "problem",
        "env_id",
        "target",
        "statement",
        "reference_sha256",
        "inventory_sha256",
        "fuel_cap",
    ];
    let pairs = keys
        .iter()
        .map(|k| {
            Ok(format!(
                "{}:{}",
                serde_json::to_string(k)?,
                serde_json::to_string(&v[*k])?
            ))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(format!("{{{}}}", pairs.join(",")).into_bytes())
}
#[test]
fn real_package_resolves_exact_published_fourfold_caps_and_local_failure_is_not_void(
) -> anyhow::Result<()> {
    let bytes = include_bytes!("../../../scripts/zk_registry/data/fuel_cap_authority_v1.json");
    let a: Value = serde_json::from_slice(bytes)?;
    let mut d = Descriptor::decode(include_bytes!(
        "../../../fixtures/m3-real-gate/descriptor-v1.json"
    ))?;
    d.fuel_authority_sha256 = hex::encode(digest(bytes));
    let (reference, r) = a["references"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, r)| r["eligible"] == true)
        .unwrap();
    let cap = json!({"version":1,"reference":reference,"problem":r["problem"],"environment":r["environment"],"statement":r["statement"],"own_declarations":r["own_declarations"],"units":{"lean":r["units"]["lean"].as_u64().unwrap()*4,"nanoda":r["units"]["nanoda"].as_u64().unwrap()*4}});
    let pkg = json!({"version":1,"mode":"certified-own","problem":r["problem"],"env_id":r["environment"],"target":[["str","Fixture"],["str","target"]],"statement":r["statement"],"reference_sha256":r["reference_export"],"inventory_sha256":"8".repeat(64),"fuel_cap":{"cap":cap,"sha256":sha(&cap)}});
    let normal = Package::decode(&canonical_package(&pkg)?, &d, false)?;
    assert_eq!(normal.caps.lean, cap["units"]["lean"].as_u64().unwrap());
    assert!(normal.certified.is_some());
    for field in ["lean", "nanoda"] {
        let mut bad = pkg.clone();
        bad["fuel_cap"]["cap"]["units"][field] = json!(cap["units"][field].as_u64().unwrap() + 1);
        bad["fuel_cap"]["sha256"] = json!(sha(&bad["fuel_cap"]["cap"]));
        let p = Package::decode(&canonical_package(&bad)?, &d, false)?;
        assert_eq!((p.caps.lean, p.caps.nanoda), (0, 0));
    }
    let mut wrong = d;
    wrong.fuel_authority_sha256 = "9".repeat(64);
    assert!(
        Package::decode(&canonical_package(&pkg)?, &wrong, false).is_err(),
        "local authority absence became a package VOID"
    );
    Ok(())
}

#[test]
fn certified_own_caps_follow_amended_reference_environment_and_mode_rules_in_all_phases(
) -> anyhow::Result<()> {
    use boole_gate_supervisor::real_reply::{Caps, Context, Reply};
    let v: Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/m3-real-gate/cap-cases.json"
    ))?;
    let d = Descriptor::decode(&hex::decode(v["descriptor_hex"].as_str().unwrap())?)?;
    for case in v["cases"].as_array().unwrap() {
        let p = Package::decode(
            &hex::decode(case["package_hex"].as_str().unwrap())?,
            &d,
            false,
        )?;
        let caps: Caps = serde_json::from_value(case["caps"].clone())?;
        assert_eq!(
            (p.caps.lean, p.caps.nanoda),
            (caps.lean, caps.nanoda),
            "{}",
            case["name"]
        );
        let c: Context = serde_json::from_value(case["context"].clone())?;
        let reply = Reply::decode(
            &hex::decode(case["reply_hex"].as_str().unwrap())?,
            &d,
            &c,
            &p.caps,
            false,
        )?;
        let expected: Value =
            serde_json::from_slice(&hex::decode(case["reply_hex"].as_str().unwrap())?)?;
        assert_eq!(reply.fuel, expected["fuel"].as_u64());
        assert_eq!(
            reply.kernels.lean.state,
            expected["kernels"]["lean"]["state"]
        );
    }
    Ok(())
}

#[test]
fn complete_package_grammar_precedes_reference_resolution() -> anyhow::Result<()> {
    let v: Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/m3-real-gate/cap-cases.json"
    ))?;
    let d = Descriptor::decode(&hex::decode(v["descriptor_hex"].as_str().unwrap())?)?;
    let raw = String::from_utf8(hex::decode(v["cases"][0]["package_hex"].as_str().unwrap())?)?;
    let parsed: Value = serde_json::from_str(&raw)?;
    let target = serde_json::to_string(&parsed["target"])?;
    for bad in [
        raw.replace(&format!("\"target\":{target}"), "\"target\":false"),
        serde_json::to_string(&parsed)?,
        raw.replace("\"version\":1", "\"version\":1,\"version\":1"),
    ] {
        assert_ne!(bad, raw);
        let p = Package::decode(bad.as_bytes(), &d, false)?;
        assert!(
            p.reference.is_none(),
            "malformed package selected a reference: {bad}"
        );
    }
    Ok(())
}

#[test]
fn measured_reference_and_cap_environment_bind_before_submission() -> anyhow::Result<()> {
    let v: Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/m3-real-gate/cap-cases.json"
    ))?;
    let d = Descriptor::decode(&hex::decode(v["descriptor_hex"].as_str().unwrap())?)?;
    let raw = String::from_utf8(hex::decode(v["cases"][0]["package_hex"].as_str().unwrap())?)?;
    let parsed: Value = serde_json::from_str(&raw)?;
    let env = parsed["fuel_cap"]["cap"]["environment"].as_str().unwrap();
    let valid = raw.replace(
        &format!("\"env_id\":\"{}\"", "7".repeat(64)),
        &format!("\"env_id\":\"{env}\""),
    );
    let p = Package::decode(valid.as_bytes(), &d, false)?;
    assert!(p.caps.lean > 0, "positive measured-reference control");
    let wrong_ref = valid.replace(
        parsed["reference_sha256"].as_str().unwrap(),
        &"b".repeat(64),
    );
    assert_eq!(
        Package::decode(wrong_ref.as_bytes(), &d, false)?.caps.lean,
        0,
        "published reference mismatch remained executable"
    );
    let wrong_env = valid.replace(
        &format!("\"env_id\":\"{env}\""),
        &format!("\"env_id\":\"{}\"", "7".repeat(64)),
    );
    assert_eq!(
        Package::decode(wrong_env.as_bytes(), &d, false)?.caps.lean,
        0,
        "published cap env mismatch remained executable"
    );
    Ok(())
}
