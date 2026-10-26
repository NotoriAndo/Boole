//! Versioned development real-gate release; never a synthetic descriptor.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub version: u64,
    pub adapter: String,
    pub reference_interpreter: Interpreter,
    pub source_sha256: String,
    pub kernel_set_digest: String,
    pub policy: Policy,
    pub caps_sha256: String,
    pub fuel_authority_sha256: String,
    pub acceptance_environment_sha256: String,
    pub acceptance_tools_sha256: String,
    pub artifact_table_sha256: String,
    pub grammar: u64,
    pub json_nesting_limit: u64,
    pub fuel: Fuel,
    pub profiles: BTreeMap<String, Profile>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interpreter {
    pub implementation: String,
    pub version: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub version: u64,
    pub rules_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fuel {
    pub lean_weight: u64,
    pub nanoda_weight: u64,
    pub cap_charge: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub assurance: String,
    pub tools: Tools,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tools {
    pub comparator: String,
    pub nanoda: String,
    pub lean_replay: String,
    pub counter_replay: String,
    pub exporter: String,
    pub lean_runtime: String,
    pub imports: String,
    pub sandbox: String,
    pub codec: String,
}
pub fn hash(text: &str) -> Result<[u8; 32]> {
    ensure!(
        text.len() == 64
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "SHA-256 grammar"
    );
    let mut out = [0; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)?;
    }
    ensure!(out != [0; 32], "zero content address");
    Ok(out)
}
pub fn ascii<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let text = serde_json::to_string(value)?;
    let mut result = String::new();
    for c in text.chars() {
        if u32::from(c) >= 127 {
            for code in c.encode_utf16(&mut [0; 2]) {
                use std::fmt::Write;
                write!(result, "\\u{code:04x}")?;
            }
        } else {
            result.push(c);
        }
    }
    Ok(result.into_bytes())
}
impl Descriptor {
    pub fn decode(raw: &[u8]) -> Result<Self> {
        ensure!(raw.len() <= 65536, "descriptor byte cap");
        let value: Self = serde_json::from_slice(raw)?;
        ensure!(ascii(&value)? == raw, "noncanonical descriptor");
        ensure!(
            value.version == 1 && value.adapter == "boole-m3-real/v1",
            "unsupported descriptor"
        );
        ensure!(
            value.reference_interpreter.implementation == "CPython"
                && value.reference_interpreter.version == "3.9.6",
            "reference interpreter"
        );
        ensure!(
            value.grammar == 1 && value.json_nesting_limit == 127,
            "grammar/nesting version"
        );
        for pin in [
            &value.source_sha256,
            &value.kernel_set_digest,
            &value.caps_sha256,
            &value.fuel_authority_sha256,
            &value.acceptance_environment_sha256,
            &value.acceptance_tools_sha256,
            &value.artifact_table_sha256,
        ] {
            hash(pin)?;
        }
        ensure!(
            value.policy.version == 2
                && hash(&value.policy.rules_sha256)?
                    == crate::codec::digest(include_bytes!(
                        "../../../scripts/zk_registry/data/error_policy_v2.json"
                    )),
            "policy v2 release"
        );
        ensure!(
            value.fuel.lean_weight > 0
                && value.fuel.nanoda_weight > 0
                && value.fuel.cap_charge == "bounded-debit/v2",
            "fuel policy"
        );
        ensure!(value.profiles.len() == 2, "complete profile map");
        for (name, assurance) in [
            ("Darwin-arm64", "development"),
            ("Linux-aarch64", "production"),
        ] {
            let profile = value
                .profiles
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("missing qualified profile"))?;
            ensure!(profile.assurance == assurance, "profile assurance");
            let tools = serde_json::to_value(&profile.tools)?;
            for pin in tools.as_object().expect("tool pins").values() {
                hash(pin.as_str().expect("string pin"))?;
            }
        }
        #[derive(Serialize)]
        struct Material<'a> {
            acceptance_environment_sha256: &'a str,
            acceptance_tools_sha256: &'a str,
            profiles: &'a BTreeMap<String, Profile>,
        }
        let material = ascii(&Material {
            acceptance_environment_sha256: &value.acceptance_environment_sha256,
            acceptance_tools_sha256: &value.acceptance_tools_sha256,
            profiles: &value.profiles,
        })?;
        ensure!(
            hash(&value.kernel_set_digest)?
                == crate::codec::digest(
                    &[b"BOOLE-M3/KERNEL-SET/v1\0".as_slice(), &material].concat()
                ),
            "kernel-set digest"
        );
        Ok(value)
    }
    pub fn digest(&self) -> [u8; 32] {
        crate::codec::digest(
            &[
                b"BOOLE-M3/REAL-GATE-DESCRIPTOR/v1\0".as_slice(),
                &ascii(self).expect("integer descriptor"),
            ]
            .concat(),
        )
    }
}
