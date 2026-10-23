//! Deliberately synthetic gate for the closed devnet only.
use crate::{GateInput, GateReply};
use boole_derivation::m2::Hash;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub adapter: String,
    pub revision: u32,
    pub fixed: bool,
    pub generator: bool,
    pub profiles: Vec<String>,
    pub kernel_set_digest: Hash,
    pub binaries: std::collections::BTreeMap<String, Hash>,
    pub implementation_sha256: Hash,
}
impl Default for Descriptor {
    fn default() -> Self {
        Self {
            adapter: "boole-devnet-stub-v1".into(),
            revision: 1,
            fixed: false,
            generator: true,
            profiles: vec![
                "darwin-aarch64".into(),
                "linux-aarch64".into(),
                "linux-x86_64".into(),
            ],
            kernel_set_digest: crate::codec::digest(b"BOOLE-M3/STUB-KERNELS/v1"),
            binaries: std::collections::BTreeMap::new(),
            implementation_sha256: crate::implementation_digest(),
        }
    }
}
impl Descriptor {
    pub fn pinned(binary_sha256: Hash) -> Self {
        let profile = crate::process::host_profile();
        Self {
            profiles: vec![profile.clone()],
            binaries: [(profile, binary_sha256)].into(),
            ..Self::default()
        }
    }

    pub fn digest(&self) -> Hash {
        crate::codec::digest(
            &[
                b"BOOLE-M3/GATE-DESCRIPTOR/v1".as_slice(),
                &serde_json::to_vec(self).expect("integer descriptor"),
            ]
            .concat(),
        )
    }
    pub fn validate(&self, chain: u64, profile: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            chain == 720926,
            "S16: synthetic stub refused outside devnet"
        );
        anyhow::ensure!(
            self.adapter == "boole-devnet-stub-v1" && self.revision == 1,
            "S16: unknown stub descriptor"
        );
        anyhow::ensure!(
            self.profiles.iter().any(|p| p == profile),
            "S16: host profile not listed"
        );
        anyhow::ensure!(
            self.profiles.len() == self.binaries.len()
                && self
                    .profiles
                    .iter()
                    .all(|p| self.binaries.get(p).is_some_and(|h| *h != [0; 32])),
            "S16: missing binary artifact pin"
        );
        anyhow::ensure!(
            self.implementation_sha256 != [0; 32],
            "S16: missing adapter source release"
        );
        anyhow::ensure!(
            self.kernel_set_digest == crate::codec::digest(b"BOOLE-M3/STUB-KERNELS/v1"),
            "S16: stub kernel pin"
        );
        Ok(())
    }
}
pub fn evaluate(descriptor: &Descriptor, input: &GateInput) -> GateReply {
    use crate::GateResult;
    let target = crate::codec::digest(
        &[
            b"BOOLE-M3/STUB-STATEMENT/v1".as_slice(),
            &input.package,
            &input.instance_seed,
        ]
        .concat(),
    );
    let result = |outcome, reason, fuel| {
        let ext = if outcome == 1 && input.lane == 2 {
            [
                vec![1, 1],
                1u64.to_be_bytes().to_vec(),
                crate::codec::digest(&input.raw).to_vec(),
                crate::codec::digest(b"stub source").to_vec(),
            ]
            .concat()
        } else {
            vec![]
        };
        GateReply::Result {
            value: GateResult {
                outcome,
                reason,
                fuel,
                statement_id: target,
                ext,
            },
        }
    };
    if let Some((outcome, reason)) = input.early {
        return result(outcome, reason, 0);
    }
    let prefix = b"BOOLE-M3-STUB-PACKAGE/v1";
    if input.package.len() != prefix.len() + 40 || !input.package.starts_with(prefix) {
        return result(3, 0x0801, 0);
    }
    let cap = u64::from_be_bytes(
        input.package[prefix.len()..prefix.len() + 8]
            .try_into()
            .expect("length"),
    );
    if cap > input.fuel_slot || cap == 0 {
        return result(3, 0x0803, 0);
    }
    if input.leaf_kind == 2 && !descriptor.generator {
        return result(3, 0x0804, 0);
    }
    let prefix = b"BOOLE-M3-STUB/v1";
    if !input.raw.starts_with(prefix) || input.raw.len() < prefix.len() + 9 {
        return result(2, 0x0104, 0);
    }
    let tag = input.raw[prefix.len()];
    let fuel = u64::from_be_bytes(
        input.raw[prefix.len() + 1..prefix.len() + 9]
            .try_into()
            .expect("length"),
    );
    if fuel > cap {
        return result(2, 0x0303, cap);
    }
    match tag {
        0 => result(1, 0, fuel),
        1 => result(2, 0x0301, fuel),
        2 => result(2, 0x0306, fuel),
        3 => result(3, 0x0801, 0),
        4 => result(3, 0x0803, 0),
        5 => result(3, 0x0804, 0),
        6 if !descriptor.fixed => GateReply::Defer {
            class: "gate-bug".into(),
        },
        6 => result(1, 0, fuel),
        7 if descriptor.fixed => result(2, 0x0301, fuel),
        7 => result(1, 0, fuel),
        8 | 9 => GateReply::Defer {
            class: "untyped-exit".into(),
        },
        10 if !descriptor.fixed => result(2, 0x0306, fuel),
        10 => result(1, 0, fuel),
        _ => result(2, 0x0104, 0),
    }
}
pub fn package(fuel_cap: u64) -> Vec<u8> {
    [
        b"BOOLE-M3-STUB-PACKAGE/v1".as_slice(),
        &fuel_cap.to_be_bytes(),
        &crate::codec::digest(b"stub target"),
    ]
    .concat()
}
pub fn export(tag: u8, fuel: u64) -> Vec<u8> {
    [
        b"BOOLE-M3-STUB/v1".as_slice(),
        &[tag],
        &fuel.to_be_bytes(),
        b"opaque development delta",
    ]
    .concat()
}
