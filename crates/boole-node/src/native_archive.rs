//! Offline recovery trusts verified blocks, never an archive's claimed balances.
//! Archives contain the canonical native NDJSON block format, starting at genesis.

use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use boole_core::native_chain::{NativeBlock, NativeChain};
use boole_core::native_network::native_testnet;
use serde::Serialize;

use crate::durability::PrivateTempDir;
use crate::native_node::{unix_time_ms, MAX_NATIVE_HISTORY_BLOCKS, MAX_NATIVE_HISTORY_BYTES};
use crate::runtime::check_block_ts_future_drift;
use crate::NativeNode;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeArchiveReceipt {
    pub network_id: String,
    pub genesis_hash: String,
    pub head_hash: String,
    pub height: u64,
    pub bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeImportReceipt {
    pub adopted: bool,
    #[serde(flatten)]
    pub archive: NativeArchiveReceipt,
}

struct VerifiedArchive {
    chain: NativeChain,
    receipt: NativeArchiveReceipt,
}

impl VerifiedArchive {
    fn read(path: &Path, expected_head: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            expected_head.len() == 64
                && expected_head
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "expected head must be a canonical lowercase 32-byte hash"
        );
        let parent = Parent::open(path)?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        let before = check_file(path, &file)?;
        let mut reader = BufReader::new(&file);
        let mut chain = NativeChain::new()?;
        let now = unix_time_ms()?;
        let mut bytes = 0u64;
        let mut line = Vec::new();
        loop {
            line.clear();
            let count = (&mut reader)
                .take(native_testnet().max_block_bytes() as u64 + 2)
                .read_until(b'\n', &mut line)?;
            if count == 0 {
                break;
            }
            bytes = bytes
                .checked_add(count as u64)
                .ok_or_else(|| anyhow::anyhow!("archive size overflow"))?;
            anyhow::ensure!(
                bytes <= MAX_NATIVE_HISTORY_BYTES,
                "native archive byte limit"
            );
            anyhow::ensure!(
                chain.blocks().len() < MAX_NATIVE_HISTORY_BLOCKS,
                "native archive block limit"
            );
            anyhow::ensure!(
                line.last() == Some(&b'\n'),
                "native archive has an incomplete or oversized line"
            );
            line.pop();
            anyhow::ensure!(
                line.len() <= native_testnet().max_block_bytes(),
                "native archive block line too large"
            );
            let block: NativeBlock = serde_json::from_slice(&line).map_err(|e| {
                anyhow::anyhow!("native archive line {}: {e}", chain.blocks().len() + 1)
            })?;
            check_block_ts_future_drift(block.header.timestamp_ms, now)?;
            chain.append(block)?;
        }
        let after = check_file(path, &file)?;
        anyhow::ensure!(
            same_version(&before, &after) && bytes == before.len(),
            "native archive changed while reading"
        );
        parent.check()?;
        anyhow::ensure!(
            chain.head_hash().to_hex() == expected_head,
            "native archive does not match expected head"
        );
        let receipt = receipt(&chain, bytes);
        Ok(Self { chain, receipt })
    }
}

