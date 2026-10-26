//! Local stub release retention, outside the protected gate source closure.
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::hex_string,
    m2::{Config, Hash},
    m3::ForkTable,
    ParentHeader,
};
use boole_devnet_archive::{
    error::availability,
    journal::{read_bound, InputJournal},
    pins::{ArtifactRole, ArtifactSource, HeaderBinding, PinStore},
};
use boole_gate_supervisor::{
    codec::digest,
    process::{host_profile, Worker},
    stub::Descriptor,
};
use std::{collections::BTreeSet, path::Path};

pub fn open(source: &InputJournal, checkpoint: Option<&Path>) -> Result<PinStore> {
    if !source
        .root()
        .join(boole_devnet_archive::pins::DIR)
        .try_exists()?
    {
        if let Some(path) = checkpoint.filter(|p| p.exists()) {
            let (raw, _) = read_bound(path, 65536)?;
            let cp: serde_json::Value = serde_json::from_slice(&raw)?;
            if cp.get("pin_journal").is_some() || cp.get("record").is_some() {
                return Err(availability(
                    0,
                    "pin-journal",
                    "S20: modern checkpoint lost its pin store; restore verified bytes",
                ));
            }
        }
    }
    PinStore::open(source)
}
/// Prefer the retained release, never silently reacquire a lost indexed object.
pub fn workers(
    pins: &mut PinStore,
    c: &Config,
    forks: &ForkTable,
    supplied: &[Worker],
) -> Result<Vec<Worker>> {
    let mut out = vec![];
    for row in &forks.rows {
        let gate = row.gate_rule_digest;
        let role = ArtifactRole::StubDescriptor {
            epoch: row.epoch,
            gate,
        };
        let descriptor = if let Some(a) = pins.view().artifact(&role) {
            let (bytes, _) = read_bound(&pins.view().artifact_path(a.digest), 65536)?;
            let d: Descriptor = serde_json::from_slice(&bytes)?;
            ensure!(d.digest() == gate, "S20: retained descriptor binding");
            d
        } else {
            let worker = supplied
                .iter()
                .find(|w| w.descriptor.digest() == gate)
                .ok_or_else(|| {
                    availability(0, "pin-gate", "S20: epoch stub descriptor is missing")
                })?;
            if worker.descriptor.adapter != "boole-devnet-stub-v1" {
                boole_devnet_archive::pins::RealGateInventoryPending {
                    epoch: row.epoch,
                    gate,
                }
                .require_inventory()?;
            }
            let raw = serde_json::to_vec(&worker.descriptor)?;
            pins.install_bytes(role, digest(&raw), &raw, false)?;
            worker.descriptor.clone()
        };
        descriptor.validate(c.chain_id, &host_profile())?;
        let role = ArtifactRole::StubBinary {
            epoch: row.epoch,
            gate,
            profile: host_profile(),
        };
        let expected = *descriptor
            .binaries
            .get(&host_profile())
            .context("S20: host binary pin")?;
        if pins.view().artifact(&role).is_none() {
            // A new descriptor/epoch may bind the very same host binary. Reuse
            // already verified own bytes even if its original path is gone.
            let retained = pins.view().artifact_path(expected);
            let path = if retained.try_exists()? {
                retained
            } else {
                supplied
                    .iter()
                    .find(|w| w.descriptor.digest() == gate && w.binary_sha256 == expected)
                    .ok_or_else(|| {
                        availability(0, "pin-gate", "S20: epoch host binary is missing")
                    })?
                    .binary
                    .clone()
            };
            Worker {
                descriptor: descriptor.clone(),
                binary: path.clone(),
                binary_sha256: expected,
            }
            .validate()?;
            pins.install_artifacts(&[ArtifactSource {
                role,
                digest: expected,
                path,
                executable: true,
            }])?;
        }
        pins.view()
            .verify_releases(&[(row.epoch, gate)].into(), &BTreeSet::new())?;
        let worker = Worker {
            descriptor,
            binary: pins.view().artifact_path(expected),
            binary_sha256: expected,
        };
        worker.validate()?;
        if !out.iter().any(|w: &Worker| w.descriptor.digest() == gate) {
            out.push(worker);
        }
    }
    Ok(out)
}
pub fn environment(pins: &mut PinStore, c: &Config, original: &Path) -> Result<()> {
    let required: BTreeSet<Hash> = std::iter::once(c.base_root)
        .chain(c.env_allowed.iter().flatten().copied())
        .collect();
    for component in &required {
        let role = ArtifactRole::Environment {
            component: *component,
        };
        if pins.view().artifact(&role).is_none() {
            pins.install_artifacts(&[ArtifactSource {
                role,
                digest: *component,
                path: original.join(hex_string(component)),
                executable: false,
            }])?;
        }
    }
    pins.view().verify_releases(&BTreeSet::new(), &required)
}
pub fn payload_binding(payload: &serde_json::Value, root: Hash) -> Result<HeaderBinding> {
    let transactions: Vec<String> = serde_json::from_value(payload["transactions"].clone())?;
    let mut header: ParentHeader = serde_json::from_value(serde_json::json!({
        "number":payload["blockNumber"], "hash":payload["blockHash"], "parentHash":payload["parentHash"],
        "timestamp":payload["timestamp"], "gasLimit":payload["gasLimit"],
        "miner":payload["feeRecipient"], "extraData":payload["extraData"]
    }))?;
    header.l1_info = transactions.first().cloned();
    header.m2_root_deposit = transactions.get(1).cloned();
    Ok(HeaderBinding {
        header,
        transactions,
        root,
    })
}
