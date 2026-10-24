//! Public, finalized archive storage profile. Indexes are diagnostic tripwires.
use crate::journal::NetworkIdentity;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveIdentity {
    pub network: NetworkIdentity,
    pub l1_anchor_number: u64,
    pub l1_anchor_hash: String,
}
#[derive(Clone)]
pub struct ArchiveProfile {
    pub identity: ArchiveIdentity,
    pub bind: SocketAddr,
    pub prune_before: Option<u64>,
}
impl ArchiveProfile {
    pub fn validate(&self) -> Result<()> {
        if self.prune_before.is_some() {
            return Err(crate::error::startup(
                20,
                "archive input pruning is refused",
            ));
        }
        if !self.bind.ip().is_loopback() {
            return Err(crate::error::startup(21, "archive bind must be loopback"));
        }
        ensure!(
            self.identity.l1_anchor_number == 0,
            "S18: devnet genesis anchor required"
        );
        ensure!(
            boole_derivation::attributes::hex_bytes(&self.identity.l1_anchor_hash, "anchor")?.len()
                == 32,
            "S18: anchor width"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub l2_number: u64,
    pub l2_hash: String,
    pub root: String,
    pub system: Vec<String>,
    pub inputs: String,
    pub verdicts: String,
}

use crate::{
    index::IndexLog,
    input::{header_needs_receipts, InputRow},
    journal::{publish_new, read_bound},
    objects::Objects,
    state_dir::{self, StateDirGuard},
    version::{file_stamp, FileStamp},
};
use anyhow::Context;
use boole_derivation::attributes::{hex_bytes, hex_string};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
pub const SEGMENT: usize = 1024;
pub const OBJ_MAX: usize = 135_266_304;
pub const REQ_MAX: usize = 65_536;
pub const CLASSES: &[&str] = &[
    "header", "receipts", "exp", "tr", "pkg", "env", "gate", "vstmt", "system",
];
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexRow {
    pub l1_number: u64,
    pub l2_number: u64,
    pub l1_hash: String,
    pub parent_hash: String,
    pub bloom_positive: bool,
    pub blob_ids: Vec<String>,
    pub l2_hash: String,
    pub root: String,
    pub inputs: String,
    pub verdicts: String,
    pub system_digest: String,
}
fn hash(s: &str) -> Result<()> {
    ensure!(
        s.len() == 66
            && s.starts_with("0x")
            && s[2..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "canonical hash"
    );
    Ok(())
}
pub fn sha(bytes: &[u8]) -> String {
    hex_string(&Sha256::digest(bytes))
}
impl IndexRow {
    pub fn validate(&self) -> Result<()> {
        for h in [
            &self.l1_hash,
            &self.parent_hash,
            &self.l2_hash,
            &self.root,
            &self.system_digest,
        ] {
            hash(h)?;
        }
        ensure!(
            self.l1_number == self.l2_number && self.blob_ids.is_empty(),
            "devnet height/blob profile"
        );
        ensure!(
            matches!(
                self.inputs.as_str(),
                "l1-rpc" | "journal" | "archive-verified"
            ) && self.verdicts == "computed",
            "archive verify labels"
        );
        Ok(())
    }
}
pub struct ArchiveStore {
    profile: ArchiveProfile,
    root: PathBuf,
    guard: StateDirGuard,
    manifest_stamp: FileStamp,
    index: IndexLog,
    objects: BTreeMap<String, Objects>,
}
impl ArchiveStore {
    pub fn open(root: &Path, profile: ArchiveProfile, repair: bool) -> Result<Self> {
        profile.validate()?;
        let guard =
            state_dir::acquire(root).map_err(|e| crate::error::startup(18, e.to_string()))?;
        let root = root.canonicalize()?;
        crate::boot::cleanup_publications(&root, &guard)?;
        let manifest = root.join("archive.manifest.json");
        let index_path = root.join("index.ndjson");
        if manifest.try_exists()? != index_path.try_exists()? {
            return Err(crate::error::startup(19,"archive manifest/index pair incomplete; preserve and recover into a fresh directory"));
        }
        if !manifest.try_exists()? {
            publish_new(&index_path, b"")?;
            publish_new(&manifest, &serde_json::to_vec(&profile.identity)?)?;
        }
        let (bytes, manifest_stamp) = read_bound(&manifest, 65536)?;
        let saved: ArchiveIdentity = serde_json::from_slice(&bytes)?;
        if saved != profile.identity {
            return Err(crate::error::startup(18, "archive store identity mismatch"));
        }
        let index = IndexLog::open(&index_path, repair)?;
        let mut objects = BTreeMap::new();
        for class in CLASSES {
            objects.insert((*class).to_owned(), Objects::open(&root.join(class))?);
        }
        let out = Self {
            profile,
            root,
            guard,
            manifest_stamp,
            index,
            objects,
        };
        out.check()?;
        if let Some(first) = out.index.rows.get(&0) {
            ensure!(
                first.l1_hash == out.profile.identity.l1_anchor_hash,
                "S18: archive index anchor mismatch"
            );
        }
        Ok(out)
    }
    pub fn identity(&self) -> &ArchiveIdentity {
        &self.profile.identity
    }
    pub fn check(&self) -> Result<()> {
        (|| -> Result<()> {
            ensure!(self.guard.is_current(), "archive writer lock changed");
            ensure!(
                file_stamp(&self.root.join("archive.manifest.json"))?.as_ref()
                    == Some(&self.manifest_stamp),
                "archive manifest changed"
            );
            for objects in self.objects.values() {
                objects.check()?;
            }
            self.index.check()
        })()
        .map_err(|e| crate::error::integrity(0, "archive-store", format!("{e:#}")))
    }
    pub fn index(&self, from: u64, limit: usize) -> Result<Vec<IndexRow>> {
        self.check()?;
        ensure!(
            (1..=SEGMENT).contains(&limit),
            "index range exceeds SEGMENT"
        );
        Ok(self
            .index
            .rows
            .range(from..)
            .take(limit)
            .map(|(_, r)| r.clone())
            .collect())
    }
    pub fn info(&self) -> serde_json::Value {
        serde_json::json!({"schema":"boole.archive.v1","identity":self.profile.identity,"covered_from":self.index.rows.first_key_value().map(|(h,_)|h),"covered_through":self.index.rows.last_key_value().map(|(h,_)|h),"input_ranges":self.input_ranges(),"verdicts":"computed","segment":SEGMENT,"obj_max":OBJ_MAX,"pending":{"segment":"no locked zstd crate; bundle is not authority","l1/blob":"M4-L5 deferred","env_normalized":"no normalized producer in v1.3"}})
    }
    pub fn input_ranges(&self) -> Vec<serde_json::Value> {
        let mut ranges: Vec<serde_json::Value> = vec![];
        for (h, row) in &self.index.rows {
            if let Some(last) = ranges.last_mut().filter(|r| r["inputs"] == row.inputs) {
                last["through"] = serde_json::json!(h);
            } else {
                ranges.push(serde_json::json!({"from":h,"through":h,"inputs":row.inputs}));
            }
        }
        ranges
    }
    pub fn ingest(&mut self, row: &InputRow, finalized: u64, selection: &Selection) -> Result<()> {
        self.check()?;
        let verified = row.verify()?;
        let h = verified.block.number;
        ensure!(h <= finalized, "archive refuses unfinalized input");
        ensure!(
            selection.l2_number == h && selection.verdicts == "computed",
            "archive requires a locally verified selection"
        );
        if h == 0 {
            ensure!(
                row.block_hash == self.profile.identity.l1_anchor_hash,
                "archive genesis mismatch"
            );
        } else {
            ensure!(
                self.index
                    .rows
                    .get(&(h - 1))
                    .is_some_and(|r| r.l1_hash == verified.block.parent_hash),
                "archive height/parent gap"
            );
        }
        let raw = hex_bytes(&row.header_rlp, "header")?;
        let positive = header_needs_receipts(&raw)?;
        let system = serde_json::to_vec(&selection.system)?;
        let system_digest = sha(&system);
        let index = IndexRow {
            l1_number: h,
            l2_number: h,
            l1_hash: row.block_hash.clone(),
            parent_hash: verified.block.parent_hash.clone(),
            bloom_positive: positive,
            blob_ids: vec![],
            l2_hash: selection.l2_hash.clone(),
            root: selection.root.clone(),
            inputs: selection.inputs.clone(),
            verdicts: selection.verdicts.clone(),
            system_digest: system_digest.clone(),
        };
        index.validate()?;
        self.objects["header"].put(&row.block_hash, &raw, |b| verify_header(&row.block_hash, b))?;
        if positive {
            let receipts = row
                .receipts
                .as_ref()
                .context("positive header without receipts")?;
            let bytes = serde_json::to_vec(receipts)?;
            self.objects["receipts"].put(&row.block_hash, &bytes, |b| {
                let mut r = row.clone();
                r.transactions = None;
                r.receipts = Some(serde_json::from_slice(b)?);
                r.verify()?;
                Ok(())
            })?;
        }
        self.objects["system"].put(&system_digest, &system, |b| {
            ensure!(sha(b) == system_digest, "system digest");
            Ok(())
        })?;
        self.index.append(index)
    }
    pub fn put_export(
        &self,
        state: &boole_derivation::m2::State,
        key: boole_derivation::m2::Key,
        bytes: &[u8],
    ) -> Result<()> {
        self.check()?;
        let open = state
            .candidates
            .get(&key)
            .and_then(|c| c.live())
            .and_then(|c| c.open.as_ref())
            .context("frozen export")?;
        let address = hex_string(&open.export_digest);
        self.objects["exp"].put(&address, bytes, |b| {
            crate::derived::verify_export(state, key, &address, b)
        })
    }
    pub fn put_transport(
        &self,
        state: &boole_derivation::m2::State,
        key: boole_derivation::m2::Key,
        t: boole_derivation::m2::Hash,
        body: &[u8],
        bytes: &[u8],
    ) -> Result<()> {
        self.check()?;
        let address = hex_string(&key.1);
        let encoded = crate::derived::transport_envelope(
            state,
            key,
            self.profile.identity.network.l2_chain_id,
            t,
            body,
            bytes,
        )?;
        self.objects["tr"].put(&address, &encoded, |b| {
            crate::derived::read_transport_envelope(&address, b)?;
            Ok(())
        })
    }
    pub fn put_content(
        &self,
        class: &str,
        pin: boole_derivation::m2::Hash,
        bytes: &[u8],
    ) -> Result<()> {
        self.check()?;
        ensure!(
            matches!(class, "env" | "gate" | "pkg" | "vstmt"),
            "not a public content class"
        );
        let address = hex_string(&pin);
        self.objects[class].put(&address, bytes, |b| {
            crate::derived::verify_content(class, &address, b, pin)
        })
    }
    pub fn object(&self, class: &str, address: &str) -> Result<Option<Vec<u8>>> {
        self.check()?;
        hash(address)?;
        if class == "tr" {
            return self.objects["tr"]
                .read(address, |b| {
                    crate::derived::read_transport_envelope(address, b)?;
                    Ok(())
                })?
                .map(|b| crate::derived::read_transport_envelope(address, &b))
                .transpose();
        }
        if !matches!(class, "exp" | "env" | "gate" | "pkg" | "vstmt") {
            return Ok(None);
        }
        self.objects[class].read(address, |b| {
            ensure!(
                !b.is_empty()
                    && b.len() <= crate::derived::content_limit(class)?
                    && sha(b) == address,
                "public object address/cap mismatch"
            );
            Ok(())
        })
    }
    pub fn header(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.check()?;
        hash(key)?;
        self.objects["header"].read(key, |b| verify_header(key, b))
    }
    pub fn receipts(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.check()?;
        hash(key)?;
        let Some(raw) = self.header(key)? else {
            return Ok(None);
        };
        if !header_needs_receipts(&raw)? {
            return Ok(None);
        }
        self.objects["receipts"].read(key, |b| {
            let r = InputRow {
                schema_version: 1,
                block_hash: key.into(),
                header_rlp: hex_string(&raw),
                receipts: Some(serde_json::from_slice(b)?),
                transactions: None,
            };
            r.verify()?;
            Ok(())
        })
    }
    pub fn system(&self, h: u64) -> Result<Option<Vec<u8>>> {
        self.check()?;
        let Some(row) = self.index.rows.get(&h) else {
            return Ok(None);
        };
        self.objects["system"].read(&row.system_digest, |b| {
            ensure!(sha(b) == row.system_digest, "system digest");
            let _: Vec<String> = serde_json::from_slice(b)?;
            Ok(())
        })
    }
}
pub fn verify_header(key: &str, raw: &[u8]) -> Result<()> {
    hash(key)?;
    ensure!(raw.len() <= 4096, "header cap");
    let row = InputRow {
        schema_version: 1,
        block_hash: key.into(),
        header_rlp: hex_string(raw),
        receipts: None,
        transactions: None,
    };
    ensure!(row.block()?.hash == key, "header hash mismatch");
    Ok(())
}
