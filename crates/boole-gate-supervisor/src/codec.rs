//! Pinned local codec; no decoder runs inside derivation.
use anyhow::{bail, ensure, Context, Result};
use boole_derivation::{attributes::hex_string, m2::RAW_CAP};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct InvalidData;
impl std::fmt::Display for InvalidData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid encoded data")
    }
}
impl std::error::Error for InvalidData {}

pub fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
pub fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= max as u64,
        "file type or size cap"
    );
    let mut bytes = vec![];
    File::open(path)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= max, "file grew beyond size cap");
    Ok(bytes)
}
pub fn private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
pub fn run_bounded(command: &mut Command, timeout: Duration) -> Result<()> {
    let mut child = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("start pinned local tool")?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            if status.code() == Some(1) {
                return Err(InvalidData.into());
            }
            ensure!(status.success(), "untyped local tool exit");
            return Ok(());
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("local tool timed out");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
pub struct Scratch(pub PathBuf);
impl Scratch {
    pub fn new(root: &Path) -> Result<Self> {
        let mut nonce = [0; 32];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce)
            .map_err(|_| anyhow::anyhow!("scratch randomness"))?;
        let path = root.join(format!("scratch-{}", hex_string(&nonce)));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone)]
pub struct Codec {
    path: Option<PathBuf>,
}
impl Codec {
    /// The devnet stub can use standards-compliant raw frames without a host tool.
    /// Other frames need the pinned full decoder; absence is an operator fault.
    pub fn portable_stub() -> Self {
        Self { path: None }
    }

    pub fn new(path: PathBuf) -> Result<Self> {
        let c = Self {
            path: Some(fs::canonicalize(path)?),
        };
        c.verify()?;
        Ok(c)
    }
    fn binary_snapshot(&self) -> Result<Vec<u8>> {
        let path = self
            .path
            .as_ref()
            .context("full pinned decoder unavailable")?;
        let pin: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/devnet-m2/zstd-pin-v1.json"))?;
        let bytes = read_bounded(path, 128 * 1024 * 1024)?;
        ensure!(
            hex_string(&digest(&bytes)).trim_start_matches("0x")
                == pin["sha256"].as_str().unwrap_or(""),
            "codec executable digest mismatch"
        );
        Ok(bytes)
    }
    // The frozen Darwin executable uses @loader_path/../lib. Bind that existing
    // runtime artifact too, then preserve its layout inside private scratch.
    #[cfg(target_os = "macos")]
    fn library_snapshot(&self) -> Result<Vec<u8>> {
        let path = self.path.as_ref().context("full decoder unavailable")?;
        let library = path
            .parent()
            .and_then(Path::parent)
            .context("decoder installation layout")?
            .join("lib/libzstd.1.dylib");
        let bytes = read_bounded(&fs::canonicalize(library)?, 128 * 1024 * 1024)?;
        ensure!(
            hex_string(&digest(&bytes)).trim_start_matches("0x")
                == "e2847c4613b386683c234913ae3b7b04299254096caf7616e3b3cd9bb97a39ab",
            "decoder runtime library digest mismatch"
        );
        Ok(bytes)
    }
    pub fn verify(&self) -> Result<()> {
        if self.path.is_some() {
            self.binary_snapshot()?;
            #[cfg(target_os = "macos")]
            self.library_snapshot()?;
        }
        Ok(())
    }
    pub fn compress(&self, raw: &[u8], root: &Path) -> Result<Vec<u8>> {
        ensure!(
            !raw.is_empty() && raw.len() as u64 <= RAW_CAP,
            "raw export size cap"
        );
        self.verify()?;
        let Some(path) = &self.path else {
            return crate::raw_frame::encode(raw);
        };
        let scratch = Scratch::new(root)?;
        let input = scratch.0.join("input");
        let output = scratch.0.join("frame");
        private_file(&input, raw)?;
        private_file(&output, &[])?;
        run_bounded(
            Command::new(path)
                .args(["-q", "-1", "--check", "--no-dictID", "--stdout"])
                .arg(&input)
                .stdout(File::options().write(true).open(&output)?),
            Duration::from_secs(30),
        )?;
        let frame = read_bounded(&output, boole_derivation::m2::Z_CAP)?;
        validate_frame(&frame, raw.len() as u64)?;
        Ok(frame)
    }
    pub fn decode(&self, frame: &[u8], raw_len: u64, root: &Path) -> Result<Vec<u8>> {
        self.verify()?;
        validate_frame(frame, raw_len)?;
        if let Some(raw) =
            crate::raw_frame::decode(frame, raw_len).map_err(|_| anyhow::Error::new(InvalidData))?
        {
            return Ok(raw);
        }
        self.path
            .as_ref()
            .context("full pinned decoder unavailable for compressed blocks")?;
        let scratch = Scratch::new(root)?;
        let input = scratch.0.join("frame");
        let output = scratch.0.join("raw");
        private_file(&input, frame)?;
        private_file(&output, &[])?;
        let bin = scratch.0.join("bin");
        fs::create_dir(&bin)?;
        #[cfg(target_os = "macos")]
        {
            let lib = scratch.0.join("lib");
            fs::create_dir(&lib)?;
            private_file(&lib.join("libzstd.1.dylib"), &self.library_snapshot()?)?;
        }
        let snapshot = bin.join("decoder");
        private_file(&snapshot, &self.binary_snapshot()?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&snapshot, fs::Permissions::from_mode(0o500))?;
        }
        let mut command = Command::new(&snapshot);
        command
            .args(["-q", "-d", "-f", "--single-thread", "-o"])
            .arg(&output)
            .arg(&input)
            .current_dir(&scratch.0);
        let run = boole_lean_runner::run_byte_adapter(
            command,
            File::open(&input)?,
            &scratch.0,
            1024,
            30_000,
            65536,
        )?;
        if !run.unavailable && run.exit_code == 1 {
            return Err(InvalidData.into());
        }
        ensure!(
            run.success && !run.unavailable,
            "decoder containment or untyped exit {}",
            run.exit_code
        );
        let raw = read_bounded(&output, raw_len as usize)?;
        ensure!(raw.len() as u64 == raw_len, "decoded size mismatch");
        Ok(raw)
    }
}

