//! Verifier-only compressed-STARK check for the EVM zkVM feasibility proof
//! (ADR-0018). This is the SMALLEST production-representative seam: it calls
//! ONLY sp1-verifier's compressed *verification* API — never `sp1-sdk` /
//! `sp1-prover` / any proving-key or proof-generation entry point — and it
//! caps the external proof size BEFORE deserialization.
//!
//! ## Scope guardrails (ADR-0018, operator msg 3513/3518/3522/3526)
//!
//! * OUTSIDE the consensus path; this crate is `publish = false` and is not
//!   wired into `boole-core` / `boole-lean-runner` / the node.
//! * Default OFF; reuses the single existing frozen proof; generates 0 new
//!   proofs. Makes NO proof-generation call and carries NO proving-key and NO
//!   DIRECT prover dependency. (Upstream packaging still pulls indirect prover
//!   code -- `slop-basefold-prover` -- into the BUILD graph; Boole never calls
//!   it. We claim only that Boole's CALLS are verifier-only, not the whole
//!   dependency graph.)
//! * Does NOT change EVM `mineable_now` (stays 0) or any reward / Base state.
//! * The ELF -> vk identity is confirmed ONCE, out of band (freeze record);
//!   this type never regenerates the vk and never runs prover setup — the vk
//!   hash is a compiled-in FIXED constant and the caller cannot supply another.
//!
//! ## The three gates (in order)
//!
//! `verify` applies, in order:
//!
//! 1. **size gate** — refuse a proof larger than [`MAX_COMPRESSED_PROOF_BYTES`]
//!    BEFORE it reaches the `bincode` (1.3.3, RUSTSEC-2025-0141 unmaintained)
//!    deserializer inside sp1-verifier, so a hostile blob cannot drive an
//!    unbounded allocation.
//! 2. **admission gate (external policy, NOT cryptographic)** — refuse public
//!    values whose header does not carry the frozen `task_contract` / `fork` /
//!    `author_oracle` digests. This is a policy filter that reads fixed offsets
//!    out of the caller-supplied pv and compares them to the frozen constants;
//!    it is NOT a cryptographic guarantee.
//! 3. **crypto gate** — the verifier-only compressed-STARK check. THIS is the
//!    only cryptographic step: the proof is bound to the FIXED vk and to the
//!    EXACT public-values byte-string (and nothing more). A different valid
//!    proof of the SAME frozen task still verifies here — this seam does not
//!    pin one specific proof blob.
//!
//! ## Cryptographic binding scope (operator msg 3526)
//!
//! The compressed STARK cryptographically binds the proof to the fixed vk and
//! to the whole public-values byte-string. The `task_contract` / `fork` /
//! `author_oracle` digests happen to live INSIDE that byte-string (header
//! offsets 0 / 64 / 128), so a proof committing to different values would carry
//! a different pv and fail the crypto gate. But the equality checks THIS module
//! performs against the frozen constants are an EXTERNAL admission filter, not
//! themselves cryptographic — we do not call them "cryptographic binding".

use sp1_verifier::compressed::{CompressedError, SP1CompressedVerifierRaw};

/// Hard upper bound on the compressed-proof byte length, enforced BEFORE the
/// bytes reach the bincode-backed deserializer inside sp1-verifier.
///
/// The frozen feasibility proof is ~1.27 MiB (the transcoded `SP1Proof`
/// `fixtures/evm-zkvm/proof-compressed.bin`). 4 MiB gives headroom while
/// refusing an oversized blob outright.
pub const MAX_COMPRESSED_PROOF_BYTES: usize = 4 * 1024 * 1024;

/// The FIXED, compiled-in verifying-key hash: `bincode([SP1Field; 8])`, the
/// KoalaBear vk hash of the frozen guest ELF (SP1 6.3.1 / circuit v6.1.0). This
/// is the ONLY key this verifier trusts; there is no vk-taking constructor on
/// the public surface, so a caller cannot inject another. Byte integrity is
/// pinned by `fixtures/evm-zkvm/SHA256SUMS`.
const FROZEN_VKEY_HASH: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/evm-zkvm/vk-hash.bin"
));

/// Length of the public-values header (6 x 32-byte digests). The admission gate
/// reads three of these; the body after it is per-case observed state.
const PV_HEADER_LEN: usize = 192;

/// External admission constants (NOT cryptographic binding) — the frozen header
/// digests the public values are REQUIRED to carry, from the feasibility freeze
/// record (`project/cases/expected.json`, single variant). Each is a
/// `(field-name, header byte range, frozen hex digest)` triple.
const ADMISSION_DIGESTS: [(&str, usize, usize, &str); 3] = [
    (
        "task_contract",
        0,
        32,
        "a71a7d71daef9bca8ceca270eae0fa180430d5ca2e0de84435e79ba87a1197f3",
    ),
    (
        "fork",
        64,
        96,
        "9e69ea039851cd44ab42db9125a69327633e012bfcf03fbc146c7201271e355b",
    ),
    (
        "author_oracle",
        128,
        160,
        "3a20d904646e7b4e910ffae2ef461c3849932a048f2512fd8c5995fd88109a3b",
    ),
];

/// Why a compressed-proof verification was rejected. Each variant is a
/// distinct, testable gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyReject {
    /// The proof byte length exceeds [`MAX_COMPRESSED_PROOF_BYTES`]; rejected
    /// BEFORE any deserialization.
    ProofTooLarge { len: usize, max: usize },
    /// The public values are shorter than the 192-byte header, so the admission
    /// gate cannot read the frozen digests.
    PublicValuesTooShort { len: usize },
    /// EXTERNAL ADMISSION (not cryptographic): a header digest does not equal
    /// the frozen constant for `field`.
    AdmissionMismatch {
        field: &'static str,
        expected: String,
        actual: String,
    },
    /// The (size-capped, admission-passing) bytes failed the cryptographic
    /// compressed-STARK check — a bad proof, wrong vk, or public values that do
    /// not match the commitment. Every crypto/parse failure surfaced by
    /// sp1-verifier collapses here.
    ProofRejected { detail: String },
}

