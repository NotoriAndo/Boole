//! Version-bound streaming evidence log using the copied R1 append transaction.
use crate::{
    durability,
    error::{availability, integrity},
    journal::{free_bytes, publish_new, regular, DISK_FLOOR},
    version::{file_stamp, stamp, FileStamp},
};
use anyhow::{ensure, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};
pub struct BoundLog {
    path: PathBuf,
    observed: FileStamp,
    digest: Sha256,
    records: BTreeMap<u64, Value>,
}
impl BoundLog {
    pub fn open(path: &Path, repair: bool) -> Result<Self> {
        if !path.try_exists()? {
            publish_new(path, b"")?;
        }
        Self::load(path, repair).map_err(|e| integrity(0, "evidence", format!("{e:#}")))
    }
    fn load(path: &Path, repair: bool) -> Result<Self> {
        let file = regular(path, repair)?;
        let before = stamp(&file.metadata()?)?;
        let mut reader = BufReader::new(file.try_clone()?);
        let mut digest = Sha256::new();
        let mut len = 0;
        let mut records = BTreeMap::new();
        loop {
            let mut raw = vec![];
            let n = Read::by_ref(&mut reader)
                .take(crate::input::ROW_MAX as u64 + 1)
                .read_until(b'\n', &mut raw)?;
            if n == 0 {
                break;
            }
            ensure!(n <= crate::input::ROW_MAX, "evidence row cap");
            if raw.last() != Some(&b'\n') {
                ensure!(
                    repair,
                    "torn evidence tail; read-only load preserved every byte"
                );
                break;
            }
            let value: Value = serde_json::from_slice(&raw)?;
            Self::record(&mut records, &value)?;
            digest.update(&raw);
            len += n as u64;
        }
        ensure!(
            stamp(&file.metadata()?)? == before && file_stamp(path)?.as_ref() == Some(&before),
            "evidence changed during replay"
        );
        if len < before.len {
            file.set_len(len)?;
            file.sync_all()?;
            durability::fsync_parent_dir(path)?;
            let checked = Self::load(path, false)?;
            ensure!(
                checked.digest.clone().finalize() == digest.clone().finalize(),
                "evidence repair changed confirmed prefix"
            );
            return Ok(checked);
        }
        Ok(Self {
            path: path.into(),
            observed: before,
            digest,
            records,
        })
    }
    fn record(records: &mut BTreeMap<u64, Value>, v: &Value) -> Result<()> {
        if matches!(v["outcome"].as_str(), Some("DERIVED" | "RECOVERED")) {
            let h = v["height"]
                .as_u64()
                .ok_or_else(|| integrity(0, "evidence", "missing height"))?;
            let summary = serde_json::json!({"height":h,"block_hash":v["block_hash"],"l1_hash":v["l1"]["hash"],"m2_root":v["m2_root"],"attribute_digest":v["attribute_digest"]});
            if let Some(old) = records.get(&h) {
                ensure!(old == &summary, "conflicting durable evidence at {h}");
            } else {
                records.insert(h, summary);
            }
        }
        Ok(())
    }
    pub fn check(&self) -> Result<()> {
        if file_stamp(&self.path)?.as_ref() != Some(&self.observed) {
            return Err(integrity(
                0,
                "evidence",
                "file changed since verified replay",
            ));
        }
        if durability::any_indeterminate_write([&self.path]) {
            return Err(integrity(
                0,
                "durability-fence",
                "indeterminate append; restart required",
            ));
        }
        Ok(())
    }
    pub fn record_at(&self, h: u64) -> Option<&Value> {
        self.records.get(&h)
    }
    pub fn append(&mut self, value: &Value) -> Result<()> {
        self.check()?;
        let h = value["height"].as_u64().unwrap_or(0);
        if free_bytes(self.path.parent().unwrap())? < DISK_FLOOR {
            return Err(availability(h, "disk", "S23: below 15 GiB disk floor"));
        }
        // Validate duplicate semantic rows before writing any byte.
        let mut records = self.records.clone();
        Self::record(&mut records, value)?;
        let raw = serde_json::to_string(value)?;
        ensure!(raw.len() < crate::input::ROW_MAX, "evidence row cap");
        let before = self.observed.clone();
        let result =
            durability::append_ndjson_line_versioned(&self.path, &raw, false, |m, created| {
                ensure!(
                    !created && stamp(m)? == before,
                    "evidence descriptor changed"
                );
                self.check()
            });
        let written = match result {
            Ok(w) => w,
            Err(e) => {
                if durability::any_indeterminate_write([&self.path]) {
                    return Err(integrity(h, "durability-fence", format!("{e:#}")));
                }
                let checked = Self::load(&self.path, false)?;
                ensure!(
                    before.same_file(&checked.observed)
                        && before.len == checked.observed.len
                        && self.digest.clone().finalize() == checked.digest.clone().finalize(),
                    "failed evidence append changed confirmed prefix"
                );
                self.observed = checked.observed;
                return Err(availability(
                    h,
                    "disk",
                    format!("append failed; confirmed prefix preserved: {e:#}"),
                ));
            }
        };
        let observed = stamp(written.metadata())?;
        ensure!(
            file_stamp(&self.path)?.as_ref() == Some(&observed)
                && observed.len == before.len + raw.len() as u64 + 1,
            "evidence publication changed"
        );
        self.digest.update(raw.as_bytes());
        self.digest.update(b"\n");
        self.observed = observed;
        self.records = records;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn torn_evidence_and_replacement_preserve_every_byte() -> Result<()> {
        let d = durability::PrivateTempDir::new("m4-evidence")?;
        let p = d.path().join("evidence.jsonl");
        let mut log = BoundLog::open(&p, false)?;
        log.append(&serde_json::json!({"height":1,"outcome":"WAITING_FOR_FINALITY"}))?;
        drop(log);
        let good = std::fs::read(&p)?;
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(&p)?;
        f.write_all(b"{torn")?;
        let torn = std::fs::read(&p)?;
        assert!(BoundLog::open(&p, false).is_err());
        assert_eq!(std::fs::read(&p)?, torn);
        let mut log = BoundLog::open(&p, true)?;
        assert_eq!(std::fs::read(&p)?, good);
        std::fs::write(&p, b"foreign")?;
        assert!(log.append(&serde_json::json!({"height":2})).is_err());
        assert_eq!(std::fs::read(&p)?, b"foreign");
        Ok(())
    }
}
