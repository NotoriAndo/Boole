//! P1.10 — pluggable prover signer so the bounty-proof signing path can use a
//! key the miner process never sees.
//!
//! `KeySigner` is the in-process path (an owned `SigningKeyV2`). `AgentSigner`
//! delegates the raw ed25519 signature to the `boole-wallet-agent` subprocess,
//! which opens an AEAD vault and signs `signing_digest_hex(payload, network_id)`
//! — byte-identical to `SigningKeyV2::sign_for_network` (see ADR-0006), so the
//! seed never enters the miner address space.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use boole_core::{signing_digest_hex, SignedEnvelope, SigningKeyV2, SIGNED_ENVELOPE_SCHEMA};
use serde_json::Value;
use zeroize::Zeroizing;

/// Produces a `boole.signed.v1` envelope for a payload, scoped to a network.
/// Implementors differ only in WHERE the ed25519 key lives.
pub trait ProofSigner {
    /// The prover verifying-key (hex32). Used for the payload's `prover` field
    /// and the envelope `pk`.
    fn pk_hex(&self) -> Result<String, String>;
    /// Sign `payload` for `network_id`, returning the assembled envelope.
    fn sign_payload(&self, payload: &Value, network_id: &str) -> Result<SignedEnvelope, String>;
}

/// In-process signer: holds the ed25519 seed for the lifetime of the sign.
pub struct KeySigner {
    key: SigningKeyV2,
}

impl KeySigner {
    pub fn new(key: SigningKeyV2) -> Self {
        Self { key }
    }
}

impl ProofSigner for KeySigner {
    fn pk_hex(&self) -> Result<String, String> {
        Ok(self.key.pk_hex())
    }

    fn sign_payload(&self, payload: &Value, network_id: &str) -> Result<SignedEnvelope, String> {
        self.key.sign_for_network(payload, Some(network_id))
    }
}

/// Out-of-process signer: the seed stays sealed in a `boole-wallet-agent` vault
/// and never enters this process. Each call shells out to the agent, piping the
/// passphrase on stdin (never argv) and reading the result from stdout.
pub struct AgentSigner {
    agent_bin: String,
    vault_path: PathBuf,
    // D#5 — wiped from memory on drop; the secret must not outlive the signer.
    passphrase: Zeroizing<String>,
}

impl AgentSigner {
    /// `agent_bin` is the `boole-wallet-agent` binary (name on PATH or an
    /// absolute path). `passphrase` is read by the caller from
    /// `BOOLE_WALLET_PASSPHRASE` (never argv).
    pub fn new(agent_bin: impl Into<String>, vault_path: PathBuf, passphrase: String) -> Self {
        Self {
            agent_bin: agent_bin.into(),
            vault_path,
            passphrase: Zeroizing::new(passphrase),
        }
    }

    /// Run the agent with `args`, pipe the passphrase line on stdin, and return
    /// trimmed stdout. Maps a non-zero exit to an `Err` carrying the agent's
    /// stderr.
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let mut child = Command::new(&self.agent_bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", self.agent_bin))?;
        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| "wallet-agent stdin unavailable".to_string())?;
            // Write the passphrase bytes and the newline separately so no
            // unzeroized temporary String holding the secret is allocated.
            stdin
                .write_all(self.passphrase.as_bytes())
                .and_then(|()| stdin.write_all(b"\n"))
                .map_err(|e| format!("write passphrase to wallet-agent: {e}"))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("wait for wallet-agent: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "wallet-agent {} failed: {}",
                args.first().copied().unwrap_or(""),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn vault_arg(&self) -> String {
        self.vault_path.to_string_lossy().into_owned()
    }
}

impl ProofSigner for AgentSigner {
    fn pk_hex(&self) -> Result<String, String> {
        self.run(&["pubkey", "--vault", &self.vault_arg()])
    }

    fn sign_payload(&self, payload: &Value, network_id: &str) -> Result<SignedEnvelope, String> {
        let pk = self.pk_hex()?;
        // The digest is a public hash of the public payload, so it is safe on
        // argv; only the passphrase (stdin) is secret.
        let digest_hex = signing_digest_hex(payload, Some(network_id));
        let signature = self.run(&[
            "sign",
            "--vault",
            &self.vault_arg(),
            "--message",
            &digest_hex,
        ])?;
        Ok(SignedEnvelope {
            schema: SIGNED_ENVELOPE_SCHEMA,
            payload: payload.clone(),
            pk,
            signature,
            network_id: Some(network_id.to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // D#5 — type-level assertion: the passphrase must live in a
    // self-zeroizing wrapper so the secret is wiped from memory on drop.
    #[test]
    fn agent_signer_passphrase_is_zeroizing() {
        let signer = AgentSigner::new("agent", PathBuf::from("/tmp/vault"), "s3cret".to_string());
        let _: &zeroize::Zeroizing<String> = &signer.passphrase;
    }
}
