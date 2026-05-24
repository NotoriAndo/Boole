//! ed25519 sign/verify primitives wrapped in `boole.signed.v1` envelopes.
//!
//! Sign/verify operates on SHA-256(canonicalize(payload)). The wrapper
//! envelope itself is NOT canonicalized — it's a transport shell so a
//! verifier can recanonicalize the inner payload independent of how the
//! envelope was serialized on the wire.
//!
//! Errors are split between "the inputs are wire-malformed" (returned as
//! `Err(detail)`) and "the inputs are well-formed but the signature does
//! not match" (returned as `Ok(false)`). Callers that pattern-match on
//! result kinds should treat them differently — wire-malformed is a 4xx
//! `bad_pk`/`bad_signature` from the operator's side, cryptographically
//! wrong is a 200 `invalid` from the verifier's side.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{canonical_json::canonicalize, Hex32, Hex64};

pub const SIGNED_ENVELOPE_SCHEMA: &str = "boole.signed.v1";

/// Domain-separation tag mixed into the signing digest when an envelope
/// is scoped to a specific network (`SigningKeyV2::sign_for_network`).
///
/// Layout of the bytes that get SHA-256'd in network-bound mode:
///
/// ```text
/// <DIGEST_NETWORK_TAG> || <network_id bytes> || b"\n" || canonical_json(payload)
/// ```
///
/// The tag is distinct from any legitimate payload prefix, so a verifier
/// recomputing the digest can recognise a network-bound signature from
/// the envelope's `network_id` field alone. Rewriting the wire
/// `network_id` (cross-network replay) or stripping it to `None` both
/// flip the digest the verifier computes, breaking the ed25519 check.
const DIGEST_NETWORK_TAG: &[u8] = b"boole.signed.v1.network::";

/// Owned ed25519 signing key wrapped to expose the hex-string surface the
/// CLI / HTTP layer wants without leaking the underlying type.
pub struct SigningKeyV2 {
    inner: SigningKey,
}

impl SigningKeyV2 {
    /// Generate a fresh key from `OsRng`. Production callers use this; tests
    /// use `from_dev_id` for determinism.
    pub fn from_random() -> Result<Self, String> {
        let inner = SigningKey::generate(&mut OsRng);
        Ok(Self { inner })
    }

    /// Deterministic test seed: `sk_seed = blake3("boole-keys-dev::" || id)`.
    /// The domain prefix matches v1's `dev_pk_hex` so dev keys collide with
    /// neither random keys nor any other identifier the protocol computes.
    pub fn from_dev_id(id: &str) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"boole-keys-dev::");
        hasher.update(id.as_bytes());
        let bytes: [u8; 32] = *hasher.finalize().as_bytes();
        Self {
            inner: SigningKey::from_bytes(&bytes),
        }
    }

    /// Reconstruct from a stored 32-byte hex seed (the on-disk `sk` field).
    pub fn from_seed_hex(seed_hex: &str) -> Result<Self, String> {
        let bytes = decode_hex32(seed_hex, "sk")?;
        Ok(Self {
            inner: SigningKey::from_bytes(&bytes),
        })
    }

    pub fn pk_hex(&self) -> String {
        hex::encode(self.inner.verifying_key().to_bytes())
    }

    pub fn sk_seed_hex(&self) -> String {
        hex::encode(self.inner.to_bytes())
    }

    /// Sign `payload` and return a `boole.signed.v1` envelope. The signing
    /// hash is `SHA-256(canonical_json(payload))`.
    ///
    /// This is the legacy entry point: it produces `network_id: None`,
    /// which keeps the digest input identical to pre-P2.10 envelopes so
    /// every previously persisted signature continues to verify. Call
    /// `sign_for_network` to scope a signature to a specific network.
    pub fn sign(&self, payload: &Value) -> Result<SignedEnvelope, String> {
        self.sign_for_network(payload, None)
    }

    /// Sign `payload` and return a `boole.signed.v1` envelope scoped to
    /// `network_id`. When `network_id` is `Some(...)`, the digest folds
    /// in a domain-separation tag (`DIGEST_NETWORK_TAG`) followed by the
    /// network bytes, so a signature produced for one network cannot be
    /// replayed against another — a verifier on the other network will
    /// recompute a different digest and the ed25519 check will fail.
    /// When `None`, this is byte-identical to the legacy `sign()` path.
    pub fn sign_for_network(
        &self,
        payload: &Value,
        network_id: Option<&str>,
    ) -> Result<SignedEnvelope, String> {
        let digest = digest_for(payload, network_id);
        let signature = self.inner.sign(&digest);
        Ok(SignedEnvelope {
            schema: SIGNED_ENVELOPE_SCHEMA,
            payload: payload.clone(),
            pk: self.pk_hex(),
            signature: hex::encode(signature.to_bytes()),
            network_id: network_id.map(|s| s.to_string()),
        })
    }
}

