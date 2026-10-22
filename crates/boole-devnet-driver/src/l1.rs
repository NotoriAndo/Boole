use crate::engine::Rpc;
use anyhow::{anyhow, bail, Result};
use boole_derivation::{InboxLog, L1Block};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub trait L1Source {
    /// Genesis may predate header fields required by the derivation fold.
    fn genesis_hash(&mut self) -> Result<String> {
        Ok(self.block(0)?.hash)
    }
    /// Offline recordings/mocks have no runtime. RPC sources verify the profile.
    fn validate_m2_inbox(&mut self) -> Result<()> {
        Ok(())
    }

    fn finalized(&mut self) -> Result<u64>;
    fn block(&mut self, number: u64) -> Result<L1Block>;
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>>;
    fn block_by_hash(&mut self, _hash: &str) -> Result<L1Block> {
        bail!("L1 source does not support anchored restart")
    }
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
    fn block_by_hash(&mut self, hash: &str) -> Result<L1Block> {
        self.rows
            .values()
            .find(|row| row.0.hash.eq_ignore_ascii_case(hash))
            .map(|row| row.0.clone())
            .ok_or_else(|| anyhow!("unknown anchored L1 hash"))
    }

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

/// SHA-256 of the closed devnet M2 inbox runtime, excluding genesis JSON encoding.
pub const M2_INBOX_RUNTIME_SHA256: &str =
    "a14704d805198421bae48bf79534bc62d974699f203abc3ed4bc751e35fe0ca6";
pub fn validate_m2_inbox_code(code: &str) -> Result<()> {
    use sha2::{Digest, Sha256};
    let bytes = boole_derivation::attributes::hex_bytes(code, "M2 inbox runtime")?;
    if format!("{:x}", Sha256::digest(bytes)) != M2_INBOX_RUNTIME_SHA256 {
        bail!("M2 inbox runtime does not match the devnet profile; use the M2 genesis");
    }
    Ok(())
}
pub struct RpcL1 {
    rpc: Rpc,
    m2_validated: bool,
}
impl RpcL1 {
    /// M2 entrypoint: validate deployed code before any Engine or fold activity.
    /// `new` remains the M1-only fixture transport entrypoint.
    pub fn new_m2(url: &str) -> Result<Self> {
        let mut source = Self::new(url)?;
        source.validate_m2_inbox()?;
        Ok(source)
    }

    pub fn new(url: &str) -> Result<Self> {
        let rpc = Rpc::new(url)?;
        if rpc.call("eth_chainId", json!([]))?
            != format!("0x{:x}", boole_derivation::config::L1_CHAIN_ID)
        {
            bail!("unexpected L1 chain ID");
        }
        Ok(Self {
            rpc,
            m2_validated: false,
        })
    }
}
impl L1Source for RpcL1 {
    fn genesis_hash(&mut self) -> Result<String> {
        let block = self
            .rpc
            .call("eth_getBlockByNumber", json!(["0x0", false]))?;
        if block["number"] != "0x0" {
            bail!("L1 genesis height mismatch");
        }
        let hash = block["hash"]
            .as_str()
            .ok_or_else(|| anyhow!("L1 genesis hash missing"))?;
        if boole_derivation::attributes::hex_bytes(hash, "L1 genesis hash")?.len() != 32 {
            bail!("L1 genesis hash length");
        }
        Ok(hash.into())
    }

    fn validate_m2_inbox(&mut self) -> Result<()> {
        if !self.m2_validated {
            let code = self.rpc.call(
                "eth_getCode",
                json!([boole_derivation::config::INBOX, "latest"]),
            )?;
            validate_m2_inbox_code(
                code.as_str()
                    .ok_or_else(|| anyhow!("M2 inbox runtime missing"))?,
            )?;
            self.m2_validated = true;
        }
        Ok(())
    }

    fn block_by_hash(&mut self, hash: &str) -> Result<L1Block> {
        Ok(serde_json::from_value(
            self.rpc.call("eth_getBlockByHash", json!([hash, false]))?,
        )?)
    }

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
