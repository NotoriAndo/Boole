use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::bounty_registry::Bounty;

/// SC.2-f1 — domain tag folded into the bounty proof identity so the
/// hash can never collide with the envelope-hash surface (or any other
/// SHA-256 use in the protocol).
pub const BOUNTY_PROOF_HASH_DOMAIN: &[u8] = b"boole.bounty.proof.v1\0";

/// SC.2-f1 — the bounty proof identity:
/// `hex(SHA-256(BOUNTY_PROOF_HASH_DOMAIN ‖ artifact))` over the
/// verifier-effective artifact bytes (`BountyProofVerifier::
/// effective_artifact`). This is the value dedup, the registry, the
/// side pool, the audit ledger, and the block's promoted rows key on —
/// NOT the envelope hash, so submitter fields the verifier ignores
/// cannot mint distinct identities for one proof.
pub fn bounty_proof_hash_hex(artifact: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(BOUNTY_PROOF_HASH_DOMAIN);
    hasher.update(artifact);
    hex::encode(hasher.finalize())
}

/// Side-band evidence a verifier can hand back to the caller alongside
/// the accept/reject bit. Keys are merged verbatim into the bounty audit
/// ledger event (P1.4 "deep state verify" needs the evidence durably
/// pinned so a later offline re-check has the full verdict input set).
///
/// Stays an open-ended JSON `Map` so individual verifier kinds can
/// introduce additional evidence without churning the trait shape.
#[derive(Debug, Clone, Default)]
pub struct VerifyOutcome {
    pub accepted: bool,
    pub evidence: Map<String, Value>,
}

/// Pluggable backend that decides whether a submitted proof envelope is
/// accepted for a given bounty. Dispatched by `bounty.verifier.kind`.
///
/// `Ok(true)` accepts the proof (the route flips status to "solved").
/// `Ok(false)` rejects without surfacing an error to the caller (the route
/// keeps status "open" and records a 200 response with `accepted=false`).
/// `Err(detail)` signals an internal verifier failure (the route maps it to
/// 502 `verifier_error` so callers can distinguish "verifier said no" from
/// "verifier could not run").
pub trait BountyProofVerifier: Send + Sync {
    fn verify(&self, bounty: &Bounty, envelope: &Value) -> Result<bool, String>;

    /// Extended surface used by the bounty proof route to capture
    /// verifier-specific evidence (e.g. Lean `checkerArtifactHash`)
    /// alongside the accept/reject bit. The default implementation
    /// delegates to `verify` with an empty evidence map so existing
    /// implementors keep compiling unchanged.
    fn verify_with_evidence(
        &self,
        bounty: &Bounty,
        envelope: &Value,
    ) -> Result<VerifyOutcome, String> {
        Ok(VerifyOutcome {
            accepted: self.verify(bounty, envelope)?,
            evidence: Map::new(),
        })
    }

    /// SC.2-f1 — the exact bytes this verifier judges for `envelope`:
    /// the verifier-effective artifact. The proof identity
    /// (`bounty_proof_hash_hex`) commits THESE bytes, so two envelopes
    /// the verifier cannot tell apart are ONE proof regardless of
    /// submitter noise (salt fields, discarded prefixes). Default: the
    /// envelope's canonical JSON — correct for verifiers that judge the
    /// whole envelope verbatim. Must be cheap and must not run the
    /// actual verification (the route calls it before the dedup peek).
    fn effective_artifact(&self, _bounty: &Bounty, envelope: &Value) -> Result<Vec<u8>, String> {
        Ok(crate::canonical_json::canonicalize(envelope))
    }

    /// SC.2-f1 — verify against the artifact the route already derived
    /// and hashed, so the judged bytes and the proof identity cannot
    /// drift apart. The route always calls THIS entry point, passing the
    /// `effective_artifact` output; implementations that judge those
    /// bytes directly (e.g. the Lean checker) override it, while
    /// envelope-verbatim verifiers keep the default delegation.
    fn verify_artifact_with_evidence(
        &self,
        bounty: &Bounty,
        envelope: &Value,
        artifact: &[u8],
    ) -> Result<VerifyOutcome, String> {
        let _ = artifact;
        self.verify_with_evidence(bounty, envelope)
    }
}