/// Return the hex-encoded SHA-256 digest of Boole canonical JSON payload bytes.
///
/// This is the same digest surface used by `boole.signed.v1`. CLI signers use
/// it to bind a caller-provided `requestHash` to the exact work payload before
/// producing a session-key signature.
pub fn canonical_payload_hash_hex(payload: &Value) -> String {
    hex::encode(sha256_canonical(payload))
}

/// Wire shape of a signed envelope. `signature` is hex64 (32-byte ed25519
/// signature × 2). `pk` is hex32 (32-byte verifying key × 2).
///
/// `network_id` (P2.10) scopes the signature to a specific network. When
/// present, it is bound into the signing digest via a domain-separation
/// tag (`DIGEST_NETWORK_TAG`), so a verifier rewriting or stripping the
/// field after the fact will recompute a different digest and the
/// ed25519 check will fail. When absent, the digest is byte-identical to
/// pre-P2.10 (`SHA-256(canonical(payload))`), so every previously stored
/// signature still verifies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedEnvelope {
    pub schema: &'static str,
    pub payload: Value,
    pub pk: String,
    pub signature: String,
    pub network_id: Option<String>,
}

impl SignedEnvelope {
    /// Re-canonicalize `payload`, recompute the SHA-256 digest (binding
    /// `network_id` when present), and verify `signature` against `pk`.
    /// Returns `Ok(true|false)` for "verification ran"; `Err(detail)`
    /// for malformed pk/signature hex.
    pub fn verify(&self) -> Result<bool, String> {
        verify_signature_with_network(
            &self.pk,
            &self.signature,
            &self.payload,
            self.network_id.as_deref(),
        )
    }
}

/// Stateless verification primitive (legacy: no network binding). The
/// `keys verify` CLI calls this directly; HTTP routes that accept
/// pre-P2.10 signed envelopes call this after pulling
/// `pk`/`signature`/`payload` out of the request body.
pub fn verify_signature(pk_hex: &str, sig_hex: &str, payload: &Value) -> Result<bool, String> {
    verify_signature_with_network(pk_hex, sig_hex, payload, None)
}

/// Stateless verification with network-id binding. When `network_id` is
/// `Some(...)` the digest the verifier recomputes folds in the same
/// domain-separation tag the signer used; cross-network replay (whether
/// the attacker rewrites the wire `network_id` or strips it to `None`)
/// flips the digest and the ed25519 check fails.
pub fn verify_signature_with_network(
    pk_hex: &str,
    sig_hex: &str,
    payload: &Value,
    network_id: Option<&str>,
) -> Result<bool, String> {
    let pk_bytes = decode_hex32(pk_hex, "pk")?;
    let sig_bytes = decode_hex64(sig_hex, "signature")?;
    let verifying = VerifyingKey::from_bytes(&pk_bytes)
        .map_err(|err| format!("bad_pk: ed25519 point invalid: {err}"))?;
    let signature = Signature::from_bytes(&sig_bytes);
    let digest = digest_for(payload, network_id);
    Ok(verifying.verify(&digest, &signature).is_ok())
}

fn sha256_canonical(payload: &Value) -> [u8; 32] {
    let bytes = canonicalize(payload);
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hasher.finalize().into()
}

/// Compute the SHA-256 digest the signer/verifier hash. When
/// `network_id` is `None` the input is exactly the canonical JSON of the
/// payload (legacy / byte-compatible with pre-P2.10). When `Some(...)`,
/// the input is prefixed with `DIGEST_NETWORK_TAG || network_id || '\n'`
/// so a verifier on a different network recomputes a different digest.
fn digest_for(payload: &Value, network_id: Option<&str>) -> [u8; 32] {
    match network_id {
        None => sha256_canonical(payload),
        Some(network) => {
            let mut hasher = Sha256::new();
            hasher.update(DIGEST_NETWORK_TAG);
            hasher.update(network.as_bytes());
            hasher.update(b"\n");
            hasher.update(canonicalize(payload));
            hasher.finalize().into()
        }
    }
}

fn decode_hex32(input: &str, field: &str) -> Result<[u8; 32], String> {
    if input.len() != 64 {
        return Err(format!(
            "bad_{field}: expected 64 hex chars, got {}",
            input.len()
        ));
    }
    let parsed = Hex32::from_hex(input)
        .map_err(|_| format!("bad_{field}: expected 64 lowercase hex chars"))?;
    Ok(*parsed.as_bytes())
}

fn decode_hex64(input: &str, field: &str) -> Result<[u8; 64], String> {
    if input.len() != 128 {
        return Err(format!(
            "bad_{field}: expected 128 hex chars, got {}",
            input.len()
        ));
    }
    let parsed = Hex64::from_hex(input)
        .map_err(|_| format!("bad_{field}: expected 128 lowercase hex chars"))?;
    Ok(*parsed.as_bytes())
}