/// Parse exactly one standard frame, including block boundaries and checksum.
/// The pinned decoder separately verifies compressed blocks and checksum bytes.
pub fn validate_frame(bytes: &[u8], raw_len: u64) -> Result<()> {
    ensure!(raw_len > 0 && raw_len <= RAW_CAP, "frame raw size cap");
    ensure!(
        bytes.get(..4) == Some(&[0x28, 0xb5, 0x2f, 0xfd]),
        "frame magic"
    );
    let descriptor = *bytes.get(4).context("frame header")?;
    ensure!(
        descriptor & 3 == 0 && descriptor & 4 != 0 && descriptor & 0x18 == 0,
        "frame dictionary/checksum/reserved flags"
    );
    let single = descriptor & 0x20 != 0;
    let mut at = 5;
    let window = if single {
        None
    } else {
        let d = *bytes.get(at).context("window header")?;
        at += 1;
        let base = 1u64
            .checked_shl(10 + u32::from(d >> 3))
            .context("window overflow")?;
        Some(base + base / 8 * u64::from(d & 7))
    };
    let size = match descriptor >> 6 {
        0 => usize::from(single),
        1 => 2,
        2 => 4,
        _ => 8,
    };
    ensure!(size != 0, "frame content size required");
    let mut field = [0; 8];
    field[..size].copy_from_slice(bytes.get(at..at + size).context("content size")?);
    at += size;
    let content = u64::from_le_bytes(field)
        .checked_add(if size == 2 { 256 } else { 0 })
        .context("content overflow")?;
    ensure!(
        content == raw_len && window.unwrap_or(content) <= content,
        "frame content size/window mismatch"
    );
    loop {
        let h = bytes.get(at..at + 3).context("block header")?;
        at += 3;
        let value = u32::from_le_bytes([h[0], h[1], h[2], 0]);
        let kind = (value >> 1) & 3;
        let len = (value >> 3) as usize;
        ensure!(kind != 3 && len <= 128 * 1024, "frame block type/size");
        at = at
            .checked_add(if kind == 1 { 1 } else { len })
            .context("block length overflow")?;
        ensure!(at <= bytes.len(), "truncated frame block");
        if value & 1 != 0 {
            break;
        }
    }
    ensure!(
        at.checked_add(4) == Some(bytes.len()),
        "frame checksum/trailing bytes"
    );
    Ok(())
}
