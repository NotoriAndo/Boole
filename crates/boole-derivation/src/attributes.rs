//! D4–D7/D17: pure input validation and canonical payload attributes.
use crate::user_tx::UserTxDecision;
use serde::{Deserialize, Serialize};

pub mod quantity {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("0x{v:x}"))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let s = String::deserialize(d)?;
        let raw = s
            .strip_prefix("0x")
            .ok_or_else(|| serde::de::Error::custom("hex quantity prefix"))?;
        u64::from_str_radix(raw, 16).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct L1Block {
    #[serde(with = "quantity")]
    pub number: u64,
    pub hash: String,
    pub parent_hash: String,
    #[serde(with = "quantity")]
    pub timestamp: u64,
    pub mix_hash: String,
    pub base_fee_per_gas: String,
    pub parent_beacon_block_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentHeader {
    #[serde(with = "quantity")]
    pub number: u64,
    pub hash: String,
    #[serde(with = "quantity")]
    pub timestamp: u64,
    #[serde(with = "quantity")]
    pub gas_limit: u64,
    pub miner: String,
    pub extra_data: String,
    #[serde(default)]
    pub l1_info: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxLog {
    pub address: String,
    pub topics: Vec<String>,
    pub block_hash: String,
    pub removed: bool,
    #[serde(with = "quantity")]
    pub transaction_index: u64,
    #[serde(with = "quantity")]
    pub log_index: u64,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PayloadAttributes {
    pub timestamp: String,
    pub prev_randao: String,
    pub suggested_fee_recipient: String,
    pub withdrawals: Vec<serde_json::Value>,
    pub parent_beacon_block_root: String,
    pub transactions: Vec<String>,
    pub no_tx_pool: bool,
    pub gas_limit: String,
    pub eip1559_params: String,
    pub min_base_fee: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ItemDecision {
    pub transaction_index: u64,
    pub log_index: u64,
    pub rule: &'static str,
    pub forward: bool,
    pub user: Option<UserTxDecision>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Derived {
    pub attributes: PayloadAttributes,
    pub decisions: Vec<ItemDecision>,
}

impl Derived {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&self.attributes)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum InputError {
    #[error("L1 height must be parent + 1")]
    Height,
    #[error("invalid input field: {0}")]
    Field(&'static str),
    #[error("log address, topic, block hash or removed flag is inconsistent")]
    Log,
    #[error("duplicate log index pair")]
    DuplicateLog,
    #[error("parent extraData must be 17 bytes, version 1")]
    ExtraData,
    #[error("parent timestamp overflow")]
    TimestampOverflow,
    #[error("L1 parent hash does not match parent L2 origin")]
    L1Link,
}

pub fn hex_bytes(s: &str, field: &'static str) -> Result<Vec<u8>, InputError> {
    hex::decode(s.strip_prefix("0x").ok_or(InputError::Field(field))?)
        .map_err(|_| InputError::Field(field))
}
pub fn hex_string(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}
pub(crate) fn fixed_bytes(s: &str, len: usize, field: &'static str) -> Result<Vec<u8>, InputError> {
    let bytes = hex_bytes(s, field)?;
    if bytes.len() != len {
        return Err(InputError::Field(field));
    }
    Ok(bytes)
}

pub fn derive(
    l1: &L1Block,
    logs: &[InboxLog],
    parent: &ParentHeader,
) -> Result<Derived, InputError> {
    if parent.number.checked_add(1) != Some(l1.number) {
        return Err(InputError::Height);
    }
    check_l1_link(l1, parent)?;
    let timestamp = l1.timestamp.max(
        parent
            .timestamp
            .checked_add(1)
            .ok_or(InputError::TimestampOverflow)?,
    );
    fixed_bytes(&parent.hash, 32, "parent hash")?;
    fixed_bytes(&l1.parent_hash, 32, "L1 parent hash")?;
    let extra = hex_bytes(&parent.extra_data, "extraData").map_err(|_| InputError::ExtraData)?;
    if extra.len() != 17 || extra[0] != 1 {
        return Err(InputError::ExtraData);
    }
    let mut fee = [0; 8];
    fee.copy_from_slice(&extra[9..17]);
    let mut attributes = PayloadAttributes {
        timestamp: format!("0x{timestamp:x}"),
        prev_randao: hex_string(&fixed_bytes(&l1.mix_hash, 32, "mixHash")?),
        suggested_fee_recipient: hex_string(&fixed_bytes(&parent.miner, 20, "miner")?),
        withdrawals: vec![],
        parent_beacon_block_root: hex_string(&fixed_bytes(
            &l1.parent_beacon_block_root,
            32,
            "beacon root",
        )?),
        transactions: vec![crate::system::l1_info(l1)?],
        no_tx_pool: true,
        gas_limit: format!("0x{:x}", parent.gas_limit),
        eip1559_params: hex_string(&extra[1..9]),
        min_base_fee: u64::from_be_bytes(fee),
    };
    let mut sorted: Vec<_> = logs.iter().collect();
    sorted.sort_by_key(|l| (l.transaction_index, l.log_index));
    let inbox = fixed_bytes(crate::config::INBOX, 20, "inbox")?;
    let topic = crate::user_tx::keccak(b"Posted(bytes)");
    let block_hash = fixed_bytes(&l1.hash, 32, "L1 hash")?;
    for log in &sorted {
        if log.removed
            || fixed_bytes(&log.address, 20, "log address").map_err(|_| InputError::Log)? != inbox
            || log.topics.len() != 1
            || fixed_bytes(&log.topics[0], 32, "log topic").map_err(|_| InputError::Log)? != topic
            || fixed_bytes(&log.block_hash, 32, "log block hash").map_err(|_| InputError::Log)?
                != block_hash
        {
            return Err(InputError::Log);
        }
    }
    if sorted.windows(2).any(|x| {
        (x[0].transaction_index, x[0].log_index) == (x[1].transaction_index, x[1].log_index)
    }) {
        return Err(InputError::DuplicateLog);
    }
    let mut decisions = Vec::new();
    for log in sorted {
        let data = hex_bytes(&log.data, "log data")?;
        let mut decision = ItemDecision {
            transaction_index: log.transaction_index,
            log_index: log.log_index,
            rule: "INBOX-ENVELOPE-INVALID",
            forward: false,
            user: None,
        };
        match crate::inbox::user_body(&data) {
            Err(rule) => decision.rule = rule,
            Ok(body) => {
                let user = crate::user_tx::filter_user(body, parent.gas_limit);
                decision.rule = user.rule;
                decision.forward = user.forward;
                if user.forward {
                    attributes.transactions.push(hex_string(body));
                }
                decision.user = Some(user);
            }
        }
        decisions.push(decision);
    }
    Ok(Derived {
        attributes,
        decisions,
    })
}
pub fn check_l1_link(l1: &L1Block, parent: &ParentHeader) -> Result<(), InputError> {
    if l1.number >= 2 {
        let deposit = parent.l1_info.as_deref().ok_or(InputError::L1Link)?;
        let origin =
            crate::system::l1_origin(deposit, parent.number).map_err(|_| InputError::L1Link)?;
        if !origin.eq_ignore_ascii_case(&l1.parent_hash) {
            return Err(InputError::L1Link);
        }
    }
    Ok(())
}
