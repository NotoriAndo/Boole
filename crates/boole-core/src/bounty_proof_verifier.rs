use serde_json::{Map, Value};

use crate::bounty_registry::Bounty;

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
}
