mod support;
use boole_derivation::m2::{Hash, Status};
use boole_gate_supervisor::{
    codec::Scratch,
    real_descriptor::Descriptor,
    real_reply::{Context, Reply},
};

#[test]
fn declared_models_reconstruct_through_the_fold_and_match_contract_vectors() -> anyhow::Result<()> {
    let vectors: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/m3-real-gate/open-layout-cases.json"
    ))?;
    let rows = vectors["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 56);
    let mut accepted = 0;
    let mut refused = 0;
    let mut errors = vec![];
    for case in rows {
        let decode = |key: &str| hex::decode(case[key].as_str().unwrap());
        let hashes = |key: &str| -> anyhow::Result<Vec<Hash>> {
            case[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| boole_gate_supervisor::real_descriptor::hash(p.as_str().unwrap()))
                .collect()
        };
        let pkgs = hashes("packages")?;
        let required = hashes("required")?;
        let mut c = support::scenario::m3_cfg();
        c.pkg_table = pkgs.clone();
        c.env_allowed = [pkgs.clone()].into();
        c.incompatible.clear();
        c.slot_table[0].lane = case["lane"].as_u64().unwrap() as u8;
        let body = decode("body_hex")?;
        let (c, s, key, mut history) = support::fixture_with_layout(
            c,
            None,
            777,
            &pkgs,
            Some(decode("package_hex")?),
            Some(decode("bundle_hex")?),
            &decode("model_hex")?,
            &required,
            Some(&body),
        );
        if !case["accepted"].as_bool().unwrap() {
            assert_ne!(
                s.candidates[&key].status(),
                Status::Checking,
                "{}",
                case["name"]
            );
            refused += 1;
            continue;
        }
        assert_eq!(
            s.candidates[&key].status(),
            Status::Checking,
            "{}",
            case["name"]
        );
        let scratch = Scratch::new(&std::env::temp_dir())?;
        let rebuilt = boole_gate_supervisor::real_rebuild::rebuild_real(
            &s,
            &c,
            key,
            &mut history,
            &scratch.0,
        );
        let rebuilt = match rebuilt {
            Ok(rebuilt) => rebuilt,
            Err(e) => {
                errors.push(format!("{}: {e:#}", case["name"]));
                continue;
            }
        };
        assert_eq!(rebuilt.body, body);
        let context = Context::from_input(&rebuilt.input);
        assert_eq!(
            serde_json::to_value(&context)?,
            case["context"],
            "{}",
            case["name"]
        );
        let d = Descriptor::decode(&decode("descriptor_hex")?)?;
        let (p, early) = boole_gate_supervisor::real_admission::authenticate(
            &rebuilt.input.package,
            &decode("inventory_hex")?,
            &d,
            &context,
            true,
            false,
        )?;
        let reply = if let Some(reply) = early {
            reply
        } else {
            boole_gate_supervisor::real_admission::transport(
                &d,
                &context,
                &p,
                Some(&decode("bundle_hex")?),
                None,
            )?
            .expect("submission decision")
        };
        assert_eq!(hex::encode(&reply), case["reply_hex"], "{}", case["name"]);
        let parsed =
            Reply::decode_with_statement(&reply, &d, &context, &p.caps, false, &p.authenticated)?;
        if context.lane == 1 {
            assert_eq!((parsed.outcome.as_str(), parsed.reason), ("REJECT", 0x0106));
            let job =
                s.m3.as_ref()
                    .unwrap()
                    .queue
                    .iter()
                    .find(|j| j.key == key)
                    .unwrap();
            let record = boole_gate_supervisor::real_supervisor::checked_real_record(
                &rebuilt.input,
                &d,
                job,
                0,
                &reply,
                &p.caps,
                false,
            )?;
            assert_eq!(
                hex::encode(record.encode()?),
                case["record_hex"],
                "{}",
                case["name"]
            );
        } else {
            assert_eq!(parsed.outcome, "DEFER");
        }
        accepted += 1;
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    assert_eq!((accepted, refused), (28, 28));
    Ok(())
}
