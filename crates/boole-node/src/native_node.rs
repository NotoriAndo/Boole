//! Durable owner-coin state for the isolated native testnet. The block log is
//! authoritative; balances, locked rewards and nonces are re-derived at boot.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::native_chain::{
    NativeBlock, NativeBlockTemplate, NativeChain, NativeTransfer, NATIVE_RECENT_FORK_BLOCKS,
};
use boole_core::native_ledger::NativePendingView;
use boole_core::native_network::native_testnet;
use boole_core::Hex32;
use serde::Serialize;

use crate::durability::{append_ndjson_line_durable, write_ndjson_rows_atomic};
use crate::runtime::check_block_ts_future_drift;
use crate::state_dir::{acquire, ensure_manifest, LedgerLockSet, StateDirGuard, StateManifest};

pub const NATIVE_BLOCKS_FILE: &str = "native-blocks.ndjson";
pub const NATIVE_MEMPOOL_FILE: &str = "native-mempool.ndjson";
pub const MAX_NATIVE_PENDING_TRANSFERS: usize = 512;
/// Operational limits for the closed-local full-replay node, not consensus.
pub const MAX_NATIVE_HISTORY_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_NATIVE_POOL_BYTES: u64 = 5 * 1024 * 1024;
pub const MAX_NATIVE_HISTORY_BLOCKS: usize = 100_000;
// A reorg first preserves the old queue plus a bounded set of orphaned
// transfers. Boot filters this union against whichever canonical log won the
// atomic replacement, so a crash cannot lose the previous pending queue.
const MAX_NATIVE_RECOVERY_TRANSFERS: usize = 2 * MAX_NATIVE_PENDING_TRANSFERS;

/// A derived, non-authoritative snapshot of the durable queue at one head.
/// Keep its ordered transactions, duplicate index and reservations together.
#[derive(Debug)]
struct PendingState {
    transfers: Vec<NativeTransfer>,
    ids: BTreeSet<Hex32>,
    view: NativePendingView,
}

/// Current authoritative-file sizes and bounded state counts. No paths, private
/// keys, process addresses or claimed remote balances are exposed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeResourceUsage {
    pub history_bytes: u64,
    pub history_limit_bytes: u64,
    pub history_blocks: usize,
    pub history_limit_blocks: usize,
    pub confirmed_transfers: usize,
    pub pending_bytes: u64,
    pub pending_limit_bytes: u64,
    pub pending_transfers: usize,
    pub pending_limit_transfers: usize,
    pub balance_entries: usize,
    pub nonce_entries: usize,
}

#[derive(Debug)]
pub struct NativeNode {
    chain: NativeChain,
    block_path: PathBuf,
    pool_path: PathBuf,
    pending: PendingState,
    confirmed: BTreeMap<Hex32, u64>,
    block_stamp: Option<FileStamp>,
    pool_stamp: Option<FileStamp>,
    manifest_path: PathBuf,
    manifest_stamp: Option<FileStamp>,
    state_guard: StateDirGuard,
    ledger_locks: LedgerLockSet,
    poisoned: bool,
}

impl NativeNode {
    pub fn open(state_dir: &Path) -> anyhow::Result<Self> {
        Self::open_inner(state_dir, true)
    }

    /// Offline export must not silently repair or canonicalize its source.
    pub(crate) fn open_preserving(state_dir: &Path) -> anyhow::Result<Self> {
        Self::open_inner(state_dir, false)
    }