/// Uses exclusive node ownership without repairing journals or upgrading the
/// manifest. Stop the node first; damaged source bytes remain untouched.
pub fn export_native_archive(
    state_dir: &Path,
    output: &Path,
) -> anyhow::Result<NativeArchiveReceipt> {
    let parent = Parent::open(output)?;
    anyhow::ensure!(!output.try_exists()?, "archive output already exists");
    let manifest = state_dir.join(crate::state_dir::STATE_MANIFEST_FILE);
    let source_directory = Parent::open(&manifest)?;
    anyhow::ensure!(
        !parent
            .path
            .canonicalize()?
            .starts_with(state_dir.canonicalize()?),
        "archive output must be outside the source state directory"
    );
    anyhow::ensure!(
        fs::symlink_metadata(&manifest)?.is_file(),
        "export requires an existing node manifest"
    );
    let node = NativeNode::open_preserving(state_dir)?;
    node.ensure_ready()?;
    let workspace = PrivateTempDir::new_in(&parent.path, ".boole-native-export")?;
    let tmp = workspace.path().join("blocks.ndjson");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&tmp)?;
    let mut writer = BufWriter::new(&file);
    let mut bytes = 0u64;
    for block in node.chain().blocks() {
        let line = serde_json::to_vec(block)?;
        bytes += line.len() as u64 + 1;
        anyhow::ensure!(
            bytes <= MAX_NATIVE_HISTORY_BYTES,
            "native archive byte limit"
        );
        writer.write_all(&line)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    file.sync_all()?;
    let verified = VerifiedArchive::read(&tmp, &node.chain().head_hash().to_hex())?;
    node.ensure_ready()?;
    parent.check()?;
    // hard_link is create-if-absent; no rename that could replace an operator file.
    fs::hard_link(&tmp, output)?;
    fs::remove_file(&tmp)?;
    parent.check()?;
    parent.held.sync_all()?;
    check_file(output, &file)?;
    node.ensure_ready()?;
    source_directory.check()?;
    Ok(verified.receipt)
}

/// Verify the entire stable source before opening the destination. Import never
/// replaces an invalid destination: preserve it and choose a fresh state directory.
pub fn import_native_archive(
    state_dir: &Path,
    blocks: &Path,
    expected_head: &str,
) -> anyhow::Result<NativeImportReceipt> {
    let verified = VerifiedArchive::read(blocks, expected_head)?;
    let mut node = NativeNode::open(state_dir)?;
    let same_head = node.chain().head_hash() == verified.chain.head_hash();
    anyhow::ensure!(
        same_head || verified.chain.outranks(node.chain()),
        "archive does not outrank the current verified chain"
    );
    let adopted = if same_head {
        false
    } else {
        node.adopt_replayed_chain(verified.chain)?
    };
    node.ensure_ready()?;
    anyhow::ensure!(
        node.chain().head_hash().to_hex() == verified.receipt.head_hash,
        "native archive publication did not reach expected head"
    );
    Ok(NativeImportReceipt {
        adopted,
        archive: verified.receipt,
    })
}

fn receipt(chain: &NativeChain, bytes: u64) -> NativeArchiveReceipt {
    NativeArchiveReceipt {
        network_id: native_testnet().network_id().to_owned(),
        genesis_hash: native_testnet().genesis_hash().to_hex(),
        head_hash: chain.head_hash().to_hex(),
        height: chain.ledger().height(),
        bytes,
    }
}

struct Parent {
    path: PathBuf,
    held: File,
}
impl Parent {
    fn open(path: &Path) -> anyhow::Result<Self> {
        let path = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let held = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        let parent = Self {
            path: path.to_owned(),
            held,
        };
        parent.check()?;
        Ok(parent)
    }
    fn check(&self) -> anyhow::Result<()> {
        let held = self.held.metadata()?;
        let current = fs::symlink_metadata(&self.path)?;
        anyhow::ensure!(current.is_dir() && held.mode() & 0o022 == 0 && same_file(&held, &current),
            "archive parent must be a stable non-symlink directory without group/world write access");
        Ok(())
    }
}
fn same_file(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}
fn same_version(a: &Metadata, b: &Metadata) -> bool {
    same_file(a, b)
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}
fn check_file(path: &Path, file: &File) -> anyhow::Result<Metadata> {
    let held = file.metadata()?;
    let current = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        held.is_file()
            && current.is_file()
            && held.nlink() == 1
            && held.mode() & 0o022 == 0
            && held.len() <= MAX_NATIVE_HISTORY_BYTES
            && same_file(&held, &current),
        "archive must remain one bounded regular single-link file without group/world write access"
    );
    Ok(held)
}
