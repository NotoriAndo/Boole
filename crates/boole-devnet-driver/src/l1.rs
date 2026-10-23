use crate::engine::Rpc;
use anyhow::{anyhow, bail, Context, Result};
use boole_derivation::{InboxLog, L1Block};
use serde_json::{json, Value};
use std::collections::BTreeMap;
fn capture_failure(height: u64, finalized: u64, error: anyhow::Error) -> anyhow::Error {
    if height > finalized {
        boole_devnet_archive::error::availability(
            height,
            "live-capture",
            format!("unfinalized input changed or is unavailable: {error:#}"),
        )
    } else if crate::engine::rpc_unavailable(&error) {
        error
    } else {
        boole_devnet_archive::error::integrity(
            height,
            "finalized-capture",
            format!("finalized input verification failed: {error:#}"),
        )
    }
}

pub trait L1Source {
    /// During verified Engine-tail replay, use only existing own journal rows.
    fn set_journal_replay(&mut self, _enabled: bool) {}
    fn input_row(&mut self, _height: u64) -> Result<boole_devnet_archive::input::InputRow> {
        bail!("L1 source does not expose authenticated journal inputs")
    }
    fn header_rlp(&mut self, height: u64) -> Result<String> {
        Ok(self.input_row(height)?.header_rlp)
    }
    fn input_journal(
        &self,
    ) -> Option<std::rc::Rc<std::cell::RefCell<boole_devnet_archive::journal::InputJournal>>> {
        None
    }
    /// Genesis may predate header fields required by the derivation fold.
    fn genesis_hash(&mut self) -> Result<String> {
        Ok(self.block(0)?.hash)
    }
    /// Offline recordings/mocks have no runtime. RPC sources verify the profile.
    fn validate_m2_inbox(&mut self) -> Result<()> {
        Ok(())
    }

    fn finalized(&mut self) -> Result<u64>;
    fn latest(&mut self) -> Result<u64> {
        self.finalized()
    }
    fn block(&mut self, number: u64) -> Result<L1Block>;
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>>;
    fn block_by_hash(&mut self, _hash: &str) -> Result<L1Block> {
        bail!("L1 source does not support anchored restart")
    }
}

