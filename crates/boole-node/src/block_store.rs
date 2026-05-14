use std::path::Path;

use boole_core::PersistedBlock;

use crate::durability::{append_ndjson_line_durable, read_stable_prefix};

#[derive(Debug, Default)]
pub struct FileBlockStore {
    blocks: Vec<PersistedBlock>,
}

impl FileBlockStore {
    pub fn recover(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let Some(raw) = read_stable_prefix(path)? else {
            return Ok(Self::default());
        };
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
        append_ndjson_line_durable(path.as_ref(), &serde_json::to_string(block)?)
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
