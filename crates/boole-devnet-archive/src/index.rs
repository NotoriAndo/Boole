//! Append-only, version-bound archive index. It never authenticates client folds.
use crate::{
    archive::IndexRow,
    durability,
    error::{availability, integrity},
    journal::{free_bytes, publish_new, regular, DISK_FLOOR},
    version::{file_stamp, stamp, FileStamp},
};
use anyhow::{ensure, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};
pub(crate) struct IndexLog {
    path: PathBuf,
    observed: FileStamp,
    digest: Sha256,
    pub rows: BTreeMap<u64, IndexRow>,
}
impl IndexLog {
    pub fn open(path: &Path, repair: bool) -> Result<Self> {
        if !path.try_exists()? {
            publish_new(path, b"")?;
        }
        Self::load(path, repair).map_err(|e| integrity(0, "archive-index", format!("{e:#}")))
    }
    fn load(path: &Path, repair: bool) -> Result<Self> {
        let file = regular(path, repair)?;
        let before = stamp(&file.metadata()?)?;
        let mut reader = BufReader::new(file.try_clone()?);
        let mut digest = Sha256::new();
        let mut len = 0;
        let mut rows = BTreeMap::new();
        loop {
            let mut raw = vec![];
            let n = Read::by_ref(&mut reader)
                .take(65537)
                .read_until(b'\n', &mut raw)?;
            if n == 0 {
                break;
            }
            ensure!(n <= 65536, "index row cap");
            if raw.last() != Some(&b'\n') {
                ensure!(repair, "torn index tail; preserved");
                break;
            }
            let row: IndexRow = serde_json::from_slice(&raw)?;
            row.validate()?;
            ensure!(
                row.l1_number == rows.len() as u64,
                "noncontiguous archive index"
            );
            if let Some(old) = rows.last_key_value().map(|(_, r): (&u64, &IndexRow)| r) {
                ensure!(row.parent_hash == old.l1_hash, "index parent mismatch");
            }
            rows.insert(row.l1_number, row);
            digest.update(&raw);
            len += n as u64;
        }
        ensure!(
            stamp(&file.metadata()?)? == before && file_stamp(path)?.as_ref() == Some(&before),
            "index changed during replay"
        );
        if len < before.len {
            file.set_len(len)?;
            file.sync_all()?;
            durability::fsync_parent_dir(path)?;
            let checked = Self::load(path, false)?;
            ensure!(
                checked.digest.clone().finalize() == digest.finalize(),
                "index repair changed confirmed prefix"
            );
            return Ok(checked);
        }
        Ok(Self {
            path: path.into(),
            observed: before,
            digest,
            rows,
        })
    }
    pub fn check(&self) -> Result<()> {
        ensure!(
            file_stamp(&self.path)?.as_ref() == Some(&self.observed),
            "index changed since replay"
        );
        ensure!(
            !durability::any_indeterminate_write([&self.path]),
            "index fenced until restart"
        );
        Ok(())
    }
    pub fn append(&mut self, row: IndexRow) -> Result<()> {
        self.check()?;
        row.validate()?;
        if let Some(old) = self.rows.get(&row.l1_number) {
            let mut replay = row.clone();
            replay.inputs = old.inputs.clone();
            ensure!(old == &replay, "conflicting index row");
            return Ok(());
        }
        ensure!(row.l1_number == self.rows.len() as u64, "index gap");
        if let Some(old) = self.rows.last_key_value().map(|(_, r)| r) {
            ensure!(old.l1_hash == row.parent_hash, "index parent mismatch");
        }
        if free_bytes(self.path.parent().unwrap())? < DISK_FLOOR {
            return Err(availability(row.l1_number, "disk", "S23: below 15 GiB"));
        }
        let raw = serde_json::to_string(&row)?;
        ensure!(raw.len() < 65536, "index cap");
        let before = self.observed.clone();
        let result =
            durability::append_ndjson_line_versioned(&self.path, &raw, false, |m, created| {
                ensure!(!created && stamp(m)? == before, "index descriptor changed");
                self.check()
            });
        match result {
            Ok(w) => {
                self.observed = stamp(w.metadata())?;
                ensure!(
                    file_stamp(&self.path)?.as_ref() == Some(&self.observed)
                        && self.observed.len == before.len + raw.len() as u64 + 1,
                    "index publication changed"
                );
                self.digest.update(raw.as_bytes());
                self.digest.update(b"\n");
                self.rows.insert(row.l1_number, row);
                Ok(())
            }
            Err(e) => {
                if durability::any_indeterminate_write([&self.path]) {
                    return Err(integrity(
                        row.l1_number,
                        "archive-index-fence",
                        format!("{e:#}"),
                    ));
                }
                let checked = Self::load(&self.path, false)?;
                ensure!(
                    before.same_file(&checked.observed)
                        && before.len == checked.observed.len
                        && checked.digest.clone().finalize() == self.digest.clone().finalize(),
                    "failed index append changed confirmed prefix"
                );
                self.observed = checked.observed;
                Err(availability(
                    row.l1_number,
                    "archive-index",
                    format!("{e:#}"),
                ))
            }
        }
    }
}
