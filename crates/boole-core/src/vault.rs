//! P1.10e — at-rest encryption primitive for wallet/session/api-key
//! material. Pure crypto + serde, no I/O. Consumers (slice P1.10f
//! `boole-wallet-agent`, slice P1.10g migration tool) wrap this primitive
//! with file persistence and passphrase prompting.
//!
//! Construction: argon2id KDF derives a 32-byte key from `(passphrase,
//! salt, params)`; that key + a random 96-bit nonce drive a
//! ChaCha20-Poly1305 AEAD seal of `plaintext` with caller-supplied `aad`.
//! The on-disk envelope is versioned JSON; binary fields are hex (matches
//! the rest of the codebase, no new base64 dep).
//!
//! Security note: AEAD alone does not distinguish "wrong passphrase" from
//! "ciphertext tampering" — both surface as `VaultError::DecryptionFailed`.
//! This is intentional: a distinguishable wrong-passphrase oracle weakens
//! the construction.

use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Key, Nonce,
};

const VAULT_SCHEMA_VERSION: u32 = 1;
const KDF_ALGO_ARGON2ID: &str = "argon2id";
const AEAD_ALGO_CHACHA20POLY1305: &str = "chacha20poly1305";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// Argon2id work-factor parameters. The defaults target ~200ms on modern
/// laptop hardware (OWASP wallet guidance). Callers MAY raise these for
/// long-lived backups; the test-only `VaultParams::test_fast` lowers them
/// to the argon2 minimum so unit tests do not pay 200ms each.
#[derive(Debug, Clone, Copy)]
pub struct VaultParams {
    pub memory_kib: u32,
    pub time_cost: u32,
    pub parallelism: u32,
}

impl Default for VaultParams {
    fn default() -> Self {
        Self {
            memory_kib: 65_536,
            time_cost: 3,
            parallelism: 1,
        }
    }
}

