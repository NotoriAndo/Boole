use crate::engine::Rpc;
use anyhow::{anyhow, bail, Result};
use boole_derivation::{InboxLog, L1Block};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub trait L1Source {
    fn finalized(&mut self) -> Result<u64>;
    fn block(&mut self, number: u64) -> Result<L1Block>;
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>>;
}

pub struct RecordedL1 {
    rows: BTreeMap<u64, (L1Block, Vec<InboxLog>)>,
    pub finalized_height: u64,
}

impl RecordedL1 {
    pub fn from_json(json: &str) -> Result<Self> {
        let rows: Vec<Value> = serde_json::from_str(json)?;
        let mut result = BTreeMap::new();
        for row in rows {
            let l1: L1Block = serde_json::from_value(row["l1"].clone())?;
            let logs = serde_json::from_value(row["logs"].clone())?;
            if result.insert(l1.number, (l1, logs)).is_some() {
                return Err(anyhow!("duplicate recorded height"));
            }
        }
        let finalized_height = *result
            .keys()
            .next_back()
            .ok_or_else(|| anyhow!("empty recording"))?;
        Ok(Self {
            rows: result,
            finalized_height,
        })
    }
}
impl L1Source for RecordedL1 {
    fn finalized(&mut self) -> Result<u64> {
        Ok(self.finalized_height)
    }
    fn block(&mut self, n: u64) -> Result<L1Block> {
        self.rows
            .get(&n)
            .map(|x| x.0.clone())
            .ok_or_else(|| anyhow!("missing recorded L1 height {n}"))
    }
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>> {
        self.rows
            .values()
            .find(|x| x.0.hash == hash)
            .map(|x| x.1.clone())
            .ok_or_else(|| anyhow!("unknown recorded L1 hash"))
    }
}

pub struct RpcL1 {
    rpc: Rpc,
}
impl RpcL1 {
    pub fn new(url: &str) -> Result<Self> {
        let rpc = Rpc::new(url)?;
        if rpc.call("eth_chainId", json!([]))?
            != format!("0x{:x}", boole_derivation::config::L1_CHAIN_ID)
        {
            bail!("unexpected L1 chain ID");
        }
        Ok(Self { rpc })
    }
}
impl L1Source for RpcL1 {
    fn finalized(&mut self) -> Result<u64> {
        let block = self
            .rpc
            .call("eth_getBlockByNumber", json!(["finalized", false]))?;
        let number = block["number"]
            .as_str()
            .and_then(|s| s.strip_prefix("0x"))
            .ok_or_else(|| anyhow!("finalized block missing number"))?;
        Ok(u64::from_str_radix(number, 16)?)
    }
    fn block(&mut self, n: u64) -> Result<L1Block> {
        Ok(serde_json::from_value(self.rpc.call(
            "eth_getBlockByNumber",
            json!([format!("0x{n:x}"), false]),
        )?)?)
    }
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>> {
        Ok(serde_json::from_value(self.rpc.call("eth_getLogs",json!([{"blockHash":hash,"address":boole_derivation::config::INBOX,"topics":[boole_derivation::config::posted_topic()]}]))?)?)
    }
}
