//! Fsynced, version-bound input journal. Every read authenticates original inputs.
use crate::{
    durability,
    error::{availability, integrity, startup},
    input::{InputRow, VerifiedInput, ROW_MAX},
    state_dir,
    version::{file_stamp, stamp, FileStamp},
};
use anyhow::{ensure, Context, Result};
use boole_derivation::attributes::{hex_bytes, hex_string};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

pub const DISK_FLOOR: u64 = 15 * 1024 * 1024 * 1024;
pub const MANIFEST: &str = "input.manifest.json";
pub const JOURNAL: &str = "input.ndjson";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkIdentity {
    pub l2_chain_id: u64,
    pub config_digest: String,
    pub fork_genesis: Value,
    pub l1_chain_id: u64,
    pub l2_genesis_hash: String,
    pub l2_gas_limit: u64,
}
impl NetworkIdentity {
    pub fn new(
        c: &boole_derivation::m2::Config,
        forks: Option<&boole_derivation::m3::ForkTable>,
        l2_genesis_hash: String,
        l2_gas_limit: u64,
    ) -> Result<Self> {
        c.validate(l2_gas_limit)
            .map_err(|s| startup(s, "invalid derivation config"))?;
        let default;
        let forks = if c.m3.is_some() {
            let f = if let Some(f) = forks {
                f
            } else {
                default = boole_derivation::m3::ForkTable::genesis(c)?;
                &default
            };
            f.validate(c)
                .map_err(|s| startup(s, "invalid fork table"))?;
            Some(f)
        } else {
            ensure!(forks.is_none(), "fork table requires M3");
            None
        };
        let _: [u8; 32] = hex_bytes(&l2_genesis_hash, "L2 genesis")?
            .try_into()
            .map_err(|_| anyhow::anyhow!("L2 genesis width"))?;
        Ok(Self {
            l2_chain_id: c.chain_id,
            config_digest: hex_string(&c.digest()),
            fork_genesis: forks
                .map(|f| serde_json::to_value(&f.rows[0]))
                .transpose()?
                .unwrap_or(Value::Null),
            l1_chain_id: boole_derivation::config::L1_CHAIN_ID,
            l2_genesis_hash,
            l2_gas_limit,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u8,
    network: NetworkIdentity,
    l1_anchor_number: u64,
    l1_anchor_hash: String,
}
#[derive(Clone)]
struct Position {
    offset: u64,
    len: usize,
    hash: String,
    parent: String,
    encoded_digest: [u8; 32],
    canonical_receipts: Vec<(String, bool)>,
    has_transaction_proof: bool,
}
pub struct InputJournal {
    root: PathBuf,
    path: PathBuf,
    guard: state_dir::StateDirGuard,
    manifest: Manifest,
    manifest_stamp: FileStamp,
    observed: FileStamp,
    positions: BTreeMap<u64, Position>,
}
pub(crate) fn regular(path: &Path, write: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(write);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    stamp(&file.metadata()?)?;
    Ok(file)
}
pub fn read_bound(path: &Path, limit: usize) -> Result<(Vec<u8>, FileStamp)> {
    let file = regular(path, false)?;
    let before = stamp(&file.metadata()?)?;
    ensure!(before.len <= limit as u64, "file byte cap");
    let mut bytes = vec![];
    (&file).take(limit as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 == before.len
            && stamp(&file.metadata()?)? == before
            && file_stamp(path)?.as_ref() == Some(&before),
        "file changed while reading"
    );
    Ok((bytes, before))
}
/// R1 create-new publication pattern: temp fsync, hard-link without replace, dir fsync.
pub fn publish_new(path: &Path, bytes: &[u8]) -> Result<()> {
    publish_new_at(path, bytes, None)
}
pub fn publish_new_at(path: &Path, bytes: &[u8], point: Option<(u64, &str)>) -> Result<()> {
    let parent = path.parent().context("new-file parent")?;
    let temp = durability::PrivateTempDir::new_in(parent, ".devnet-new")?;
    let staged = temp.path().join("value");
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&staged)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    if let Some((h, role)) = point {
        crate::fault::hit("B9a", h, role)?;
    }
    std::fs::hard_link(&staged, path)?;
    if let Some((h, role)) = point {
        crate::fault::hit("B9", h, role)?;
    }
    std::fs::remove_file(&staged)?;
    if let Some((h, role)) = point {
        crate::fault::hit("B9b", h, role)?;
    }
    durability::fsync_parent_dir(path)?;
    ensure!(
        file_stamp(path)?.as_ref() == Some(&stamp(&file.metadata()?)?),
        "new-file publication changed"
    );
    Ok(())
}
impl InputJournal {
    pub fn open(
        root: &Path,
        expected: NetworkIdentity,
        fresh: impl FnOnce() -> Result<InputRow>,
        repair: bool,
    ) -> Result<Self> {
        crate::fault::validate_environment()?;
        let guard = state_dir::acquire(root).map_err(|e| startup(18, e.to_string()))?;
        let root = std::fs::canonicalize(root)?;
        crate::boot::cleanup_publications(&root, &guard)?;
        let path = root.join(JOURNAL);
        let manifest_path = root.join(MANIFEST);
        let a = manifest_path.try_exists()?;
        let b = path.try_exists()?;
        if a != b {
            return Err(startup(19,"manifest/journal pair is incomplete; preserve it and recover into a fresh directory"));
        }
        if free_bytes(&root)? < DISK_FLOOR {
            return Err(startup(23, "below 15 GiB disk floor"));
        }
        if !a {
            for name in ["checkpoint.json", "evidence.jsonl", "identity.json"] {
                if root.join(name).try_exists()? {
                    return Err(startup(19,"existing state has no input journal; preserve it and use a fresh directory"));
                }
            }
            let first = fresh()?;
            let block = first.verify()?.block;
            ensure!(block.number == 0, "journal anchor must be genesis");
            let manifest = Manifest {
                schema_version: 1,
                network: expected.clone(),
                l1_anchor_number: 0,
                l1_anchor_hash: block.hash,
            };
            durability::append_ndjson_line_durable(&path, &serde_json::to_string(&first)?)?;
            publish_new(&manifest_path, &serde_json::to_vec(&manifest)?)?;
        }
        let (raw, manifest_stamp) = read_bound(&manifest_path, 65536)?;
        let manifest: Manifest = serde_json::from_slice(&raw)
            .map_err(|e| startup(18, format!("manifest format: {e}")))?;
        if manifest.schema_version != 1
            || manifest.network != expected
            || manifest.l1_anchor_number != 0
        {
            return Err(startup(18, "store identity mismatch"));
        }
        let (positions, observed) = Self::load(&path, &manifest, repair)
            .map_err(|e| integrity(0, "input", format!("{e:#}")))?;
        let out = Self {
            root,
            path,
            guard,
            manifest,
            manifest_stamp,
            observed,
            positions,
        };
        out.check()?;
        Ok(out)
    }
    fn load(
        path: &Path,
        manifest: &Manifest,
        repair: bool,
    ) -> Result<(BTreeMap<u64, Position>, FileStamp)> {
        let mut file = regular(path, repair)?;
        let before = stamp(&file.metadata()?)?;
        let mut reader = BufReader::new(file.try_clone()?);
        let mut positions = BTreeMap::new();
        let mut offset = 0;
        loop {
            let mut raw = vec![];
            let count = Read::by_ref(&mut reader)
                .take(ROW_MAX as u64 + 1)
                .read_until(b'\n', &mut raw)?;
            if count == 0 {
                break;
            }
            ensure!(count <= ROW_MAX, "journal row exceeds cap");
            if raw.last() != Some(&b'\n') {
                ensure!(
                    repair,
                    "torn journal tail; read-only load preserved every byte"
                );
                break;
            }
            let row: InputRow = serde_json::from_slice(&raw).map_err(|e| {
                integrity(positions.len() as u64, "input", format!("journal row: {e}"))
            })?;
            let verified = row
                .verify()
                .map_err(|e| integrity(positions.len() as u64, "input", e.to_string()))?;
            let h = verified.block.number;
            ensure!(h == positions.len() as u64, "noncontiguous journal height");
            if h == 0 {
                ensure!(
                    verified.block.hash == manifest.l1_anchor_hash,
                    "journal anchor mismatch"
                );
            } else {
                ensure!(
                    positions
                        .get(&(h - 1))
                        .is_some_and(|p: &Position| p.hash == verified.block.parent_hash),
                    "journal parent link mismatch"
                );
            }
            positions.insert(
                h,
                Position {
                    offset,
                    len: count,
                    hash: verified.block.hash,
                    parent: verified.block.parent_hash,
                    encoded_digest: Sha256::digest(&raw).into(),
                    has_transaction_proof: row.transactions.is_some(),
                    canonical_receipts: verified.canonical_receipts,
                },
            );
            offset += count as u64;
        }
        ensure!(!positions.is_empty(), "journal anchor absent");
        ensure!(
            stamp(&file.metadata()?)? == before && file_stamp(path)?.as_ref() == Some(&before),
            "journal changed during replay"
        );
        if offset < before.len {
            // Explicit repair only, after every complete row has authenticated.
            file.set_len(offset)?;
            file.sync_all()?;
            durability::fsync_parent_dir(path)?;
            for pos in positions.values() {
                file.seek(SeekFrom::Start(pos.offset))?;
                let mut bytes = vec![0; pos.len];
                file.read_exact(&mut bytes)?;
                ensure!(
                    <[u8; 32]>::from(Sha256::digest(&bytes)) == pos.encoded_digest,
                    "journal changed during tail repair"
                );
            }
        }
        let after = stamp(&file.metadata()?)?;
        ensure!(
            file_stamp(path)?.as_ref() == Some(&after),
            "journal path changed"
        );
        Ok((positions, after))
    }
    pub fn cleanup(&self, pins: &[[u8; 32]], checkpoint: Option<&Path>) -> Result<()> {
        self.check()?;
        crate::boot::cleanup(&self.root, &self.guard, pins, checkpoint)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn tip(&self) -> u64 {
        *self.positions.last_key_value().expect("anchor").0
    }
    pub fn hash(&self, h: u64) -> Option<&str> {
        self.positions.get(&h).map(|p| p.hash.as_str())
    }
    pub fn height_for_hash(&self, hash: &str) -> Option<u64> {
        self.positions
            .iter()
            .find_map(|(h, p)| p.hash.eq_ignore_ascii_case(hash).then_some(*h))
    }
    pub fn check(&self) -> Result<()> {
        if !self.guard.is_current()
            || file_stamp(&self.path)?.as_ref() != Some(&self.observed)
            || file_stamp(&self.root.join(MANIFEST))?.as_ref() != Some(&self.manifest_stamp)
            || durability::any_indeterminate_write([&self.path])
        {
            return Err(integrity(
                self.tip(),
                "file-version",
                "journal ownership/version/durability changed",
            ));
        }
        if free_bytes(&self.root)? < DISK_FLOOR {
            return Err(availability(
                self.tip(),
                "disk",
                "S23: below 15 GiB disk floor",
            ));
        }
        Ok(())
    }
    fn read_row(&self, h: u64) -> Result<(InputRow, VerifiedInput)> {
        self.check()?;
        let pos = self.positions.get(&h).ok_or_else(|| {
            availability(h, "journal", "input row absent; no historical RPC fallback")
        })?;
        let mut file = regular(&self.path, false)?;
        ensure!(
            stamp(&file.metadata()?)? == self.observed,
            "journal descriptor changed"
        );
        file.seek(SeekFrom::Start(pos.offset))?;
        let mut raw = vec![0; pos.len];
        file.read_exact(&mut raw)?;
        ensure!(
            <[u8; 32]>::from(Sha256::digest(&raw)) == pos.encoded_digest,
            "journal row changed"
        );
        let row: InputRow = serde_json::from_slice(&raw)?;
        let verified = row
            .verify()
            .map_err(|e| integrity(h, "input", e.to_string()))?;
        ensure!(
            verified.block.number == h
                && verified.block.hash == pos.hash
                && verified.block.parent_hash == pos.parent,
            "journal position identity"
        );
        self.check()?;
        Ok((row, verified))
    }
    pub fn get(&self, h: u64) -> Result<VerifiedInput> {
        Ok(self.read_row(h)?.1)
    }
    pub fn row(&self, h: u64) -> Result<InputRow> {
        Ok(self.read_row(h)?.0)
    }
    pub fn append(&mut self, row: &InputRow) -> Result<()> {
        self.check()?;
        let verified = row.verify()?;
        let h = verified.block.number;
        ensure!(
            h == self
                .tip()
                .checked_add(1)
                .context("journal height overflow")?
                && verified.block.parent_hash == self.positions[&(h - 1)].hash,
            "journal append parent/height"
        );
        if free_bytes(&self.root)? < DISK_FLOOR {
            return Err(availability(h, "disk", "S23: below 15 GiB disk floor"));
        }
        let raw = serde_json::to_string(row)?;
        ensure!(raw.len() < ROW_MAX, "journal row cap");
        let before = self.observed.clone();
        let result = durability::append_ndjson_line_versioned(
            &self.path,
            &raw,
            false,
            |metadata, created| {
                ensure!(
                    !created && stamp(metadata)? == before,
                    "journal append descriptor changed"
                );
                self.check()
            },
        );
        let written = match result {
            Ok(written) => written,
            Err(e) => {
                if durability::any_indeterminate_write([&self.path]) {
                    return Err(integrity(
                        h,
                        "durability-fence",
                        format!("indeterminate input append; restart required: {e:#}"),
                    ));
                }
                if !durability::any_indeterminate_write([&self.path])
                    && e.to_string() == "append failed; confirmed prefix restored"
                {
                    let (positions, observed) = Self::load(&self.path, &self.manifest, false)?;
                    ensure!(
                        before.same_file(&observed),
                        "journal inode changed during failed append"
                    );
                    ensure!(
                        positions.len() == self.positions.len()
                            && positions.iter().all(|(h, p)| self
                                .positions
                                .get(h)
                                .is_some_and(|old| old.encoded_digest == p.encoded_digest)),
                        "failed append changed confirmed journal prefix"
                    );
                    self.observed = observed;
                }
                return Err(availability(
                    h,
                    "disk",
                    format!("append failed, retry requires confirmed prefix: {e:#}"),
                ));
            }
        };
        let observed = stamp(written.metadata())?;
        ensure!(
            file_stamp(&self.path)?.as_ref() == Some(&observed)
                && observed.len == before.len + raw.len() as u64 + 1,
            "journal output changed before publication"
        );
        let mut encoded = raw.into_bytes();
        encoded.push(b'\n');
        self.positions.insert(
            h,
            Position {
                offset: before.len,
                len: encoded.len(),
                hash: verified.block.hash,
                parent: verified.block.parent_hash,
                encoded_digest: Sha256::digest(&encoded).into(),
                has_transaction_proof: row.transactions.is_some(),
                canonical_receipts: verified.canonical_receipts,
            },
        );
        self.observed = observed;
        self.check()
    }
    pub fn inspect(root: &Path) -> Result<u64> {
        let path = root.join(MANIFEST);
        let (raw, before) = read_bound(&path, 65536)?;
        let manifest: Manifest = serde_json::from_slice(&raw)?;
        ensure!(manifest.schema_version == 1, "manifest schema");
        let (positions, _) = Self::load(&root.join(JOURNAL), &manifest, false)?;
        ensure!(
            file_stamp(&path)?.as_ref() == Some(&before),
            "manifest changed during read-only inspection"
        );
        Ok(*positions.last_key_value().context("anchor")?.0)
    }
    /// A missing transaction proof is not evidence that an own post was absent.
    pub fn transaction_receipt_since(
        &self,
        tx: &str,
        finalized_before: u64,
    ) -> Result<Option<(u64, bool)>> {
        self.check()?;
        if let Some(found) = self.transaction_receipt(tx)? {
            return Ok(Some(found));
        }
        let missing = self
            .positions
            .range(finalized_before.saturating_add(1)..)
            .any(|(_, p)| !p.has_transaction_proof);
        if missing {
            return Err(availability(self.tip(),"archive-tx-proof","own post may be in an archive-filled finalized gap; preserve posting intent until authenticated transaction proofs are restored"));
        }
        Ok(None)
    }
    pub fn transaction_receipt(&self, tx: &str) -> Result<Option<(u64, bool)>> {
        self.check()?;
        for (h, position) in self.positions.iter().rev() {
            if position
                .canonical_receipts
                .iter()
                .any(|(hash, _)| hash.eq_ignore_ascii_case(tx))
            {
                let verified = self.get(*h)?;
                return Ok(verified
                    .canonical_receipts
                    .iter()
                    .find(|(hash, _)| hash.eq_ignore_ascii_case(tx))
                    .map(|(_, success)| (*h, *success)));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
thread_local! { static TEST_FREE_BYTES:std::cell::Cell<Option<u64>>=const{std::cell::Cell::new(None)}; }
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn free_bytes(path: &Path) -> Result<u64> {
    #[cfg(test)]
    if let Some(value) = TEST_FREE_BYTES.with(std::cell::Cell::take) {
        return Ok(value);
    }
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let path = CString::new(path.as_os_str().as_bytes())?;
    let mut value = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), value.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let value = unsafe { value.assume_init() };
    u64::try_from(u128::from(value.f_bavail) * u128::from(value.f_frsize))
        .context("disk byte count overflow")
}
#[cfg(not(unix))]
pub fn free_bytes(_path: &Path) -> Result<u64> {
    anyhow::bail!("S23 requires supported unix disk accounting")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::empty_fixture;
    #[test]
    fn b9_manifest_and_initial_evidence_aliases_are_recovered_before_single_link_reads(
    ) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        let d = durability::PrivateTempDir::new("m4-init-aliases")?;
        drop(open(d.path(), false));
        drop(crate::log::BoundLog::open(
            &d.path().join("evidence.jsonl"),
            false,
        )?);
        let mut originals = vec![];
        for name in [MANIFEST, "evidence.jsonl"] {
            let target = d.path().join(name);
            originals.push((target.clone(), std::fs::read(&target)?));
            let stage = durability::PrivateTempDir::new_in(d.path(), ".devnet-new")?;
            std::fs::hard_link(&target, stage.path().join("value"))?;
            std::mem::forget(stage);
            assert_eq!(std::fs::metadata(target)?.nlink(), 2);
        }
        let journal = InputJournal::open(d.path(), network(), || unreachable!(), false)?;
        assert_eq!(journal.get(0)?.block.number, 0);
        drop(crate::log::BoundLog::open(
            &d.path().join("evidence.jsonl"),
            false,
        )?);
        for (p, bytes) in originals {
            assert_eq!(std::fs::read(&p)?, bytes);
            assert_eq!(std::fs::metadata(p)?.nlink(), 1);
        }
        Ok(())
    }
    #[test]
    fn s18_identity_rejects_an_empty_fork_table_without_panicking() {
        let config = boole_derivation::m2::Config::devnet();
        let empty = boole_derivation::m3::ForkTable { rows: vec![] };
        assert!(
            NetworkIdentity::new(&config, Some(&empty), hex_string(&[1; 32]), 30_000_000).is_err()
        );
    }
    fn network() -> NetworkIdentity {
        NetworkIdentity::new(
            &boole_derivation::m2::Config::devnet(),
            None,
            hex_string(&[1; 32]),
            30_000_000,
        )
        .unwrap()
    }
    fn open(root: &Path, repair: bool) -> InputJournal {
        InputJournal::open(root, network(), || Ok(empty_fixture(0, [0; 32])), repair).unwrap()
    }
    #[test]
    fn t76_torn_tail_refuses_read_only_and_repairs_only_explicitly() {
        let dir = durability::PrivateTempDir::new("m4-torn").unwrap();
        let journal = open(dir.path(), false);
        drop(journal);
        let path = dir.path().join(JOURNAL);
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{\"torn\":")
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(InputJournal::inspect(dir.path()).is_err());
        assert!(InputJournal::open(dir.path(), network(), || unreachable!(), false).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let journal = open(dir.path(), true);
        assert_eq!(journal.tip(), 0);
        assert_eq!(journal.get(0).unwrap().block.number, 0);
        assert!(std::fs::read(&path).unwrap().ends_with(b"\n"));
    }
    #[test]
    fn t76_changed_file_blocks_reads_and_rollbacks_never_skip_a_height() {
        let dir = durability::PrivateTempDir::new("m4-append").unwrap();
        let mut journal = open(dir.path(), false);
        let parent = hex_bytes(journal.hash(0).unwrap(), "hash")
            .unwrap()
            .try_into()
            .unwrap();
        let row = empty_fixture(1, parent);
        for fail_sync in [false, true] {
            durability::fail_next_append(durability::AppendFault {
                write_bytes: (!fail_sync).then_some(5),
                fail_sync,
                fail_rollback: false,
            });
            assert!(journal.append(&row).is_err());
            assert_eq!(journal.tip(), 0);
            journal.get(0).unwrap();
        }
        journal.append(&row).unwrap();
        assert_eq!(journal.tip(), 1);
        OpenOptions::new()
            .append(true)
            .open(&journal.path)
            .unwrap()
            .write_all(b"external")
            .unwrap();
        assert!(journal.get(0).is_err());
        assert!(journal.append(&row).is_err());
    }
    #[test]
    fn t76_failed_rollback_fences_the_journal_until_process_restart() {
        let dir = durability::PrivateTempDir::new("m4-fence").unwrap();
        let mut journal = open(dir.path(), false);
        let row = empty_fixture(
            1,
            hex_bytes(journal.hash(0).unwrap(), "hash")
                .unwrap()
                .try_into()
                .unwrap(),
        );
        durability::fail_next_append(durability::AppendFault {
            write_bytes: Some(5),
            fail_sync: false,
            fail_rollback: true,
        });
        assert!(journal.append(&row).is_err());
        let before = std::fs::read(&journal.path).unwrap();
        assert!(journal.append(&row).is_err());
        assert!(journal.get(0).is_err());
        assert_eq!(std::fs::read(&journal.path).unwrap(), before);
        assert_eq!(journal.tip(), 0);
    }
    #[test]
    fn s18_s19_lock_identity_and_presence_refuse_before_editing() {
        let dir = durability::PrivateTempDir::new("m4-identity").unwrap();
        let journal = open(dir.path(), false);
        assert!(
            matches!(InputJournal::open(dir.path(),network(),||unreachable!(),false),Err(e) if e.to_string().contains("S18"))
        );
        drop(journal);
        let before = std::fs::read(dir.path().join(MANIFEST)).unwrap();
        let mut wrong = network();
        wrong.l2_chain_id += 1;
        assert!(
            matches!(InputJournal::open(dir.path(),wrong,||unreachable!(),false),Err(e) if e.to_string().contains("S18"))
        );
        assert_eq!(std::fs::read(dir.path().join(MANIFEST)).unwrap(), before);
        std::fs::rename(dir.path().join(JOURNAL), dir.path().join("preserved-input")).unwrap();
        assert!(
            matches!(InputJournal::open(dir.path(),network(),||unreachable!(),false),Err(e) if e.to_string().contains("S19"))
        );
        assert_eq!(std::fs::read(dir.path().join(MANIFEST)).unwrap(), before);
    }

    #[test]
    fn s23_existing_store_refuses_startup_below_the_fixed_disk_floor() -> Result<()> {
        let dir = durability::PrivateTempDir::new("m4-existing-disk")?;
        let journal = InputJournal::open(
            dir.path(),
            network(),
            || Ok(empty_fixture(0, [0; 32])),
            false,
        )?;
        drop(journal);
        let before = std::fs::read(dir.path().join(JOURNAL))?;
        TEST_FREE_BYTES.with(|v| v.set(Some(DISK_FLOOR - 1)));
        let result = InputJournal::open(dir.path(), network(), || unreachable!(), false);
        TEST_FREE_BYTES.with(|v| v.set(None));
        assert!(
            matches!(
                result
                    .as_ref()
                    .err()
                    .and_then(|e| e.downcast_ref::<crate::error::Error>()),
                Some(crate::error::Error::Startup { number: 23, .. })
            ),
            "an existing store must enforce S23 before replay"
        );
        assert_eq!(std::fs::read(dir.path().join(JOURNAL))?, before);
        let journal = InputJournal::open(dir.path(), network(), || unreachable!(), false)?;
        TEST_FREE_BYTES.with(|v| v.set(Some(DISK_FLOOR - 1)));
        let paused = match journal.get(0) {
            Ok(_) => panic!("runtime floor must pause reads before derivation"),
            Err(e) => e,
        };
        TEST_FREE_BYTES.with(|v| v.set(None));
        assert!(matches!(
            paused.downcast_ref::<crate::error::Error>(),
            Some(crate::error::Error::Availability { class: "disk", .. })
        ));
        assert_eq!(
            journal.get(0)?.block.number,
            0,
            "space restored: same height resumes"
        );
        assert_eq!(std::fs::read(dir.path().join(JOURNAL))?, before);
        Ok(())
    }
}