impl VaultParams {
    #[cfg(test)]
    pub fn test_fast() -> Self {
        Self {
            memory_kib: 8,
            time_cost: 1,
            parallelism: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct KdfHeader {
    algo: String,
    #[serde(rename = "memoryKiB")]
    memory_kib: u32,
    #[serde(rename = "timeCost")]
    time_cost: u32,
    parallelism: u32,
    salt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AeadHeader {
    algo: String,
    nonce: String,
}

/// Sealed envelope. Serializes to a stable versioned JSON shape so future
/// vault revisions can be detected at open() time and rejected with a
/// typed `UnsupportedVersion`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptedVault {
    version: u32,
    kdf: KdfHeader,
    aead: AeadHeader,
    ciphertext: String,
}

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("vault: kdf failure: {0}")]
    KdfFailure(String),
    #[error("vault: decryption failed (wrong passphrase, tampered envelope, or aad mismatch)")]
    DecryptionFailed,
    #[error("vault: unsupported version: {0}")]
    UnsupportedVersion(u32),
    #[error("vault: unsupported kdf algo: {0}")]
    UnsupportedKdfAlgo(String),
    #[error("vault: unsupported aead algo: {0}")]
    UnsupportedAeadAlgo(String),
    #[error("vault: invalid hex in field {0}")]
    InvalidHex(&'static str),
    #[error("vault: invalid salt length (expected {expected}, got {actual})")]
    InvalidSaltLength { expected: usize, actual: usize },
    #[error("vault: invalid nonce length (expected {expected}, got {actual})")]
    InvalidNonceLength { expected: usize, actual: usize },
    #[error("vault: invalid kdf params: {0}")]
    InvalidKdfParams(String),
    #[error("vault: envelope serde error: {0}")]
    Serde(String),
    #[error("vault: internal error: {0}")]
    Internal(&'static str),
}

impl EncryptedVault {
    pub fn seal(
        passphrase: &[u8],
        plaintext: &[u8],
        aad: &[u8],
        params: VaultParams,
    ) -> Result<Self, VaultError> {
        let mut salt = [0_u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);

        let key = derive_key(passphrase, &salt, &params)?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| VaultError::Internal("aead encrypt"))?;

        Ok(Self {
            version: VAULT_SCHEMA_VERSION,
            kdf: KdfHeader {
                algo: KDF_ALGO_ARGON2ID.to_string(),
                memory_kib: params.memory_kib,
                time_cost: params.time_cost,
                parallelism: params.parallelism,
                salt: hex::encode(salt),
            },
            aead: AeadHeader {
                algo: AEAD_ALGO_CHACHA20POLY1305.to_string(),
                nonce: hex::encode(nonce_bytes),
            },
            ciphertext: hex::encode(ciphertext),
        })
    }

    pub fn open(&self, passphrase: &[u8], aad: &[u8]) -> Result<Vec<u8>, VaultError> {
        if self.version != VAULT_SCHEMA_VERSION {
            return Err(VaultError::UnsupportedVersion(self.version));
        }
        if self.kdf.algo != KDF_ALGO_ARGON2ID {
            return Err(VaultError::UnsupportedKdfAlgo(self.kdf.algo.clone()));
        }
        if self.aead.algo != AEAD_ALGO_CHACHA20POLY1305 {
            return Err(VaultError::UnsupportedAeadAlgo(self.aead.algo.clone()));
        }

        let salt = hex::decode(&self.kdf.salt).map_err(|_| VaultError::InvalidHex("salt"))?;
        if salt.len() != SALT_LEN {
            return Err(VaultError::InvalidSaltLength {
                expected: SALT_LEN,
                actual: salt.len(),
            });
        }
        let nonce = hex::decode(&self.aead.nonce).map_err(|_| VaultError::InvalidHex("nonce"))?;
        if nonce.len() != NONCE_LEN {
            return Err(VaultError::InvalidNonceLength {
                expected: NONCE_LEN,
                actual: nonce.len(),
            });
        }
        let ciphertext =
            hex::decode(&self.ciphertext).map_err(|_| VaultError::InvalidHex("ciphertext"))?;

        let params = VaultParams {
            memory_kib: self.kdf.memory_kib,
            time_cost: self.kdf.time_cost,
            parallelism: self.kdf.parallelism,
        };
        let key = derive_key(passphrase, &salt, &params)?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
        cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad,
                },
            )
            .map_err(|_| VaultError::DecryptionFailed)
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, VaultError> {
        serde_json::to_vec(self).map_err(|e| VaultError::Serde(e.to_string()))
    }

    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self, VaultError> {
        serde_json::from_slice(bytes).map_err(|e| VaultError::Serde(e.to_string()))
    }
}

fn derive_key(
    passphrase: &[u8],
    salt: &[u8],
    params: &VaultParams,
) -> Result<[u8; KEY_LEN], VaultError> {
    let argon_params = Params::new(
        params.memory_kib,
        params.time_cost,
        params.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|e| VaultError::InvalidKdfParams(e.to_string()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon_params);
    let mut key = [0_u8; KEY_LEN];
    argon
        .hash_password_into(passphrase, salt, &mut key)
        .map_err(|e| VaultError::KdfFailure(e.to_string()))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSPHRASE: &[u8] = b"correct-horse-battery-staple";
    const PLAINTEXT: &[u8] = b"ed25519 secret seed bytes (32-byte hex placeholder)";
    const AAD: &[u8] = b"boole-vault.v1:wallet/default";

    #[test]
    fn vault_seal_then_open_roundtrips_plaintext() {
        let vault = EncryptedVault::seal(PASSPHRASE, PLAINTEXT, AAD, VaultParams::test_fast())
            .expect("seal");
        let recovered = vault.open(PASSPHRASE, AAD).expect("open");
        assert_eq!(recovered, PLAINTEXT);
    }

    #[test]
    fn vault_open_with_wrong_passphrase_returns_decryption_failed() {
        let vault = EncryptedVault::seal(PASSPHRASE, PLAINTEXT, AAD, VaultParams::test_fast())
            .expect("seal");
        let err = vault
            .open(b"wrong-passphrase", AAD)
            .expect_err("wrong passphrase must fail");
        assert!(
            matches!(err, VaultError::DecryptionFailed),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn vault_open_with_modified_aad_returns_decryption_failed() {
        let vault = EncryptedVault::seal(PASSPHRASE, PLAINTEXT, AAD, VaultParams::test_fast())
            .expect("seal");
        let err = vault
            .open(PASSPHRASE, b"boole-vault.v1:wallet/other")
            .expect_err("modified aad must fail");
        assert!(
            matches!(err, VaultError::DecryptionFailed),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn vault_open_with_tampered_ciphertext_returns_decryption_failed() {
        let mut vault = EncryptedVault::seal(PASSPHRASE, PLAINTEXT, AAD, VaultParams::test_fast())
            .expect("seal");
        let mut ct = hex::decode(&vault.ciphertext).expect("decode ct");
        let last = ct.len() - 1;
        ct[last] ^= 0x01;
        vault.ciphertext = hex::encode(ct);
        let err = vault
            .open(PASSPHRASE, AAD)
            .expect_err("tampered ciphertext must fail");
        assert!(
            matches!(err, VaultError::DecryptionFailed),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn vault_seal_uses_random_salt_and_nonce_so_two_seals_differ() {
        let a = EncryptedVault::seal(PASSPHRASE, PLAINTEXT, AAD, VaultParams::test_fast())
            .expect("seal a");
        let b = EncryptedVault::seal(PASSPHRASE, PLAINTEXT, AAD, VaultParams::test_fast())
            .expect("seal b");
        assert_ne!(a.kdf.salt, b.kdf.salt, "salt must be random");
        assert_ne!(a.aead.nonce, b.aead.nonce, "nonce must be random");
        assert_ne!(
            a.ciphertext, b.ciphertext,
            "ciphertext must differ across seals"
        );
    }

    #[test]
    fn vault_json_roundtrip_preserves_envelope_and_still_decrypts() {
        let vault = EncryptedVault::seal(PASSPHRASE, PLAINTEXT, AAD, VaultParams::test_fast())
            .expect("seal");
        let bytes = vault.to_json_bytes().expect("encode");
        let parsed = EncryptedVault::from_json_bytes(&bytes).expect("decode");
        assert_eq!(parsed, vault);
        let recovered = parsed.open(PASSPHRASE, AAD).expect("open after json");
        assert_eq!(recovered, PLAINTEXT);
    }

    #[test]
    fn vault_open_rejects_unsupported_version_with_typed_error() {
        let mut vault = EncryptedVault::seal(PASSPHRASE, PLAINTEXT, AAD, VaultParams::test_fast())
            .expect("seal");
        vault.version = 999;
        let err = vault
            .open(PASSPHRASE, AAD)
            .expect_err("future version must be rejected");
        assert!(
            matches!(err, VaultError::UnsupportedVersion(999)),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn vault_open_rejects_unsupported_kdf_algo() {
        let mut vault = EncryptedVault::seal(PASSPHRASE, PLAINTEXT, AAD, VaultParams::test_fast())
            .expect("seal");
        vault.kdf.algo = "scrypt".to_string();
        let err = vault
            .open(PASSPHRASE, AAD)
            .expect_err("unknown kdf must be rejected");
        assert!(
            matches!(err, VaultError::UnsupportedKdfAlgo(ref s) if s == "scrypt"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn vault_open_rejects_unsupported_aead_algo() {
        let mut vault = EncryptedVault::seal(PASSPHRASE, PLAINTEXT, AAD, VaultParams::test_fast())
            .expect("seal");
        vault.aead.algo = "aes-gcm".to_string();
        let err = vault
            .open(PASSPHRASE, AAD)
            .expect_err("unknown aead must be rejected");
        assert!(
            matches!(err, VaultError::UnsupportedAeadAlgo(ref s) if s == "aes-gcm"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn vault_envelope_schema_is_stable_versioned_json() {
        let vault = EncryptedVault::seal(PASSPHRASE, PLAINTEXT, AAD, VaultParams::test_fast())
            .expect("seal");
        let json: serde_json::Value =
            serde_json::from_slice(&vault.to_json_bytes().expect("bytes")).expect("parse json");
        assert_eq!(json["version"], 1);
        assert_eq!(json["kdf"]["algo"], "argon2id");
        assert_eq!(json["kdf"]["memoryKiB"], 8);
        assert_eq!(json["kdf"]["timeCost"], 1);
        assert_eq!(json["kdf"]["parallelism"], 1);
        assert_eq!(json["aead"]["algo"], "chacha20poly1305");
        assert!(json["kdf"]["salt"].is_string());
        assert!(json["aead"]["nonce"].is_string());
        assert!(json["ciphertext"].is_string());
    }
}