    fn open_inner(state_dir: &Path, repair: bool) -> anyhow::Result<Self> {
        let state_guard = acquire(state_dir)?;
        let state_dir = state_guard.dir().canonicalize()?;
        let block_path = LedgerLockSet::canonical_path(&state_dir.join(NATIVE_BLOCKS_FILE))?;
        let pool_path = LedgerLockSet::canonical_path(&state_dir.join(NATIVE_MEMPOOL_FILE))?;
        let ledger_locks = LedgerLockSet::acquire([block_path.clone(), pool_path.clone()])?;
        check_file_budget(&block_path, MAX_NATIVE_HISTORY_BYTES)?;
        check_file_budget(&pool_path, MAX_NATIVE_POOL_BYTES)?;
        let network = native_testnet();
        let mut manifest = StateManifest::now(
            network.network_id(),
            env!("CARGO_PKG_VERSION"),
            &network.genesis_hash().to_hex(),
        );
        manifest
            .schema_versions
            .insert("native_storage".to_string(), 1);
        if repair {
            ensure_manifest(&state_dir, &manifest)?;
        } else {
            crate::state_dir::verify_manifest_read_only(&state_dir, &manifest)?;
        }
        let manifest_path = state_dir.join("state.manifest.json");
        let mut chain = NativeChain::new()?;
        let now = unix_time_ms()?;
        if let Some(raw) = read_native_log(&block_path, MAX_NATIVE_HISTORY_BYTES, repair)? {
            for (index, line) in raw.lines().enumerate() {
                anyhow::ensure!(
                    index < MAX_NATIVE_HISTORY_BLOCKS,
                    "native history block limit"
                );
                anyhow::ensure!(
                    line.len() <= network.max_block_bytes(),
                    "native block line too large"
                );
                let block: NativeBlock = serde_json::from_str(line).map_err(|error| {
                    anyhow::anyhow!("native block log line {}: {error}", index + 1)
                })?;
                check_block_ts_future_drift(block.header.timestamp_ms, now)?;
                chain.append(block)?;
            }
        }
        let confirmed = confirmed_index(&chain);
        let mut pending = Vec::new();
        if let Some(raw) = read_native_log(&pool_path, MAX_NATIVE_POOL_BYTES, repair)? {
            for line in raw.lines() {
                anyhow::ensure!(
                    line.len() <= network.max_transfer_bytes(),
                    "native transfer line too large"
                );
                anyhow::ensure!(
                    pending.len() < MAX_NATIVE_RECOVERY_TRANSFERS,
                    "native mempool too large"
                );
                let transfer: NativeTransfer = serde_json::from_str(line)?;
                // Invalid signatures cannot be caused by an honest reorg.
                // Preserve such a journal as corruption instead of erasing it.
                transfer.validated_fields()?;
                pending.push(transfer);
            }
        }
        let retained = retain_pending(&chain, &confirmed, &pending)?;
        if pending != retained.transfers && repair {
            write_pool(&pool_path, &retained.transfers)?;
        }
        let block_stamp = file_stamp(&block_path)?;
        let pool_stamp = file_stamp(&pool_path)?;
        let manifest_stamp = file_stamp(&manifest_path)?;
        anyhow::ensure!(
            manifest_stamp.is_some(),
            "native state manifest disappeared"
        );
        let node = Self {
            chain,
            block_path,
            pool_path,
            pending: retained,
            confirmed,
            block_stamp,
            pool_stamp,
            manifest_path,
            manifest_stamp,
            state_guard,
            ledger_locks,
            poisoned: false,
        };
        node.ensure_ready()?;
        Ok(node)
    }

    pub fn chain(&self) -> &NativeChain {
        &self.chain
    }

    pub fn pending(&self) -> &[NativeTransfer] {
        &self.pending.transfers
    }

    pub fn is_pending(&self, id: &Hex32) -> bool {
        self.pending.ids.contains(id)
    }

    pub fn pending_view(&self) -> anyhow::Result<&NativePendingView> {
        self.ensure_ready()?;
        Ok(&self.pending.view)
    }

    pub fn confirmed_height(&self, id: &Hex32) -> Option<u64> {
        self.confirmed.get(id).copied()
    }

    /// File stamps are checked before exposing these cached lengths. The map
    /// counts are constant-time; observing capacity never sums all accounts or
    /// replays history/pending transfers.
    pub fn resource_usage(&self) -> anyhow::Result<NativeResourceUsage> {
        self.ensure_ready()?;
        Ok(NativeResourceUsage {
            history_bytes: self.block_stamp.as_ref().map_or(0, |stamp| stamp.len),
            history_limit_bytes: MAX_NATIVE_HISTORY_BYTES,
            history_blocks: self.chain.blocks().len(),
            history_limit_blocks: MAX_NATIVE_HISTORY_BLOCKS,
            confirmed_transfers: self.confirmed.len(),
            pending_bytes: self.pool_stamp.as_ref().map_or(0, |stamp| stamp.len),
            pending_limit_bytes: MAX_NATIVE_POOL_BYTES,
            pending_transfers: self.pending.transfers.len(),
            pending_limit_transfers: MAX_NATIVE_PENDING_TRANSFERS,
            balance_entries: self.chain.ledger().balance_entry_count(),
            nonce_entries: self.chain.ledger().nonce_entry_count(),
        })
    }

