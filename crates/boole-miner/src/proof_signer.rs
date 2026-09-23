//! P1.10 — pluggable prover signer so the bounty-proof signing path can use a
//! key the miner process never sees.
//!
//! `KeySigner` is the in-process path (an owned `SigningKeyV2`). `AgentSigner`
//! delegates the raw ed25519 signature to the `boole-wallet-agent` subprocess,
//! which opens an AEAD vault and signs `signing_digest_hex(payload, network_id)`
//! — byte-identical to `SigningKeyV2::sign_for_network` (see ADR-0006), so the
//! seed never enters the miner address space.

use std::ffi::OsStr;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};

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
    /// `agent_bin` is an explicit absolute path to the trusted signing agent.
    /// The passphrase is never passed through argv or the child's environment.
    pub fn new(agent_bin: impl Into<String>, vault_path: PathBuf, passphrase: String) -> Self {
        Self {
            agent_bin: agent_bin.into(),
            vault_path,
            passphrase: Zeroizing::new(passphrase),
        }
    }

    /// Preferred short-lived owner flow: one bounded passphrase line from stdin.
    /// Explicit callers do not consult BOOLE_WALLET_PASSPHRASE in this mode.
    pub fn from_stdin(agent_bin: impl Into<String>, vault_path: PathBuf) -> Result<Self, String> {
        let mut bytes = Zeroizing::new(Vec::new());
        std::io::stdin()
            .lock()
            .take(boole_core::vault::MAX_VAULT_PASSPHRASE_BYTES as u64 + 3)
            .read_until(b'\n', &mut bytes)
            .map_err(|_| "wallet passphrase stdin read failed")?;
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
        if bytes.is_empty()
            || bytes.len() > boole_core::vault::MAX_VAULT_PASSPHRASE_BYTES
            || bytes.iter().any(|b| matches!(b, 0 | b'\n' | b'\r'))
        {
            return Err(
                "wallet passphrase must be one nonempty line of at most 4096 bytes without NUL"
                    .to_string(),
            );
        }
        let passphrase =
            std::str::from_utf8(&bytes).map_err(|_| "wallet passphrase must be UTF-8")?;
        Ok(Self::new(agent_bin, vault_path, passphrase.to_string()))
    }

    /// Run the agent with `args`, pipe the passphrase line on stdin, and return
    /// trimmed stdout. Child diagnostics are not safe to echo: an overridden
    /// agent can write supplied secrets to stderr.
    fn run(&self, args: &[&str], response: crate::WalletAgentResponse) -> Result<String, String> {
        let args: Vec<_> = args.iter().map(OsStr::new).collect();
        crate::run_wallet_agent(
            Path::new(&self.agent_bin),
            &args,
            crate::WalletAgentInput::Passphrase(self.passphrase.as_bytes()),
            response,
        )
    }

    fn vault_arg(&self) -> String {
        self.vault_path.to_string_lossy().into_owned()
    }
}

impl ProofSigner for AgentSigner {
    fn pk_hex(&self) -> Result<String, String> {
        self.run(
            &["pubkey", "--vault", &self.vault_arg()],
            crate::WalletAgentResponse::PublicKey,
        )
    }

    fn sign_payload(&self, payload: &Value, network_id: &str) -> Result<SignedEnvelope, String> {
        let pk = self.pk_hex()?;
        // The digest is a public hash of the public payload, so it is safe on
        // argv; only the passphrase (stdin) is secret.
        let digest_hex = signing_digest_hex(payload, Some(network_id));
        let signature = self.run(
            &[
                "sign",
                "--vault",
                &self.vault_arg(),
                "--message",
                &digest_hex,
            ],
            crate::WalletAgentResponse::Signature,
        )?;
        if !boole_core::verify_signature_with_network(&pk, &signature, payload, Some(network_id))
            .unwrap_or(false)
        {
            return Err(
                "wallet-agent returned a signature that does not authorize this payload/network"
                    .to_string(),
            );
        }
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
