//! Bounded subprocess protocol for the explicitly selected wallet-agent binary.
//! An operator-selected executable is trusted with the supplied secret. This
//! is environment hygiene and lifecycle control, not a malicious-code sandbox.

use std::ffi::OsStr;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use zeroize::{Zeroize, Zeroizing};

use crate::llm_driver::{run_bounded_process, ProcessError};

pub enum WalletAgentInput<'a> {
    /// One already-collected passphrase, excluding the terminating newline.
    Passphrase(&'a [u8]),
    /// Keep passphrase/migration input inside the agent, without a parent copy.
    Inherit,
}

#[derive(Clone, Copy)]
pub enum WalletAgentResponse {
    PublicKey,
    Signature,
}

/// Never search an ambient PATH for a program that will receive wallet secrets.
/// The installed sibling is the default; an override is an explicit trust choice.
pub fn resolve_wallet_agent_binary() -> Result<PathBuf, String> {
    let candidate = match std::env::var_os("BOOLE_WALLET_AGENT_BIN") {
        Some(path) => PathBuf::from(path),
        None => std::env::current_exe()
            .map_err(|_| "wallet-agent executable location unavailable")?
            .parent()
            .ok_or("wallet-agent executable has no parent")?
            .join("boole-wallet-agent"),
    };
    checked_binary(&candidate)
}

fn checked_binary(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(
            "wallet-agent requires an absolute executable path; no PATH search is allowed"
                .to_string(),
        );
    }
    let path = path.canonicalize().map_err(|_| "wallet-agent executable not found; install the sibling binary or set an absolute BOOLE_WALLET_AGENT_BIN")?;
    let metadata = path
        .metadata()
        .map_err(|_| "wallet-agent executable metadata unavailable")?;
    let parent = path
        .parent()
        .ok_or("wallet-agent executable has no parent")?
        .metadata()
        .map_err(|_| "wallet-agent executable parent unavailable")?;
    if !metadata.is_file()
        || metadata.mode() & 0o022 != 0
        || metadata.mode() & 0o111 == 0
        || !parent.is_dir()
        || parent.mode() & 0o022 != 0
    {
        return Err("wallet-agent must be an executable regular file in a directory without group/world write access".to_string());
    }
    Ok(path)
}

pub fn run_wallet_agent(
    binary: &Path,
    args: &[&OsStr],
    input: WalletAgentInput<'_>,
    response: WalletAgentResponse,
) -> Result<String, String> {
    let bytes = Zeroizing::new(run_with_timeout(
        binary,
        args,
        input,
        Duration::from_secs(60),
    )?);
    let text = std::str::from_utf8(&bytes).map_err(|_| "wallet-agent response is not UTF-8")?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    let expected = match response {
        WalletAgentResponse::PublicKey => 64,
        WalletAgentResponse::Signature => 128,
    };
    if text.len() != expected
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("wallet-agent response is not canonical public-key/signature hex".to_string());
    }
    if matches!(response, WalletAgentResponse::PublicKey) {
        let raw: [u8; 32] = hex::decode(text)
            .map_err(|_| "wallet-agent key encoding")?
            .try_into()
            .map_err(|_| "wallet-agent key length")?;
        let key = ed25519_dalek::VerifyingKey::from_bytes(&raw)
            .map_err(|_| "wallet-agent returned an invalid Ed25519 key")?;
        if key.is_weak() {
            return Err("wallet-agent returned a weak Ed25519 key".to_string());
        }
    }
    Ok(text.to_string())
}

fn run_with_timeout(
    binary: &Path,
    args: &[&OsStr],
    input: WalletAgentInput<'_>,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    let secret = match input {
        WalletAgentInput::Passphrase(bytes) => {
            if bytes.is_empty()
                || bytes.len() > boole_core::vault::MAX_VAULT_PASSPHRASE_BYTES
                || bytes.iter().any(|b| matches!(b, 0 | b'\n' | b'\r'))
            {
                return Err("wallet-agent passphrase must be one nonempty line of at most 4096 bytes without NUL".to_string());
            }
            let mut secret = Zeroizing::new(bytes.to_vec());
            secret.push(b'\n');
            Some(secret)
        }
        WalletAgentInput::Inherit => None,
    };
    let binary = checked_binary(binary)?;
    let mut command = Command::new(binary);
    command
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .stdin(if secret.is_some() {
            Stdio::piped()
        } else {
            Stdio::inherit()
        });
    run_bounded_process(
        &mut command,
        secret.as_deref().map(|s| s.as_slice()),
        timeout,
        4096,
    )
    .map_err(|error| match error {
        ProcessError::Exit {
            code, mut stderr, ..
        } => {
            stderr.zeroize();
            format!("wallet-agent failed with code {code} (child diagnostics withheld)")
        }
        ProcessError::Timeout { .. } => {
            "wallet-agent timed out; child termination requested".to_string()
        }
        ProcessError::OutputLimit { .. } => {
            "wallet-agent exceeded 4096-byte output limit; child termination requested".to_string()
        }
        _ => "wallet-agent process I/O or startup failed".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn wallet_process_bounds_silent_children_and_does_not_inherit_parent_environment() {
        let start = Instant::now();
        let error = run_with_timeout(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new("sleep 5")],
            WalletAgentInput::Passphrase(b"test-only"),
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(error.contains("timed out"));
        assert!(start.elapsed() < Duration::from_secs(1));
        let output = run_with_timeout(
            Path::new("/usr/bin/env"),
            &[],
            WalletAgentInput::Inherit,
            Duration::from_secs(1),
        )
        .unwrap();
        let text = std::str::from_utf8(&output).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.lines().any(|line| line == "PATH=/usr/bin:/bin"));
        assert!(text.lines().any(|line| line == "LANG=C.UTF-8"));
        for secret in [
            b"".as_slice(),
            b"one\ntwo",
            b"one\rtwo",
            b"one\0two",
            &vec![b'p'; 4097],
        ] {
            assert!(run_wallet_agent(
                Path::new("/nonexistent/never-spawn"),
                &[],
                WalletAgentInput::Passphrase(secret),
                WalletAgentResponse::PublicKey
            )
            .unwrap_err()
            .contains("passphrase must"));
        }
    }
}
