//! Replay writes only to a freshly spawned, auditor-owned private Engine.
use crate::private_engine::{operation as private_operation, PrivateSide, Tracked, Tracker};
use crate::{rpc::RpcNode, Inputs, Node};
use anyhow::{ensure, Context, Result};
use boole_derivation::{m2::Config, m3::ForkTable, InboxLog, L1Block};
use boole_devnet_driver::{
    driver::{Driver, StepOutcome},
    engine::Engine,
    l1::L1Source,
};
use boole_gate_supervisor::supervisor::Supervisor;
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn private_unavailable(error: &anyhow::Error) -> bool {
    error.is::<PrivateSide>() && crate::private_engine::unavailable(error)
}
#[derive(Debug)]
pub struct Failure {
    pub height: u64,
    pub class: &'static str,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "replay {} at {}", self.class, self.height)
    }
}
impl std::error::Error for Failure {}
fn at<T>(height: u64, class: &'static str, result: Result<T>) -> Result<T> {
    result.map_err(|error| {
        if error.is::<Failure>() {
            return error;
        }
        let (height, class) = match error.downcast_ref::<boole_devnet_archive::error::Error>() {
            Some(
                boole_devnet_archive::error::Error::Availability { height, .. }
                | boole_devnet_archive::error::Error::Integrity { height, .. },
            ) => (*height, "input"),
            _ => (height, class),
        };
        error.context(Failure { height, class })
    })
}
pub fn report_failure(receipt: &mut Value, error: &anyhow::Error, status: &str) {
    let (height, class) = if let Some(f) = error.downcast_ref::<Failure>() {
        (f.height, f.class)
    } else {
        match error.downcast_ref::<boole_devnet_archive::error::Error>() {
            Some(
                boole_devnet_archive::error::Error::Availability { height, .. }
                | boole_devnet_archive::error::Error::Integrity { height, .. },
            ) => (*height, "input"),
            _ => (completed(receipt), "replay-setup"),
        }
    };
    let input_overrides_resource = class == "input"
        && !receipt["resource_stop"].is_null()
        && !error.is::<crate::resource::ResourceStop>();
    if !crate::succeeded(receipt) && !input_overrides_resource {
        return;
    }
    if crate::private_engine::report(receipt, error) {
        return;
    }
    if let Some(stop) = error.downcast_ref::<crate::resource::ResourceStop>() {
        crate::mark_resource_stop(receipt, height, class, *stop);
        if error.is::<PrivateSide>() {
            receipt["resource_stop"]["side"] = json!("private-engine");
        }
        receipt["checks"]["execution"] = json!("resource-stop");
    } else if private_unavailable(error) {
        crate::mark_resource_stop(
            receipt,
            height,
            class,
            crate::resource::ResourceStop {
                resource: "private-engine-unavailable",
                limit: 0,
            },
        );
        receipt["resource_stop"]["side"] = json!("private-engine");
        receipt["checks"]["execution"] = json!("resource-stop");
    } else {
        crate::mark_mismatch(receipt, height, class);
        receipt["checks"]["execution"] = json!(status);
    }
}
fn completed(receipt: &Value) -> u64 {
    receipt["replayed_blocks"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}
pub fn report_cleanup(receipt: &mut Value, stopped: bool, issues: usize) {
    receipt["private_engine_cleanup"] = json!({"stopped":stopped,"log_issues":issues});
    if !stopped || issues > 0 {
        if !crate::succeeded(receipt) {
            receipt["checks"]["engine-cleanup"] = json!(if receipt["resource_stop"].is_null() {
                "mismatch"
            } else {
                "failed"
            });
            return;
        }
        let height = completed(receipt);
        crate::mark_mismatch(receipt, height, "engine-cleanup");
        receipt["checks"]["execution"] = json!("mismatch");
    }
}
pub use crate::private_engine::PrivateEngine;
struct Source<'a> {
    inputs: &'a mut dyn Inputs,
    hashes: BTreeMap<String, u64>,
}
impl L1Source for Source<'_> {
    fn finalized(&mut self) -> Result<u64> {
        self.inputs.check()?;
        Ok(self.inputs.tip())
    }
    fn input_row(&mut self, h: u64) -> Result<boole_devnet_archive::input::InputRow> {
        self.inputs.row(h)
    }
    fn block(&mut self, h: u64) -> Result<L1Block> {
        Ok(self.inputs.row(h)?.verify()?.block)
    }
    fn block_by_hash(&mut self, hash: &str) -> Result<L1Block> {
        self.block(*self.hashes.get(hash).context("private replay origin")?)
    }
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>> {
        Ok(self
            .inputs
            .row(*self.hashes.get(hash).context("private replay logs")?)?
            .verify()?
            .logs)
    }
}
/// Return the first replay mismatch, or the exact number of reproduced blocks.
#[allow(clippy::too_many_arguments)]
pub fn replay(
    inputs: &mut dyn Inputs,
    audited: &mut dyn Node,
    c: Config,
    forks: Option<ForkTable>,
    supervisor: Option<Supervisor>,
    private: &PrivateEngine,
    receipt: &mut Value,
) -> Result<()> {
    receipt["replayed_blocks"] = json!("0");
    replay_engine(
        inputs,
        audited,
        c,
        forks,
        supervisor,
        at(0, "replay-setup", private_operation(private.engine()))?,
        &private.http,
        receipt,
    )
}
#[allow(clippy::too_many_arguments)]
fn replay_engine<E: Engine>(
    inputs: &mut dyn Inputs,
    audited: &mut dyn Node,
    c: Config,
    forks: Option<ForkTable>,
    supervisor: Option<Supervisor>,
    engine: E,
    private_http: &str,
    receipt: &mut Value,
) -> Result<()> {
    receipt["replayed_blocks"] = json!("0");
    let tracker = Tracker::default();
    tracker.begin(0, "replay-setup");
    let mut engine = Tracked::new(engine, tracker.clone());
    let genesis = at(0, "replay-setup", private_operation(engine.header(0)))?;
    at(
        0,
        "replay-setup",
        (|| -> Result<()> {
            ensure!(
                genesis.number == 0 && genesis.hash == audited.header(0)?.hash,
                "private Engine must be fresh with the audited genesis"
            );
            Ok(())
        })(),
    )?;
    let tip = inputs.tip();
    let mut hashes = BTreeMap::new();
    for h in 0..=tip {
        let row = at(h, "input", inputs.row(h))?;
        hashes.insert(at(h, "input", row.verify())?.block.hash, h);
    }
    let source = Source { inputs, hashes };
    let gas = genesis.gas_limit;
    let mut driver = at(
        0,
        "replay-setup",
        (|| -> Result<_> {
            Ok(if let Some(f) = forks {
                Driver::with_m3(
                    source,
                    engine,
                    c,
                    gas,
                    supervisor.context("replay supervisor")?,
                    f,
                )?
            } else {
                Driver::with_config(source, engine, c, gas)?
            })
        })(),
    )?;
    for h in 1..=tip {
        tracker.begin(h, "fold");
        let mut phase = "fold";
        if let Err(error) = (|| -> Result<()> {
            ensure!(
                matches!(
                    driver.step(h).map_err(anyhow::Error::new)?,
                    StepOutcome::Derived(_)
                ),
                "private replay waited for finality"
            );
            phase = "prefix";
            tracker.phase(phase);
            let actual = private_operation(driver.engine.head())?;
            let expected = audited.header(h)?;
            ensure!(
                actual.hash == expected.hash && actual.number == h,
                "private replay L2 hash"
            );
            phase = "ledger";
            tracker.phase(phase);
            let mut private_node = private_operation(RpcNode::new(private_http))?;
            ensure!(
                private_operation(private_node.receipt_statuses(h))?
                    == audited.receipt_statuses(h)?
                    && private_operation(private_node.depositor_balance(h))?
                        == audited.depositor_balance(h)?,
                "private replay execution ledger"
            );
            phase = "input";
            driver.l1.inputs.check()?;
            Ok(())
        })() {
            failed_step(receipt, h, phase, error, &tracker, driver.l1.inputs.check());
            return Ok(());
        }
        receipt["replayed_blocks"] = json!(h.to_string());
    }
    receipt["checks"]["execution"] = json!("ok");
    receipt["replayed_blocks"] = json!(tip.to_string());
    Ok(())
}

