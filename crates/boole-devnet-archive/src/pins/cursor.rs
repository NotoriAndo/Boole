//! Writer-owned incremental cursor. Read-only auditors always use the full path.
use super::*;
#[derive(Clone, PartialEq, Eq)]
pub(super) struct DirectoryStamp {
    modified: std::time::SystemTime,
    #[cfg(unix)]
    id: (u64, u64, i64, i64),
    len: u64,
}
pub(super) fn directory_stamp(path: &Path) -> Result<DirectoryStamp> {
    let m = std::fs::symlink_metadata(path)?;
    ensure!(
        m.is_dir() && !m.file_type().is_symlink(),
        "pin records directory type"
    );
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Ok(DirectoryStamp {
        modified: m.modified()?,
        len: m.len(),
        #[cfg(unix)]
        id: (m.dev(), m.ino(), m.ctime(), m.ctime_nsec()),
    })
}
#[derive(Clone)]
pub(super) struct VerifiedRow {
    id: Option<Hash>,
    pub digest: Hash,
    pub block: boole_derivation::L1Block,
}
impl ReadOnlyPins {
    pub(super) fn verified_row(&self, source: &dyn PinInputs, h: u64) -> Result<VerifiedRow> {
        let id = source.pin_row_id(h)?;
        if self.writer && id.is_some() {
            if let Some(row) = self
                .verified_rows
                .lock()
                .map_err(|_| anyhow::anyhow!("pin row cache poisoned"))?
                .get(&h)
                .filter(|row| row.id == id)
                .cloned()
            {
                return Ok(row);
            }
        }
        let raw = source.pin_row(h)?;
        let row = VerifiedRow {
            id,
            digest: row_hash(&raw)?,
            block: raw.verify()?.block,
        };
        if self.writer && id.is_some() {
            self.verified_rows
                .lock()
                .map_err(|_| anyhow::anyhow!("pin row cache poisoned"))?
                .insert(h, row.clone());
        }
        Ok(row)
    }
    pub(super) fn verify_all(&self, source: &dyn PinInputs) -> Result<()> {
        source.pin_check()?;
        for (&h, digest) in &self.index.inputs {
            ensure!(
                self.verified_row(source, h)?.digest == *digest,
                "retained input differs from its pin"
            );
        }
        for binding in self.index.bindings.values() {
            binding.verify(
                source.pin_network(),
                &self.verified_row(source, binding.header.number)?.block,
            )?;
        }
        source.pin_check()
    }
}
// Validate only touched keys in a sparse scratch index. No mutation of the live
// index (and no full-index clone) precedes durable publication.
pub(super) fn validate_event(index: &Index, event: &Event, sequence: u64) -> Result<()> {
    let mut delta = Index {
        bootstrap: index.bootstrap,
        bootstrapped: index.bootstrapped,
        ..Index::default()
    };
    match event {
        Event::Artifacts { values } => {
            for a in values {
                if let Some(old) = index.artifacts.get(&a.role) {
                    delta.artifacts.insert(a.role.clone(), old.clone());
                }
            }
        }
        Event::Prepared {
            binding,
            candidates,
            inputs,
        } => {
            let key = (binding.header.number, binding.header.hash.clone());
            if let Some(old) = index.bindings.get(&key) {
                delta.bindings.insert(key, old.clone());
            }
            for pin in candidates {
                if let Some(old) = index.candidates.get(&pin.key) {
                    delta.candidates.insert(pin.key, old.clone());
                }
            }
            for (h, _) in inputs {
                if let Some(old) = index.inputs.get(h) {
                    delta.inputs.insert(*h, *old);
                }
            }
        }
        Event::Selected { height, hash, .. } => {
            let key = (*height, hash.clone());
            if let Some(b) = index.bindings.get(&key) {
                delta.bindings.insert(key, b.clone());
            }
            for h in [Some(*height), height.checked_sub(1)].into_iter().flatten() {
                if let Some(old) = index.selected.get(&h) {
                    delta.selected.insert(h, old.clone());
                }
            }
        }
        Event::Bootstrap { .. } | Event::Bootstrapped => {}
    }
    apply(&mut delta, event, sequence)
}
impl PinStore {
    pub(super) fn refresh_tail(&mut self, source: &InputJournal) -> Result<()> {
        source.check()?;
        if self.view.manifest.network != *source.network()
            || self.view.manifest.l1_anchor_hash != source.anchor_hash()
            || self.view.root.parent() != Some(source.root())
        {
            return Err(startup(
                20,
                "incremental pin cursor source identity/store differs",
            ));
        }
        let root = self.view.root.join("records");
        let before = directory_stamp(&root)?;
        if self.tail_dirty || before != self.view.records_stamp {
            let all = names(&root)?;
            if all.len() < self.view.files.len() {
                return Err(availability(
                    self.view.height(),
                    "pin-record",
                    "S20: pin prefix removed",
                ));
            }
            if !all
                .iter()
                .take(self.view.files.len())
                .eq(self.view.files.iter().map(|f| &f.0))
            {
                return Err(availability(
                    self.view.height(),
                    "pin-record",
                    "S20: verified pin prefix names changed",
                ));
            }
            for name in all.into_iter().skip(self.view.files.len()) {
                let sequence = self.view.files.len() as u64;
                let path = root.join(&name);
                let (raw, stamp) = pin_read(&path, RECORD_MAX)?;
                #[cfg(test)]
                PIN_COST.with(|c| {
                    let (r, w, i) = c.get();
                    c.set((r + 1, w, i));
                });
                let digest = frame_hash(&self.view.manifest, &raw)?;
                ensure!(
                    name == format!("{sequence:020}-{}.json", hex::encode(digest)),
                    "pin suffix name/digest"
                );
                let frame: Frame = serde_json::from_slice(&raw)?;
                ensure!(
                    frame.version == 1
                        && frame.sequence == sequence
                        && frame.previous == self.view.last,
                    "pin suffix chain/version"
                );
                validate_event(&self.view.index, &frame.event, sequence)?;
                if let Event::Prepared {
                    binding, inputs, ..
                } = &frame.event
                {
                    binding.verify(
                        source.network(),
                        &self.view.verified_row(source, binding.header.number)?.block,
                    )?;
                    for (h, digest) in inputs {
                        ensure!(
                            self.view.verified_row(source, *h)?.digest == *digest,
                            "new pin input digest differs"
                        );
                    }
                }
                // A retry after a failed publication confirms durability before
                // its complete suffix is adopted into the writer's cursor.
                std::fs::File::open(&path)?.sync_all()?;
                crate::durability::fsync_parent_dir(&path)?;
                apply(&mut self.view.index, &frame.event, sequence)?;
                if let Event::Artifacts { values } = frame.event {
                    for a in values {
                        self.view
                            .artifact_stamps
                            .insert(a.digest, pin_stamp(&self.view.artifact_path(a.digest))?);
                    }
                }
                self.view.files.push((name, stamp));
                self.view.last = digest;
            }
            ensure!(
                directory_stamp(&root)? == before,
                "pin suffix changed while reading"
            );
            self.view.records_stamp = before;
            self.tail_dirty = false;
        }
        // Exact restored versions are hashed afresh; immutable own bytes reuse
        // their already verified version. No old record body is read here.
        for a in self.view.index.artifacts.values() {
            let path = self.view.artifact_path(a.digest);
            let now = pin_stamp(&path)?;
            if self.view.artifact_stamps.get(&a.digest) != Some(&now) {
                self.view.artifact_stamps.insert(a.digest, now);
            }
        }
        for a in self.view.index.artifacts.values() {
            self.view.verify_artifact(a)?;
        }
        self.view.check()
    }
}