/// The opt-in library source used by durable driver/client entrypoints.
/// Legacy recording sources remain available for pure v1.3 replay.
pub struct JournalSource<L> {
    pub inner: L,
    pub journal: std::rc::Rc<std::cell::RefCell<boole_devnet_archive::journal::InputJournal>>,
    remote_tip: Option<u64>,
    live_head: Option<(u64, String)>,
    acquisition: boole_devnet_archive::objects::Objects,
    journal_replay: bool,
}
impl<L: L1Source> JournalSource<L> {
    pub fn open(
        mut inner: L,
        root: &std::path::Path,
        network: boole_devnet_archive::journal::NetworkIdentity,
        repair: bool,
    ) -> Result<Self> {
        let journal = boole_devnet_archive::journal::InputJournal::open(
            root,
            network,
            || {
                anyhow::ensure!(inner.finalized()?==0,"fresh TIP_ONLY journal requires its genesis anchor; use verified journal recovery for an existing chain");
                inner.input_row(0)
            },
            repair,
        )?;
        let acquisition =
            boole_devnet_archive::objects::Objects::open(&journal.root().join("input-cache"))?;
        Ok(Self {
            inner,
            journal: std::rc::Rc::new(std::cell::RefCell::new(journal)),
            remote_tip: None,
            live_head: None,
            acquisition,
            journal_replay: false,
        })
    }
    fn cached(&self, hash: &str) -> Result<Option<boole_devnet_archive::input::InputRow>> {
        let result = self.acquisition.read(hash, |bytes| {
            let row: boole_devnet_archive::input::InputRow = serde_json::from_slice(bytes)?;
            row.verify()?;
            anyhow::ensure!(
                row.block_hash == hash && row.transactions.is_some() && row.receipts.is_some(),
                "incomplete acquisition proof"
            );
            Ok(())
        });
        match result {
            Ok(Some(bytes)) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Ok(None) => Ok(None),
            Err(_) => Ok(None),
        }
    }
    fn refresh(&mut self) -> Result<u64> {
        use boole_derivation::{
            attributes::{hex_bytes, hex_string},
            user_tx::keccak,
        };
        use boole_devnet_archive::error::{availability, integrity};
        self.journal.borrow().check()?;
        let tip = crate::engine::retry_rpc(|| self.inner.finalized())?;
        let saved = self.journal.borrow().tip();
        if tip < saved || self.remote_tip.is_some_and(|old| tip < old) {
            return Err(integrity(
                tip,
                "l1-anchor",
                "finalized height moved backwards",
            ));
        }
        let header = crate::engine::retry_rpc(|| self.inner.header_rlp(tip))
            .map_err(|e| capture_failure(tip, tip, e))?;
        let hash = hex_string(&keccak(&hex_bytes(&header, "finalized header")?));
        let latest = crate::engine::retry_rpc(|| self.inner.latest())?;
        if latest < tip || latest - tip > 1024 {
            return Err(availability(
                tip,
                "rpc",
                "bounded live acquisition window unavailable",
            ));
        }
        let live_hash = if latest == tip {
            hash.clone()
        } else {
            hex_string(&keccak(&hex_bytes(
                &crate::engine::retry_rpc(|| self.inner.header_rlp(latest))
                    .map_err(|e| capture_failure(latest, tip, e))?,
                "live header",
            )?))
        };
        if self.live_head.as_ref() != Some(&(latest, live_hash.clone()))
            || self.remote_tip != Some(tip)
        {
            // Unfinalized cache is not fold authority. Only heights at/above the
            // current finalized tip are read from RPC, including on restart.
            for h in tip..=latest {
                let expected = if h == tip {
                    hash.clone()
                } else if h == latest {
                    live_hash.clone()
                } else {
                    hex_string(&keccak(&hex_bytes(
                        &crate::engine::retry_rpc(|| self.inner.header_rlp(h))
                            .map_err(|e| capture_failure(h, tip, e))?,
                        "live header",
                    )?))
                };
                if self.journal.borrow().hash(h) == Some(expected.as_str())
                    || self.cached(&expected)?.is_some()
                {
                    continue;
                }
                let row = (|| -> Result<_> {
                    let row = crate::engine::retry_rpc(|| self.inner.input_row(h))?;
                    row.verify()?;
                    anyhow::ensure!(
                        row.block_hash == expected,
                        "live input changed during capture"
                    );
                    Ok(row)
                })()
                .map_err(|e| capture_failure(h, tip, e))?;
                let bytes = serde_json::to_vec(&row)?;
                self.acquisition.put(&expected, &bytes, |bytes| {
                    let row: boole_devnet_archive::input::InputRow = serde_json::from_slice(bytes)?;
                    row.verify()?;
                    anyhow::ensure!(row.block_hash == expected, "acquisition address mismatch");
                    Ok(())
                })?;
            }
            self.live_head = Some((latest, live_hash));
        }
        if tip == saved {
            if self.journal.borrow().hash(tip) != Some(hash.as_str()) {
                return Err(integrity(tip, "l1-anchor", "finalized tip hash changed"));
            }
        } else {
            // Authenticate the entire cached segment back from the independently
            // obtained finality hash before publishing even its first row.
            let mut rows = vec![];
            let mut cursor = hash;
            for h in (saved + 1..=tip).rev() {
                let row=self.cached(&cursor)?.ok_or_else(||availability(h,"input-cache","missing authenticated input below current tip; verified archive/journal recovery required"))?;
                let block = row.block()?;
                if block.number != h {
                    return Err(integrity(h, "input-cache", "cached header height mismatch"));
                }
                cursor = block.parent_hash;
                rows.push(row);
            }
            if self.journal.borrow().hash(saved) != Some(cursor.as_str()) {
                return Err(integrity(
                    saved,
                    "l1-anchor",
                    "new finalized segment does not extend the journal",
                ));
            }
            for row in rows.iter().rev() {
                self.journal.borrow_mut().append(row)?;
                boole_devnet_archive::fault::hit("B1", row.block()?.number, "driver")?;
            }
        }
        self.remote_tip = Some(tip);
        Ok(tip)
    }
}

