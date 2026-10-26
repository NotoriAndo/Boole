//! Streaming D4 materialization for Mac development; the draft is not a seal.
use crate::codec::{digest, private_file};
use anyhow::{ensure, Context, Result};
use boole_derivation::m2::Hash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};
const CHUNK: u64 = 67_108_864;
const BLOCK: usize = 65_536;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolver {
    pub root: PathBuf,
    #[serde(with = "manifest_map")]
    pub manifests: BTreeMap<Hash, Hash>,
}
#[derive(Debug)]
pub struct Artifact {
    pub path: PathBuf,
    pub sha256: Hash,
    pub bytes: u64,
    pub max_read: usize,
}
impl Artifact {
    pub fn open(&self) -> Result<File> {
        let (file, meta) = readonly(&self.path)?;
        ensure!(meta.len() == self.bytes, "artifact size changed");
        Ok(file)
    }
    pub fn verify(&self) -> Result<()> {
        self.verify_until(crate::real_deadline::Deadline::after(30000))
    }
    pub fn verify_until(&self, deadline: crate::real_deadline::Deadline) -> Result<()> {
        deadline.check()?;
        let (mut f, before) = readonly(&self.path)?;
        let mut h = Sha256::new();
        let mut b = [0; BLOCK];
        loop {
            let n = deadline.read(&mut f, &mut b)?;
            if n == 0 {
                break;
            }
            h.update(&b[..n]);
        }
        stable(&self.path, &f, &before)?;
        ensure!(
            Hash::from(h.finalize()) == self.sha256,
            "artifact changed after verification"
        );
        Ok(())
    }
}
// D4 metadata canonicalizes object keys in sorted order, unlike the real gate's
// explicitly ordered descriptor. These structs follow that sorted wire order.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    activation: bool,
    assembled: Assembled,
    authentication: Pending,
    chunks: Vec<Chunk>,
    format: String,
    max_chunk_bytes: u64,
    profile: String,
    role: String,
    seal: String,
    status: String,
    storage: String,
    storage_reason: String,
    version: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Assembled {
    bytes: u64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    reason: String,
    value: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Chunk {
    bytes: u64,
    index: u64,
    object: String,
    offset: u64,
    sha256: String,
}
fn hash(text: &str) -> Result<Hash> {
    ensure!(
        text.len() == 64
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "D4 SHA grammar"
    );
    let mut out = [0; 32];
    for (i, n) in out.iter_mut().enumerate() {
        *n = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)?;
    }
    Ok(out)
}
#[cfg(unix)]
fn same(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    (
        a.dev(),
        a.ino(),
        a.len(),
        a.mtime(),
        a.mtime_nsec(),
        a.ctime(),
        a.ctime_nsec(),
    ) == (
        b.dev(),
        b.ino(),
        b.len(),
        b.mtime(),
        b.mtime_nsec(),
        b.ctime(),
        b.ctime_nsec(),
    )
}
#[cfg(not(unix))]
fn same(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.len() == b.len() && a.modified().ok() == b.modified().ok()
}
fn readonly(path: &Path) -> Result<(File, fs::Metadata)> {
    ensure!(fs::canonicalize(path)? == path, "artifact symlink/alias");
    let before = fs::symlink_metadata(path)?;
    ensure!(
        before.is_file() && !before.file_type().is_symlink(),
        "artifact file type"
    );
    let file = File::open(path)?;
    ensure!(
        same(&before, &file.metadata()?),
        "artifact changed while opening"
    );
    Ok((file, before))
}
fn stable(path: &Path, file: &File, before: &fs::Metadata) -> Result<()> {
    ensure!(
        fs::canonicalize(path)? == path
            && same(before, &file.metadata()?)
            && same(before, &fs::symlink_metadata(path)?),
        "artifact changed while reading"
    );
    Ok(())
}
fn readonly_mode(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o400))?;
    }
    Ok(())
}
impl Resolver {
    pub fn resolve(&self, id: Hash, output: &Path) -> Result<Artifact> {
        self.resolve_until(id, output, crate::real_deadline::Deadline::after(600000))
    }
    pub fn resolve_until(
        &self,
        id: Hash,
        output: &Path,
        deadline: crate::real_deadline::Deadline,
    ) -> Result<Artifact> {
        deadline.check()?;
        let root = fs::canonicalize(&self.root)?;
        let output = fs::canonicalize(output)?;
        let pin = self
            .manifests
            .get(&id)
            .context("missing artifact manifest pin")?;
        let path = root.join("objects").join(hex(pin));
        let (mut source, meta) = readonly(&path)?;
        ensure!(meta.len() <= 16 * 1024 * 1024, "D4 metadata cap");
        let text = deadline.read_bounded(&path, 16 * 1024 * 1024)?;
        // Recheck the independently opened metadata handle and pathname too.
        let mut actual = vec![];
        let mut buf = [0; 65536];
        loop {
            let n = deadline.read(&mut source, &mut buf)?;
            if n == 0 {
                break;
            }
            ensure!(actual.len() + n <= 16 * 1024 * 1024, "metadata grew");
            actual.extend_from_slice(&buf[..n]);
        }
        ensure!(
            text == actual && digest(&text) == *pin,
            "D4 outer manifest pin"
        );
        let manifest: Manifest = serde_json::from_slice(&text)?;
        let mut canonical = serde_json::to_vec(&manifest)?;
        canonical.push(b'\n');
        ensure!(canonical == text, "D4 canonical manifest");
        ensure!(
            manifest.format == "EENV15-chunk-manifest-draft-v1"
                && manifest.version == 1
                && manifest.status == "draft"
                && manifest.seal == "unsealed"
                && !manifest.activation
                && manifest.profile == "Darwin-arm64",
            "D4 draft/profile header"
        );
        ensure!(
            manifest.storage == "manifest-only"
                && !manifest.storage_reason.is_empty()
                && manifest.authentication.value.is_none()
                && manifest.authentication.reason
                    == "D7: reviewed D/N release authentication and certificate issuance pending",
            "D4 draft authentication"
        );
        ensure!(
            manifest.role.len() <= 64
                && manifest
                    .role
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_lowercase)
                && manifest
                    .role
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "D4 role"
        );
        ensure!(
            manifest.max_chunk_bytes == CHUNK
                && manifest.chunks.len() <= 100000
                && hash(&manifest.assembled.sha256)? == id,
            "D4 chunk limit/assembled identity"
        );
        let mut offset = 0u64;
        for (i, c) in manifest.chunks.iter().enumerate() {
            hash(&c.sha256)?;
            ensure!(
                c.index == i as u64
                    && c.offset == offset
                    && c.bytes > 0
                    && c.bytes <= CHUNK
                    && (i + 1 == manifest.chunks.len() || c.bytes == CHUNK)
                    && c.object == format!("chunks/{}", c.sha256),
                "D4 chunk order/offset/size/address"
            );
            offset = offset.checked_add(c.bytes).context("D4 size overflow")?;
        }
        ensure!(offset == manifest.assembled.bytes, "D4 assembled size");
        stable(&path, &source, &meta)?;
        let mut nonce = [0; 32];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce)
            .map_err(|_| anyhow::anyhow!("D4 staging randomness"))?;
        let staging = output.join(format!(".d4-{}.stage", hex(&nonce)));
        private_file(&staging, &[])?;
        let attempted = (|| -> Result<Artifact> {
            let mut out = File::options().write(true).open(&staging)?;
            let mut full = Sha256::new();
            let mut max_read = 0;
            // Exactly one read-only chunk handle and one 64 KiB buffer, regardless of
            // assembled size. Artifacts never enter GateInput's Vec or worker stdin.
            for c in &manifest.chunks {
                deadline.check()?;
                let p = root.join(&c.object);
                let (mut f, before) = readonly(&p)?;
                ensure!(before.len() == c.bytes, "D4 chunk file size");
                let mut ch = Sha256::new();
                let mut remaining = c.bytes;
                let mut b = [0; BLOCK];
                while remaining > 0 {
                    let n =
                        deadline.read(&mut f, &mut b[..(remaining.min(BLOCK as u64) as usize)])?;
                    ensure!(n > 0, "D4 truncated chunk");
                    max_read = max_read.max(n);
                    ch.update(&b[..n]);
                    full.update(&b[..n]);
                    out.write_all(&b[..n])?;
                    deadline.check()?;
                    remaining -= n as u64;
                }
                ensure!(deadline.read(&mut f, &mut b[..1])? == 0, "D4 chunk grew");
                stable(&p, &f, &before)?;
                ensure!(
                    Hash::from(ch.finalize()) == hash(&c.sha256)?,
                    "D4 chunk digest"
                );
            }
            ensure!(Hash::from(full.finalize()) == id, "D4 assembled digest");
            out.flush()?;
            out.sync_all()?;
            deadline.check()?;
            readonly_mode(&staging)?;
            out.sync_all()?;
            let target = output.join(hex(&id));
            deadline.check()?;
            fs::hard_link(&staging, &target)?;
            File::open(&output)?.sync_all()?;
            Ok(Artifact {
                path: target,
                sha256: id,
                bytes: offset,
                max_read,
            })
        })();
        // Failed staging is read-only unpublished evidence; successful publication is
        // a second hard link, with no overwrite and no duplicate data allocation.
        if attempted.is_err() {
            readonly_mode(&staging)?;
            File::open(&output)?.sync_all()?;
        }
        attempted
    }
}
fn hex(hash: &Hash) -> String {
    boole_derivation::attributes::hex_string(hash)[2..].into()
}

