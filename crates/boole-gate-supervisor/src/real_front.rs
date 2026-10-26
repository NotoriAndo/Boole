//! Private materialization protocol v1; separate from transport-decoder v2.
//! No kernel or proof is run here.
use crate::{
    real_deadline::Deadline,
    real_descriptor::{ascii, Descriptor},
    real_reply::Context,
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Phase {
    version: u8,
    context_sha256: String,
    gate_rule_digest: String,
    descriptor_sha256: String,
    mode: String,
    public_counter: bool,
    pub need: String,
    reply: Option<String>,
}
impl Phase {
    pub fn decode(
        raw: &[u8],
        d: &Descriptor,
        c: &Context,
        mode: &str,
        public_counter: bool,
        deadline: Deadline,
    ) -> Result<Self> {
        deadline.check()?;
        ensure!(raw.len() <= 65536, "preparation channel cap");
        let v: Self = serde_json::from_slice(raw)?;
        let hex = |b: &[u8]| boole_derivation::attributes::hex_string(b)[2..].to_owned();
        ensure!(
            ascii(&v)? == raw
                && v.version == 1
                && v.context_sha256 == hex(&c.digest()?)
                && v.gate_rule_digest == hex(&d.digest())
                && v.descriptor_sha256 == hex(&crate::codec::digest(&ascii(d)?))
                && v.mode == mode
                && v.public_counter == public_counter,
            "preparation channel binding"
        );
        ensure!(
            ["reference", "state", "ready", "reply"].contains(&v.need.as_str())
                && (v.need == "reply") == v.reply.is_some(),
            "preparation channel state"
        );
        ensure!(
            (mode == "reference" && ["reference", "reply"].contains(&v.need.as_str()))
                || (mode == "state" && ["state", "ready", "reply"].contains(&v.need.as_str())),
            "reference boundary skipped"
        );
        Ok(v)
    }
    pub fn reply(&self) -> Result<Option<Vec<u8>>> {
        self.reply
            .as_ref()
            .map(|s| {
                ensure!(
                    s.len() % 2 == 0
                        && s.bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                    "preparation reply encoding"
                );
                (0..s.len())
                    .step_by(2)
                    .map(|n| Ok(u8::from_str_radix(&s[n..n + 2], 16)?))
                    .collect()
            })
            .transpose()
    }
}

/// Preparation cannot claim completed mathematical work, even with a valid row.
pub fn check_reply(
    raw: &[u8],
    d: &Descriptor,
    c: &Context,
    caps: &crate::real_reply::Caps,
    authenticated: &crate::real_reply::AuthenticatedStatement,
) -> Result<()> {
    let r = crate::real_reply::Reply::decode_with_statement(raw, d, c, caps, false, authenticated)?;
    ensure!(
        r.outcome != "ACCEPT"
            && r.counter_identity.is_none()
            && r.fuel == Some(0)
            && r.kernels.lean.state == "not-run"
            && r.kernels.nanoda.state == "not-run",
        "preparation cannot execute kernels or accept"
    );
    Ok(())
}
