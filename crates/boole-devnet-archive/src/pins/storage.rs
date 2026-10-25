//! Append-only new-file pin journal: bytes -> record -> selected binding.
//! Extra complete records after a crash retain more bytes; nothing is erased.
use super::{
    ArtifactRole, ArtifactSource, CandidatePin, HeaderBinding, IntervalUnion, RetentionPolicy,
};
use crate::{
    error::{availability, integrity, startup},
    journal::{publish_new, read_bound, InputJournal, NetworkIdentity, ReadOnlyJournal},
    state_dir::{self, StateDirGuard},
    version::{file_stamp, FileStamp},
};
use anyhow::{ensure, Context, Result};
use boole_derivation::{
    attributes::hex_string,
    m2::{Hash, Key},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
pub const DIR: &str = "pins-v1";
const MANIFEST: &str = "manifest.json";
const RECORD_MAX: usize = 64 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u8,
    network: NetworkIdentity,
    l1_anchor_hash: String,
    retention: bool,
    bootstrap_allowed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub role: ArtifactRole,
    pub digest: Hash,
    pub bytes: u64,
    pub executable: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Event {
    Artifacts {
        values: Vec<Artifact>,
    },
    Bootstrap {
        through: u64,
    },
    Bootstrapped,
    Prepared {
        binding: Box<HeaderBinding>,
        candidates: Vec<CandidatePin>,
        inputs: Vec<(u64, Hash)>,
    },
    Selected {
        height: u64,
        hash: String,
        root: Hash,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    version: u8,
    sequence: u64,
    previous: Hash,
    event: Event,
}
#[derive(Clone, Default)]
struct Index {
    candidates: BTreeMap<Key, CandidatePin>,
    intervals: IntervalUnion,
    inputs: BTreeMap<u64, Hash>,
    bindings: BTreeMap<(u64, String), HeaderBinding>,
    selected: BTreeMap<u64, String>,
    selected_at: BTreeMap<u64, u64>,
    artifacts: BTreeMap<ArtifactRole, Artifact>,
    bootstrap: Option<u64>,
    bootstrapped: bool,
}
fn hash(bytes: &[u8]) -> Hash {
    Sha256::digest(bytes).into()
}
fn row_hash(row: &crate::input::InputRow) -> Result<Hash> {
    Ok(hash(
        &[
            b"BOOLE-M4/INPUT-PIN/v1\0".as_slice(),
            &serde_json::to_vec(row)?,
        ]
        .concat(),
    ))
}
fn frame_hash(manifest: &Manifest, raw: &[u8]) -> Result<Hash> {
    Ok(hash(
        &[
            b"BOOLE-M4/PIN-JOURNAL/v1\0".as_slice(),
            &serde_json::to_vec(manifest)?,
            raw,
        ]
        .concat(),
    ))
}
fn typed(error: anyhow::Error, height: u64) -> anyhow::Error {
    if error.is::<crate::error::Error>() {
        return error;
    }
    if error.is::<std::io::Error>() {
        availability(
            height,
            "pin-io",
            format!("S20: pin input unavailable: {error:#}"),
        )
    } else {
        integrity(
            height,
            "pin-journal",
            format!("pin journal validation: {error:#}"),
        )
    }
}
fn names(root: &Path) -> Result<Vec<String>> {
    let mut result = vec![];
    for e in std::fs::read_dir(root)? {
        let e = e?;
        let name = e
            .file_name()
            .into_string()
            .map_err(|_| anyhow::anyhow!("pin journal filename"))?;
        if name == state_dir::STATE_LOCK_FILE
            || name == state_dir::STATE_MANIFEST_FILE
            || name == "corrupt"
            || name.starts_with(".devnet-new-")
        {
            continue;
        }
        ensure!(
            e.file_type()?.is_file() && !e.file_type()?.is_symlink() && name.ends_with(".json"),
            "pin journal entry type"
        );
        result.push(name);
    }
    result.sort();
    Ok(result)
}
fn apply(index: &mut Index, event: &Event, sequence: u64) -> Result<()> {
    match event {
        Event::Bootstrap { through } => {
            ensure!(!index.bootstrapped, "bootstrap is already sealed");
            if let Some(old) = index.bootstrap {
                ensure!(old == *through, "bootstrap ceiling changed");
            }
            index.bootstrap = Some(*through);
        }
        Event::Bootstrapped => {
            index.bootstrapped = true;
        }
        Event::Artifacts { values } => {
            for a in values {
                if let Some(old) = index.artifacts.get(&a.role) {
                    ensure!(
                        old.digest == a.digest
                            && old.bytes == a.bytes
                            && old.executable == a.executable,
                        "pin role rebound"
                    );
                }
                index.artifacts.insert(a.role.clone(), a.clone());
            }
        }
        Event::Prepared {
            binding,
            candidates,
            inputs,
        } => {
            let h = binding.header.number;
            let key = (h, binding.header.hash.clone());
            if let Some(old) = index.bindings.get(&key) {
                ensure!(
                    serde_json::to_vec(old)? == serde_json::to_vec(binding)?,
                    "pin header rebound"
                );
            }
            index.bindings.insert(key, binding.as_ref().clone());
            for &(h, digest) in inputs {
                if let Some(old) = index.inputs.get(&h) {
                    ensure!(*old == digest, "pin input rebound");
                }
                index.inputs.insert(h, digest);
            }
            for pin in candidates {
                pin.validate(h)?;
                let mut pin = pin.clone();
                if let Some(old) = index.candidates.get(&pin.key) {
                    ensure!(
                        (
                            old.commit_height,
                            old.open_height,
                            old.completion_height,
                            old.reveal_height
                        ) == (
                            pin.commit_height,
                            pin.open_height,
                            pin.completion_height,
                            pin.reveal_height
                        ),
                        "pin frozen positions rebound"
                    );
                    pin.reasons.extend(old.reasons.iter().cloned());
                    pin.gates.extend(old.gates.iter().copied());
                    pin.components.extend(old.components.iter().copied());
                }
                index.intervals.extend(&pin.intervals());
                index.candidates.insert(pin.key, pin);
            }
        }
        Event::Selected { height, hash, root } => {
            let binding = index
                .bindings
                .get(&(*height, hash.clone()))
                .context("selected pin header missing")?;
            ensure!(binding.root == *root, "selected pin root");
            if *height > 0 {
                let previous = index.selected.get(&(height - 1)).ok_or_else(|| {
                    availability(
                        *height,
                        "pin-binding",
                        "S20: previous L2 binding is not selected",
                    )
                })?;
                ensure!(
                    binding.header.parent_hash.as_ref() == Some(previous),
                    "pin selected parent chain"
                );
            }
            if let Some(old) = index.selected.get(height) {
                ensure!(old == hash, "selected pin rollback/replacement");
            }
            index.selected.insert(*height, hash.clone());
            index.selected_at.entry(*height).or_insert(sequence);
        }
    }
    Ok(())
}
pub trait PinInputs {
    fn pin_network(&self) -> &NetworkIdentity;
    fn pin_anchor(&self) -> &str;
    fn pin_tip(&self) -> u64;
    fn pin_row(&self, h: u64) -> Result<crate::input::InputRow>;
    fn pin_check(&self) -> Result<()>;
}
impl PinInputs for InputJournal {
    fn pin_network(&self) -> &NetworkIdentity {
        self.network()
    }
    fn pin_anchor(&self) -> &str {
        self.anchor_hash()
    }
    fn pin_tip(&self) -> u64 {
        self.tip()
    }
    fn pin_row(&self, h: u64) -> Result<crate::input::InputRow> {
        self.row(h)
    }
    fn pin_check(&self) -> Result<()> {
        self.check()
    }
}
impl PinInputs for ReadOnlyJournal {
    fn pin_network(&self) -> &NetworkIdentity {
        self.network()
    }
    fn pin_anchor(&self) -> &str {
        self.anchor_hash()
    }
    fn pin_tip(&self) -> u64 {
        self.tip()
    }
    fn pin_row(&self, h: u64) -> Result<crate::input::InputRow> {
        self.row(h)
    }
    fn pin_check(&self) -> Result<()> {
        self.check()
    }
}
/// Source-preserving; no writer lock, cleanup, repair or quarantine is performed.
pub struct ReadOnlyPins {
    root: PathBuf,
    manifest: Manifest,
    manifest_stamp: FileStamp,
    marker_stamp: FileStamp,
    files: Vec<(String, FileStamp)>,
    last: Hash,
    index: Index,
    artifact_stamps: BTreeMap<Hash, Option<FileStamp>>,
}
impl ReadOnlyPins {
    pub fn open(root: &Path, network: &NetworkIdentity, anchor: &str) -> Result<Option<Self>> {
        Self::open_inner(root, network, anchor).map_err(|e| typed(e, 0))
    }
    fn open_inner(root: &Path, network: &NetworkIdentity, anchor: &str) -> Result<Option<Self>> {
        let root = root.join(DIR);
        if !root.try_exists()? {
            return Ok(None);
        }
        ensure!(
            std::fs::symlink_metadata(&root)?.is_dir()
                && !std::fs::symlink_metadata(&root)?.file_type().is_symlink(),
            "pin directory type"
        );
        let (raw, manifest_stamp) = read_bound(&root.join(MANIFEST), 65536)?;
        let manifest: Manifest = serde_json::from_slice(&raw)?;
        if manifest.schema_version != 1
            || manifest.network != *network
            || manifest.l1_anchor_hash != anchor
        {
            return Err(startup(20, "pin manifest identity/version differs"));
        }
        RetentionPolicy {
            enabled: manifest.retention,
        }
        .verify_mode()?;
        let (marker, marker_stamp) =
            read_bound(&root.join("objects/retention.required.json"), 65536)?;
        ensure!(
            serde_json::from_slice::<Manifest>(&marker)? == manifest,
            "retention marker identity"
        );
        let mut index = Index::default();
        let mut files = vec![];
        let mut last = [0; 32];
        for (seq, name) in names(&root.join("records"))?.into_iter().enumerate() {
            let number = name
                .get(..20)
                .and_then(|n| n.parse::<u64>().ok())
                .context("pin record sequence name")?;
            if number > seq as u64 {
                return Err(availability(
                    0,
                    "pin-record",
                    "S20: missing pin record; restore the complete prefix",
                ));
            }
            let (raw, stamp) = read_bound(&root.join("records").join(&name), RECORD_MAX)?;
            let digest = frame_hash(&manifest, &raw)?;
            ensure!(
                name == format!("{seq:020}-{}.json", hex::encode(digest)),
                "pin record name/sequence/digest"
            );
            let frame: Frame = serde_json::from_slice(&raw)?;
            ensure!(
                frame.version == 1 && frame.sequence == seq as u64 && frame.previous == last,
                "pin chain/version"
            );
            apply(&mut index, &frame.event, frame.sequence)?;
            last = digest;
            files.push((name, stamp));
        }
        let artifact_stamps = index
            .artifacts
            .values()
            .map(|a| {
                Ok((
                    a.digest,
                    file_stamp(&root.join("objects").join(hex_string(&a.digest)))?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let view = Self {
            root,
            manifest,
            manifest_stamp,
            marker_stamp,
            files,
            last,
            index,
            artifact_stamps,
        };
        view.check()?;
        Ok(Some(view))
    }
    pub fn check(&self) -> Result<()> {
        self.check_inner().map_err(|e| typed(e, self.height()))
    }
    fn check_inner(&self) -> Result<()> {
        if file_stamp(&self.root.join("objects/retention.required.json"))?.as_ref()
            != Some(&self.marker_stamp)
            || file_stamp(&self.root.join(MANIFEST))?.as_ref() != Some(&self.manifest_stamp)
            || names(&self.root.join("records"))?
                != self.files.iter().map(|v| v.0.clone()).collect::<Vec<_>>()
        {
            return Err(availability(
                self.height(),
                "pin-version",
                "pin source changed during read",
            ));
        }
        for (name, stamp) in &self.files {
            if file_stamp(&self.root.join("records").join(name))?.as_ref() != Some(stamp) {
                return Err(availability(
                    self.height(),
                    "pin-version",
                    "pin record version changed",
                ));
            }
        }
        for (digest, before) in &self.artifact_stamps {
            if file_stamp(&self.artifact_path(*digest))? != *before {
                return Err(availability(
                    self.height(),
                    "pin-version",
                    "pinned artifact changed during read",
                ));
            }
        }
        Ok(())
    }
    pub fn has_selected(&self, height: u64) -> bool {
        self.index.selected.contains_key(&height)
    }
    pub fn height(&self) -> u64 {
        self.index.selected.last_key_value().map_or(0, |(h, _)| *h)
    }
    pub fn binding(&self, h: u64) -> Result<&HeaderBinding> {
        let hash =
            self.index.selected.get(&h).ok_or_else(|| {
                availability(h, "pin-binding", "S20: selected L2 binding missing")
            })?;
        self.index
            .bindings
            .get(&(h, hash.clone()))
            .ok_or_else(|| availability(h, "pin-binding", "S20: L2 header bytes missing"))
    }
    pub fn prepared(&self, h: u64, hash: &str) -> Option<&HeaderBinding> {
        self.index.bindings.get(&(h, hash.to_owned()))
    }
    pub fn artifact_path(&self, digest: Hash) -> PathBuf {
        self.root.join("objects").join(hex_string(&digest))
    }
    pub fn artifact(&self, role: &ArtifactRole) -> Option<&Artifact> {
        self.index.artifacts.get(role)
    }
    pub fn intervals(&self) -> &IntervalUnion {
        &self.index.intervals
    }
    fn verify_artifact(&self, a: &Artifact) -> Result<()> {
        let path = self.artifact_path(a.digest);
        if !path.try_exists()? {
            return Err(availability(
                self.height(),
                "pin-artifact",
                "S20: retained artifact missing; restore verified bytes",
            ));
        }
        let (raw, observed) = read_bound(&path, 256 * 1024 * 1024).map_err(|e| {
            integrity(
                self.height(),
                "pin-artifact",
                format!("retained artifact: {e:#}"),
            )
        })?;
        if self.artifact_stamps.get(&a.digest).and_then(Option::as_ref) != Some(&observed) {
            return Err(availability(
                self.height(),
                "pin-version",
                "artifact version changed before verification",
            ));
        }
        if raw.len() as u64 != a.bytes || hash(&raw) != a.digest {
            return Err(integrity(
                self.height(),
                "pin-artifact",
                "retained artifact bytes differ",
            ));
        }
        Ok(())
    }
    pub fn verify(
        &self,
        source: &dyn PinInputs,
        required: &[CandidatePin],
        through: u64,
    ) -> Result<()> {
        self.verify_required(source, required, through, None)
    }
    fn verify_required(
        &self,
        source: &dyn PinInputs,
        required: &[CandidatePin],
        through: u64,
        prospective: Option<&HeaderBinding>,
    ) -> Result<()> {
        self.check()?;
        source.pin_check()?;
        ensure!(
            self.manifest.network == *source.pin_network()
                && self.manifest.l1_anchor_hash == source.pin_anchor(),
            "pin source identity"
        );
        for a in self.index.artifacts.values() {
            self.verify_artifact(a)?;
        }
        let mut ranges = IntervalUnion::default();
        for req in required {
            req.validate(through)?;
            if !self
                .index
                .candidates
                .get(&req.key)
                .is_some_and(|p| p.covers(req))
            {
                return Err(availability(
                    through,
                    "pin-coverage",
                    "S20: durable candidate pin missing",
                ));
            }
            self.verify_releases(&req.gates, &req.components)?;
            ranges.extend(&req.intervals());
        }
        for (&lo, &hi) in &ranges.0 {
            if hi > source.pin_tip() {
                return Err(availability(
                    hi,
                    "pin-input",
                    "S20: retained journal range missing",
                ));
            }
            for h in lo..=hi {
                let expected = self.index.inputs.get(&h).ok_or_else(|| {
                    availability(h, "pin-input", "S20: interior input pin missing")
                })?;
                let row = source.pin_row(h)?;
                if row_hash(&row)? != *expected {
                    return Err(integrity(
                        h,
                        "pin-input",
                        "retained row differs from durable pin",
                    ));
                }
                let binding = if let Some(b) = prospective.filter(|b| b.header.number == h) {
                    b
                } else {
                    self.binding(h)?
                };
                binding
                    .verify(source.pin_network(), &row.verify()?.block)
                    .map_err(|e| integrity(h, "pin-binding", format!("{e:#}")))?;
            }
        }
        if prospective.is_none() && self.height() < through {
            return Err(availability(
                through,
                "pin-height",
                "S20: pin verification height missing",
            ));
        }
        source.pin_check()?;
        self.check()
    }
    pub fn verify_releases(
        &self,
        gates: &std::collections::BTreeSet<(u32, Hash)>,
        components: &std::collections::BTreeSet<Hash>,
    ) -> Result<()> {
        for (epoch, gate) in gates {
            let desc = self
                .index
                .artifacts
                .get(&ArtifactRole::StubDescriptor {
                    epoch: *epoch,
                    gate: *gate,
                })
                .ok_or_else(|| {
                    availability(
                        self.height(),
                        "pin-gate",
                        "S20: old gate descriptor pin missing",
                    )
                })?;
            self.verify_artifact(desc)?;
            let (raw, _) = read_bound(&self.artifact_path(desc.digest), 65536)?;
            ensure!(
                hash(&[b"BOOLE-M3/GATE-DESCRIPTOR/v1".as_slice(), &raw].concat()) == *gate,
                "stub gate descriptor domain binding"
            );
            let value: serde_json::Value = serde_json::from_slice(&raw)?;
            ensure!(
                value["adapter"] == "boole-devnet-stub-v1",
                "real gate requires transitive inventory"
            );
            let mut found = false;
            for (role, a) in &self.index.artifacts {
                if let ArtifactRole::StubBinary {
                    epoch: e,
                    gate: g,
                    profile,
                } = role
                {
                    if e == epoch && g == gate {
                        let expected: Hash =
                            serde_json::from_value(value["binaries"][profile].clone())?;
                        ensure!(
                            expected == a.digest && a.executable,
                            "stub binary descriptor binding"
                        );
                        self.verify_artifact(a)?;
                        found = true;
                    }
                }
            }
            if !found {
                return Err(availability(
                    self.height(),
                    "pin-gate",
                    "S20: old gate binary pin missing",
                ));
            }
        }
        for component in components {
            let a = self
                .index
                .artifacts
                .get(&ArtifactRole::Environment {
                    component: *component,
                })
                .ok_or_else(|| {
                    availability(
                        self.height(),
                        "pin-environment",
                        "S20: environment artifact pin missing",
                    )
                })?;
            ensure!(a.digest == *component, "stub environment content binding");
            self.verify_artifact(a)?;
        }
        self.check()
    }
    pub fn chain(
        &self,
        head: &boole_derivation::ParentHeader,
    ) -> Result<BTreeMap<u64, HeaderBinding>> {
        self.check()?;
        let mut out = BTreeMap::new();
        let mut hash = head.hash.clone();
        for h in (0..=head.number).rev() {
            let b = self.prepared(h, &hash).ok_or_else(|| {
                availability(
                    h,
                    "pin-binding",
                    "S20: own L2 binding missing; no historical fallback",
                )
            })?;
            if h == head.number {
                ensure!(
                    serde_json::to_vec(&b.header)? == serde_json::to_vec(head)?,
                    "pinned head differs from Engine head"
                );
            }
            if h > 0 {
                hash = b.header.parent_hash.clone().context("pinned parent")?;
            }
            out.insert(h, b.clone());
        }
        self.check()?;
        Ok(out)
    }
    pub fn anchor(&self, h: u64) -> Result<PinAnchor> {
        let b = self.binding(h)?;
        Ok(PinAnchor {
            version: 1,
            record: self.last,
            height: h,
            l2_hash: b.header.hash.clone(),
            root: b.root,
        })
    }
    pub fn verify_anchor(&self, a: &PinAnchor) -> Result<()> {
        if a.version != 1 || !self.contains_record(a.record) {
            return Err(availability(
                a.height,
                "pin-anchor",
                "S20: checkpoint pin-journal prefix missing",
            ));
        }
        let seq = self
            .files
            .iter()
            .position(|(n, _)| n.ends_with(&format!("-{}.json", hex::encode(a.record))))
            .context("pin prefix")? as u64;
        if self
            .index
            .selected_at
            .get(&a.height)
            .is_none_or(|selected| *selected > seq)
        {
            return Err(availability(
                a.height,
                "pin-anchor",
                "S20: selected binding is outside checkpoint pin prefix",
            ));
        }
        let b = self.binding(a.height)?;
        ensure!(
            b.header.hash == a.l2_hash && b.root == a.root,
            "checkpoint pin binding differs"
        );
        self.check()
    }
    pub fn verify_header(&self, h: u64, header: &boole_derivation::ParentHeader) -> Result<()> {
        ensure!(
            serde_json::to_vec(&self.binding(h)?.header)? == serde_json::to_vec(header)?,
            "durable L2 header differs from observed header"
        );
        Ok(())
    }
    pub fn record_id(&self) -> Hash {
        self.last
    }
    pub fn contains_record(&self, hash: Hash) -> bool {
        self.files
            .iter()
            .any(|(name, _)| name.ends_with(&format!("-{}.json", hex::encode(hash))))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinAnchor {
    pub version: u8,
    pub record: Hash,
    pub height: u64,
    pub l2_hash: String,
    pub root: Hash,
}
pub struct PinStore {
    view: ReadOnlyPins,
    guard: StateDirGuard,
    records_guard: StateDirGuard,
    objects_guard: StateDirGuard,
}
impl PinStore {
    pub fn open(source: &InputJournal) -> Result<Self> {
        source.check()?;
        let root = source.root().join(DIR);
        let existed = root.try_exists()?;
        let guard = state_dir::acquire(&root)?;
        let records_guard = state_dir::acquire(&root.join("records"))?;
        crate::boot::cleanup_publications(&root, &guard)?;
        crate::boot::cleanup_publications(&root.join("records"), &records_guard)?;
        let objects_guard = state_dir::acquire(&root.join("objects"))?;
        crate::boot::cleanup_publications(&root.join("objects"), &objects_guard)?;
        if !root.join(MANIFEST).try_exists()? {
            if existed && !names(&root.join("records"))?.is_empty() {
                return Err(availability(
                    0,
                    "pin-manifest",
                    "S20: pin manifest missing with committed records; restore verified bytes",
                ));
            }
            let m = Manifest {
                schema_version: 1,
                network: source.network().clone(),
                l1_anchor_hash: source.anchor_hash().into(),
                retention: true,
                bootstrap_allowed: source.tip() > 0,
            };
            publish_new(&root.join(MANIFEST), &serde_json::to_vec(&m)?)?;
            publish_new(
                &root.join("objects").join("retention.required.json"),
                &serde_json::to_vec(&m)?,
            )?;
        }
        if !root.join("objects/retention.required.json").try_exists()?
            && names(&root.join("records"))?.is_empty()
        {
            let (raw, _) = read_bound(&root.join(MANIFEST), 65536)?;
            let m: Manifest = serde_json::from_slice(&raw)?;
            ensure!(
                m.network == *source.network()
                    && m.l1_anchor_hash == source.anchor_hash()
                    && m.schema_version == 1
                    && m.retention,
                "S20: initial pin manifest identity"
            );
            publish_new(&root.join("objects/retention.required.json"), &raw)?;
        }
        let view = ReadOnlyPins::open(source.root(), source.network(), source.anchor_hash())?
            .context("pin journal")?;
        for a in view.index.artifacts.values() {
            view.verify_artifact(a)?;
        }
        Ok(Self {
            view,
            guard,
            records_guard,
            objects_guard,
        })
    }
    pub fn view(&self) -> &ReadOnlyPins {
        &self.view
    }
    pub fn bootstrap_allowed(&self) -> bool {
        self.view.manifest.bootstrap_allowed && !self.view.index.bootstrapped
    }
    pub fn begin_bootstrap(&mut self, through: u64) -> Result<()> {
        if self.view.index.bootstrap.is_some() {
            ensure!(
                self.view.index.bootstrap == Some(through),
                "bootstrap ceiling"
            );
            return Ok(());
        }
        ensure!(
            self.bootstrap_allowed(),
            "S20: historical L2 bootstrap is not authorized for this own store"
        );
        self.append(Event::Bootstrap { through })
    }
    pub fn bootstrap_through(&self) -> Option<u64> {
        if self.bootstrap_allowed() {
            self.view.index.bootstrap
        } else {
            None
        }
    }
    pub fn finish_bootstrap(&mut self) -> Result<()> {
        if !self.view.index.bootstrapped {
            self.append(Event::Bootstrapped)?;
        }
        Ok(())
    }
    pub fn refresh(&mut self, source: &InputJournal) -> Result<()> {
        source.check()?;
        let next = ReadOnlyPins::open(source.root(), source.network(), source.anchor_hash())?
            .context("pin journal")?;
        ensure!(
            next.files
                .iter()
                .map(|v| &v.0)
                .eq(self.view.files.iter().map(|v| &v.0)),
            "pin record set changed outside this writer"
        );
        for a in next.index.artifacts.values() {
            next.verify_artifact(a)?;
        }
        self.view = next;
        Ok(())
    }
    pub fn install_bytes(
        &mut self,
        role: ArtifactRole,
        digest: Hash,
        bytes: &[u8],
        executable: bool,
    ) -> Result<()> {
        self.view.check()?;
        if let Some(old) = self.view.artifact(&role) {
            ensure!(old.digest == digest, "pin artifact role changed");
            return self.view.verify_artifact(old);
        }
        ensure!(
            bytes.len() <= 256 * 1024 * 1024 && hash(bytes) == digest,
            "inline pin bytes binding/cap"
        );
        let path = self.view.artifact_path(digest);
        if !path.try_exists()? {
            publish_new(&path, bytes)?;
        } else {
            ensure!(
                read_bound(&path, 256 * 1024 * 1024)?.0 == bytes,
                "existing pinned bytes"
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let exec = executable
                || self
                    .view
                    .index
                    .artifacts
                    .values()
                    .any(|a| a.digest == digest && a.executable);
            std::fs::set_permissions(
                &path,
                std::fs::Permissions::from_mode(if exec { 0o500 } else { 0o400 }),
            )?;
            std::fs::File::open(&path)?.sync_all()?;
            crate::durability::fsync_parent_dir(&path)?;
        }
        self.view.artifact_stamps.insert(digest, file_stamp(&path)?);
        crate::fault::hit("K4-PIN-BYTES", 0, "pins")?;
        self.append(Event::Artifacts {
            values: vec![Artifact {
                role,
                digest,
                bytes: bytes.len() as u64,
                executable,
            }],
        })?;
        crate::fault::hit("K4-PIN-RECORD", 0, "pins")
    }
    pub fn objects(&self) -> PathBuf {
        self.view.root.join("objects")
    }
    fn append(&mut self, event: Event) -> Result<()> {
        ensure!(
            self.guard.is_current()
                && self.records_guard.is_current()
                && self.objects_guard.is_current(),
            "pin writer lock changed"
        );
        self.view.check()?;
        let frame = Frame {
            version: 1,
            sequence: self.view.files.len() as u64,
            previous: self.view.last,
            event,
        };
        let raw = serde_json::to_vec(&frame)?;
        ensure!(raw.len() <= RECORD_MAX, "pin record size cap");
        let digest = frame_hash(&self.view.manifest, &raw)?;
        let name = format!("{:020}-{}.json", frame.sequence, hex::encode(digest));
        let mut next = self.view.index.clone();
        apply(&mut next, &frame.event, frame.sequence)?;
        let path = self.view.root.join("records").join(&name);
        publish_new(&path, &raw)?;
        self.view
            .files
            .push((name, file_stamp(&path)?.context("published pin record")?));
        self.view.last = digest;
        self.view.index = next;
        self.view.check()
    }
    pub fn install_artifacts(&mut self, values: &[ArtifactSource]) -> Result<()> {
        self.view.check()?;
        let mut added = vec![];
        for source in values {
            if let Some(old) = self.view.artifact(&source.role) {
                ensure!(old.digest == source.digest, "pin artifact role changed");
                self.view.verify_artifact(old)?;
                continue;
            }
            let (raw, _) = read_bound(&source.path, 256 * 1024 * 1024)?;
            ensure!(hash(&raw) == source.digest, "pin artifact source digest");
            let path = self.view.artifact_path(source.digest);
            if !path.try_exists()? {
                publish_new(&path, &raw)?;
            } else {
                ensure!(
                    read_bound(&path, 256 * 1024 * 1024)?.0 == raw,
                    "pin artifact existing bytes"
                );
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    &path,
                    std::fs::Permissions::from_mode(
                        if source.executable
                            || self
                                .view
                                .index
                                .artifacts
                                .values()
                                .chain(added.iter())
                                .any(|a: &Artifact| a.digest == source.digest && a.executable)
                        {
                            0o500
                        } else {
                            0o400
                        },
                    ),
                )?;
                std::fs::File::open(&path)?.sync_all()?;
                crate::durability::fsync_parent_dir(&path)?;
            }
            self.view
                .artifact_stamps
                .insert(source.digest, file_stamp(&path)?);
            added.push(Artifact {
                role: source.role.clone(),
                digest: source.digest,
                bytes: raw.len() as u64,
                executable: source.executable,
            });
        }
        if !added.is_empty() {
            crate::fault::hit("K4-PIN-BYTES", 0, "pins")?;
            self.append(Event::Artifacts { values: added })?;
            crate::fault::hit("K4-PIN-RECORD", 0, "pins")?;
        }
        Ok(())
    }
    pub fn prepare(
        &mut self,
        source: &InputJournal,
        binding: HeaderBinding,
        required: &[CandidatePin],
    ) -> Result<()> {
        source.check()?;
        ensure!(
            self.view.manifest.network == *source.network()
                && self.view.manifest.l1_anchor_hash == source.anchor_hash(),
            "pin writer source identity"
        );
        binding.verify(source.network(), &source.get(binding.header.number)?.block)?;
        if let Some(hash) = self.view.index.selected.get(&binding.header.number) {
            ensure!(
                *hash == binding.header.hash,
                "pinned canonical L2 binding cannot move backwards or change"
            );
        }
        let mut inputs = BTreeMap::new();
        let mut changed = vec![];
        for req in required {
            req.validate(binding.header.number)?;
            if self
                .view
                .index
                .candidates
                .get(&req.key)
                .is_none_or(|old| !old.covers(req))
            {
                changed.push(req.clone());
            }
            for (&lo, &hi) in &req.intervals().0 {
                for h in lo..=hi {
                    if !self.view.index.inputs.contains_key(&h) {
                        inputs.insert(h, row_hash(&source.row(h)?)?);
                    }
                }
            }
        }
        if let Some(old) = self
            .view
            .prepared(binding.header.number, &binding.header.hash)
        {
            ensure!(
                serde_json::to_vec(old)? == serde_json::to_vec(&binding)?,
                "prepared header rebound"
            );
        }
        if self
            .view
            .prepared(binding.header.number, &binding.header.hash)
            .is_none()
            || !changed.is_empty()
            || !inputs.is_empty()
        {
            self.append(Event::Prepared {
                binding: Box::new(binding.clone()),
                candidates: changed,
                inputs: inputs.into_iter().collect(),
            })?;
        }
        self.view
            .verify_required(source, required, binding.header.number, Some(&binding))?;
        crate::fault::hit("K4-PIN-PREPARED", binding.header.number, "pins")
    }
    pub fn select(&mut self, h: u64, hash: &str, root: Hash) -> Result<()> {
        if self.view.index.selected.get(&h).is_some_and(|v| v == hash) {
            return Ok(());
        }
        self.append(Event::Selected {
            height: h,
            hash: hash.into(),
            root,
        })?;
        crate::fault::hit("K4-PIN-SELECTED", h, "pins")
    }
}
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