/// The outcome of a verifier-only compressed-STARK check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// Every gate passed: the size cap, the external admission filter, and the
    /// cryptographic compressed-STARK check against the FIXED vk.
    Accepted,
    /// The proof was rejected; the reason names the gate that fired.
    Rejected(VerifyReject),
}

/// A verifier-only compressed-STARK checker bound to the FIXED frozen vkey hash.
///
/// Constructing it performs NO prover setup and NO vk regeneration — the
/// ELF -> vk identity is confirmed once out of band (ADR-0018 (f)); this type
/// only ever verifies, and only ever against the compiled-in frozen key.
#[derive(Debug, Clone)]
pub struct CompressedProofVerifier {
    /// The trusted vkey-hash bytes (bincode-serialized `[SP1Field; 8]`).
    vkey_hash: Vec<u8>,
    /// Input-size cap applied before deserialization.
    max_proof_bytes: usize,
}

impl CompressedProofVerifier {
    /// Production constructor: the trusted key is the compiled-in frozen vk
    /// hash. The caller CANNOT supply a different key — this is the only public
    /// way to build the verifier.
    pub fn frozen() -> Self {
        Self {
            vkey_hash: FROZEN_VKEY_HASH.to_vec(),
            max_proof_bytes: MAX_COMPRESSED_PROOF_BYTES,
        }
    }

    /// The frozen vk-hash bytes this verifier trusts (for tests / introspection).
    pub fn frozen_vkey_hash() -> &'static [u8] {
        FROZEN_VKEY_HASH
    }

    /// The input-size cap this verifier enforces before deserialization.
    pub fn max_proof_bytes(&self) -> usize {
        self.max_proof_bytes
    }

    /// TEST-ONLY: build with an explicit (wrong) vk hash to prove the crypto
    /// gate rejects any key other than the frozen one. Not on the production
    /// surface (`pub(crate)` + `cfg(test)`); external callers only get
    /// [`frozen`](Self::frozen).
    #[cfg(test)]
    pub(crate) fn with_vkey_hash_for_test(vkey_hash: Vec<u8>) -> Self {
        Self {
            vkey_hash,
            max_proof_bytes: MAX_COMPRESSED_PROOF_BYTES,
        }
    }

    /// Verify a compressed proof against the FIXED vk and the given public
    /// values. Applies the size gate, then the external admission gate, then
    /// the cryptographic compressed-STARK check. This is the ONLY sp1-verifier
    /// entry point this crate calls; there is no prove path.
    pub fn verify(&self, proof_bytes: &[u8], public_values: &[u8]) -> VerifyOutcome {
        // 1) SIZE GATE — refuse oversized input before it reaches the
        // bincode-backed deserializer (bounded-input defense).
        if proof_bytes.len() > self.max_proof_bytes {
            return VerifyOutcome::Rejected(VerifyReject::ProofTooLarge {
                len: proof_bytes.len(),
                max: self.max_proof_bytes,
            });
        }

        // 2) ADMISSION GATE — external policy, NOT cryptographic.
        if let Some(reject) = admission_check(public_values) {
            return VerifyOutcome::Rejected(reject);
        }

        // 3) CRYPTO GATE — verifier-only compressed-STARK check; the proof is
        // bound to the FIXED vk and the exact public-values bytes here.
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

/// The external admission filter (see [`ADMISSION_DIGESTS`]). Returns the first
/// mismatch, or `None` if the header carries every frozen digest. This is a
/// policy check on caller-supplied bytes; it is NOT cryptographic.
fn admission_check(pv: &[u8]) -> Option<VerifyReject> {
    if pv.len() < PV_HEADER_LEN {
        return Some(VerifyReject::PublicValuesTooShort { len: pv.len() });
    }
    for (field, lo, hi, expected) in ADMISSION_DIGESTS {
        let actual = hex::encode(&pv[lo..hi]);
        if actual != expected {
            return Some(VerifyReject::AdmissionMismatch {
                field,
                expected: expected.to_string(),
                actual,
            });
        }
    }
    None
}

/// Render a [`CompressedError`] as a stable, human-readable reason string.
fn compressed_error_detail(err: &CompressedError) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real frozen public values (782 B), whose header carries the three
    /// frozen admission digests. Used to reach the crypto gate in-crate.
    const FROZEN_PV: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/evm-zkvm/public-values.bin"
    ));

    /// The real frozen compressed proof (~1.27 MiB), transcoded to the
    /// verify-only `SP1Proof` bincode form.
    const FROZEN_PROOF: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/evm-zkvm/proof-compressed.bin"
    ));

    /// WRONG-VK REJECT: the same real proof + real pv, but verified against a
    /// vk hash that is NOT the frozen one, is rejected by the crypto gate. This
    /// uses the test-only injection seam that the public surface withholds, so
    /// it also documents that a caller cannot smuggle in an arbitrary vk.
    #[test]
    fn wrong_vkey_hash_is_rejected() {
        let mut bad = CompressedProofVerifier::frozen_vkey_hash().to_vec();
        bad[0] ^= 0xff; // corrupt the first field word
        let verifier = CompressedProofVerifier::with_vkey_hash_for_test(bad);
        match verifier.verify(FROZEN_PROOF, FROZEN_PV) {
            VerifyOutcome::Rejected(VerifyReject::ProofRejected { .. }) => {}
            other => panic!("expected ProofRejected for wrong vk, got {other:?}"),
        }
    }
}
