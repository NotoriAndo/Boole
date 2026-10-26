//! Local durable coverage is a second check, after the actual K4 rebuild.
use super::{require, AuditFailure, Inputs};
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    m2::{Config, Hash, State},
    m3::ForkTable,
    ParentHeader,
};
use boole_devnet_archive::{error::Error, pins::ReadOnlyPins};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
pub(crate) type Observed = BTreeMap<u64, (ParentHeader, Hash)>;
pub(crate) fn tx_digest(txs: &[String]) -> Result<Hash> {
    Ok(Sha256::digest(serde_json::to_vec(txs)?).into())
}
fn unavailable(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<Error>(),
        Some(Error::Availability { .. })
    ) || error.is::<std::io::Error>()
}
fn pending(receipt: &mut Value) {
    receipt["k4"]["status"] = json!("pending");
    receipt["k4"]["pin_retention"] = json!("local-unavailable");
    receipt["k4"]["checked"] = json!("0");
    receipt["k4"]["verification_height"] = Value::Null;
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn verify(
    inputs: &dyn Inputs,
    view: &Result<Option<ReadOnlyPins>>,
    state: &State,
    c: &Config,
    forks: Option<&ForkTable>,
    observed: &Observed,
    rebuilt: super::k4::Coverage,
    receipt: &mut Value,
) -> std::result::Result<(), AuditFailure> {
    if c.m3.is_none() {
        return Ok(());
    }
    receipt["k4"]["rebuilt"] = json!(rebuilt.checked.to_string());
    pending(receipt);
    let view = match view {
        Ok(Some(view)) => view,
        Ok(None) => return Ok(()),
        Err(e) if unavailable(e) => return Ok(()),
        Err(_) => {
            return Err(AuditFailure {
                height: state.height(),
                class: "K4",
                resource: None,
            })
        }
    };
    let result = (|| -> Result<bool> {
        let Some(source) = inputs.pin_source() else {
            return Ok(false);
        };
        let required = boole_devnet_archive::pins::requirements(
            state,
            c,
            forks.context("K4 fork table")?,
            false,
        )?;
        ensure!(
            required.len() == rebuilt.total && rebuilt.checked == rebuilt.total,
            "K4 rebuild/retention set differs"
        );
        view.verify(source, &required, state.height())?;
        for (&h, (header, txs)) in observed {
            view.verify_header(h, header)?;
            ensure!(
                tx_digest(&view.binding(h)?.transactions)? == *txs,
                "durable L2 transaction binding differs"
            );
        }
        for pin in &required {
            for (&lo, &hi) in &pin.intervals().0 {
                for h in lo..=hi {
                    ensure!(observed.contains_key(&h), "retained L2 observation missing");
                }
            }
        }
        let tip = view.binding(state.height())?;
        ensure!(
            tip.root == state.root
                && observed
                    .get(&state.height())
                    .is_some_and(|(h, _)| h.hash == tip.header.hash),
            "K4 verification tip differs"
        );
        view.check()?;
        Ok(true)
    })();
    match result {
        Ok(true) => {
            receipt["k4"]["status"] = json!("ok");
            receipt["k4"]["pin_retention"] = json!("ok");
            receipt["k4"]["checked"] = json!(rebuilt.checked.to_string());
            receipt["k4"]["verification_height"] = json!(state.height().to_string());
        }
        Ok(false) => {}
        Err(e) if unavailable(&e) => {}
        Err(_) => {
            return Err(AuditFailure {
                height: state.height(),
                class: "K4",
                resource: None,
            })
        }
    }
    Ok(())
}
pub(crate) fn finish(
    view: &Result<Option<ReadOnlyPins>>,
    receipt: &mut Value,
    height: u64,
) -> std::result::Result<(), AuditFailure> {
    if receipt["k4"]["status"] != "ok" {
        return Ok(());
    }
    if let Ok(Some(view)) = view {
        if let Err(e) = view.check() {
            if unavailable(&e) {
                pending(receipt)
            } else {
                require(height, "K4", false)?
            }
        }
    }
    Ok(())
}
