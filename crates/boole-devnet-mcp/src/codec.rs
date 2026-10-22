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
            ensure!(status.success(), "local tool failed");
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
    path: PathBuf,
}
impl Codec {
    pub fn new(path: PathBuf) -> Result<Self> {
        let c = Self {
            path: fs::canonicalize(path)?,
        };
        c.verify()?;
        Ok(c)
    }
    fn verify(&self) -> Result<()> {
        let pin: serde_json::Value =
            serde_json::from_str(include_str!("../../../fixtures/devnet-m2/zstd-pin-v1.json"))?;
        let binary = read_bounded(&self.path, 128 * 1024 * 1024)?;
        ensure!(
            hex_string(&digest(&binary)).trim_start_matches("0x")
                == pin["sha256"].as_str().unwrap_or(""),
            "codec executable digest mismatch"
        );
        Ok(())
    }
    pub fn compress(&self, raw: &[u8], root: &Path) -> Result<Vec<u8>> {
        ensure!(
            !raw.is_empty() && raw.len() as u64 <= RAW_CAP,
            "raw export size cap"
        );
        self.verify()?;
        let scratch = Scratch::new(root)?;
        let input = scratch.0.join("input");
        let output = scratch.0.join("frame");
        private_file(&input, raw)?;
        private_file(&output, &[])?;
        run_bounded(
            Command::new(&self.path)
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
        validate_frame(frame, raw_len)?;
        self.verify()?;
        let scratch = Scratch::new(root)?;
        let input = scratch.0.join("frame");
        let output = scratch.0.join("raw");
        private_file(&input, frame)?;
        private_file(&output, &[])?;
        run_bounded(
            Command::new(&self.path)
                .args(["-q", "-d", "--stdout"])
                .arg(&input)
                .stdout(File::options().write(true).open(&output)?),
            Duration::from_secs(30),
        )?;
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