fn failed_step(
    receipt: &mut Value,
    h: u64,
    phase: &'static str,
    error: anyhow::Error,
    tracker: &Tracker,
    checked: Result<()>,
) {
    if checked.is_err_and(|e| !e.is::<crate::resource::ResourceStop>()) {
        crate::mark_mismatch(receipt, h, "input");
        receipt["checks"]["execution"] = json!("mismatch");
        return;
    }
    let class = if error
        .downcast_ref::<boole_devnet_driver::driver::Stall>()
        .is_some_and(|s| {
            matches!(
                s.kind,
                boole_devnet_driver::driver::StallKind::VerdictPending { .. }
                    | boole_devnet_driver::driver::StallKind::Verdict { .. }
            )
        }) {
        "verdict"
    } else {
        phase
    };
    let erased_private = error
        .downcast_ref::<boole_devnet_driver::driver::Stall>()
        .is_some_and(|s| {
            matches!(
                s.kind,
                boole_devnet_driver::driver::StallKind::VerdictPending { .. }
                    | boole_devnet_driver::driver::StallKind::Availability { .. }
            )
        });
    let error = if erased_private {
        if let Some(mut fault) = tracker.unresolved() {
            fault.phase = class;
            error.context(fault)
        } else {
            error
        }
    } else {
        error
    };
    report_failure(
        receipt,
        &error.context(Failure { height: h, class }),
        "mismatch",
    );
}

#[cfg(test)]
#[path = "replay_loop_tests.rs"]
mod loop_tests;

#[cfg(test)]
#[path = "private_engine_live_tests.rs"]
mod private_live;
