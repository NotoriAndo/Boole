use crate::{engine::Engine, l1::L1Source};
use anyhow::{bail, Context, Result};
use boole_derivation::{
    derive_with_m2,
    m2::{Config, State},
    Derived, ParentHeader,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
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
    config: Config,
    l2_gas_limit: u64,
    m2: State,
    pending: Option<State>,
    selected_head: Option<String>,
    journal: Option<PathBuf>,
    checkpoint: Option<Value>,
}
impl<L: L1Source, E: Engine> Driver<L, E> {
    pub fn new(l1: L, engine: E) -> Self {
        Self::construct(l1, engine, Config::devnet(), 30_000_000)
            .expect("valid built-in devnet config")
    }
    pub fn with_config(mut l1: L, engine: E, config: Config, l2_gas_limit: u64) -> Result<Self> {
        l1.validate_m2_inbox()?;
        Self::construct(l1, engine, config, l2_gas_limit)
    }
    fn construct(l1: L, engine: E, config: Config, l2_gas_limit: u64) -> Result<Self> {
        config
            .validate(l2_gas_limit)
            .map_err(|s| anyhow::anyhow!("M2 startup invariant S{s}"))?;
        let m2 = State::new(&config, l2_gas_limit)?;
        Ok(Self {
            l1,
            engine,
            stalled: None,
            evidence: None,
            config,
            l2_gas_limit,
            m2,
            pending: None,
            selected_head: None,
            journal: None,
            checkpoint: None,
        })
    }
    pub fn m2_state(&self) -> &State {
        &self.m2
    }
    pub fn set_journal(&mut self, path: &Path) -> Result<()> {
        self.checkpoint = match std::fs::read(path) {
            Ok(bytes) => Some(serde_json::from_slice(&bytes).context("decode M2 checkpoint")?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).context("read M2 checkpoint"),
        };
        self.journal = Some(path.to_owned());
        Ok(())
    }
    fn config_digest(&self) -> Result<String> {
        Ok(boole_derivation::attributes::hex_string(&Sha256::digest(
            serde_json::to_vec(&json!({"config":self.config,"l2_gas_limit":self.l2_gas_limit}))?,
        )))
    }
    fn check_m2_transport(&mut self, logs: &[boole_derivation::InboxLog]) -> Result<()> {
        for log in logs {
            if let Ok(bytes) = boole_derivation::attributes::hex_bytes(&log.data, "log") {
                if let Ok((2..=15, _)) = boole_derivation::inbox::item(&bytes) {
                    self.l1.validate_m2_inbox()?;
                    break;
                }
            }
        }
        Ok(())
    }
    fn state_for_parent(&mut self, parent: &ParentHeader) -> Result<State> {
        if let Some(saved) = &self.checkpoint {
            if saved["version"] != 1
                || saved["config_digest"] != self.config_digest()?
                || saved["height"].as_u64().is_none_or(|h| h > parent.number)
            {
                bail!("M2 checkpoint configuration or height mismatch");
            }
        }
        if self.m2.height() == parent.number {
            if self
                .selected_head
                .as_ref()
                .is_some_and(|hash| hash != &parent.hash)
            {
                bail!("cached L2 parent changed");
            }
            return Ok(self.m2.clone());
        }
        let mut state = State::new(&self.config, self.l2_gas_limit)?;
        let mut previous = self.engine.header(0).context("L2 genesis for re-fold")?;
        for height in 1..=parent.number {
            let header = if height == parent.number {
                parent.clone()
            } else {
                self.engine
                    .header(height)
                    .context("L2 history for re-fold")?
            };
            if header.number != height
                || header.parent_hash.as_deref() != Some(previous.hash.as_str())
            {
                bail!("L2 re-fold parentHash chain mismatch");
            }
            let deposit = header
                .l1_info
                .as_deref()
                .context("L2 history missing L1-info")?;
            let origin = boole_derivation::system::l1_origin(deposit, height)
                .context("decode re-fold L1-info")?;
            let l1 = self
                .l1
                .block_by_hash(&origin)
                .context("anchored L1 re-fold")?;
            if l1.number != height || !l1.hash.eq_ignore_ascii_case(&origin) {
                bail!("anchored L1 re-fold identity mismatch");
            }
            let logs = self
                .l1
                .inbox_logs(&origin)
                .context("anchored re-fold logs")?;
            self.check_m2_transport(&logs)?;
            let (derived, next) = derive_with_m2(&state, &self.config, &l1, &logs, &previous)
                .context("pure anchored re-fold")?;
            if derived.attributes.transactions.first().map(String::as_str) != Some(deposit) {
                bail!("anchored L1-info bytes mismatch");
            }
            if let Some(saved) = &self.checkpoint {
                if saved["height"] == height
                    && (saved["m2_root"] != boole_derivation::attributes::hex_string(&next.root)
                        || saved["l2_hash"] != header.hash
                        || saved["l1_hash"] != l1.hash)
                {
                    bail!("M2 checkpoint disagrees with anchored re-fold");
                }
            }
            state = next;
            previous = header;
        }
        self.m2 = state.clone();
        self.selected_head = Some(parent.hash.clone());
        Ok(state)
    }
    fn persist_checkpoint(&mut self, state: &State, record: &Value) -> Result<()> {
        let Some(path) = &self.journal else {
            return Ok(());
        };
        let value = json!({"version":1,"height":state.height(),"m2_root":boole_derivation::attributes::hex_string(&state.root),"l2_hash":record["block_hash"],"l1_hash":record["l1"]["hash"],"config_digest":self.config_digest()?});
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        let mut created = false;
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            created = true;
            serde_json::to_writer(&mut file, &value)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            std::fs::rename(&temporary, path)?;
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                File::open(parent)?.sync_all()?;
            }
            Ok(())
        })();
        if result.is_err() && created {
            let _ = std::fs::remove_file(&temporary);
        }
        result.context("persist M2 checkpoint")?;
        self.checkpoint = Some(value);
        Ok(())
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
        self.complete_step(height, record, result, Derived::canonical_bytes)
    }

    fn complete_step(
        &mut self,
        height: u64,
        mut record: Value,
        result: Result<StepOutcome>,
        canonical: impl FnOnce(&Derived) -> serde_json::Result<Vec<u8>>,
    ) -> std::result::Result<StepOutcome, Stall> {
        let result = result.and_then(|outcome| {
            match &outcome {
                StepOutcome::WaitingForFinality => {
                    record["outcome"] = json!("WAITING_FOR_FINALITY")
                }
                StepOutcome::Derived(derived) => {
                    record["outcome"] = json!("DERIVED");
                    record["derived"] = json!(derived);
                    let bytes = canonical(derived).context("canonical attribute encoding")?;
                    use sha2::{Digest, Sha256};
                    record["attribute_digest"] = json!(boole_derivation::attributes::hex_string(
                        &Sha256::digest(bytes)
                    ));
                }
            }
            Ok(outcome)
        });
        if let Err(error) = &result {
            record["outcome"] = json!("STALL");
            record["error"] = json!(format!("{error:#}"));
            record["m2_root"] = json!(boole_derivation::attributes::hex_string(&self.m2.root));
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
        let checkpoint_attempt = result.is_ok();
        let pending = self.pending.take();
        let result = result.and_then(|outcome| {
            if let Some(state) = pending {
                let hash = record["block_hash"]
                    .as_str()
                    .context("selected block hash missing")?
                    .to_owned();
                self.persist_checkpoint(&state, &record)?;
                self.selected_head = Some(hash);
                self.m2 = state;
            }
            Ok(outcome)
        });
        if checkpoint_attempt {
            if let Err(error) = &result {
                record["outcome"] = json!("STALL");
                record["error"] = json!(format!("{error:#}"));
                record["m2_root"] = json!(boole_derivation::attributes::hex_string(&self.m2.root));
                if let Some(file) = &mut self.evidence {
                    let appended = (|| -> Result<()> {
                        serde_json::to_writer(&mut *file, &record)?;
                        file.write_all(b"\n")?;
                        file.flush()?;
                        Ok(())
                    })();
                    if let Err(write_error) = appended {
                        let stall = Stall {
                            height,
                            reason: format!("{error:#}; append STALL evidence: {write_error:#}"),
                        };
                        self.stalled = Some(stall.clone());
                        return Err(stall);
                    }
                }
            }
        }
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
        self.check_m2_transport(&logs)?;
        let previous = self.state_for_parent(&parent)?;
        let (derived, next) = derive_with_m2(&previous, &self.config, &l1, &logs, &parent)
            .context("pure derivation input")?;
        record["m2_root"] = json!(boole_derivation::attributes::hex_string(&next.root));
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
        self.pending = Some(next);
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

#[cfg(test)]
mod tests {
    use super::*;
    use boole_derivation::derive;
    struct Unused;
    impl L1Source for Unused {
        fn finalized(&mut self) -> Result<u64> {
            panic!("sticky stall must not read L1")
        }
        fn block(&mut self, _n: u64) -> Result<boole_derivation::L1Block> {
            panic!("unused")
        }
        fn inbox_logs(&mut self, _hash: &str) -> Result<Vec<boole_derivation::InboxLog>> {
            panic!("unused")
        }
    }
    impl Engine for Unused {
        fn head(&mut self) -> Result<boole_derivation::ParentHeader> {
            panic!("sticky stall must not read head")
        }
        fn call(&mut self, _method: &str, _params: Value) -> Result<Value> {
            panic!("sticky stall must not call Engine")
        }
    }
    #[test]
    fn canonical_encoding_failure_keeps_stall_state() {
        let rows: Vec<Value> = serde_json::from_str(include_str!(
            "../../../fixtures/devnet-m1/s1b-recorded-main.json"
        ))
        .unwrap();
        let row = &rows[0];
        let d = derive(
            &serde_json::from_value(row["l1"].clone()).unwrap(),
            &serde_json::from_value::<Vec<_>>(row["logs"].clone()).unwrap(),
            &serde_json::from_value(row["parent"].clone()).unwrap(),
        )
        .unwrap();
        let mut driver = Driver::new(Unused, Unused);
        let error = driver
            .complete_step(
                1,
                json!({"height":1,"engine_calls":[]}),
                Ok(StepOutcome::Derived(Box::new(d))),
                |_| serde_json::from_str::<Value>("invalid").map(|_| vec![]),
            )
            .unwrap_err();
        assert_eq!(driver.last_failure().map(|x| x.height), Some(1));
        assert_eq!(driver.step(1).unwrap_err().reason, error.reason);
    }
}
