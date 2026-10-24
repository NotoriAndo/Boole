//! Ethereum header/receipt authentication for the node-local input journal.
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::{hex_bytes, hex_string},
    rlp,
    user_tx::keccak,
    InboxLog, L1Block,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ROW_MAX: usize = 135_266_304;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputRow {
    pub schema_version: u8,
    pub block_hash: String,
    pub header_rlp: String,
    /// Full, transaction-ordered typed receipt bytes, never eth_getLogs authority.
    pub receipts: Option<Vec<String>>,
    /// Optional full transaction trie proof for local posting reconciliation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transactions: Option<Vec<String>>,
}
#[derive(Clone, Debug)]
pub struct VerifiedInput {
    pub block: L1Block,
    pub logs: Vec<InboxLog>,
    pub canonical_receipts: Vec<(String, bool)>,
}

fn canonical_integer(bytes: &[u8], maximum: usize) -> Result<()> {
    ensure!(
        bytes.len() <= maximum && bytes.first() != Some(&0),
        "noncanonical header/receipt integer"
    );
    Ok(())
}
fn number(bytes: &[u8]) -> Result<u64> {
    canonical_integer(bytes, 8)?;
    Ok(bytes.iter().fold(0, |n, b| (n << 8) | u64::from(*b)))
}
fn quantity(bytes: &[u8]) -> String {
    let full = hex::encode(bytes);
    let digits = full.trim_start_matches('0');
    format!("0x{}", if digits.is_empty() { "0" } else { digits })
}
pub fn quantity_bytes(value: &Value) -> Result<Vec<u8>> {
    let s = value
        .as_str()
        .context("RPC quantity missing")?
        .strip_prefix("0x")
        .context("quantity prefix")?;
    ensure!(
        !s.is_empty() && s.len() <= 64 && (s.len() == 1 || !s.starts_with('0')),
        "quantity canonical form/width"
    );
    let mut bytes = hex::decode(if s.len() % 2 == 1 {
        format!("0{s}")
    } else {
        s.to_owned()
    })?;
    if bytes == [0] {
        bytes.clear();
    }
    Ok(bytes)
}
fn data(value: &Value, size: Option<usize>) -> Result<Vec<u8>> {
    let raw = hex_bytes(value.as_str().context("RPC data missing")?, "RPC data")?;
    if let Some(size) = size {
        ensure!(raw.len() == size, "RPC data width");
    }
    Ok(raw)
}
/// Pinned EL header schema: pre-London/London/Shanghai only for genesis;
/// Cancun (20) and Prague/Fusaka (21) for derived devnet origins.
pub fn header_from_rpc(v: &Value) -> Result<Vec<u8>> {
    let mut fields = vec![];
    for (key, size) in [
        ("parentHash", 32),
        ("sha3Uncles", 32),
        ("miner", 20),
        ("stateRoot", 32),
        ("transactionsRoot", 32),
        ("receiptsRoot", 32),
        ("logsBloom", 256),
    ] {
        fields.push(rlp::bytes(&data(&v[key], Some(size))?));
    }
    for key in ["difficulty", "number", "gasLimit", "gasUsed", "timestamp"] {
        fields.push(rlp::bytes(&quantity_bytes(&v[key])?));
    }
    let extra = data(&v["extraData"], None)?;
    ensure!(extra.len() <= 32, "header extraData cap");
    fields.extend([
        rlp::bytes(&extra),
        rlp::bytes(&data(&v["mixHash"], Some(32))?),
        rlp::bytes(&data(&v["nonce"], Some(8))?),
    ]);
    let optional = [
        ("baseFeePerGas", None),
        ("withdrawalsRoot", Some(32)),
        ("blobGasUsed", None),
        ("excessBlobGas", None),
        ("parentBeaconBlockRoot", Some(32)),
        ("requestsHash", Some(32)),
    ];
    let mut missing = false;
    for (key, size) in optional {
        if v[key].is_null() {
            missing = true;
            continue;
        }
        ensure!(!missing, "header optional field hole");
        fields.push(rlp::bytes(&if size.is_some() {
            data(&v[key], size)?
        } else {
            quantity_bytes(&v[key])?
        }));
    }
    let raw = rlp::list(&fields);
    ensure!(
        hex_string(&keccak(&raw))
            .eq_ignore_ascii_case(v["hash"].as_str().context("RPC header hash")?),
        "RPC header keccak mismatch"
    );
    decode_header(&raw)?;
    Ok(raw)
}
fn decode_header(raw: &[u8]) -> Result<(L1Block, [u8; 32], [u8; 256])> {
    let list = rlp::decode(raw)
        .map_err(anyhow::Error::msg)?
        .children
        .context("header must be a list")?;
    ensure!(
        list.iter().all(|f| f.children.is_none()),
        "nested header field"
    );
    ensure!(
        matches!(list.len(), 15 | 16 | 17 | 20 | 21),
        "unsupported EL header field set"
    );
    for (i, size) in [
        (0, 32),
        (1, 32),
        (2, 20),
        (3, 32),
        (4, 32),
        (5, 32),
        (6, 256),
        (13, 32),
        (14, 8),
    ] {
        ensure!(list[i].bytes.len() == size, "header field width");
    }
    canonical_integer(list[7].bytes, 32)?;
    let n = number(list[8].bytes)?;
    let _gas = number(list[9].bytes)?;
    let _used = number(list[10].bytes)?;
    let timestamp = number(list[11].bytes)?;
    ensure!(list[12].bytes.len() <= 32, "header extraData cap");
    ensure!(
        n == 0 || list.len() >= 20,
        "derived origin requires Cancun header"
    );
    if list.len() > 15 {
        canonical_integer(list[15].bytes, 32)?;
    }
    if list.len() > 16 {
        ensure!(list[16].bytes.len() == 32, "withdrawals root width");
    }
    if list.len() >= 20 {
        number(list[17].bytes)?;
        number(list[18].bytes)?;
        ensure!(list[19].bytes.len() == 32, "beacon root width");
    }
    if list.len() == 21 {
        ensure!(list[20].bytes.len() == 32, "requests hash width");
    }
    let block = L1Block {
        number: n,
        hash: hex_string(&keccak(raw)),
        parent_hash: hex_string(list[0].bytes),
        timestamp,
        mix_hash: hex_string(list[13].bytes),
        base_fee_per_gas: if list.len() > 15 {
            quantity(list[15].bytes)
        } else {
            "0x0".into()
        },
        parent_beacon_block_root: if list.len() >= 20 {
            hex_string(list[19].bytes)
        } else {
            hex_string(&[0; 32])
        },
    };
    if n > 0 {
        boole_derivation::system::l1_info(&block)?;
    }
    Ok((block, list[5].bytes.try_into()?, list[6].bytes.try_into()?))
}
pub fn bloom_contains(bloom: &[u8; 256], bytes: &[u8]) -> bool {
    let hash = keccak(bytes);
    (0..3).all(|i| {
        let bit = usize::from(u16::from_be_bytes([hash[2 * i], hash[2 * i + 1]]) & 2047);
        bloom[255 - bit / 8] & (1 << (bit % 8)) != 0
    })
}
fn add_bloom(bloom: &mut [u8; 256], bytes: &[u8]) {
    let hash = keccak(bytes);
    for i in 0..3 {
        let bit = usize::from(u16::from_be_bytes([hash[2 * i], hash[2 * i + 1]]) & 2047);
        bloom[255 - bit / 8] |= 1 << (bit % 8);
    }
}
pub fn header_needs_receipts(raw: &[u8]) -> Result<bool> {
    let (_, _, bloom) = decode_header(raw)?;
    Ok(bloom_contains(
        &bloom,
        &hex_bytes(boole_derivation::config::INBOX, "inbox")?,
    ) && bloom_contains(
        &bloom,
        &hex_bytes(&boole_derivation::config::posted_topic(), "topic")?,
    ))
}
pub fn receipt_from_rpc(v: &Value) -> Result<Vec<u8>> {
    let mut logs = vec![];
    for log in v["logs"].as_array().context("receipt logs")? {
        let topics = log["topics"]
            .as_array()
            .context("receipt topics")?
            .iter()
            .map(|t| Ok(rlp::bytes(&data(t, Some(32))?)))
            .collect::<Result<Vec<_>>>()?;
        ensure!(topics.len() <= 4, "receipt topic count");
        logs.push(rlp::list(&[
            rlp::bytes(&data(&log["address"], Some(20))?),
            rlp::list(&topics),
            rlp::bytes(&data(&log["data"], None)?),
        ]));
    }
    let raw = rlp::list(&[
        rlp::bytes(&quantity_bytes(&v["status"])?),
        rlp::bytes(&quantity_bytes(&v["cumulativeGasUsed"])?),
        rlp::bytes(&data(&v["logsBloom"], Some(256))?),
        rlp::list(&logs),
    ]);
    let kind = if v["type"].is_null() {
        0
    } else {
        number(&quantity_bytes(&v["type"])?)?
    };
    ensure!(kind <= 4, "unsupported L1 receipt type");
    Ok(if kind == 0 {
        raw
    } else {
        [vec![kind as u8], raw].concat()
    })
}

