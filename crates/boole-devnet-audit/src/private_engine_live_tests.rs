//! Opt-in loopback I17. Only children created here may be stopped.
use super::*;
use anyhow::Context;
use boole_devnet_archive::{
    durability::PrivateTempDir,
    journal::{NetworkIdentity, ReadOnlyJournal},
};
use boole_devnet_driver::engine::RpcEngine;
use boole_gate_supervisor::{
    codec::Codec,
    process::{Envelope, Worker},
    supervisor::{Host, Mode},
};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    process::Command,
    rc::Rc,
};
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
fn gate(c: &Config, f: &ForkTable, source: &Path, scratch: &Path) -> Result<Supervisor> {
    let workers: Vec<Worker> = read(&source.join("gates.json"))?;
    Supervisor::new(
        c,
        f,
        Mode::Verify,
        workers,
        Some(match std::env::var_os("BOOLE_DEVNET_ZSTD") {
            Some(path) => Codec::new(path.into())?,
            None => Codec::portable_stub(),
        }),
        &scratch.join("cache"),
        scratch,
        &source.join("artifacts"),
        Host::local()?,
        Envelope::default(),
    )
}
fn inputs(source: &Path, network: &NetworkIdentity) -> Result<crate::JournalInputs> {
    Ok(crate::JournalInputs(ReadOnlyJournal::open(
        &source.join("a-state"),
        network,
    )?))
}
fn source(input: &mut dyn Inputs) -> Result<Source<'_>> {
    let mut hashes = BTreeMap::new();
    for h in 0..=input.tip() {
        hashes.insert(input.row(h)?.block_hash, h);
    }
    Ok(Source {
        inputs: input,
        hashes,
    })
}
struct StopAtRebuild {
    inner: RpcEngine,
    owner: Rc<RefCell<PrivateEngine>>,
    due: u64,
    stopped: Rc<RefCell<Option<u64>>>,
}
impl Engine for StopAtRebuild {
    fn head(&mut self) -> Result<boole_derivation::ParentHeader> {
        self.inner.head()
    }
    fn header(&mut self, h: u64) -> Result<boole_derivation::ParentHeader> {
        if h > 0 && self.stopped.borrow().is_none() && self.inner.head()?.number + 1 >= self.due {
            self.owner.borrow_mut().stop()?;
            *self.stopped.borrow_mut() = Some(h);
        }
        self.inner.header(h)
    }
    fn transactions(&mut self, h: u64) -> Result<Vec<String>> {
        self.inner.transactions(h)
    }
    fn call(&mut self, m: &str, p: Value) -> Result<Value> {
        self.inner.call(m, p)
    }
}
#[test]
fn i17_private_engine_rebuild_stop_restart_and_exact_positive_receipts() -> Result<()> {
    let Some(source_dir) = std::env::var_os("BOOLE_AUDIT_I17_SOURCE").map(PathBuf::from) else {
        return Ok(());
    };
    let output = PathBuf::from(std::env::var_os("BOOLE_AUDIT_I17_OUTPUT").context("I17 output")?);
    let op =
        PathBuf::from(std::env::var_os("BOOLE_DEVNET_OP_RETH").context("owned Engine binary")?);
    let genesis =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/devnet-m3/l2-genesis-v1.3.json");
    let c: Config = read(&source_dir.join("config.json"))?;
    let f: ForkTable = read(&source_dir.join("forks.json"))?;
    let reference_work = PrivateTempDir::new_in(&output, "reference")?;
    let mut reference = PrivateEngine::start(&op, &genesis, reference_work.path())?;
    let mut engine = reference.engine()?;
    let initial = engine.header(0)?;
    let network = NetworkIdentity::new(&c, Some(&f), initial.hash, initial.gas_limit)?;
    let mut own = inputs(&source_dir, &network)?;
    let tip = own.tip();
    let mut a = Driver::with_m3(
        source(&mut own)?,
        engine,
        c.clone(),
        initial.gas_limit,
        gate(&c, &f, &source_dir, reference_work.path())?,
        f.clone(),
    )?;
    let mut due = None;
    for h in 1..=tip {
        a.step(h)?;
        if due.is_none()
            && !a
                .m2_state()
                .m3
                .as_ref()
                .context("M3 state")?
                .results
                .is_empty()
        {
            due = Some(h);
        }
    }
    let expected = a.engine.head()?.hash;
    let recorded: Value = read(
        &source_dir
            .parent()
            .context("live recording directory")?
            .join("recording.json"),
    )?;
    ensure!(
        recorded["rows"]
            .as_array()
            .context("live rows")?
            .last()
            .context("live tip")?["hash"]
            == expected,
        "reference differs from independently recorded live hash"
    );
    drop(a);
    let due = due.context("I17 needs a real M3 verdict")?;
    let mut audited = RpcNode::new(&reference.http)?;
    let pure_work = PrivateTempDir::new_in(&output, "pure")?;
    let mut pure_gate = gate(&c, &f, &source_dir, pure_work.path())?;
    let expected_heads = crate::ExpectedHeads {
        l1_number: tip,
        l1_hash: own.row(tip)?.block_hash,
        l2: None,
    };
    let mut pure = crate::audit_in(
        &mut own,
        &mut audited,
        &c,
        Some(&f),
        Some(&mut pure_gate),
        &expected_heads,
        pure_work.path(),
    );
    ensure!(crate::succeeded(&pure), "pure reference audit {pure}");
    pure["mode"] = json!("replay");
    let baseline: Value = read(&source_dir.join("audit-journal-replay.json"))?;
    let baseline = serde_json::to_vec(&baseline)?;
    let failed_work = PrivateTempDir::new_in(&output, "interrupted")?;
    let failed = Rc::new(RefCell::new(PrivateEngine::start(
        &op,
        &genesis,
        failed_work.path(),
    )?));
    let stopped = Rc::new(RefCell::new(None));
    let engine = StopAtRebuild {
        inner: failed.borrow().engine()?,
        owner: failed.clone(),
        due,
        stopped: stopped.clone(),
    };
    let mut receipt = pure.clone();
    let private_http = failed.borrow().http.clone();
    replay_engine(
        &mut inputs(&source_dir, &network)?,
        &mut audited,
        c.clone(),
        Some(f.clone()),
        Some(gate(&c, &f, &source_dir, failed_work.path())?),
        engine,
        &private_http,
        &mut receipt,
    )?;
    ensure!(
        receipt["first_mismatch"].is_null()
            && receipt["resource_stop"]["side"] == "private-engine"
            && receipt["resource_stop"]["check"] == "verdict",
        "{receipt}"
    );
    let due_text = due.to_string();
    let origin_text = stopped.borrow().unwrap().to_string();
    ensure!(
        receipt["resource_stop"]["height"].as_str() == Some(due_text.as_str()),
        "wrong replay height"
    );
    ensure!(
        receipt["resource_stop"]["origin_height"].as_str() == Some(origin_text.as_str()),
        "lost origin height"
    );
    ensure!(
        failed.borrow_mut().stop().is_ok(),
        "interrupted child cleanup"
    );
    report_cleanup(&mut receipt, true, failed.borrow().log_issues()?);
    crate::recheck_inputs(&mut receipt, &inputs(&source_dir, &network)?);
    let child = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "replay::private_live::receipt_child",
            "--nocapture",
        ])
        .env("BOOLE_AUDIT_PRIVATE_RECEIPT", receipt.to_string())
        .output()?;
    ensure!(child.status.code() == Some(2), "private stop CLI exit");
    std::fs::write(
        output.join("rebuild-stop.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    drop(failed);
    let mut positive = None;
    for label in ["retry", "positive-control"] {
        let work = PrivateTempDir::new_in(&output, label)?;
        let mut private = PrivateEngine::start(&op, &genesis, work.path())?;
        let mut receipt = pure.clone();
        replay(
            &mut inputs(&source_dir, &network)?,
            &mut audited,
            c.clone(),
            Some(f.clone()),
            Some(gate(&c, &f, &source_dir, work.path())?),
            &private,
            &mut receipt,
        )?;
        ensure!(
            private.engine()?.head()?.hash == expected,
            "retry final hash"
        );
        let stopped = private.stop().is_ok();
        report_cleanup(&mut receipt, stopped, private.log_issues()?);
        crate::recheck_inputs(&mut receipt, &inputs(&source_dir, &network)?);
        ensure!(
            crate::succeeded(&receipt) && receipt["checks"]["execution"] == "ok" && stopped,
            "{receipt}"
        );
        let bytes = serde_json::to_vec(&receipt)?;
        ensure!(
            bytes == baseline,
            "positive audit differs from independently recorded normal CLI receipt"
        );
        if let Some(expected) = &positive {
            ensure!(
                expected == &bytes,
                "positive audit bytes changed after retry"
            );
        } else {
            positive = Some(bytes.clone());
        }
        std::fs::write(output.join(format!("{label}.json")), bytes)?;
    }
    reference.stop()?;
    ensure!(reference.log_issues()? == 0, "reference log issues");
    std::fs::write(
        output.join("acceptance.json"),
        serde_json::to_vec_pretty(
            &json!({"replayed_blocks":tip,"stop_height":due,"origin_height":stopped.borrow().unwrap(),"exit_code":2,"local_side":"private-engine","null_mismatch":true,"positive_receipts_byte_equal":true,"normal_cli_baseline_byte_equal":true,"reference_matches_live_recording":true,"final_hash":expected,"cleanup":true}),
        )?,
    )?;
    Ok(())
}
#[test]
fn receipt_child() {
    if let Ok(raw) = std::env::var("BOOLE_AUDIT_PRIVATE_RECEIPT") {
        crate::emit_receipt(&serde_json::from_str::<Value>(&raw).unwrap());
    }
}