    /// Admission reserves nonce/balance only in the pending view. It does not
    /// debit the canonical ledger or claim that the transfer is confirmed.
    pub fn submit_transfer(&mut self, transfer: NativeTransfer) -> anyhow::Result<bool> {
        self.ensure_writable()?;
        transfer.validated_fields()?;
        let id = transfer.id();
        if self.confirmed.contains_key(&id) || self.pending.ids.contains(&id) {
            return Ok(false);
        }
        anyhow::ensure!(
            self.pending.transfers.len() < MAX_NATIVE_PENDING_TRANSFERS,
            "native mempool is full"
        );
        let reservation = self.pending.view.prepare(&transfer.envelope()?)?;
        let publish = append_bounded(
            &self.pool_path,
            &serde_json::to_string(&transfer)?,
            MAX_NATIVE_POOL_BYTES,
        )
        .and_then(|()| file_stamp(&self.pool_path));
        match publish {
            Ok(stamp) => self.pool_stamp = stamp,
            Err(error) => {
                self.poisoned = true;
                return Err(error);
            }
        }
        reservation.commit();
        self.pending.transfers.push(transfer);
        self.pending.ids.insert(id);
        Ok(true)
    }

    pub fn template(
        &self,
        producer_pk: &str,
        reward_pk: &str,
        timestamp_ms: u64,
    ) -> anyhow::Result<NativeBlockTemplate> {
        self.ensure_writable()?;
        check_block_ts_future_drift(timestamp_ms, unix_time_ms()?)?;
        self.chain
            .template(producer_pk, reward_pk, timestamp_ms, self.pending())
    }

    /// Verify a complete candidate, reusing only a byte-equivalent prefix of
    /// our own verified history. Long forks still replay from genesis. No
    /// externally declared balance or checkpoint is read.
    pub fn adopt_chain(&mut self, blocks: &[NativeBlock]) -> anyhow::Result<bool> {
        self.ensure_writable()?;
        check_candidate_budget(blocks)?;
        let common = common_prefix_len(self.chain.blocks(), blocks);
        let candidate = if common as u64 >= self.chain.earliest_recent_fork_height() {
            let mut candidate = self.chain.fork_at(common as u64)?;
            for block in &blocks[common..] {
                candidate.append(block.clone())?;
            }
            candidate
        } else {
            NativeChain::replay(blocks)?
        };
        self.publish_candidate(candidate)
    }

    /// A bounded live-peer fork supplies only its new suffix. The prefix can
    /// only come from this node's recent verified history; every suffix block
    /// still proves linkage, PoW, signatures and accounting independently.
    pub fn adopt_recent_suffix(
        &mut self,
        common_height: u64,
        suffix: &[NativeBlock],
    ) -> anyhow::Result<bool> {
        self.ensure_writable()?;
        anyhow::ensure!(
            !suffix.is_empty() && suffix.len() <= NATIVE_RECENT_FORK_BLOCKS,
            "native recent suffix block limit"
        );
        anyhow::ensure!(
            common_height
                .checked_add(suffix.len() as u64)
                .is_some_and(|height| height <= MAX_NATIVE_HISTORY_BLOCKS as u64),
            "native history block limit"
        );
        check_candidate_budget(suffix)?;
        let mut candidate = self.chain.fork_at(common_height)?;
        for block in suffix {
            candidate.append(block.clone())?;
        }
        check_candidate_budget(candidate.blocks())?;
        self.publish_candidate(candidate)
    }

    /// NativeChain cannot be deserialized or built without core verification.
    /// A streamed archive can transfer its verified candidate without replaying
    /// signatures a second time or cloning the complete history again.
    pub(crate) fn adopt_replayed_chain(&mut self, candidate: NativeChain) -> anyhow::Result<bool> {
        self.ensure_writable()?;
        check_candidate_budget(candidate.blocks())?;
        self.publish_candidate(candidate)
    }

