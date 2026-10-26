//! Package allowances resolved from the pinned authority, independent of the
//! Python adapter. No fitted value or factor is changed by this consumer.
use crate::{
    codec::digest,
    real_descriptor::{hash, Descriptor},
    real_reply::Caps,
};
use anyhow::{ensure, Context, Result};
use boole_derivation::m2::Hash;
use serde_json::{json, Value};
#[derive(Debug)]
pub struct Package {
    pub caps: Caps,
    pub reference: Option<Hash>,
    pub certified: Option<(String, Hash)>,
    pub environment: Option<String>,
    pub authenticated: crate::real_reply::AuthenticatedStatement,
}
fn exact(v: &Value, keys: &[&str]) -> Result<()> {
    let object = v.as_object().context("package object")?;
    ensure!(
        object.len() == keys.len() && keys.iter().all(|k| object.contains_key(*k)),
        "package fields"
    );
    Ok(())
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().context("package string")
}
fn positive(v: &Value) -> Result<u64> {
    let n = v.as_u64().context("package u64")?;
    ensure!(n > 0, "positive package allowance");
    Ok(n)
}
fn sorted_digest(v: &Value) -> Result<String> {
    Ok(boole_derivation::attributes::hex_string(&digest(&serde_json::to_vec(v)?))[2..].into())
}
impl Package {
    pub fn decode(raw: &[u8], d: &Descriptor, public_counter: bool) -> Result<Self> {
        let zero = || Self {
            caps: Caps {
                lean: 0,
                nanoda: 0,
                statement: "0".repeat(64),
            },
            reference: None,
            certified: None,
            environment: None,
            authenticated: crate::real_reply::AuthenticatedStatement::NoValidStatement,
        };
        let parsed = (|| -> Result<_> {
            ensure!(raw.len() <= 1048576, "package cap");
            let v = crate::real_json::parse(
                raw,
                &[
                    "version",
                    "mode",
                    "problem",
                    "env_id",
                    "target",
                    "statement",
                    "reference_sha256",
                    "inventory_sha256",
                    "fuel_cap",
                ],
            )?;
            crate::real_json::target(&v["target"])?;
            ensure!(
                v["version"].as_u64() == Some(1)
                    && ["full", "certified-own"].contains(&text(&v, "mode")?),
                "package grammar"
            );
            let statement = text(&v, "statement")?.to_owned();
            hash(&statement)?;
            hash(text(&v, "env_id")?)?;
            hash(text(&v, "inventory_sha256")?)?;
            let reference = hash(text(&v, "reference_sha256")?)?;
            let problem = text(&v, "problem")?;
            ensure!(
                !problem.is_empty() && problem.len() <= 1024,
                "problem label"
            );
            Ok((v, statement, reference))
        })();
        let Ok((v, statement, reference)) = parsed else {
            return Ok(zero());
        };
        let invalid = || Self {
            caps: Caps {
                lean: 0,
                nanoda: 0,
                statement: statement.clone(),
            },
            reference: Some(reference),
            certified: None,
            environment: Some(
                text(&v, "env_id")
                    .expect("authenticated environment")
                    .into(),
            ),
            authenticated: crate::real_reply::AuthenticatedStatement::Valid(statement.clone()),
        };
        if v["mode"] == "full" {
            if !public_counter {
                return Ok(invalid());
            }
            return Ok(Self {
                caps: Caps {
                    lean: 262_144_000_000_000,
                    nanoda: 34_359_738_368_000_000,
                    statement,
                },
                reference: Some(reference),
                certified: None,
                environment: Some(text(&v, "env_id")?.into()),
                authenticated: crate::real_reply::AuthenticatedStatement::Valid(
                    text(&v, "statement")?.into(),
                ),
            });
        }
        let resolved = (|| -> Result<_> {
            let authority_bytes =
                include_bytes!("../../../scripts/zk_registry/data/fuel_cap_authority_v1.json");
            ensure!(
                hash(&d.fuel_authority_sha256)? == digest(authority_bytes),
                "local authority release mismatch"
            );
            let a: Value = serde_json::from_slice(authority_bytes)?;
            ensure!(
                a["version"].as_u64() == Some(1) && a["factors"] == json!({"lean":4,"nanoda":4}),
                "fourfold authority"
            );
            let binding = &v["fuel_cap"];
            exact(binding, &["cap", "sha256"])?;
            let cap = &binding["cap"];
            exact(
                cap,
                &[
                    "version",
                    "reference",
                    "problem",
                    "environment",
                    "statement",
                    "own_declarations",
                    "units",
                ],
            )?;
            ensure!(
                cap["version"].as_u64() == Some(1)
                    && sorted_digest(cap)? == text(binding, "sha256")?,
                "cap digest/version"
            );
            for k in ["reference", "environment", "statement", "own_declarations"] {
                hash(text(cap, k)?)?;
            }
            let reference_row = &a["references"][text(cap, "reference")?];
            ensure!(
                reference_row["path"] == "certified-own-delta/v1"
                    && reference_row["eligible"].as_bool() == Some(true)
                    && sorted_digest(reference_row)? == text(cap, "reference")?,
                "measured reference"
            );
            for k in ["problem", "environment", "statement", "own_declarations"] {
                ensure!(reference_row[k] == cap[k], "cap reference binding");
            }
            ensure!(
                cap["problem"] == v["problem"]
                    && cap["statement"] == v["statement"]
                    && cap["environment"] == v["env_id"],
                "package cap identity"
            );
            ensure!(
                hash(text(reference_row, "reference_export")?)? == reference,
                "package measured reference binding"
            );
            exact(&cap["units"], &["lean", "nanoda"])?;
            exact(&reference_row["units"], &["lean", "nanoda"])?;
            let lean = positive(&cap["units"]["lean"])?;
            let nanoda = positive(&cap["units"]["nanoda"])?;
            for (k, value, maximum) in [
                ("lean", lean, 262_144_000_000_000u64),
                ("nanoda", nanoda, 34_359_738_368_000_000u64),
            ] {
                ensure!(
                    positive(&reference_row["units"][k])?.checked_mul(4) == Some(value)
                        && value <= maximum,
                    "exact fourfold package cap"
                );
            }
            let environment = text(cap, "environment")?.to_owned();
            let env = &a["environments"][&environment];
            ensure!(
                env["eligible"].as_bool() == Some(true)
                    && env["certified_kernels"] == json!(["lean", "nanoda"]),
                "certified environment"
            );
            let profile = &env["profiles"]["Darwin-arm64"];
            hash(text(profile, "certificate_sha256")?)?;
            let state = hash(text(profile, "state_sha256")?)?;
            Ok(Self {
                caps: Caps {
                    lean,
                    nanoda,
                    statement: statement.clone(),
                },
                reference: Some(reference),
                certified: Some((environment, state)),
                environment: Some(
                    text(&v, "env_id")
                        .expect("authenticated environment")
                        .into(),
                ),
                authenticated: crate::real_reply::AuthenticatedStatement::Valid(statement.clone()),
            })
        })();
        // A missing descriptor-bound local authority is provisioning failure, never
        // a published cap VOID. Invalid package bindings keep zero executable budget
        // so front-only failures still preserve the adapter's first-failure order.
        ensure!(
            hash(&d.fuel_authority_sha256)?
                == digest(include_bytes!(
                    "../../../scripts/zk_registry/data/fuel_cap_authority_v1.json"
                )),
            "local authority unavailable"
        );
        Ok(resolved.unwrap_or_else(|_| invalid()))
    }
}
