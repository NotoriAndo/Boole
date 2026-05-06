use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use boole_core::PersistedBlock;

#[derive(Debug, Default)]
pub struct FileBlockStore {
    blocks: Vec<PersistedBlock>,
}

impl FileBlockStore {
    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw_bytes = fs::read(path)?;
        let stable_len = stable_jsonl_prefix_len(&raw_bytes);
        if stable_len < raw_bytes.len() {
            OpenOptions::new()
                .write(true)
                .open(path)?
                .set_len(stable_len as u64)?;
        }
        let raw = String::from_utf8(raw_bytes[..stable_len].to_vec())?;
        let mut blocks = Vec::new();
        for (i, line) in raw.lines().filter(|line| !line.is_empty()).enumerate() {
            let block: PersistedBlock = serde_json::from_str(line).map_err(|err| {
                anyhow::anyhow!("blockStore: line {} invalid JSON: {}", i + 1, err)
            })?;
            block.validate_shape()?;
            if block.height != i as u64 {
                anyhow::bail!(
                    "blockStore: line {} has height {}, expected {}",
                    i + 1,
                    block.height,
                    i
                );
            }
            if let Some(prev) = blocks.last() {
                let prev: &PersistedBlock = prev;
                if block.prev_c != prev.c {
                    anyhow::bail!(
                        "blockStore: line {} prevC {} does not match previous c {}",
                        i + 1,
                        block.prev_c,
                        prev.c
                    );
                }
            }
            blocks.push(block);
        }
        Ok(Self { blocks })
    }

    pub fn append(path: impl AsRef<Path>, block: &PersistedBlock) -> anyhow::Result<()> {
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{}", serde_json::to_string(block)?)?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    }

    pub fn blocks(&self) -> &[PersistedBlock] {
        &self.blocks
    }

    pub fn latest(&self) -> Option<&PersistedBlock> {
        self.blocks.last()
    }

    pub fn size(&self) -> usize {
        self.blocks.len()
    }
}

fn stable_jsonl_prefix_len(bytes: &[u8]) -> usize {
    if bytes.is_empty() || bytes.last() == Some(&b'\n') {
        return bytes.len();
    }
    bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .unwrap_or(0)
}
