//! T77: journal-only history with a transport that panics below its current tip.
use anyhow::{bail, Result};
use boole_derivation::{attributes::hex_bytes, InboxLog, L1Block, ParentHeader};
use boole_derivation::{attributes::hex_string, rlp, user_tx::keccak};
use boole_devnet_archive::{
    durability::PrivateTempDir,
    input::{receipts_root, InputRow},
    journal::{InputJournal, NetworkIdentity},
};
use boole_devnet_driver::{
    driver::Driver,
    engine::Engine,
    l1::{JournalSource, L1Source},
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn empty_fixture(number: u64, parent: [u8; 32]) -> InputRow {
    let fields = vec![
        rlp::bytes(&parent),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&[0; 20]),
        rlp::bytes(&[0; 32]),
        rlp::bytes(&[0; 32]),
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
        receipts: None,
        transactions: None,
    }
}

#[derive(Clone)]
struct TipOnly {
    rows: Vec<InputRow>,
    tip: u64,
    strict: bool,
}
impl TipOnly {
    fn new() -> Self {
        let mut rows = vec![empty_fixture(0, [0; 32])];
        for h in 1..=4 {
            let parent = hex_bytes(&rows.last().unwrap().block_hash, "hash")
                .unwrap()
                .try_into()
                .unwrap();
            rows.push(empty_fixture(h, parent));
        }
        Self {
            rows,
            tip: 0,
            strict: false,
        }
    }
    fn read(&self, h: u64) {
        assert!(
            !self.strict || h >= self.tip,
            "TIP_ONLY transport read below tip: {h} < {}",
            self.tip
        );
    }
}
impl L1Source for TipOnly {
    fn finalized(&mut self) -> Result<u64> {
        Ok(self.tip)
    }
    fn block(&mut self, h: u64) -> Result<L1Block> {
        self.read(h);
        self.rows[h as usize].block()
    }
    fn block_by_hash(&mut self, hash: &str) -> Result<L1Block> {
        let row = self.rows.iter().find(|r| r.block_hash == hash).unwrap();
        self.read(row.block()?.number);
        row.block()
    }
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>> {
        let row = self.rows.iter().find(|r| r.block_hash == hash).unwrap();
        self.read(row.block()?.number);
        Ok(row.verify()?.logs)
    }
    fn input_row(&mut self, h: u64) -> Result<InputRow> {
        self.read(h);
        Ok(self.rows[h as usize].clone())
    }
}
#[derive(Clone)]
struct MemoryEngine {
    head: ParentHeader,
    headers: BTreeMap<u64, ParentHeader>,
    txs: BTreeMap<u64, Vec<String>>,
    attributes: Value,
}
impl MemoryEngine {
    fn new() -> Self {
        let rows: Value = serde_json::from_str(include_str!(
            "../../../fixtures/devnet-m1/s1b-recorded-main.json"
        ))
        .unwrap();
        let mut head: ParentHeader = serde_json::from_value(rows[0]["parent"].clone()).unwrap();
        head.timestamp = 1_790_000_000;
        Self {
            headers: BTreeMap::from([(0, head.clone())]),
            head,
            txs: BTreeMap::new(),
            attributes: Value::Null,
        }
    }
}
impl Engine for MemoryEngine {
    fn head(&mut self) -> Result<ParentHeader> {
        Ok(self.head.clone())
    }
    fn header(&mut self, h: u64) -> Result<ParentHeader> {
        Ok(self.headers[&h].clone())
    }
    fn transactions(&mut self, h: u64) -> Result<Vec<String>> {
        Ok(self.txs[&h].clone())
    }
    fn call(&mut self, method: &str, p: Value) -> Result<Value> {
        Ok(match method {
            "engine_forkchoiceUpdatedV3" => {
                if p[1].is_null() {
                    let h = self.head.number + 1;
                    let txs: Vec<String> =
                        serde_json::from_value(self.attributes["transactions"].clone())?;
                    self.head.parent_hash = Some(self.head.hash.clone());
                    self.head.hash = p[0]["headBlockHash"].as_str().unwrap().into();
                    self.head.number = h;
                    self.head.timestamp = u64::from_str_radix(
                        self.attributes["timestamp"]
                            .as_str()
                            .unwrap()
                            .trim_start_matches("0x"),
                        16,
                    )?;
                    self.head.l1_info = Some(txs[0].clone());
                    self.head.m2_root_deposit = Some(txs[1].clone());
                    self.headers.insert(h, self.head.clone());
                    self.txs.insert(h, txs);
                } else {
                    self.attributes = p[1].clone();
                }
                json!({"payloadStatus":{"status":"VALID"},"payloadId":"0x1"})
            }
            "engine_getPayloadV5" => {
                json!({"executionPayload":{"transactions":self.attributes["transactions"],"blockNumber":format!("0x{:x}",self.head.number+1),"blockHash":format!("0x{:064x}",self.head.number+1),"parentHash":self.head.hash,"timestamp":self.attributes["timestamp"],"gasLimit":self.attributes["gasLimit"]}})
            }
            "engine_newPayloadV4" => {
                self.attributes = p[0].clone();
                json!({"status":"VALID"})
            }
            _ => bail!("unexpected engine method"),
        })
    }
}
#[test]
fn t77_driver_restarts_from_verified_rows_without_any_historical_l1_read() -> Result<()> {
    let dir = PrivateTempDir::new("m4-tip-only")?;
    let engine = MemoryEngine::new();
    let config = boole_derivation::m2::Config::devnet();
    let network = NetworkIdentity::new(&config, None, engine.head.hash.clone(), 30_000_000)?;
    let mut raw = TipOnly::new();
    // Simulate already captured node-owned inputs independently of replay.
    let mut journal = InputJournal::open(
        dir.path(),
        network.clone(),
        || Ok(raw.rows[0].clone()),
        false,
    )?;
    journal.append(&raw.rows[1])?;
    journal.append(&raw.rows[2])?;
    drop(journal);
    raw.tip = 2;
    let source = JournalSource::open(raw, dir.path(), network.clone(), false)?;
    let mut a = Driver::new(source, engine);
    a.step(1)?;
    a.step(2)?;
    let engine = a.engine.clone();
    let mut raw = a.l1.inner.clone();
    drop(a);
    raw.tip = 3;
    raw.strict = true;
    let source = JournalSource::open(raw, dir.path(), network, false)?;
    let mut restarted = Driver::new(source, engine);
    restarted.step(3)?;
    assert_eq!(restarted.m2_state().height(), 3);
    assert_eq!(restarted.l1.journal.borrow().tip(), 3);
    Ok(())
}
