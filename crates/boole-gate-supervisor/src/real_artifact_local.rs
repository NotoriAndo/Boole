//! Local content-address transport v1. Integrity is not release authentication:
//! this manifest never grants a production profile or replaces D/E certificates.
use super::{hash, readonly, stable, BLOCK, CHUNK};
use crate::{codec::digest, real_deadline::Deadline};
use anyhow::{ensure, Context, Result};
use boole_derivation::m2::Hash;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Profile {
    #[serde(rename = "Darwin-arm64")]
    DarwinArm64,
    #[serde(rename = "Linux-aarch64")]
    LinuxAarch64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Environment,
    ReferenceExport,
    CertifiedState,
    Certificate,
    Inventory,
    Runtime,
    Tool,
    Package,
    Decoder,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Content {
    bytes: u64,
    sha256: String,
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
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    assembled: Content,
    chunks: Vec<Chunk>,
    format: String,
    profile: Profile,
    role: Role,
    version: u8,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Binding {
    pub sha256: Hash,
    pub manifest: Hash,
    pub bytes: u64,
    pub profile: Profile,
    pub role: Role,
}
#[derive(Debug)]
pub struct HeldArtifact {
    binding: Binding,
    path: PathBuf,
    file: Arc<Mutex<File>>,
    before: fs::Metadata,
    max_read: usize,
}
impl HeldArtifact {
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn max_read(&self) -> usize {
        self.max_read
    }
    pub fn verify(&self, deadline: Deadline) -> Result<()> {
        self.verify_read_size(deadline).map(|_| ())
    }
    fn verify_read_size(&self, deadline: Deadline) -> Result<usize> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| anyhow::anyhow!("artifact handle unavailable"))?;
        stable(&self.path, &file, &self.before)?;
        #[cfg(test)]
        deadline_tests::verification_started(&self.path);
        file.seek(SeekFrom::Start(0))?;
        let mut sha = Sha256::new();
        let mut b = [0; BLOCK];
        let mut size = 0u64;
        let mut max_read = 0;
        let limit = self
            .binding
            .bytes
            .checked_add(1)
            .context("artifact verification bound overflow")?;
        loop {
            let remaining = limit.saturating_sub(size);
            if remaining == 0 {
                break;
            }
            let n = deadline.read(&mut *file, &mut b[..remaining.min(BLOCK as u64) as usize])?;
            if n == 0 {
                break;
            }
            sha.update(&b[..n]);
            #[cfg(test)]
            deadline_tests::VERIFY_BYTES.with(|v| v.set(v.get() + n));
            max_read = max_read.max(n);
            size = size
                .checked_add(n as u64)
                .context("artifact size overflow")?;
        }
        stable(&self.path, &file, &self.before)?;
        ensure!(
            size == self.binding.bytes && Hash::from(sha.finalize()) == self.binding.sha256,
            "held artifact content changed"
        );
        Ok(max_read)
    }
    /// Independent cursors share a read-only descriptor; no pathname reopen and
    /// no caller can obtain a writable descriptor through this API.
    pub fn reader(&self, deadline: Deadline) -> Result<HeldReader> {
        self.verify(deadline)?;
        Ok(HeldReader {
            file: self.file.clone(),
            path: self.path.clone(),
            before: self.before.clone(),
            cursor: 0,
        })
    }
}
pub struct HeldReader {
    file: Arc<Mutex<File>>,
    path: PathBuf,
    before: fs::Metadata,
    cursor: u64,
}
impl Read for HeldReader {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        let mut f = self
            .file
            .lock()
            .map_err(|_| std::io::Error::other("artifact handle unavailable"))?;
        stable(&self.path, &f, &self.before).map_err(std::io::Error::other)?;
        f.seek(SeekFrom::Start(self.cursor))?;
        let n = f.read(b)?;
        stable(&self.path, &f, &self.before).map_err(std::io::Error::other)?;
        self.cursor = self
            .cursor
            .checked_add(n as u64)
            .ok_or_else(|| std::io::Error::other("artifact cursor overflow"))?;
        Ok(n)
    }
}
pub struct ResolverV1 {
    pub root: PathBuf,
    pub manifests: BTreeMap<Hash, Hash>,
    pub profile: String,
}
impl ResolverV1 {
    pub fn resolve(&self, id: Hash, output: &Path, deadline: Deadline) -> Result<HeldArtifact> {
        deadline.check()?;
        let profile = match self.profile.as_str() {
            "Darwin-arm64" => Profile::DarwinArm64,
            "Linux-aarch64" => Profile::LinuxAarch64,
            _ => anyhow::bail!("unsupported local artifact profile"),
        };
        let root = fs::canonicalize(&self.root)?;
        let output = fs::canonicalize(output)?;
        let directory = boole_lean_runner::artifact_handles::private_directory(&output)?;
        loop {
            deadline.check()?;
            match directory.try_lock() {
                Ok(()) => break,
                Err(fs::TryLockError::WouldBlock) => {
                    std::thread::sleep(std::time::Duration::from_millis(1))
                }
                Err(fs::TryLockError::Error(e)) => return Err(e.into()),
            }
        }
        let pin = *self
            .manifests
            .get(&id)
            .context("missing local artifact manifest")?;
        let metadata = root.join("objects").join(hex::encode_pin(pin));
        let (mut f, before) = readonly(&metadata)?;
        ensure!(before.len() <= 16 * 1024 * 1024, "local manifest byte cap");
        let mut raw = Vec::new();
        let mut b = [0; BLOCK];
        loop {
            let n = deadline.read(&mut f, &mut b)?;
            if n == 0 {
                break;
            }
            ensure!(raw.len() + n <= 16 * 1024 * 1024, "local manifest grew");
            raw.extend_from_slice(&b[..n]);
        }
        stable(&metadata, &f, &before)?;
        ensure!(digest(&raw) == pin, "local manifest pin");
        let m: Manifest = serde_json::from_slice(&raw)?;
        let mut canonical = serde_json::to_vec(&m)?;
        canonical.push(b'\n');
        ensure!(
            raw == canonical && m.format == "BOOLE-M3/LOCAL-ARTIFACT/v1" && m.version == 1,
            "local artifact version/canonical grammar"
        );
        ensure!(m.profile == profile, "local artifact profile");
        ensure!(hash(&m.assembled.sha256)? == id, "local assembled identity");
        ensure!(m.chunks.len() <= 100000, "local chunk count cap");
        let mut offset = 0u64;
        for (i, c) in m.chunks.iter().enumerate() {
            hash(&c.sha256)?;
            ensure!(
                c.index == i as u64
                    && c.offset == offset
                    && c.bytes > 0
                    && c.bytes <= CHUNK
                    && (i + 1 == m.chunks.len() || c.bytes == CHUNK)
                    && c.object == format!("chunks/{}", c.sha256),
                "local chunk order/offset/size/address"
            );
            offset = offset
                .checked_add(c.bytes)
                .context("local chunk size overflow")?;
        }
        ensure!(offset == m.assembled.bytes, "local assembled length");
        let binding = Binding {
            sha256: id,
            manifest: pin,
            bytes: offset,
            profile,
            role: m.role,
        };
        let target = output.join(hex::encode_pin(id));
        let stage = output.join(format!(".local-artifact-{}.stage", hex::encode_pin(id)));
        retire_partial_stage(&stage, &directory)?;
        if target.try_exists()? {
            return open_held(target, binding, 0, deadline, None);
        }
        let mut out = boole_lean_runner::artifact_handles::create_private(&stage)?;
        #[cfg(test)]
        deadline_tests::stage_created(&stage);
        let result = (|| {
            stage_matches(&stage, &out)?;
            let mut assembled = Sha256::new();
            let mut max_read = 0;
            for c in &m.chunks {
                let path = root.join(&c.object);
                let (mut input, before) = readonly(&path)?;
                ensure!(before.len() == c.bytes, "local chunk length");
                let mut remaining = c.bytes;
                let mut ch = Sha256::new();
                let mut b = [0; BLOCK];
                while remaining > 0 {
                    let n = deadline
                        .read(&mut input, &mut b[..remaining.min(BLOCK as u64) as usize])?;
                    ensure!(n > 0, "local chunk truncated");
                    ch.update(&b[..n]);
                    assembled.update(&b[..n]);
                    out.write_all(&b[..n])?;
                    deadline.check()?;
                    remaining -= n as u64;
                    max_read = max_read.max(n);
                }
                ensure!(
                    deadline.read(&mut input, &mut b[..1])? == 0,
                    "local chunk grew"
                );
                stable(&path, &input, &before)?;
                ensure!(
                    Hash::from(ch.finalize()) == hash(&c.sha256)?,
                    "local chunk digest"
                );
            }
            ensure!(
                Hash::from(assembled.finalize()) == id,
                "local assembled digest"
            );
            out.flush()?;
            out.sync_all()?;
            out.set_permissions(readonly_permissions())?;
            out.sync_all()?;
            deadline.check()?;
            stage_matches(&stage, &out)?;
            fs::hard_link(&stage, &target)?;
            directory.sync_all()?;
            stage_matches(&target, &out)?;
            #[cfg(test)]
            deadline_tests::publication_paused();
            stage_matches(&stage, &out)?;
            fs::remove_file(&stage)?;
            directory.sync_all()?;
            // The directory lock covers publication and unlink. Hash only the
            // assembly stream, then bind the held read-only descriptor to that
            // exact writer inode after its last metadata mutation.
            open_held(target, binding, max_read, deadline, Some(&out))
        })();
        match result {
            Ok(artifact) => Ok(artifact),
            Err(error) => {
                // Cleanup cannot change the original stop's retry class. Keep
                // the typed cause even if the filesystem is failing as well.
                Err(if preserve_staging(&out, &directory).is_err() {
                    error.context("artifact staging cleanup also unavailable")
                } else {
                    error
                })
            }
        }
    }
}
fn retire_partial_stage(stage: &Path, directory: &File) -> Result<()> {
    match fs::symlink_metadata(stage) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
        Ok(_) => (),
    }
    let old = boole_lean_runner::artifact_handles::open_regular(stage)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = old.metadata()?;
        ensure!(
            meta.uid() == directory.metadata()?.uid()
                && matches!(meta.permissions().mode() & 0o7777, 0o400 | 0o600),
            "foreign partial artifact stage"
        );
    }
    stage_matches(stage, &old)?;
    fs::remove_file(stage)?;
    directory.sync_all()?;
    Ok(())
}
fn preserve_staging(stage: &File, output: &File) -> Result<()> {
    #[cfg(test)]
    ensure!(
        !deadline_tests::FAIL_CLEANUP.with(|v| v.get()),
        "fixture cleanup I/O unavailable"
    );
    stage.set_permissions(readonly_permissions())?;
    stage.sync_all()?;
    output.sync_all()?;
    Ok(())
}
fn readonly_permissions() -> fs::Permissions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::Permissions::from_mode(0o400)
    }
    #[cfg(not(unix))]
    {
        unreachable!("Unix artifact handles required")
    }
}
fn stage_matches(path: &Path, file: &File) -> Result<()> {
    let named = fs::symlink_metadata(path)?;
    let held = file.metadata()?;
    ensure!(
        named.is_file() && !named.file_type().is_symlink() && super::same(&named, &held),
        "artifact staging pathname changed"
    );
    Ok(())
}
fn open_held(
    path: PathBuf,
    binding: Binding,
    max_read: usize,
    deadline: Deadline,
    assembled: Option<&File>,
) -> Result<HeldArtifact> {
    let (file, before) = readonly(&path)?;
    ensure!(before.permissions().readonly(), "local object is writable");
    ensure!(before.len() == binding.bytes, "local object length");
    if let Some(writer) = assembled {
        ensure!(
            super::same(&before, &writer.metadata()?),
            "published artifact differs from verified assembly handle"
        );
    }
    let mut held = HeldArtifact {
        binding,
        path,
        file: Arc::new(Mutex::new(file)),
        before,
        max_read,
    };
    if assembled.is_none() {
        held.max_read = held.verify_read_size(deadline)?;
    }
    deadline.check()?;
    Ok(held)
}
// Keep the ordinary lowercase content address codec independent of a dev dependency.
mod hex {
    pub(super) fn encode_pin(h: [u8; 32]) -> String {
        boole_derivation::attributes::hex_string(&h)[2..].to_owned()
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;
    thread_local! { pub(super) static FAIL_CLEANUP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    thread_local! { static STAGE_SWAP: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) }; }
    thread_local! { static PUBLICATION_PAUSE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) }; }
    thread_local! { static VERIFY_GROW: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; pub(super) static VERIFY_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
    #[cfg(unix)]
    pub(super) fn verification_started(path: &Path) {
        if VERIFY_GROW.with(|v| v.replace(false)) {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
            let mut f = fs::OpenOptions::new().append(true).open(path).unwrap();
            f.write_all(&vec![0; 1024 * 1024]).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o400)).unwrap();
        }
    }
    #[cfg(not(unix))]
    pub(super) fn verification_started(_path: &Path) {}
    #[test]
    fn new_publication_reads_one_stream_and_fast_path_reports_its_read_size() -> Result<()> {
        let (root, r, id) = fixture()?;
        let (result, reads) = crate::real_deadline::slow_reads(usize::MAX, 0, || {
            r.resolve(id, &root.0.join("out"), Deadline::after(30000))
        });
        let a = result?;
        assert_eq!(reads, 4, "assembly was rehashed through another stream");
        assert_eq!(a.max_read(), 6);
        let b = r.resolve(id, &root.0.join("out"), Deadline::after(30000))?;
        assert_eq!(
            b.max_read(),
            6,
            "cached verification hid its actual read size"
        );
        Ok(())
    }
    #[test]
    #[cfg(unix)]
    fn held_verification_stops_at_expected_bytes_plus_one_after_growth() -> Result<()> {
        let (root, r, id) = fixture()?;
        let a = r.resolve(id, &root.0.join("out"), Deadline::after(30000))?;
        VERIFY_GROW.with(|v| v.set(true));
        VERIFY_BYTES.with(|v| v.set(0));
        assert!(a.verify(Deadline::after(30000)).is_err());
        assert_eq!(
            VERIFY_BYTES.with(|v| v.get()),
            7,
            "verify read beyond its bound"
        );
        Ok(())
    }
    pub(super) fn publication_paused() {
        if let Some(action) = PUBLICATION_PAUSE.with(|v| v.borrow_mut().take()) {
            action();
        }
    }
    #[test]
    fn concurrent_resolvers_return_handles_that_remain_valid_after_publication() -> Result<()> {
        use std::{sync::mpsc, thread, time::Duration};
        let (root, r, id) = fixture()?;
        let output = root.0.join("out");
        let second = ResolverV1 {
            root: r.root.clone(),
            manifests: r.manifests.clone(),
            profile: r.profile.clone(),
        };
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first_out = output.clone();
        let first = thread::spawn(move || {
            PUBLICATION_PAUSE.with(|v| {
                *v.borrow_mut() = Some(Box::new(move || {
                    ready_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                }))
            });
            r.resolve(id, &first_out, Deadline::after(30000))
        });
        ready_rx.recv_timeout(Duration::from_secs(5))?;
        let (second_tx, second_rx) = mpsc::channel();
        let other = thread::spawn(move || {
            second_tx
                .send(second.resolve(id, &output, Deadline::after(30000)))
                .unwrap();
        });
        // The old fast path returns a handle while the first writer still has
        // a staging alias. A locked implementation waits until publication ends.
        let early = second_rx.recv_timeout(Duration::from_millis(500));
        release_tx.send(())?;
        let a = first.join().unwrap()?;
        let b = match early {
            Ok(v) => v,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                second_rx.recv_timeout(Duration::from_secs(5))?
            }
            Err(e) => return Err(e.into()),
        }?;
        other.join().unwrap();
        a.verify(Deadline::after(30000))?;
        b.verify(Deadline::after(30000))?;
        Ok(())
    }
    #[test]
    fn repeated_transient_stops_retain_at_most_one_partial_stage_per_object() -> Result<()> {
        let (root, r, id) = fixture()?;
        let output = root.0.join("out");
        for _ in 0..8 {
            let (result, _) = crate::real_deadline::slow_reads(2, 50, || {
                r.resolve(id, &output, Deadline::after(30))
            });
            assert!(crate::real_deadline::is_transient(&result.unwrap_err()));
            let stages = fs::read_dir(&output)?
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().ends_with(".stage"))
                .count();
            assert!(
                stages <= 1,
                "transient retries accumulated {stages} abandoned stages"
            );
        }
        let good = r.resolve(id, &output, Deadline::after(30000))?;
        good.verify(Deadline::after(30000))?;
        assert!(!fs::read_dir(&output)?
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().ends_with(".stage")));
        Ok(())
    }
    pub(super) fn stage_created(stage: &Path) {
        if let Some(victim) = STAGE_SWAP.with(|v| v.borrow_mut().take()) {
            fs::remove_file(stage).unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink(victim, stage).unwrap();
        }
    }
    fn fixture() -> Result<(crate::codec::Scratch, ResolverV1, Hash)> {
        let root = crate::codec::Scratch::new(&std::env::temp_dir())?;
        for name in ["objects", "chunks", "out"] {
            fs::create_dir(root.0.join(name))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.0.join("out"), fs::Permissions::from_mode(0o700))?;
        }
        let raw = b"public";
        let id = digest(raw);
        let h = hex::encode_pin(id);
        fs::write(root.0.join("chunks").join(&h), raw)?;
        let m = serde_json::json!({"assembled":{"bytes":raw.len(),"sha256":h},"chunks":[{"bytes":raw.len(),"index":0,"object":format!("chunks/{h}"),"offset":0,"sha256":h}],"format":"BOOLE-M3/LOCAL-ARTIFACT/v1","profile":"Linux-aarch64","role":"reference-export","version":1});
        let mut bytes = serde_json::to_vec(&m)?;
        bytes.push(b'\n');
        let pin = digest(&bytes);
        fs::write(root.0.join("objects").join(hex::encode_pin(pin)), bytes)?;
        let resolver = ResolverV1 {
            root: root.0.clone(),
            manifests: BTreeMap::from([(id, pin)]),
            profile: "Linux-aarch64".into(),
        };
        Ok((root, resolver, id))
    }
    #[test]
    #[cfg(unix)]
    fn stage_symlink_swap_never_writes_or_chmods_the_victim() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let (root, r, id) = fixture()?;
        let victim = root.0.join("victim");
        fs::write(&victim, b"preserve")?;
        fs::set_permissions(&victim, fs::Permissions::from_mode(0o600))?;
        STAGE_SWAP.with(|v| *v.borrow_mut() = Some(victim.clone()));
        let result = r.resolve(id, &root.0.join("out"), Deadline::after(30000));
        assert!(result.is_err(), "swapped staging pathname must not publish");
        assert_eq!(
            fs::read(&victim)?,
            b"preserve",
            "staging path wrote into victim"
        );
        assert_eq!(
            fs::metadata(&victim)?.permissions().mode() & 0o777,
            0o600,
            "staging path chmod-ed victim"
        );
        Ok(())
    }
    #[test]
    fn slow_local_chunk_is_transient_and_never_publishes_a_partial_object() -> Result<()> {
        slow_failure(false)
    }
    #[test]
    fn cleanup_io_failure_preserves_the_first_physical_stop() -> Result<()> {
        slow_failure(true)
    }
    fn slow_failure(cleanup_fails: bool) -> Result<()> {
        let root = crate::codec::Scratch::new(&std::env::temp_dir())?;
        for n in ["objects", "chunks", "out"] {
            fs::create_dir(root.0.join(n))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root.0.join("out"), fs::Permissions::from_mode(0o700))?;
        }
        let raw = b"public";
        let id = digest(raw);
        let h = hex::encode_pin(id);
        fs::write(root.0.join("chunks").join(&h), raw)?;
        let value = serde_json::json!({"assembled":{"bytes":raw.len(),"sha256":h},"chunks":[{"bytes":raw.len(),"index":0,"object":format!("chunks/{h}"),"offset":0,"sha256":h}],"format":"BOOLE-M3/LOCAL-ARTIFACT/v1","profile":"Linux-aarch64","role":"reference-export","version":1});
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        let pin = digest(&bytes);
        fs::write(root.0.join("objects").join(hex::encode_pin(pin)), bytes)?;
        let resolver = ResolverV1 {
            root: root.0.clone(),
            manifests: BTreeMap::from([(id, pin)]),
            profile: "Linux-aarch64".into(),
        };
        FAIL_CLEANUP.with(|v| v.set(cleanup_fails));
        let (result, reads) = crate::real_deadline::slow_reads(2, 50, || {
            resolver.resolve(id, &root.0.join("out"), Deadline::after(30))
        });
        FAIL_CLEANUP.with(|v| v.set(false));
        let error = result.unwrap_err();
        assert!(crate::real_deadline::is_transient(&error));
        assert_eq!(reads, 3);
        assert!(!root.0.join("out").join(h).exists());
        Ok(())
    }
}
