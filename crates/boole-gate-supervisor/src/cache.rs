//! Authenticated node-local cache. Every hit is rebound to reconstructed inputs.
use crate::{
    codec::{digest, private_file, read_bounded, Scratch},
    GateInput, GateResult,
};
use anyhow::{ensure, Result};
use boole_derivation::m2::Hash;
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct CacheKey {
    pub gate: Hash,
    pub queue_index: u64,
    pub commit_hash: Hash,
    pub kind: u8,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    key: CacheKey,
    input_digest: Hash,
    epoch: u32,
    result: GateResult,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Signed {
    entry: Entry,
    mac: Vec<u8>,
}
pub struct Cache {
    root: PathBuf,
    key: hmac::Key,
}
impl Cache {
    pub fn new(root: &Path) -> Result<Self> {
        if !root.exists() {
            let mut b = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                b.mode(0o700);
            }
            b.create(root)?;
        }
        ensure!(
            std::fs::symlink_metadata(root)?.is_dir()
                && !std::fs::symlink_metadata(root)?.file_type().is_symlink(),
            "cache directory type"
        );
        let root = std::fs::canonicalize(root)?;
        let key_path = root.join("cache.key");
        if !key_path.exists() {
            let mut bytes = [0; 32];
            SystemRandom::new()
                .fill(&mut bytes)
                .map_err(|_| anyhow::anyhow!("cache key randomness"))?;
            private_file(&key_path, &bytes)?;
        }
        let bytes = read_bounded(&key_path, 32)?;
        ensure!(bytes.len() == 32, "cache key size");
        Ok(Self {
            root,
            key: hmac::Key::new(hmac::HMAC_SHA256, &bytes),
        })
    }
    pub fn entry_path(&self, key: &CacheKey) -> PathBuf {
        self.root.join(format!(
            "{}.json",
            boole_derivation::attributes::hex_string(&digest(
                &serde_json::to_vec(key).expect("cache key")
            ))
        ))
    }
    pub fn load(&self, key: &CacheKey, input: &GateInput, epoch: u32) -> Option<GateResult> {
        let bytes = read_bounded(&self.entry_path(key), 65536).ok()?;
        let signed: Signed = serde_json::from_slice(&bytes).ok()?;
        hmac::verify(
            &self.key,
            &serde_json::to_vec(&signed.entry).ok()?,
            &signed.mac,
        )
        .ok()?;
        if &signed.entry.key != key
            || signed.entry.epoch != epoch
            || signed.entry.input_digest != digest(&serde_json::to_vec(input).ok()?)
        {
            return None;
        }
        Some(signed.entry.result)
    }
    pub fn save(
        &self,
        key: &CacheKey,
        input: &GateInput,
        epoch: u32,
        result: &GateResult,
    ) -> Result<()> {
        let entry = Entry {
            key: key.clone(),
            input_digest: digest(&serde_json::to_vec(input)?),
            epoch,
            result: result.clone(),
        };
        let mac = hmac::sign(&self.key, &serde_json::to_vec(&entry)?)
            .as_ref()
            .to_vec();
        let signed = Signed { entry, mac };
        let temp = Scratch::new(&self.root)?;
        let path = temp.0.join("entry");
        private_file(&path, &serde_json::to_vec(&signed)?)?;
        std::fs::rename(path, self.entry_path(key))?;
        Ok(())
    }
}