    fn publish_candidate(&mut self, candidate: NativeChain) -> anyhow::Result<bool> {
        if !candidate.outranks(&self.chain) {
            return Ok(false);
        }
        let common = common_prefix_len(self.chain.blocks(), candidate.blocks());
        let mut confirmed = self.confirmed.clone();
        confirmed.retain(|_, height| *height <= common as u64);
        for block in &candidate.blocks()[common..] {
            for transfer in &block.transfers {
                confirmed.insert(transfer.id(), block.header.height);
            }
        }
        let mut recovery = Vec::new();
        let orphan_limit = MAX_NATIVE_RECOVERY_TRANSFERS - self.pending.transfers.len();
        let mut seen = self.pending.ids.clone();
        'blocks: for block in &self.chain.blocks()[common..] {
            for transfer in &block.transfers {
                if recovery.len() == orphan_limit {
                    break 'blocks;
                }
                let id = transfer.id();
                if !confirmed.contains_key(&id) && seen.insert(id) {
                    recovery.push(transfer.clone());
                }
            }
        }
        // Old canonical transfers precede the old pending queue: an orphaned
        // nonce or funding credit can be a prerequisite for a queued successor.
        // Keep room for every old pending input in the durable recovery union.
        recovery.extend(self.pending.transfers.iter().cloned());
        let retained = retain_pending(&candidate, &confirmed, &recovery)?;
        let publish = (|| -> anyhow::Result<()> {
            if recovery != self.pending.transfers {
                write_pool(&self.pool_path, &recovery)?;
                self.pool_stamp = file_stamp(&self.pool_path)?;
            }
            write_ndjson_rows_atomic(&self.block_path, candidate.blocks())?;
            self.block_stamp = file_stamp(&self.block_path)?;
            self.chain = candidate;
            self.confirmed = confirmed;
            write_pool(&self.pool_path, &retained.transfers)?;
            self.pool_stamp = file_stamp(&self.pool_path)?;
            self.pending = retained;
            Ok(())
        })();
        if let Err(error) = publish {
            self.poisoned = true;
            return Err(error.context("native reorg publication requires restart and recovery"));
        }
        Ok(true)
    }

    /// Readiness includes live ownership and all authoritative file identities.
    pub fn ensure_ready(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.poisoned
                && self.state_guard.is_current()
                && self.ledger_locks.is_current()
                && !self.ledger_locks.has_indeterminate_write(),
            "native state ownership or durability lost; restart and recover"
        );
        anyhow::ensure!(
            file_stamp(&self.block_path)? == self.block_stamp
                && file_stamp(&self.pool_path)? == self.pool_stamp
                && file_stamp(&self.manifest_path)? == self.manifest_stamp,
            "native state file changed outside this writer; restart and recover"
        );
        Ok(())
    }

    fn ensure_writable(&self) -> anyhow::Result<()> {
        self.ensure_ready()
    }

    /// Returns false only for a byte-equivalent previously accepted block.
    /// A successful append is durable before the new balance becomes visible.
    pub fn submit_block(&mut self, block: NativeBlock) -> anyhow::Result<bool> {
        self.ensure_writable()?;
        check_block_ts_future_drift(block.header.timestamp_ms, unix_time_ms()?)?;
        if block.header.height > 0 {
            if let Ok(index) = usize::try_from(block.header.height - 1) {
                if self.chain.blocks().get(index) == Some(&block) {
                    return Ok(false);
                }
            }
        }
        anyhow::ensure!(
            self.chain.blocks().len() < MAX_NATIVE_HISTORY_BLOCKS,
            "native history block limit"
        );
        let prepared = self.chain.prepare(block)?;
        let publish = append_bounded(
            &self.block_path,
            &serde_json::to_string(prepared.block())?,
            MAX_NATIVE_HISTORY_BYTES,
        )
        .and_then(|()| file_stamp(&self.block_path));
        match publish {
            Ok(stamp) => self.block_stamp = stamp,
            Err(error) => {
                self.poisoned = true;
                return Err(error);
            }
        }
        if let Err(error) = self.chain.commit(prepared) {
            self.poisoned = true;
            return Err(error);
        }
        let block = self.chain.blocks().last().expect("committed block");
        for transfer in &block.transfers {
            self.confirmed.insert(transfer.id(), block.header.height);
        }
        let retained = match retain_pending(&self.chain, &self.confirmed, self.pending()) {
            Ok(retained) => retained,
            Err(error) => {
                self.poisoned = true;
                return Err(error);
            }
        };
        if retained.transfers != self.pending.transfers {
            if let Err(error) = write_pool(&self.pool_path, &retained.transfers).and_then(|()| {
                self.pool_stamp = file_stamp(&self.pool_path)?;
                Ok(())
            }) {
                // The block is already authoritative. A restart reconstructs
                // the ledger and discards included/stale pending rows.
                self.poisoned = true;
                return Err(
                    error.context("native block committed; pending cleanup requires recovery")
                );
            }
        }
        self.pending = retained;
        Ok(true)
    }
}

