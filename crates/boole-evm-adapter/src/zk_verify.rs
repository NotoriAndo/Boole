//! Verifier-only compressed-STARK check for the EVM zkVM feasibility proof
//! (ADR-0018). This is the SMALLEST production-representative seam: it calls
//! ONLY sp1-verifier's compressed *verification* API — never `sp1-sdk` /
//! `sp1-prover` / any proving-key or proof-generation entry point — and it
//! caps the external proof size BEFORE deserialization.
//!
//! ## Scope guardrails (ADR-0018, operator msg 3513/3518)
//!
//! * OUTSIDE the consensus path; this crate is `publish = false` and is not
//!   wired into `boole-core` / `boole-lean-runner` / the node.
//! * Default OFF; reuses the single existing frozen proof; generates 0 new
//!   proofs; carries NO prover / proving-key dependency.
//! * Does NOT change EVM `mineable_now` (stays 0) or any reward / Base state.
//! * The ELF -> vk identity is confirmed ONCE, out of band (boot / release);
//!   this type never regenerates the vk and never runs prover setup.
//!
//! ## Why the size cap comes first
//!
//! [`SP1CompressedVerifierRaw::verify_with_public_values`] hands the proof
//! bytes to a `bincode` (1.3.3, RUSTSEC-2025-0141 unmaintained) deserializer.
//! We refuse an oversized blob BEFORE that call so a malformed / hostile input
//! cannot drive an unbounded allocation. The cap is on the INPUT bytes and
//! changes no consensus value.

use sp1_verifier::compressed::{CompressedError, SP1CompressedVerifierRaw};

/// Hard upper bound on the compressed-proof byte length, enforced BEFORE the
/// bytes reach the bincode-backed deserializer inside sp1-verifier.
///
/// The frozen feasibility proofs are ~1.27 MiB (`proof-single.bin` 1 273 351,
/// `proof-batch.bin` 1 278 036 — ADR-0018 confirmed facts #1). 4 MiB gives
/// headroom while refusing an oversized blob outright.
pub const MAX_COMPRESSED_PROOF_BYTES: usize = 4 * 1024 * 1024;

/// Why a compressed-proof verification was rejected. Each variant is a
/// distinct, testable gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyReject {
    /// The proof byte length exceeds [`MAX_COMPRESSED_PROOF_BYTES`]; rejected
    /// BEFORE any deserialization.
    ProofTooLarge { len: usize, max: usize },
    /// The (size-capped) bytes are not a valid compressed proof, the public
    /// values do not match the commitment, or the vkey hash is wrong — every
    /// parse / crypto failure surfaced by sp1-verifier collapses here.
    ProofRejected { detail: String },
}

/// The outcome of a verifier-only compressed-STARK check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// The compressed STARK verified against the preloaded vkey hash and the
    /// given public values.
    Accepted,
    /// The proof was rejected; the reason names the gate that fired.
    Rejected(VerifyReject),
}

/// A verifier-only compressed-STARK checker holding a PRELOADED fixed vkey
/// hash (the frozen 8-word `[SP1Field; 8]`, bincode-serialized).
///
/// Constructing it performs NO prover setup and NO vk regeneration — the
/// ELF -> vk identity is confirmed once out of band (ADR-0018 (f)); this type
/// only ever verifies.
#[derive(Debug, Clone)]
pub struct CompressedProofVerifier {
    /// Preloaded, fixed vkey-hash bytes (bincode-serialized `[SP1Field; 8]`).
    vkey_hash: Vec<u8>,
    /// Input-size cap applied before deserialization.
    max_proof_bytes: usize,
}

impl CompressedProofVerifier {
    /// Build with the preloaded fixed vkey-hash bytes. No prover, no ELF, no
    /// setup — verifier-only.
    pub fn new(vkey_hash: Vec<u8>) -> Self {
        Self {
            vkey_hash,
            max_proof_bytes: MAX_COMPRESSED_PROOF_BYTES,
        }
    }

    /// The input-size cap this verifier enforces before deserialization.
    pub fn max_proof_bytes(&self) -> usize {
        self.max_proof_bytes
    }

    /// Verify a compressed proof against the preloaded vkey hash and the given
    /// public values. The proof size is checked BEFORE deserialization.
    ///
    /// This is the ONLY sp1-verifier entry point this crate calls; there is no
    /// prove path.
    pub fn verify(&self, proof_bytes: &[u8], public_values: &[u8]) -> VerifyOutcome {
        // SIZE GATE — refuse oversized input before it reaches the
        // bincode-backed deserializer (bounded-input defense).
        if proof_bytes.len() > self.max_proof_bytes {
            return VerifyOutcome::Rejected(VerifyReject::ProofTooLarge {
                len: proof_bytes.len(),
                max: self.max_proof_bytes,
            });
        }

        // VERIFY GATE — verifier-only compressed-STARK check.
        match SP1CompressedVerifierRaw::verify_with_public_values(
            proof_bytes,
            public_values,
            &self.vkey_hash,
        ) {
            Ok(()) => VerifyOutcome::Accepted,
            Err(err) => VerifyOutcome::Rejected(VerifyReject::ProofRejected {
                detail: compressed_error_detail(&err),
            }),
        }
    }
}

/// Render a [`CompressedError`] as a stable, human-readable reason string.
fn compressed_error_detail(err: &CompressedError) -> String {
    err.to_string()
}
