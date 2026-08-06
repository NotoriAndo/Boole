//! Digest values and preimage framing for the emitter contract
//! (EMITTER-CONTRACT-v1_2-final §2 / §4).
//!
//! A [`Digest`] is a tagged object `{algorithm, hex}` — `algorithm` is the hash
//! primitive (`blake3` for derived protocol identities, `sha256` for file/text
//! evidence) and `hex` is the 64-character lowercase hex of the 32 raw bytes.
//! Preimages are built with [`push_field`] framing: `u64_le(len) ++ bytes` for
//! every component, so a component boundary is never ambiguous.

use boole_core::h_protocol;
use serde::Serialize;
use sha2::{Digest as _, Sha256};

/// The hash primitive that produced a [`Digest`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DigestAlg {
    /// Derived protocol identity (matches boole-core's `h_protocol`).
    Blake3,
    /// File / source-byte / text evidence.
    Sha256,
}

/// A tagged 32-byte digest: `{algorithm, hex}` with `hex` lowercase, length 64.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Digest {
    pub algorithm: DigestAlg,
    pub hex: String,
}

impl Digest {
    /// Decode the 32 raw bytes this digest names. The `hex` field always holds
    /// 64 lowercase hex characters when produced by the constructors here.
    pub fn raw(&self) -> [u8; 32] {
        let mut out = [0u8; 32];
        hex::decode_to_slice(&self.hex, &mut out)
            .expect("Digest.hex is always 64 lowercase hex characters");
        out
    }
}

/// Append one `push_field`-framed component to `buf`: `u64_le(len) ++ bytes`.
pub fn push_field(buf: &mut Vec<u8>, bytes: &[u8]) {
    buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    buf.extend_from_slice(bytes);
}

/// A BLAKE3 protocol digest over `domain` then the single, already-`push_field`
/// -framed `preimage` buffer. This mirrors `h_protocol(domain, &[preimage])`;
/// callers build `preimage` by concatenating [`push_field`] components in the
/// order the contract §4 fixes.
pub fn protocol_digest(domain: &[u8], preimage: &[u8]) -> Digest {
    Digest {
        algorithm: DigestAlg::Blake3,
        hex: h_protocol(domain, &[preimage]).to_hex(),
    }
}

/// A SHA-256 evidence digest over the exact `bytes` given (no framing).
pub fn sha256_digest(bytes: &[u8]) -> Digest {
    Digest {
        algorithm: DigestAlg::Sha256,
        hex: hex::encode(Sha256::digest(bytes)),
    }
}