impl<L> std::ops::Deref for JournalSource<L> {
    type Target = L;
    fn deref(&self) -> &L {
        &self.inner
    }
}
impl<L> std::ops::DerefMut for JournalSource<L> {
    fn deref_mut(&mut self) -> &mut L {
        &mut self.inner
    }
}
impl<L: L1Source> L1Source for JournalSource<L> {
    fn set_journal_replay(&mut self, enabled: bool) {
        self.journal_replay = enabled;
    }
    fn genesis_hash(&mut self) -> Result<String> {
        Ok(self.journal.borrow().get(0)?.block.hash)
    }
    fn validate_m2_inbox(&mut self) -> Result<()> {
        self.inner.validate_m2_inbox()
    }
    fn finalized(&mut self) -> Result<u64> {
        if self.journal_replay {
            self.journal.borrow().check()?;
            return Ok(self.journal.borrow().tip());
        }
        self.refresh()
    }
    fn block(&mut self, n: u64) -> Result<L1Block> {
        if !self.journal_replay
            && (self.remote_tip.is_none()
                || self.remote_tip == Some(n)
                || n > self.journal.borrow().tip())
        {
            self.refresh()?;
        }
        Ok(self.journal.borrow().get(n)?.block)
    }
    fn inbox_logs(&mut self, hash: &str) -> Result<Vec<InboxLog>> {
        let h = self.journal.borrow().height_for_hash(hash).ok_or_else(|| {
            boole_devnet_archive::error::availability(
                0,
                "journal",
                "unknown origin hash; no historical RPC fallback",
            )
        })?;
        self.block(h)?;
        Ok(self.journal.borrow().get(h)?.logs)
    }
    fn block_by_hash(&mut self, hash: &str) -> Result<L1Block> {
        let h = self.journal.borrow().height_for_hash(hash).ok_or_else(|| {
            boole_devnet_archive::error::availability(
                0,
                "journal",
                "unknown origin hash; no historical RPC fallback",
            )
        })?;
        self.block(h)
    }
    fn input_journal(
        &self,
    ) -> Option<std::rc::Rc<std::cell::RefCell<boole_devnet_archive::journal::InputJournal>>> {
        Some(self.journal.clone())
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
    fn latest(&mut self) -> Result<u64> {
        let v = self.rpc.call("eth_blockNumber", json!([]))?;
        Ok(u64::from_str_radix(
            v.as_str()
                .and_then(|s| s.strip_prefix("0x"))
                .context("latest height")?,
            16,
        )?)
    }

    fn header_rlp(&mut self, height: u64) -> Result<String> {
        let v = self.rpc.call(
            "eth_getBlockByNumber",
            json!([format!("0x{height:x}"), false]),
        )?;
        Ok(boole_derivation::attributes::hex_string(
            &boole_devnet_archive::input::header_from_rpc(&v)?,
        ))
    }
    fn input_row(&mut self, height: u64) -> Result<boole_devnet_archive::input::InputRow> {
        use boole_devnet_archive::input::{header_from_rpc, InputRow};
        let tag = format!("0x{height:x}");
        let header = self.rpc.call("eth_getBlockByNumber", json!([tag, false]))?;
        header_from_rpc(&header)?;
        let receipts = match self
            .rpc
            .call("eth_getBlockReceipts", json!([header["hash"]]))
        {
            Ok(v) => v.as_array().context("block receipt list")?.clone(),
            Err(e) if e.to_string().contains("-32601") => {
                let mut rows = vec![];
                for tx in header["transactions"]
                    .as_array()
                    .context("L1 transaction hashes")?
                {
                    rows.push(self.rpc.call("eth_getTransactionReceipt", json!([tx]))?);
                }
                rows
            }
            Err(e) => return Err(e),
        };
        let mut transactions = vec![];
        for hash in header["transactions"]
            .as_array()
            .context("L1 transaction hashes")?
        {
            let raw = self
                .rpc
                .call("eth_getRawTransactionByHash", json!([hash]))?;
            let raw = raw.as_str().context("raw L1 transaction unavailable")?;
            anyhow::ensure!(
                boole_derivation::attributes::hex_string(&boole_derivation::user_tx::keccak(
                    &boole_derivation::attributes::hex_bytes(raw, "L1 transaction")?
                ))
                .eq_ignore_ascii_case(hash.as_str().context("L1 transaction hash")?),
                "raw L1 transaction hash mismatch"
            );
            transactions.push(raw.to_owned());
        }
        let mut row = InputRow::from_rpc(&header, Some(&receipts))?;
        row.transactions = Some(transactions);
        row.verify()?;
        anyhow::ensure!(row.block()?.number == height, "captured L1 height mismatch");
        Ok(row)
    }
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
