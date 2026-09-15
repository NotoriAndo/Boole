//! Durable owner-coin state for the isolated native testnet. The block log is
//! authoritative; balances, locked rewards and nonces are re-derived at boot.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use boole_core::native_chain::{NativeBlock, NativeBlockTemplate, NativeChain, NativeTransfer};
use boole_core::native_ledger::NativePendingView;
use boole_core::native_network::native_testnet;
use boole_core::Hex32;

use crate::durability::{
    append_ndjson_line_durable, read_stable_prefix, write_ndjson_lines_atomic,
};
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

#[derive(Debug)]
pub struct NativeNode {
    chain: NativeChain,
    block_path: PathBuf,
    pool_path: PathBuf,
    pending: Vec<NativeTransfer>,
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
        ensure_manifest(&state_dir, &manifest)?;
        let manifest_path = state_dir.join("state.manifest.json");
        let mut chain = NativeChain::new()?;
        let now = unix_time_ms()?;
        if let Some(raw) = read_stable_prefix(&block_path)? {
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
        if let Some(raw) = read_stable_prefix(&pool_path)? {
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
        if pending != retained {
            write_pool(&pool_path, &retained)?;
        }
        let block_stamp = file_stamp(&block_path)?;
        let pool_stamp = file_stamp(&pool_path)?;
        let manifest_stamp = file_stamp(&manifest_path)?;
        anyhow::ensure!(
            manifest_stamp.is_some(),
            "native state manifest disappeared"
        );
        Ok(Self {
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
        })
    }

    pub fn chain(&self) -> &NativeChain {
        &self.chain
    }

    pub fn pending(&self) -> &[NativeTransfer] {
        &self.pending
    }

    pub fn pending_view(&self) -> anyhow::Result<NativePendingView> {
        let mut view = self.chain.ledger().pending_view()?;
        for transfer in &self.pending {
            view.push(&transfer.envelope()?)?;
        }
        Ok(view)
    }

    pub fn confirmed_height(&self, id: &Hex32) -> Option<u64> {
        self.confirmed.get(id).copied()
    }

    /// Admission reserves nonce/balance only in the pending view. It does not
    /// debit the canonical ledger or claim that the transfer is confirmed.
    pub fn submit_transfer(&mut self, transfer: NativeTransfer) -> anyhow::Result<bool> {
        self.ensure_writable()?;
        transfer.validated_fields()?;
        let id = transfer.id();
        if self.confirmed.contains_key(&id) || self.pending.iter().any(|row| row.id() == id) {
            return Ok(false);
        }
        anyhow::ensure!(
            self.pending.len() < MAX_NATIVE_PENDING_TRANSFERS,
            "native mempool is full"
        );
        self.pending_view()?.push(&transfer.envelope()?)?;
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
        self.pending.push(transfer);
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
            .template(producer_pk, reward_pk, timestamp_ms, &self.pending)
    }

    /// Verify a complete candidate independently, then atomically publish it
    /// only if cumulative work/tie-break wins. No declared balances are read.
    pub fn adopt_chain(&mut self, blocks: &[NativeBlock]) -> anyhow::Result<bool> {
        self.ensure_writable()?;
        anyhow::ensure!(
            blocks.len() <= MAX_NATIVE_HISTORY_BLOCKS,
            "native history block limit"
        );
        let mut bytes = 0u64;
        for block in blocks {
            bytes = bytes
                .checked_add(serde_json::to_vec(block)?.len() as u64 + 1)
                .ok_or_else(|| anyhow::anyhow!("native history size overflow"))?;
            anyhow::ensure!(
                bytes <= MAX_NATIVE_HISTORY_BYTES,
                "native history byte limit"
            );
        }
        let now = unix_time_ms()?;
        for block in blocks {
            check_block_ts_future_drift(block.header.timestamp_ms, now)?;
        }
        let candidate = NativeChain::replay(blocks)?;
        if !candidate.outranks(&self.chain) {
            return Ok(false);
        }
        let confirmed = confirmed_index(&candidate);
        let mut recovery = self.pending.clone();
        let mut seen: std::collections::BTreeSet<Hex32> =
            recovery.iter().map(NativeTransfer::id).collect();
        for block in self.chain.blocks() {
            for transfer in &block.transfers {
                let id = transfer.id();
                if recovery.len() < MAX_NATIVE_RECOVERY_TRANSFERS
                    && !confirmed.contains_key(&id)
                    && seen.insert(id)
                {
                    recovery.push(transfer.clone());
                }
            }
        }
        let retained = retain_pending(&candidate, &confirmed, &recovery)?;
        let lines: Vec<String> = candidate
            .blocks()
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<_, _>>()?;
        let publish = (|| -> anyhow::Result<()> {
            if recovery != self.pending {
                write_pool(&self.pool_path, &recovery)?;
                self.pool_stamp = file_stamp(&self.pool_path)?;
            }
            write_ndjson_lines_atomic(&self.block_path, &lines)?;
            self.block_stamp = file_stamp(&self.block_path)?;
            self.chain = candidate;
            self.confirmed = confirmed;
            write_pool(&self.pool_path, &retained)?;
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
        let retained = match retain_pending(&self.chain, &self.confirmed, &self.pending) {
            Ok(retained) => retained,
            Err(error) => {
                self.poisoned = true;
                return Err(error);
            }
        };
        if retained != self.pending {
            if let Err(error) = write_pool(&self.pool_path, &retained).and_then(|()| {
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
            self.pending = retained;
        }
        Ok(true)
    }
}

fn check_file_budget(path: &Path, limit: u64) -> anyhow::Result<u64> {
    let len = file_stamp(path)?.map_or(0, |stamp| stamp.len);
    anyhow::ensure!(len <= limit, "native state file exceeds byte limit");
    Ok(len)
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
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "native state path is not a regular file"
    );
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    #[cfg(unix)]
    anyhow::ensure!(metadata.nlink() == 1, "native state has a hard-link alias");
    Ok(Some(FileStamp {
        len: metadata.len(),
        modified: metadata.modified()?,
        #[cfg(unix)]
        identity: (metadata.dev(), metadata.ino()),
        #[cfg(unix)]
        change_time: (metadata.ctime(), metadata.ctime_nsec()),
    }))
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

fn retain_pending(
    chain: &NativeChain,
    confirmed: &BTreeMap<Hex32, u64>,
    candidates: &[NativeTransfer],
) -> anyhow::Result<Vec<NativeTransfer>> {
    let mut view = chain.ledger().pending_view()?;
    let mut retained = Vec::new();
    for transfer in candidates {
        if confirmed.contains_key(&transfer.id()) || retained.len() == MAX_NATIVE_PENDING_TRANSFERS
        {
            continue;
        }
        if view.push(&transfer.envelope()?).is_ok() {
            retained.push(transfer.clone());
        }
    }
    Ok(retained)
}

fn write_pool(path: &Path, pending: &[NativeTransfer]) -> anyhow::Result<()> {
    let lines: Vec<String> = pending
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<_, _>>()?;
    write_ndjson_lines_atomic(path, &lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::durability::{fail_next_append, AppendFault, PrivateTempDir};
    use boole_core::SigningKeyV2;

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