fn compact(path: &[u8], leaf: bool) -> Vec<u8> {
    let odd = path.len() % 2;
    let flag = if leaf { 2 } else { 0 };
    let mut nibbles = vec![flag + odd as u8];
    if odd == 0 {
        nibbles.push(0);
    }
    nibbles.extend(path);
    nibbles
        .chunks_exact(2)
        .map(|v| (v[0] << 4) | v[1])
        .collect()
}
fn child(raw: Vec<u8>) -> Vec<u8> {
    if raw.len() < 32 {
        raw
    } else {
        rlp::bytes(&keccak(&raw))
    }
}
fn trie(rows: &[(Vec<u8>, Vec<u8>)], at: usize) -> Vec<u8> {
    if rows.len() == 1 {
        return rlp::list(&[
            rlp::bytes(&compact(&rows[0].0[at..], true)),
            rlp::bytes(&rows[0].1),
        ]);
    }
    let common = (at..rows[0].0.len())
        .take_while(|i| rows.iter().all(|r| r.0.get(*i) == Some(&rows[0].0[*i])))
        .count();
    if common > 0 {
        return rlp::list(&[
            rlp::bytes(&compact(&rows[0].0[at..at + common], false)),
            child(trie(rows, at + common)),
        ]);
    }
    let mut branch = vec![rlp::bytes(&[]); 17];
    for (nibble, slot) in branch.iter_mut().enumerate().take(16) {
        let group: Vec<_> = rows
            .iter()
            .filter(|r| r.0.get(at) == Some(&(nibble as u8)))
            .cloned()
            .collect();
        if !group.is_empty() {
            *slot = child(trie(&group, at + 1));
        }
    }
    if let Some(row) = rows.iter().find(|r| r.0.len() == at) {
        branch[16] = rlp::bytes(&row.1);
    }
    rlp::list(&branch)
}
pub fn receipts_root(receipts: &[Vec<u8>]) -> [u8; 32] {
    if receipts.is_empty() {
        return keccak(&rlp::bytes(&[]));
    }
    let rows: Vec<_> = receipts
        .iter()
        .enumerate()
        .map(|(i, r)| {
            (
                rlp::uint(i as u64)
                    .iter()
                    .flat_map(|b| [b >> 4, b & 15])
                    .collect(),
                r.clone(),
            )
        })
        .collect();
    keccak(&trie(&rows, 0))
}
/// Ordered transaction trie authentication, independent of archive metadata.
pub fn verify_transactions(header: &[u8], transactions: &[String]) -> Result<Vec<String>> {
    decode_header(header)?;
    ensure!(transactions.len() <= 65536, "transaction count cap");
    let txs: Vec<_> = transactions
        .iter()
        .map(|tx| hex_bytes(tx, "transaction proof").map_err(Into::into))
        .collect::<Result<_>>()?;
    ensure!(
        txs.iter().all(|tx| !tx.is_empty())
            && txs.iter().map(Vec::len).sum::<usize>() <= ROW_MAX / 2,
        "transaction proof cap/shape"
    );
    let fields = rlp::decode(header)
        .map_err(anyhow::Error::msg)?
        .children
        .context("header")?;
    ensure!(
        receipts_root(&txs).as_slice() == fields[4].bytes,
        "transaction trie root mismatch"
    );
    Ok(txs.iter().map(|tx| hex_string(&keccak(tx))).collect())
}
pub fn empty_transactions(header: &[u8]) -> Result<bool> {
    decode_header(header)?;
    let fields = rlp::decode(header)
        .map_err(anyhow::Error::msg)?
        .children
        .context("header")?;
    Ok(fields[4].bytes == receipts_root(&[]))
}
impl InputRow {
    pub fn from_rpc(header: &Value, receipts: Option<&[Value]>) -> Result<Self> {
        let raw = header_from_rpc(header)?;
        let row = Self {
            schema_version: 1,
            block_hash: header["hash"].as_str().context("header hash")?.to_owned(),
            header_rlp: hex_string(&raw),
            receipts: receipts
                .map(|rows| {
                    rows.iter()
                        .map(|r| -> Result<_> { Ok(hex_string(&receipt_from_rpc(r)?)) })
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?,
            transactions: None,
        };
        row.verify()?;
        Ok(row)
    }
    pub fn verify(&self) -> Result<VerifiedInput> {
        ensure!(self.schema_version == 1, "input row schema");
        let raw = hex_bytes(&self.header_rlp, "journal header")?;
        ensure!(raw.len() <= 4096, "header cap");
        let (block, root, bloom) = decode_header(&raw)?;
        ensure!(
            block.hash.eq_ignore_ascii_case(&self.block_hash),
            "journal header keccak mismatch"
        );
        let needed = header_needs_receipts(&raw)?;
        let Some(receipts) = &self.receipts else {
            ensure!(!needed, "bloom-positive origin requires full receipts");
            ensure!(
                self.transactions.is_none(),
                "transaction proof requires full receipts"
            );
            return Ok(VerifiedInput {
                block,
                logs: vec![],
                canonical_receipts: vec![],
            });
        };
        ensure!(receipts.len() <= 65536, "receipt count cap");
        let bytes: Vec<_> = receipts
            .iter()
            .map(|r| Ok(hex_bytes(r, "journal receipt")?))
            .collect::<Result<_>>()?;
        ensure!(
            bytes.iter().map(Vec::len).sum::<usize>() <= ROW_MAX / 2,
            "receipt byte cap"
        );
        ensure!(receipts_root(&bytes) == root, "receipt trie root mismatch");
        let inbox = hex_bytes(boole_derivation::config::INBOX, "inbox")?;
        let posted = hex_bytes(&boole_derivation::config::posted_topic(), "posted")?;
        let mut total_bloom = [0u8; 256];
        let mut global = 0u64;
        let mut out = vec![];
        let mut previous_gas = 0;
        let mut statuses = vec![];
        for (tx, raw) in bytes.iter().enumerate() {
            let payload = if raw.first().is_some_and(|b| *b < 0x80) {
                ensure!(matches!(raw[0], 1..=4), "receipt type");
                &raw[1..]
            } else {
                raw.as_slice()
            };
            let fields = rlp::decode(payload)
                .map_err(anyhow::Error::msg)?
                .children
                .context("receipt list")?;
            ensure!(
                fields.len() == 4 && fields[..3].iter().all(|f| f.children.is_none()),
                "receipt shape"
            );
            ensure!(
                fields[0].bytes.is_empty() || fields[0].bytes == [1],
                "receipt status"
            );
            statuses.push(fields[0].bytes == [1]);
            let gas = number(fields[1].bytes)?;
            ensure!(gas >= previous_gas, "receipt cumulative gas order");
            previous_gas = gas;
            ensure!(fields[2].bytes.len() == 256, "receipt bloom width");
            let mut computed = [0u8; 256];
            for log in fields[3].children.as_ref().context("receipt log list")? {
                let f = log.children.as_ref().context("receipt log fields")?;
                ensure!(
                    f.len() == 3
                        && f[0].children.is_none()
                        && f[2].children.is_none()
                        && f[0].bytes.len() == 20,
                    "receipt log shape"
                );
                let topics = f[1].children.as_ref().context("log topics")?;
                ensure!(
                    topics.len() <= 4
                        && topics
                            .iter()
                            .all(|t| t.children.is_none() && t.bytes.len() == 32),
                    "log topics shape"
                );
                add_bloom(&mut computed, f[0].bytes);
                for topic in topics {
                    add_bloom(&mut computed, topic.bytes);
                }
                if f[0].bytes == inbox && topics.first().is_some_and(|t| t.bytes == posted) {
                    out.push(InboxLog {
                        address: hex_string(f[0].bytes),
                        topics: topics.iter().map(|t| hex_string(t.bytes)).collect(),
                        block_hash: block.hash.clone(),
                        removed: false,
                        transaction_index: tx as u64,
                        log_index: global,
                        data: hex_string(f[2].bytes),
                    });
                }
                global = global.checked_add(1).context("log index overflow")?;
            }
            ensure!(
                computed.as_slice() == fields[2].bytes,
                "receipt bloom disagrees with logs"
            );
            for i in 0..256 {
                total_bloom[i] |= computed[i];
            }
        }
        ensure!(total_bloom == bloom, "header bloom disagrees with receipts");
        ensure!(
            needed || out.is_empty(),
            "bloom-negative origin contains inbox log"
        );
        let canonical_receipts = if let Some(transactions) = &self.transactions {
            ensure!(
                transactions.len() == receipts.len(),
                "transaction/receipt count mismatch"
            );
            verify_transactions(&raw, transactions)?
                .into_iter()
                .zip(statuses)
                .collect()
        } else {
            vec![]
        };
        Ok(VerifiedInput {
            block,
            logs: out,
            canonical_receipts,
        })
    }
    pub fn block(&self) -> Result<L1Block> {
        Ok(decode_header(&hex_bytes(&self.header_rlp, "header")?)?.0)
    }
}

/// Synthetic authenticated fixture for portable journal and driver tests.
/// Native qualification separately checks these encodings against pinned anvil.
#[cfg(test)]
pub(crate) fn empty_fixture(number: u64, parent: [u8; 32]) -> InputRow {
    let fields = vec![
        rlp::bytes(&parent),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&[0; 20]),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&receipts_root(&[])),
        rlp::bytes(&receipts_root(&[])),
        rlp::bytes(&[0; 256]),
        rlp::uint(0),
        rlp::uint(number),
        rlp::uint(60_000_000),
        rlp::uint(0),
        rlp::uint(1_790_000_000 + 12 * number),
        rlp::bytes(&[]),
        rlp::bytes(&[1; 32]),
        rlp::bytes(&[0; 8]),
        rlp::uint(1),
        rlp::bytes(&[0; 32]),
        rlp::uint(0),
        rlp::uint(0),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&[0; 32]),
    ];
    let raw = rlp::list(&fields);
    InputRow {
        schema_version: 1,
        block_hash: hex_string(&keccak(&raw)),
        header_rlp: hex_string(&raw),
        receipts: Some(vec![]),
        transactions: Some(vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn replace(row: &mut InputRow, index: usize, encoded: Vec<u8>) {
        let raw = hex_bytes(&row.header_rlp, "fixture").unwrap();
        let decoded = rlp::decode(&raw).unwrap().children.unwrap();
        let mut fields: Vec<_> = decoded
            .iter()
            .map(|f| raw[f.span.clone()].to_vec())
            .collect();
        fields[index] = encoded;
        let raw = rlp::list(&fields);
        row.block_hash = hex_string(&keccak(&raw));
        row.header_rlp = hex_string(&raw);
    }
    #[test]
    fn mutated_header_and_bloom_positive_without_receipts_are_refused() {
        let mut row = empty_fixture(1, [0; 32]);
        let mut raw = hex_bytes(&row.header_rlp, "fixture").unwrap();
        *raw.last_mut().unwrap() ^= 1;
        row.header_rlp = hex_string(&raw);
        assert!(row.verify().unwrap_err().to_string().contains("keccak"));
        let mut row = empty_fixture(1, [0; 32]);
        let mut bloom = [0; 256];
        add_bloom(
            &mut bloom,
            &hex_bytes(boole_derivation::config::INBOX, "inbox").unwrap(),
        );
        add_bloom(
            &mut bloom,
            &hex_bytes(&boole_derivation::config::posted_topic(), "topic").unwrap(),
        );
        replace(&mut row, 6, rlp::bytes(&bloom));
        assert!(row.verify().is_err());
        assert!(empty_fixture(1, [0; 32]).verify().unwrap().logs.is_empty());
    }
    #[test]
    fn receipt_root_and_block_global_log_positions_are_authenticated() {
        assert_eq!(
            hex::encode(receipts_root(&[])),
            "56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421"
        );
        let inbox = hex_bytes(boole_derivation::config::INBOX, "inbox").unwrap();
        let topic = hex_bytes(&boole_derivation::config::posted_topic(), "topic").unwrap();
        let log = |address: &[u8], topics: Vec<Vec<u8>>| {
            rlp::list(&[
                rlp::bytes(address),
                rlp::list(&topics),
                rlp::bytes(b"public"),
            ])
        };
        let mut b1 = [0; 256];
        add_bloom(&mut b1, &[9; 20]);
        let mut b2 = [0; 256];
        add_bloom(&mut b2, &inbox);
        add_bloom(&mut b2, &topic);
        let receipt = |gas, bloom: &[u8], logs: Vec<Vec<u8>>| {
            rlp::list(&[
                rlp::uint(1),
                rlp::uint(gas),
                rlp::bytes(bloom),
                rlp::list(&logs),
            ])
        };
        let first = receipt(21000, &b1, vec![log(&[9; 20], vec![])]);
        let second = [
            vec![2],
            receipt(50000, &b2, vec![log(&inbox, vec![rlp::bytes(&topic)])]),
        ]
        .concat();
        let bytes = vec![first, second];
        let mut row = empty_fixture(1, [0; 32]);
        replace(&mut row, 5, rlp::bytes(&receipts_root(&bytes)));
        let mut bloom = b1;
        for i in 0..256 {
            bloom[i] |= b2[i];
        }
        replace(&mut row, 6, rlp::bytes(&bloom));
        row.receipts = Some(bytes.iter().map(|v| hex_string(v)).collect());
        row.transactions = None;
        let original = row.clone();
        row.receipts.as_mut().unwrap().remove(0);
        assert!(row.verify().is_err());
        let verified = original.verify().unwrap();
        assert_eq!(verified.logs.len(), 1);
        assert_eq!(verified.logs[0].transaction_index, 1);
        assert_eq!(verified.logs[0].log_index, 1);
        let mut wrong = original.clone();
        replace(&mut wrong, 6, rlp::bytes(&[0; 256]));
        assert!(wrong.verify().is_err());
        let mut wrong = original;
        wrong.receipts.as_mut().unwrap().swap(0, 1);
        assert!(wrong.verify().is_err());
    }
}
