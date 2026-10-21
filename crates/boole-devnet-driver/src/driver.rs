use crate::{engine::Engine, l1::L1Source};
use anyhow::{bail, Context, Result};
use boole_derivation::{derive, Derived};
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};

#[derive(Debug)]
pub enum StepOutcome {
    Derived(Box<Derived>),
    WaitingForFinality,
}

#[derive(Debug, Clone)]
pub struct Stall {
    pub height: u64,
    pub reason: String,
}
impl std::fmt::Display for Stall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "stalled at {}: {}", self.height, self.reason)
    }
}
impl std::error::Error for Stall {}

pub struct Driver<L, E> {
    pub l1: L,
    pub engine: E,
    stalled: Option<Stall>,
    evidence: Option<File>,
}
impl<L: L1Source, E: Engine> Driver<L, E> {
    pub fn new(l1: L, engine: E) -> Self {
        Self {
            l1,
            engine,
            stalled: None,
            evidence: None,
        }
    }
    pub fn set_evidence(&mut self, path: &Path) -> Result<()> {
        self.evidence = Some(OpenOptions::new().create(true).append(true).open(path)?);
        Ok(())
    }
    pub fn last_failure(&self) -> Option<&Stall> {
        self.stalled.as_ref()
    }
    pub fn step(&mut self, height: u64) -> std::result::Result<StepOutcome, Stall> {
        if let Some(stall) = &self.stalled {
            return Err(stall.clone());
        }
        let mut record = json!({"height":height,"engine_calls":[]});
        let result = self.try_step(height, &mut record);
        match &result {
            Ok(StepOutcome::WaitingForFinality) => {
                record["outcome"] = json!("WAITING_FOR_FINALITY")
            }
            Ok(StepOutcome::Derived(derived)) => {
                record["outcome"] = json!("DERIVED");
                record["derived"] = json!(derived);
                let bytes = derived.canonical_bytes().map_err(|e| Stall {
                    height,
                    reason: e.to_string(),
                })?;
                use sha2::{Digest, Sha256};
                record["attribute_digest"] = json!(boole_derivation::attributes::hex_string(
                    &Sha256::digest(bytes)
                ));
            }
            Err(error) => {
                record["outcome"] = json!("STALL");
                record["error"] = json!(format!("{error:#}"));
            }
        }
        let persisted = (|| -> Result<()> {
            if let Some(file) = &mut self.evidence {
                serde_json::to_writer(&mut *file, &record)?;
                file.write_all(b"\n")?;
                file.flush()?;
            }
            Ok(())
        })();
        let result = result.and_then(|outcome| persisted.map(|_| outcome));
        match result {
            Ok(outcome) => Ok(outcome),
            Err(error) => {
                let stall = Stall {
                    height,
                    reason: format!("{error:#}"),
                };
                self.stalled = Some(stall.clone());
                Err(stall)
            }
        }
    }

    fn call(&mut self, record: &mut Value, method: &str, params: Value) -> Result<Value> {
        let result = self.engine.call(method, params.clone());
        let response = match &result {
            Ok(value) => json!({"result":value}),
            Err(error) => json!({"error":format!("{error:#}")}),
        };
        if let Some(calls) = record["engine_calls"].as_array_mut() {
            calls.push(json!({"method":method,"params":params,"response":response}));
        }
        result.with_context(|| format!("Engine {method}"))
    }

    fn try_step(&mut self, height: u64, record: &mut Value) -> Result<StepOutcome> {
        let finalized = self.l1.finalized().context("L1 finalized")?;
        record["finalized"] = json!(finalized);
        if height > finalized {
            return Ok(StepOutcome::WaitingForFinality);
        }
        let parent = self.engine.head().context("L2 parent")?;
        if parent.number.checked_add(1) != Some(height) {
            bail!("restart height must be node head + 1");
        }
        let l1 = self.l1.block(height).context("L1 block")?;
        if l1.number != height {
            bail!("L1 returned wrong height");
        }
        let logs = self.l1.inbox_logs(&l1.hash).context("L1 inbox logs")?;
        let reread = self.l1.block(height).context("L1 block reread")?;
        if serde_json::to_value(&l1)? != serde_json::to_value(&reread)? {
            bail!("L1 block changed between reads");
        }
        let derived = derive(&l1, &logs, &parent).context("pure derivation input")?;
        record["l1"] = json!(l1);
        record["parent"] = json!(parent);
        record["attributes"] = json!(derived.attributes);
        record["decisions"] = json!(derived.decisions);
        let state = json!({"headBlockHash":parent.hash,"safeBlockHash":parent.hash,"finalizedBlockHash":parent.hash});
        let built = self.call(
            record,
            "engine_forkchoiceUpdatedV3",
            json!([state, derived.attributes]),
        )?;
        valid(&built["payloadStatus"])?;
        let payload_id = built["payloadId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("VALID build missing payloadId")?;
        let envelope = self.call(record, "engine_getPayloadV5", json!([payload_id]))?;
        let payload = &envelope["executionPayload"];
        check_payload_transactions(payload, &derived)?;
        if payload["blockNumber"] != format!("0x{height:x}")
            || payload["parentHash"] != parent.hash
            || payload["timestamp"] != derived.attributes.timestamp
            || payload["gasLimit"] != derived.attributes.gas_limit
        {
            bail!("payload header disagrees with derivation");
        }
        // Fence L1 again before importing a payload built by the client.
        let final_read = self.l1.block(height).context("L1 pre-import reread")?;
        if serde_json::to_value(&l1)? != serde_json::to_value(&final_read)? {
            bail!("L1 block changed during build");
        }
        let imported = self.call(
            record,
            "engine_newPayloadV4",
            json!([payload, [], derived.attributes.parent_beacon_block_root, []]),
        )?;
        valid(&imported)?;
        let hash = payload["blockHash"]
            .as_str()
            .context("payload missing blockHash")?;
        let selected = self.call(
            record,
            "engine_forkchoiceUpdatedV3",
            json!([{"headBlockHash":hash,"safeBlockHash":hash,"finalizedBlockHash":hash},null]),
        )?;
        valid(&selected["payloadStatus"])?;
        let head = self.engine.head().context("L2 post-selection head")?;
        if head.number != height || head.hash != hash {
            bail!("L2 head does not match selected payload");
        }
        record["block_hash"] = json!(hash);
        Ok(StepOutcome::Derived(Box::new(derived)))
    }
}

fn valid(status: &Value) -> Result<()> {
    if status["status"] != "VALID" {
        bail!("non-VALID Engine payload status: {status}");
    }
    Ok(())
}

pub fn check_payload_transactions(payload: &Value, derived: &Derived) -> Result<()> {
    let txs = payload["transactions"]
        .as_array()
        .context("payload transactions missing")?;
    let expected = &derived.attributes.transactions;
    if txs.first().and_then(Value::as_str) != expected.first().map(String::as_str) {
        bail!("payload L1-info deposit missing or changed");
    }
    let mut remaining = expected.iter();
    for tx in txs {
        let raw = tx.as_str().context("non-string payload transaction")?;
        if !remaining.any(|expected| expected == raw) {
            bail!("payload transactions are not an order-preserving subsequence");
        }
    }
    Ok(())
}