mod manifest_map {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        value: &BTreeMap<Hash, Hash>,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        value
            .iter()
            .map(|(k, v)| (hex(k), hex(v)))
            .collect::<BTreeMap<_, _>>()
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<BTreeMap<Hash, Hash>, D::Error> {
        let values = BTreeMap::<String, String>::deserialize(deserializer)?;
        values
            .into_iter()
            .map(|(k, v)| {
                Ok((
                    hash(&k).map_err(serde::de::Error::custom)?,
                    hash(&v).map_err(serde::de::Error::custom)?,
                ))
            })
            .collect()
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;
    #[test]
    fn d4_chunk_slow_read_expires_the_original_deadline_without_publication() -> Result<()> {
        let root = crate::codec::Scratch::new(&std::env::temp_dir())?;
        for name in ["objects", "chunks", "out"] {
            fs::create_dir(root.0.join(name))?;
        }
        let raw = b"data";
        let id = digest(raw);
        let idhex = hex(&id);
        fs::write(root.0.join("chunks").join(&idhex), raw)?;
        let v = serde_json::json!({"activation":false,"assembled":{"bytes":4,"sha256":idhex},"authentication":{"reason":"D7: reviewed D/N release authentication and certificate issuance pending","value":null},"chunks":[{"bytes":4,"index":0,"object":format!("chunks/{}",hex(&id)),"offset":0,"sha256":hex(&id)}],"format":"EENV15-chunk-manifest-draft-v1","max_chunk_bytes":CHUNK,"profile":"Darwin-arm64","role":"reference-export","seal":"unsealed","status":"draft","storage":"manifest-only","storage_reason":"fixture","version":1});
        let mut manifest = serde_json::to_vec(&v)?;
        manifest.push(b'\n');
        let pin = digest(&manifest);
        fs::write(root.0.join("objects").join(hex(&pin)), manifest)?;
        let resolver = Resolver {
            root: root.0.clone(),
            manifests: BTreeMap::from([(id, pin)]),
        };
        let out = root.0.join("out");
        let (result, reads) = crate::real_deadline::slow_reads(4, 300, || {
            resolver.resolve_until(id, &out, crate::real_deadline::Deadline::after(200))
        });
        let error = result.unwrap_err();
        ensure!(error.is::<crate::real_deadline::Expired>(), "{error:#}");
        ensure!(reads == 5, "did not reach the chunk reader: {reads}");
        ensure!(
            !out.join(hex(&id)).exists(),
            "expired materialization was published"
        );
        Ok(())
    }
}
