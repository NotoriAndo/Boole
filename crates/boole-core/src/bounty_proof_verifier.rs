use serde_json::Value;

use crate::bounty_registry::Bounty;

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
}