fn check_file_budget(path: &Path, limit: u64) -> anyhow::Result<u64> {
    let len = file_stamp(path)?.map_or(0, |stamp| stamp.len);
    anyhow::ensure!(len <= limit, "native state file exceeds byte limit");
    Ok(len)
}

fn check_candidate_budget(blocks: &[NativeBlock]) -> anyhow::Result<()> {
    anyhow::ensure!(
        blocks.len() <= MAX_NATIVE_HISTORY_BLOCKS,
        "native history block limit"
    );
    let mut bytes = 0u64;
    let now = unix_time_ms()?;
    for block in blocks {
        bytes = bytes
            .checked_add(serde_json::to_vec(block)?.len() as u64 + 1)
            .ok_or_else(|| anyhow::anyhow!("native history size overflow"))?;
        anyhow::ensure!(
            bytes <= MAX_NATIVE_HISTORY_BYTES,
            "native history byte limit"
        );
        check_block_ts_future_drift(block.header.timestamp_ms, now)?;
    }
    Ok(())
}

fn append_bounded(path: &Path, line: &str, limit: u64) -> anyhow::Result<()> {
    let len = check_file_budget(path, limit)?;
    anyhow::ensure!(
        len + (line.len() as u64) < limit,
        "native state file exceeds byte limit"
    );
    append_ndjson_line_durable(path, line)
}

#[derive(Debug, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64),
    #[cfg(unix)]
    change_time: (i64, i64),
}

fn file_stamp(path: &Path) -> anyhow::Result<Option<FileStamp>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    stamp(&metadata).map(Some)
}

fn stamp(metadata: &fs::Metadata) -> anyhow::Result<FileStamp> {
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "native state path is not a regular file"
    );
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    #[cfg(unix)]
    anyhow::ensure!(metadata.nlink() == 1, "native state has a hard-link alias");
    Ok(FileStamp {
        len: metadata.len(),
        modified: metadata.modified()?,
        #[cfg(unix)]
        identity: (metadata.dev(), metadata.ino()),
        #[cfg(unix)]
        change_time: (metadata.ctime(), metadata.ctime_nsec()),
    })
}

fn read_native_log(path: &Path, maximum: u64, repair: bool) -> anyhow::Result<Option<String>> {
    let file = match OpenOptions::new()
        .read(true)
        .write(repair)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let before = stamp(&file.metadata()?)?;
    anyhow::ensure!(
        before.len <= maximum && file_stamp(path)?.as_ref() == Some(&before),
        "native log is oversized or changed before read"
    );
    let mut bytes = Vec::new();
    (&file).take(maximum + 1).read_to_end(&mut bytes)?;
    let after = stamp(&file.metadata()?)?;
    anyhow::ensure!(
        bytes.len() as u64 == before.len
            && before == after
            && file_stamp(path)?.as_ref() == Some(&after),
        "native log changed while reading"
    );
    let stable = crate::durability::stable_jsonl_prefix_len(&bytes);
    if stable < bytes.len() {
        anyhow::ensure!(
            repair,
            "source-preserving export refuses a torn native log; no bytes repaired"
        );
        file.set_len(stable as u64)?;
        file.sync_all()?;
        bytes.truncate(stable);
    }
    Ok(Some(String::from_utf8(bytes)?))
}

pub(crate) fn unix_time_ms() -> anyhow::Result<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

fn confirmed_index(chain: &NativeChain) -> BTreeMap<Hex32, u64> {
    chain
        .blocks()
        .iter()
        .flat_map(|block| {
            block
                .transfers
                .iter()
                .map(|transfer| (transfer.id(), block.header.height))
        })
        .collect()
}

fn common_prefix_len(left: &[NativeBlock], right: &[NativeBlock]) -> usize {
    // A block hash omits producer_signature and commits to transfers via the
    // root only. Comparing hashes would let mutated remote bodies/signatures
    // impersonate our validated prefix without passing independent checks.
    left.iter().zip(right).take_while(|(a, b)| a == b).count()
}

