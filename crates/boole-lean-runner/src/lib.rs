//! Rust wrapper seam for Lean verifier execution.
//!
//! Lean remains the proof/checking truth source. This crate provides a small,
//! deterministic process boundary around `lake exec boole_check <proof.lean>`
//! and returns an evidence envelope that can be recorded by Boole runtime code.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeanRunnerConfig {
    pub verifier_hash: String,
    pub package_dir: PathBuf,
    pub checker_exe: String,
    pub timeout_ms: u64,
    pub memory_limit_mb: u64,
}

impl LeanRunnerConfig {
    pub fn new(verifier_hash: impl Into<String>) -> Self {
        Self {
            verifier_hash: verifier_hash.into(),
            package_dir: PathBuf::from("."),
            checker_exe: "boole_check".to_string(),
            timeout_ms: 10_000,
            memory_limit_mb: 512,
        }
    }

    pub fn with_package_dir(mut self, package_dir: impl Into<PathBuf>) -> Self {
        self.package_dir = package_dir.into();
        self
    }

    pub fn with_checker_exe(mut self, checker_exe: impl Into<String>) -> Self {
        self.checker_exe = checker_exe.into();
        self
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    pub fn with_memory_limit_mb(mut self, memory_limit_mb: u64) -> Self {
        self.memory_limit_mb = memory_limit_mb;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeanRunnerEvidence {
    pub verifier_hash: String,
    pub checker: String,
    pub checker_exe: String,
    pub checker_artifact_hash: String,
    pub package_dir: String,
    pub lean_version: String,
    pub lake_version: String,
    pub timeout_ms: u64,
    pub memory_limit_mb: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeanCheckResult {
    pub accepted: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub evidence: LeanRunnerEvidence,
}

#[derive(Debug, Clone)]
pub struct LeanRunner {
    config: LeanRunnerConfig,
}

impl LeanRunner {
    pub fn new(config: LeanRunnerConfig) -> Self {
        Self { config }
    }

    pub fn check_file(&self, proof_path: impl AsRef<Path>) -> Result<LeanCheckResult> {
        let proof_path = proof_path.as_ref();
        if !proof_path.is_file() {
            return Err(anyhow!(
                "Lean proof file does not exist: {}",
                proof_path.display()
            ));
        }
        if !self.config.package_dir.is_dir() {
            return Err(anyhow!(
                "Lean package directory does not exist: {}",
                self.config.package_dir.display()
            ));
        }

        let evidence = self.evidence()?;
        let output = Command::new("lake")
            .arg("exec")
            .arg(&self.config.checker_exe)
            .arg(proof_path)
            .current_dir(&self.config.package_dir)
            .stdin(Stdio::null())
            .output()
            .with_context(|| {
                format!(
                    "failed to run lake exec {} in {}",
                    self.config.checker_exe,
                    self.config.package_dir.display()
                )
            })?;

        Ok(LeanCheckResult {
            accepted: output.status.success(),
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            evidence,
        })
    }

    pub fn evidence(&self) -> Result<LeanRunnerEvidence> {
        Ok(LeanRunnerEvidence {
            verifier_hash: self.config.verifier_hash.clone(),
            checker: format!("lake exec {}", self.config.checker_exe),
            checker_exe: self.config.checker_exe.clone(),
            checker_artifact_hash: checker_artifact_hash(&self.config.package_dir)?,
            package_dir: self.config.package_dir.display().to_string(),
            lean_version: command_version("lean")?,
            lake_version: command_version("lake")?,
            timeout_ms: self.config.timeout_ms,
            memory_limit_mb: self.config.memory_limit_mb,
        })
    }
}

fn checker_artifact_hash(package_dir: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    for relative in ["lakefile.lean", "BooleCheck/Main.lean"] {
        let path = package_dir.join(relative);
        let bytes = std::fs::read(&path)
            .with_context(|| format!("failed to read checker artifact {}", path.display()))?;
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update(bytes);
        hasher.update([0]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn command_version(command: &str) -> Result<String> {
    let output = Command::new(command)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to execute `{command} --version`"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "`{} --version` failed: {}",
            command,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_records_verifier_hash() {
        let cfg = LeanRunnerConfig::new("abc");
        assert_eq!(cfg.verifier_hash, "abc");
        assert_eq!(cfg.checker_exe, "boole_check");
        assert_eq!(cfg.timeout_ms, 10_000);
        assert_eq!(cfg.memory_limit_mb, 512);
    }
}
