use crate::codec::{digest, private_file, read_bounded, Scratch};
use anyhow::{bail, ensure, Result};
use boole_derivation::attributes::hex_string;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct Precheck {
    pub executable: PathBuf,
    pub sha256: [u8; 32],
    pub timeout: Duration,
}
impl Precheck {
    /// Exit 0 = local accept, 10 = deterministic reject; anything else is ERROR.
    pub fn run(
        &self,
        root: &Path,
        export: &[u8],
        problem: &[u8],
        env: [u8; 32],
        format: u8,
    ) -> Result<()> {
        let executable = std::fs::canonicalize(&self.executable)?;
        ensure!(
            digest(&read_bounded(&executable, 128 * 1024 * 1024)?) == self.sha256,
            "pre-check executable digest mismatch"
        );
        let scratch = Scratch::new(root)?;
        let e = scratch.0.join("export");
        let p = scratch.0.join("problem");
        private_file(&e, export)?;
        private_file(&p, problem)?;
        let mut child = Command::new(&executable)
            .arg("--export-file")
            .arg(&e)
            .arg("--problem-file")
            .arg(&p)
            .arg("--env-id")
            .arg(hex_string(&env))
            .arg("--export-format")
            .arg(format.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                match status.code() {
                    Some(0) => return Ok(()),
                    Some(10) => bail!("local pre-check deterministic reject"),
                    _ => bail!("local pre-check tool error"),
                }
            }
            if start.elapsed() >= self.timeout {
                let _ = child.kill();
                let _ = child.wait();
                bail!("local pre-check timed out");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}