fn retain_pending(
    chain: &NativeChain,
    confirmed: &BTreeMap<Hex32, u64>,
    candidates: &[NativeTransfer],
) -> anyhow::Result<PendingState> {
    let mut view = chain.ledger().pending_view()?;
    let mut retained = Vec::new();
    let mut ids = BTreeSet::new();
    for transfer in candidates {
        let id = transfer.id();
        if confirmed.contains_key(&id)
            || ids.contains(&id)
            || retained.len() == MAX_NATIVE_PENDING_TRANSFERS
        {
            continue;
        }
        if view.push(&transfer.envelope()?).is_ok() {
            retained.push(transfer.clone());
            ids.insert(id);
        }
    }
    Ok(PendingState {
        transfers: retained,
        ids,
        view,
    })
}

fn write_pool(path: &Path, pending: &[NativeTransfer]) -> anyhow::Result<()> {
    write_ndjson_rows_atomic(path, pending)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::durability::{fail_atomic_rewrite, fail_next_append, AppendFault, PrivateTempDir};
    use boole_core::SigningKeyV2;

    #[test]
    fn recent_reorg_publication_failures_preserve_pending_dependencies_on_restart() {
        for stage in 0..3 {
            for after_rename in [false, true] {
                let dir = PrivateTempDir::new("boole-native-reorg-fault").unwrap();
                let key = SigningKeyV2::from_dev_id("native-reorg-fault-owner");
                let sign = |template: NativeBlockTemplate| {
                    let block = template.mine(0, 2_000_000).unwrap().unwrap();
                    let auth = key
                        .sign_for_network(
                            &block.authorization_payload().unwrap(),
                            Some(native_testnet().network_id()),
                        )
                        .unwrap();
                    block.authorize(&auth).unwrap()
                };
                let mut node = NativeNode::open(dir.path()).unwrap();
                for height in 1..=10 {
                    let block = sign(
                        node.template(&key.pk_hex(), &key.pk_hex(), height * 60_000)
                            .unwrap(),
                    );
                    node.submit_block(block).unwrap();
                }
                let mut candidate = node.chain().clone();
                let transfers: Vec<_> = (0..2).map(|nonce: u64| NativeTransfer::try_from(
                    &key.sign_for_network(&serde_json::json!({
                        "schema": "boole.transfer.v1", "from": key.pk_hex(), "to": key.pk_hex(),
                        "amount": "1", "fee": "1000", "nonce": nonce.to_string(), "validBefore": "100"
                    }), Some(native_testnet().network_id())).unwrap()).unwrap()).collect();
                node.submit_transfer(transfers[0].clone()).unwrap();
                node.submit_block(sign(
                    node.template(&key.pk_hex(), &key.pk_hex(), 660_000)
                        .unwrap(),
                ))
                .unwrap();
                node.submit_transfer(transfers[1].clone()).unwrap();
                let former = node.chain().clone();
                for height in 11..=12 {
                    candidate
                        .append(sign(
                            candidate
                                .template(&key.pk_hex(), &key.pk_hex(), height * 60_000, &[])
                                .unwrap(),
                        ))
                        .unwrap();
                }
                let path = if stage == 1 {
                    &node.block_path
                } else {
                    &node.pool_path
                };
                fail_atomic_rewrite(path, after_rename, usize::from(stage == 2));
                let error = node
                    .adopt_recent_suffix(10, &candidate.blocks()[10..])
                    .unwrap_err();
                assert!(format!("{error:#}").contains("injected atomic rewrite failure"));
                assert!(node.ensure_ready().is_err());
                assert!(node.pending_view().is_err());
                assert!(node
                    .adopt_recent_suffix(10, &candidate.blocks()[10..])
                    .is_err());
                drop(node);
                let recovered = NativeNode::open(dir.path()).unwrap();
                let new_history = stage == 2 || (stage == 1 && after_rename);
                assert_eq!(
                    recovered.chain(),
                    if new_history { &candidate } else { &former }
                );
                assert_eq!(
                    recovered.pending(),
                    if new_history {
                        &transfers[..]
                    } else {
                        &transfers[1..]
                    }
                );
                assert_eq!(
                    recovered.pending_view().unwrap().next_nonce(&key.pk_hex()),
                    2
                );
                assert_eq!(
                    recovered.confirmed_height(&transfers[0].id()),
                    if new_history { None } else { Some(11) }
                );
            }
        }
    }

    #[test]
    fn failed_pending_append_never_reserves_a_nonce_or_balance_after_recovery() {
        for fault in [
            AppendFault {
                write_bytes: Some(7),
                ..Default::default()
            },
            AppendFault {
                fail_sync: true,
                ..Default::default()
            },
        ] {
            let dir = PrivateTempDir::new("boole-native-pending-fault").unwrap();
            let key = SigningKeyV2::from_dev_id("native-pending-fault-owner");
            let receiver = SigningKeyV2::from_dev_id("native-pending-fault-recipient");
            let mut node = NativeNode::open(dir.path()).unwrap();
            for height in 1..=10 {
                let mined = node
                    .template(&key.pk_hex(), &key.pk_hex(), height * 60_000)
                    .unwrap()
                    .mine(0, 2_000_000)
                    .unwrap()
                    .unwrap();
                let auth = key
                    .sign_for_network(
                        &mined.authorization_payload().unwrap(),
                        Some(native_testnet().network_id()),
                    )
                    .unwrap();
                node.submit_block(mined.authorize(&auth).unwrap()).unwrap();
            }
            let transfer = NativeTransfer::try_from(
                &key.sign_for_network(
                    &serde_json::json!({
                        "schema": "boole.transfer.v1", "from": key.pk_hex(),
                        "to": receiver.pk_hex(), "amount": "1", "fee": "1000",
                        "nonce": "0", "validBefore": "100"
                    }),
                    Some(native_testnet().network_id()),
                )
                .unwrap(),
            )
            .unwrap();
            let available = node
                .pending_view()
                .unwrap()
                .available_balance(&key.pk_hex());
            let canonical = node.chain().clone();
            fail_next_append(fault);
            assert!(node.submit_transfer(transfer.clone()).is_err());
            assert_eq!(node.chain(), &canonical);
            assert!(node.pending().is_empty());
            assert!(
                node.pending_view().is_err(),
                "indeterminate state is fenced"
            );
            assert!(node.submit_transfer(transfer.clone()).is_err());
            drop(node);
            let mut recovered = NativeNode::open(dir.path()).unwrap();
            assert_eq!(recovered.chain(), &canonical);
            assert!(recovered.pending().is_empty());
            assert_eq!(
                recovered.pending_view().unwrap().next_nonce(&key.pk_hex()),
                0
            );
            assert_eq!(
                recovered
                    .pending_view()
                    .unwrap()
                    .available_balance(&key.pk_hex()),
                available
            );
            assert!(recovered.submit_transfer(transfer.clone()).unwrap());
            assert!(!recovered.submit_transfer(transfer).unwrap());
            assert_eq!(
                recovered.pending_view().unwrap().next_nonce(&key.pk_hex()),
                1
            );
            assert_eq!(
                recovered
                    .pending_view()
                    .unwrap()
                    .available_balance(&key.pk_hex()),
                available - 1001
            );
        }
    }

    #[test]
    fn failed_native_append_never_publishes_a_reward_and_requires_recovery() {
        for fault in [
            AppendFault {
                write_bytes: Some(7),
                ..Default::default()
            },
            AppendFault {
                fail_sync: true,
                ..Default::default()
            },
        ] {
            let dir = PrivateTempDir::new("boole-native-append-fault").unwrap();
            let key = SigningKeyV2::from_dev_id("native-append-fault-producer");
            let block;
            {
                let mut node = NativeNode::open(dir.path()).unwrap();
                let mined = node
                    .template(&key.pk_hex(), &key.pk_hex(), 60_000)
                    .unwrap()
                    .mine(0, 2_000_000)
                    .unwrap()
                    .unwrap();
                let auth = key
                    .sign_for_network(
                        &mined.authorization_payload().unwrap(),
                        Some(native_testnet().network_id()),
                    )
                    .unwrap();
                block = mined.authorize(&auth).unwrap();
                fail_next_append(fault);
                assert!(node.submit_block(block.clone()).is_err());
                assert_eq!(node.chain().ledger().balance(&key.pk_hex()), 0);
                assert_eq!(node.chain().ledger().height(), 0);
                assert!(node.ensure_ready().is_err());
                assert!(node.submit_block(block.clone()).is_err());
            }
            let mut node = NativeNode::open(dir.path()).unwrap();
            assert_eq!(node.chain().ledger().height(), 0);
            assert!(node.submit_block(block).unwrap());
            assert_eq!(
                node.chain().ledger().balance(&key.pk_hex()),
                50_000 * 100_000_000
            );
        }
    }
}
