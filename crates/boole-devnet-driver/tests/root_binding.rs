//! M2-I7: actual Engine imports reject a test-only divergent local fold.
#[allow(dead_code)]
mod support;
use anyhow::{Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    m2::{Config, State},
    InboxLog, L1Block, ParentHeader,
};
use boole_devnet_driver::{
    driver::{Driver, StallKind},
    engine::{Engine, RpcEngine},
    l1::{L1Source, RecordedL1},
};
use serde_json::{json, Value};
use std::fs;
use support::*;
struct Capture {
    inner: RpcEngine,
    payload: Option<Value>,
}
impl Engine for Capture {
    fn head(&mut self) -> Result<ParentHeader> {
        self.inner.head()
    }
    fn header(&mut self, h: u64) -> Result<ParentHeader> {
        self.inner.header(h)
    }
    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let value = self.inner.call(method, params)?;
        if method == "engine_getPayloadV5" {
            self.payload = Some(value["executionPayload"].clone());
        }
        Ok(value)
    }
}
struct MutatedFold {
    source: RecordedL1,
    affected_hash: String,
}
impl L1Source for MutatedFold {
    fn finalized(&mut self) -> Result<u64> {
        self.source.finalized()
    }
    fn block(&mut self, h: u64) -> Result<L1Block> {
        self.source.block(h)
    }
    fn block_by_hash(&mut self, h: &str) -> Result<L1Block> {
        self.source.block_by_hash(h)
    }
    fn inbox_logs(&mut self, h: &str) -> Result<Vec<InboxLog>> {
        let mut logs = self.source.inbox_logs(h)?;
        if h == self.affected_hash {
            // Test-only mutation: the local fold omits the first COMMIT event.
            let index = logs
                .iter()
                .position(|l| {
                    hex_bytes(&l.data, "log")
                        .ok()
                        .and_then(|b| {
                            boole_derivation::inbox::item(&b)
                                .ok()
                                .map(|(kind, _)| kind == 2)
                        })
                        .unwrap_or(false)
                })
                .context("mutation COMMIT")?;
            logs.remove(index);
        }
        Ok(logs)
    }
}
#[test]
fn root_binding_live_divergence_and_mid_round_restart() -> Result<()> {
    let Some(binary) = pinned("BOOLE_DEVNET_OP_RETH", "op_reth")? else {
        return Ok(());
    };
    let mut rt = Runtime::new("m2root-divergence")?;
    let fixture: Value = serde_json::from_slice(&fs::read(
        support::m2::current_genesis()
            .parent()
            .context("fixtures")?
            .join("commit-reveal-v1.2.json"),
    )?)?;
    let rows = fixture["heights"].as_array().context("heights")?;
    let text = serde_json::to_string(rows)?;
    let config: Config = serde_json::from_value(fixture["config"].clone())?;
    let genesis = rt.path.join("l2-genesis.json");
    fs::write(&genesis, serde_json::to_vec_pretty(&fixture["l2_genesis"])?)?;
    let a = rt.node(&binary, &genesis, "a", 0)?;
    let b = rt.node(&binary, &genesis, "b", 1)?;
    let c = rt.node(&binary, &genesis, "c", 2)?;
    let mut da = Driver::with_config(
        RecordedL1::from_json(&text)?,
        Capture {
            inner: a.engine()?,
            payload: None,
        },
        config.clone(),
        30_000_000,
    )?;
    let mut db = Driver::with_config(
        MutatedFold {
            source: RecordedL1::from_json(&text)?,
            affected_hash: rows[5]["l1"]["hash"].as_str().context("hash")?.into(),
        },
        b.engine()?,
        config.clone(),
        30_000_000,
    )?;
    let mut dc = Driver::with_config(
        RecordedL1::from_json(&text)?,
        c.engine()?,
        config.clone(),
        30_000_000,
    )?;
    da.set_evidence(&rt.path.join("a.jsonl"))?;
    db.set_evidence(&rt.path.join("b-mutant.jsonl"))?;
    dc.set_evidence(&rt.path.join("c.jsonl"))?;
    let mut comparisons = vec![];
    let mut payloads = vec![];
    let mut model = State::new(&config, 30_000_000)?;
    for h in 1..=28 {
        da.step(h)?;
        let payload = da.engine.payload.clone().context("A payload")?;
        payloads.push(payload.clone());
        dc.import_payload(h, payload.clone())?;
        let row = &rows[(h - 1) as usize];
        let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
        let logs: Vec<InboxLog> = serde_json::from_value(row["logs"].clone())?;
        let mut inputs = vec![];
        for log in logs {
            let bytes = hex_bytes(&log.data, "log")?;
            if let Ok((kind, body)) = boole_derivation::inbox::item(&bytes) {
                if (2..=15).contains(&kind) {
                    inputs.push(boole_derivation::m2::Input {
                        kind,
                        body: body.to_vec(),
                        transaction_index: log.transaction_index,
                        log_index: log.log_index,
                    });
                }
            }
        }
        model = model.step(&config, &l1, &inputs)?.state;
        assert_eq!(da.m2_state().root, model.root);
        assert_eq!(dc.m2_state().root, model.root);
        let ah = a.block()?;
        let ch = c.block()?;
        assert_eq!(ah["hash"], ch["hash"]);
        let root = boole_derivation::system::read_m2_root(
            payload["transactions"][1].as_str().context("M2ROOT")?,
            h,
            &l1.hash,
        )?;
        assert_eq!(root, model.root);
        if h < 6 {
            db.import_payload(h, payload)?;
        } else {
            let error = db
                .import_payload(h, payload)
                .expect_err("divergent fold must refuse A's payload");
            assert_eq!(error.kind, StallKind::M2Root { height: 6 });
            assert_eq!(db.m2_state().height(), 5);
            assert_eq!(b.engine()?.head()?.number, 5);
        }
        comparisons.push(json!({"height":h,"a_block_hash":ah["hash"],"c_block_hash":ch["hash"],"pure_root":hex_string(&model.root),"block_root":hex_string(&root),"b_height":db.m2_state().height(),"b_stall_height":if h>=6 {Some(6)}else{None}}));
    }
    // Restart the actual Engine process mid-round from its unchanged height5.
    rt.stop("b")?;
    let b = rt.node(&binary, &genesis, "b", 1)?;
    let mut recovered = Driver::with_config(
        RecordedL1::from_json(&text)?,
        b.engine()?,
        config.clone(),
        30_000_000,
    )?;
    recovered.set_evidence(&rt.path.join("b-recovered.jsonl"))?;
    for (index, payload) in payloads.into_iter().enumerate().skip(5) {
        recovered.import_payload(index as u64 + 1, payload)?;
    }
    assert_eq!(recovered.m2_state().root, da.m2_state().root);
    assert_eq!(b.block()?["hash"], a.block()?["hash"]);
    let recording = json!({"schema":"boole-devnet-m2root-binding-v1.2","config":config,"l2_genesis":fixture["l2_genesis"],"source_recording":"commit-reveal-v1.2.json","comparisons":comparisons,"first_divergence":6,"mutant_adopted_height":5,"restart_height":5,"recovered_height":28,"recovered_root":hex_string(&recovered.m2_state().root),"strict_cleanup":"PASS","provenance":{"generator":"crates/boole-devnet-driver/tests/root_binding.rs","generator_sha256":hex_string(&support::m2::hash(include_bytes!("root_binding.rs"))),"pins":fixture_json("pins.json")?,"engine_profile":engine_profile()}});
    rt.finish(true)?;
    if let Some(path) = std::env::var_os("BOOLE_DEVNET_ROOT_RECORDING") {
        fs::write(path, serde_json::to_vec_pretty(&recording)?)?;
    }
    Ok(())
}
