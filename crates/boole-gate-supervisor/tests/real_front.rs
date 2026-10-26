use anyhow::Result;
use boole_gate_supervisor::{
    codec::digest,
    real_deadline::Deadline,
    real_descriptor::{ascii, Descriptor},
    real_front::Phase,
    real_reply::{AuthenticatedStatement, Caps, Context},
};
use serde::Serialize;
#[derive(Serialize, Clone)]
struct Packet {
    version: u8,
    context_sha256: String,
    gate_rule_digest: String,
    descriptor_sha256: String,
    mode: String,
    public_counter: bool,
    need: String,
    reply: Option<String>,
}
#[test]
fn private_front_packet_is_bound_and_cannot_claim_an_early_acceptance() -> Result<()> {
    let d = Descriptor::decode(include_bytes!(
        "../../../fixtures/m3-real-gate/descriptor-v1.json"
    ))?;
    let v: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../fixtures/m3-real-gate/reply-cases.json"
    ))?;
    let c: Context = serde_json::from_value(v["context"].clone())?;
    let caps: Caps = serde_json::from_value(v["caps"].clone())?;
    let good = Packet {
        version: 1,
        context_sha256: hex::encode(c.digest()?),
        gate_rule_digest: hex::encode(d.digest()),
        descriptor_sha256: hex::encode(digest(&ascii(&d)?)),
        mode: "reference".into(),
        public_counter: true,
        need: "reference".into(),
        reply: None,
    };
    Phase::decode(
        &ascii(&good)?,
        &d,
        &c,
        "reference",
        true,
        Deadline::after(1000),
    )?;
    for field in [
        "version",
        "context",
        "gate",
        "descriptor",
        "mode",
        "qualification",
        "state-order",
        "fabricated-reply",
    ] {
        let mut p = good.clone();
        match field {
            "version" => p.version = 2,
            "context" => p.context_sha256 = "f".repeat(64),
            "gate" => p.gate_rule_digest = "f".repeat(64),
            "descriptor" => p.descriptor_sha256 = "f".repeat(64),
            "mode" => p.mode = "state".into(),
            "qualification" => p.public_counter = false,
            "state-order" => p.need = "state".into(),
            _ => p.reply = Some("00".into()),
        }
        assert!(
            Phase::decode(
                &ascii(&p)?,
                &d,
                &c,
                "reference",
                true,
                Deadline::after(1000)
            )
            .is_err(),
            "{field}"
        );
    }
    let accepted = v["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "accept")
        .unwrap();
    assert!(boole_gate_supervisor::real_front::check_reply(
        &hex::decode(accepted["raw_hex"].as_str().unwrap())?,
        &d,
        &c,
        &caps,
        &AuthenticatedStatement::Valid(caps.statement.clone())
    )
    .is_err());
    Ok(())
}
