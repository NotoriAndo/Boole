//! A1's package/cap decisions, before decoder, reference or certified-state I/O.
use crate::{
    real_descriptor::{ascii, hash, Descriptor},
    real_json,
    real_package::Package,
    real_reply::{AuthenticatedStatement, Context, Reply},
};
use anyhow::{ensure, Result};
pub fn authenticate(
    raw: &[u8],
    inventory: &[u8],
    d: &Descriptor,
    c: &Context,
    public_counter: bool,
    fork: bool,
) -> Result<(Package, Option<Vec<u8>>)> {
    ensure!(
        crate::codec::digest(inventory) == hash(&d.artifact_table_sha256)?,
        "inventory address"
    );
    let i = real_json::parse(inventory, &["version", "environment", "base", "library"])?;
    #[derive(serde::Serialize)]
    struct Inventory<'a> {
        version: &'a serde_json::Value,
        environment: &'a serde_json::Value,
        base: &'a serde_json::Value,
        library: &'a serde_json::Value,
    }
    ensure!(
        ascii(&Inventory {
            version: &i["version"],
            environment: &i["environment"],
            base: &i["base"],
            library: &i["library"]
        })? == inventory,
        "sorted inventory maps"
    );
    ensure!(i["version"].as_u64() == Some(1), "inventory version");
    hash(
        i["environment"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("inventory environment"))?,
    )?;
    for group in ["base", "library"] {
        let m = i[group]
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("inventory group"))?;
        for (name, digest) in m {
            let n: serde_json::Value = serde_json::from_str(name)?;
            real_json::target(&n)?;
            ensure!(ascii(&n)? == name.as_bytes(), "inventory name");
            hash(
                digest
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("inventory payload digest"))?,
            )?;
        }
    }
    ensure!(
        !i["base"]
            .as_object()
            .unwrap()
            .keys()
            .any(|k| i["library"].get(k).is_some()),
        "ambiguous inventory"
    );
    ensure!(
        crate::codec::digest(raw) == hash(&c.package_sha256)?,
        "package address"
    );
    let p = Package::decode(raw, d, public_counter)?;
    if matches!(p.authenticated, AuthenticatedStatement::Valid(_)) {
        let v: serde_json::Value = serde_json::from_slice(raw)?;
        ensure!(
            v["env_id"] == i["environment"]
                && v["inventory_sha256"].as_str() == Some(d.artifact_table_sha256.as_str()),
            "package outside local inventory/environment"
        );
    }
    let row = if fork {
        Some("quarantine_void")
    } else if p.authenticated == AuthenticatedStatement::NoValidStatement {
        Some("package_invalid")
    } else if p.caps.lean == 0
        || p.caps.nanoda == 0
        || (p.certified.is_some()
            && p.caps
                .lean
                .checked_mul(d.fuel.lean_weight)
                .and_then(|l| {
                    p.caps
                        .nanoda
                        .checked_mul(d.fuel.nanoda_weight)
                        .and_then(|n| l.checked_add(n))
                })
                .is_none_or(|total| total > c.slot_fuel))
    {
        Some("fuel_binding_invalid")
    } else if c.leaf_kind == 2 {
        Some("generator_unavailable")
    } else if c.lane != 1 {
        Some("node_fault")
    } else {
        None
    };
    let local = row
        .map(|r| Reply::reconstructed_with_statement(r, d, c, &p.caps, fork, &p.authenticated))
        .transpose()?;
    Ok((p, local))
}

/// Submission transport decisions after complete package/cap/kind/lane checks.
/// `decoder` pins are trusted controller bindings, never packet-selected values.
pub fn transport(
    d: &Descriptor,
    c: &Context,
    p: &Package,
    bundle: Option<&[u8]>,
    decoder: Option<(
        &[u8],
        boole_derivation::m2::Hash,
        boole_derivation::m2::Hash,
    )>,
) -> Result<Option<Vec<u8>>> {
    let row = if let Some((status, pin, transport)) = decoder {
        match crate::real_decoder::Status::decode(status, d, c, pin, transport) {
            Ok(s) => s.decision(bundle, c)?,
            Err(_) => Some("node_fault"),
        }
    } else {
        None
    };
    let row = if row.is_some() {
        row
    } else if let Some(raw) = bundle {
        if crate::codec::digest(raw) != hash(&c.export_digest)? {
            Some("input_integrity")
        } else {
            // Package authentication already checked this scalar grammar.
            let environment = match &p.authenticated {
                AuthenticatedStatement::Valid(_) => p.environment.as_deref(),
                _ => None,
            };
            if environment != Some(c.env_id.as_str()) {
                Some("front_invalid")
            } else if raw.len() as u64 > boole_derivation::m2::RAW_CAP {
                Some("structural_cap")
            } else {
                const MAGIC: &[u8] = b"BOOLE-M3/EXPORT-BUNDLE/v1\0";
                let framed = raw
                    .strip_prefix(MAGIC)
                    .and_then(|v| {
                        v.get(..8)
                            .map(|len| (v, u64::from_be_bytes(len.try_into().expect("length"))))
                    })
                    .is_some_and(|(v, n)| n > 0 && n == v.len() as u64 - 8);
                if !framed {
                    Some("front_invalid")
                } else {
                    None
                }
            }
        }
    } else {
        Some("node_fault")
    };
    row.map(|name| {
        Reply::reconstructed_with_statement(name, d, c, &p.caps, false, &p.authenticated)
    })
    .transpose()
}
