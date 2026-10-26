use boole_gate_supervisor::{
    real_descriptor::Descriptor,
    real_reply::{Caps, Context, Reply},
};
#[test]
fn closed_real_reply_rows_kernel_debits_identity_void_and_leaf_cap() {
    let d = Descriptor::decode(include_bytes!(
        "../../../fixtures/m3-real-gate/descriptor-v1.json"
    ))
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/m3-real-gate/reply-cases.json"
    ))
    .unwrap();
    let c: Context = serde_json::from_value(v["context"].clone()).unwrap();
    let caps: Caps = serde_json::from_value(v["caps"].clone()).unwrap();
    let mut errors = vec![];
    for case in v["cases"].as_array().unwrap() {
        let raw = hex::decode(case["raw_hex"].as_str().unwrap()).unwrap();
        let auth = match case["authenticated"].as_str().unwrap_or("valid") {
            "no-valid" => {
                boole_gate_supervisor::real_reply::AuthenticatedStatement::NoValidStatement
            }
            "unknown" | "none" => {
                boole_gate_supervisor::real_reply::AuthenticatedStatement::Unknown
            }
            _ => boole_gate_supervisor::real_reply::AuthenticatedStatement::Valid(
                caps.statement.clone(),
            ),
        };
        let got = Reply::decode_with_statement(
            &raw,
            &d,
            &c,
            &caps,
            case["fork_authorized"].as_bool().unwrap(),
            &auth,
        );
        if got.is_ok() != case["valid"].as_bool().unwrap() {
            errors.push((case["name"].clone(), got.err().map(|e| e.to_string())));
        }
    }
    assert!(
        errors.is_empty(),
        "real reply validation disagreements: {errors:?}"
    );
}

#[test]
fn quarantine_fallback_requires_the_explicit_completed_authentication_sentinel(
) -> anyhow::Result<()> {
    use boole_gate_supervisor::{codec::digest, real_reply::AuthenticatedStatement as Auth};
    let d = Descriptor::decode(include_bytes!(
        "../../../fixtures/m3-real-gate/descriptor-v1.json"
    ))?;
    let fixture: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/m3-real-gate/reply-cases.json"
    ))?;
    let c: Context = serde_json::from_value(fixture["context"].clone())?;
    let caps = Caps {
        lean: 0,
        nanoda: 0,
        statement: "0".repeat(64),
    };
    let raw = Reply::reconstructed_with_statement(
        "quarantine_void",
        &d,
        &c,
        &caps,
        true,
        &Auth::NoValidStatement,
    )?;
    let reply = Reply::decode_with_statement(&raw, &d, &c, &caps, true, &Auth::NoValidStatement)?;
    let mut bytes = b"BOOLE-M3/QUARANTINE-STATEMENT/v1\0".to_vec();
    bytes.extend(c.chain_id.to_be_bytes());
    for s in [&c.problem_id, &c.commit_hash, &c.env_id, &c.package_sha256] {
        bytes.extend(hex::decode(s)?);
    }
    bytes.extend(0x0802u16.to_be_bytes());
    assert_eq!(reply.statement_id, hex::encode(digest(&bytes)));
    for (fork, auth) in [
        (false, Auth::NoValidStatement),
        (true, Auth::Unknown),
        (true, Auth::Valid("a".repeat(64))),
    ] {
        assert!(Reply::decode_with_statement(&raw, &d, &c, &caps, fork, &auth).is_err());
    }
    for s in ["0".repeat(64), c.void_statement()?, "b".repeat(64)] {
        let mut wrong = reply.clone();
        wrong.statement_id = s;
        assert!(Reply::decode_with_statement(
            &boole_gate_supervisor::real_descriptor::ascii(&wrong)?,
            &d,
            &c,
            &caps,
            true,
            &Auth::NoValidStatement
        )
        .is_err());
    }
    Ok(())
}
