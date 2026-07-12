use serde_json::{json, Map, Value};

/// Standardized JSON error envelope for boole-node 4xx/5xx HTTP responses.
///
/// Wire shape: `{ok:false, reason:<kebab>, field?, detail?, ...extra}`.
/// Mirrors the typed-rejection pattern used by pof's dispatcher so cross-runtime
/// CLI/agent code can pattern-match on `reason` without per-runtime branching.
#[derive(Debug, Clone)]
pub struct HttpError {
    pub status: u16,
    pub reason: &'static str,
    field: Option<String>,
    detail: Option<String>,
    extra: Map<String, Value>,
}

impl HttpError {
    fn new(status: u16, reason: &'static str) -> Self {
        Self {
            status,
            reason,
            field: None,
            detail: None,
            extra: Map::new(),
        }
    }

    pub fn unexpected_field(field: impl Into<String>) -> Self {
        Self::new(400, "unexpected_field").with_field(field)
    }

    pub fn missing_field(field: impl Into<String>) -> Self {
        Self::new(400, "missing_field").with_field(field)
    }

    pub fn bad_hex(field: impl Into<String>) -> Self {
        Self::new(400, "bad_hex").with_field(field)
    }

    pub fn malformed_pk() -> Self {
        Self::new(400, "malformed_pk")
    }

    /// `POST /submit` (N2.1) when the envelope carries no agent-wallet
    /// `session` block and the node was booted with the secure default
    /// `allow_anonymous_submit = false`. A bare prover pk cannot prove it
    /// owns the reward it claims, so anonymous submits are rejected before
    /// admission rather than silently credited. Operators opt into the
    /// legacy unauthenticated path explicitly (`--allow-anonymous-submit`).
    pub fn unauthenticated_submit() -> Self {
        Self::new(401, "unauthenticated_submit")
    }

    /// Returned by `/sessions*` routes when the node was booted without
    /// `LocalNodeConfig.session_registry_path`. The agent-wallet plan
    /// keeps the registry opt-in so legacy embeddings can stay quiet.
    pub fn session_registry_disabled() -> Self {
        Self::new(400, "session_registry_disabled")
    }

    /// `GET /sessions/{pk}` / `POST /sessions/{pk}/revoke` when the
    /// session registry is enabled but the key was never registered.
    pub fn session_not_found(session_pk: impl Into<String>) -> Self {
        Self::new(404, "session_not_found")
            .with_extra("sessionPk", Value::String(session_pk.into()))
    }

    /// `POST /submit` when the envelope names a `submittedBy` that the
    /// session registry has never seen. The agent-wallet plan binds the
    /// submitter to a registered `SessionState`, so an unknown key cannot
    /// pass admission even if the POW body is otherwise valid.
    pub fn session_unknown(session_pk: impl Into<String>) -> Self {
        Self::new(404, "session_unknown").with_extra("sessionPk", Value::String(session_pk.into()))
    }

    /// `POST /submit` when the named session exists in the registry but
    /// has been revoked. Revocation is sticky once recorded.
    pub fn session_revoked(session_pk: impl Into<String>) -> Self {
        Self::new(403, "session_revoked").with_extra("sessionPk", Value::String(session_pk.into()))
    }

    /// `POST /submit` when the named session exists but is outside its
    /// activation/expiry window or otherwise fails policy validation at the
    /// current node height.
    pub fn session_denied(session_pk: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(403, "session_denied")
            .with_extra("sessionPk", Value::String(session_pk.into()))
            .with_detail(detail)
    }

    /// P1.6 (audit) — a signed-envelope route where the signature is valid but
    /// the signer is NOT AUTHORIZED for the action (e.g. registering/revoking a
    /// session it does not own, or announcing/restatusing a bounty without
    /// being on the operator allowlist). Authentication ≠ authorization: a valid
    /// signature only proves WHO signed, not that they may perform the write.
    pub fn unauthorized_signer(detail: impl Into<String>) -> Self {
        Self::new(403, "unauthorized_signer").with_detail(detail)
    }

    /// `POST /submit` when the envelope's `rewardRecipient` does not match
    /// the registered session's `fixedRewardRecipient`. The plan binds
    /// the reward sink to the session at register-time so a compromised
    /// agent cannot redirect rewards.
    pub fn reward_recipient_mismatch(
        expected: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self::new(403, "reward_recipient_mismatch")
            .with_extra("expected", Value::String(expected.into()))
            .with_extra("actual", Value::String(actual.into()))
    }

    /// `POST /submit` when the `(submittedBy, nonce)` pair has already
    /// been admitted. Dedup state is persistent (NDJSON ledger) so
    /// restart-replay still rejects.
    pub fn nonce_replayed(session_pk: impl Into<String>, nonce: impl Into<String>) -> Self {
        Self::new(409, "nonce_replayed")
            .with_extra("sessionPk", Value::String(session_pk.into()))
            .with_extra("nonce", Value::String(nonce.into()))
    }

    pub fn receipt_store_disabled() -> Self {
        Self::new(400, "receipt_store_disabled")
    }

    pub fn receipt_not_found(receipt_id: impl Into<String>) -> Self {
        Self::new(404, "receipt_not_found")
            .with_extra("receiptId", Value::String(receipt_id.into()))
    }

    pub fn payment_required(
        scheme: impl Into<String>,
        amount: impl Into<String>,
        request_hash: impl Into<String>,
        pay_to: impl Into<String>,
        x402_version: impl Into<String>,
    ) -> Self {
        Self::new(402, "payment_required")
            .with_extra("scheme", Value::String(scheme.into()))
            .with_extra("amount", Value::String(amount.into()))
            .with_extra("requestHash", Value::String(request_hash.into()))
            .with_extra("payTo", Value::String(pay_to.into()))
            .with_extra("x402Version", Value::String(x402_version.into()))
    }

    pub fn payment_invalid(scheme: impl Into<String>, x402_version: impl Into<String>) -> Self {
        Self::new(403, "payment_invalid")
            .with_extra("scheme", Value::String(scheme.into()))
            .with_extra("x402Version", Value::String(x402_version.into()))
    }

    pub fn x402_version_unsupported(
        x402_version: impl Into<String>,
        accepted_versions: Vec<String>,
    ) -> Self {
        Self::new(400, "x402_version_unsupported")
            .with_extra("x402Version", Value::String(x402_version.into()))
            .with_extra("acceptedVersions", json!(accepted_versions))
    }

    pub fn work_not_found(id: impl Into<String>) -> Self {
        Self::new(404, "work_not_found").with_extra("id", Value::String(id.into()))
    }

    pub fn bounty_not_found(id: impl Into<String>) -> Self {
        Self::new(404, "bounty_not_found").with_extra("id", Value::String(id.into()))
    }

    pub fn bad_proof_hash() -> Self {
        Self::new(400, "bad_proof_hash")
    }

    /// §SC W1.b — the payload's claimed `proofHash` is well-formed hex-32
    /// but does not equal the server-derived hash of the accompanying
    /// proof envelope (`hex(SHA-256(canonical_json(envelope)))`). Without
    /// this rejection the claimed string would flow into the audit
    /// ledger, the bounty side pool, and the `block.v3` preimage as "the
    /// hash of the verified proof" while being submitter-chosen.
    pub fn proof_hash_mismatch(expected: impl Into<String>, got: impl Into<String>) -> Self {
        Self::new(400, "proof_hash_mismatch")
            .with_extra("expected", Value::String(expected.into()))
            .with_extra("got", Value::String(got.into()))
    }

    pub fn bad_prover() -> Self {
        Self::new(400, "bad_prover")
    }

    pub fn no_verifier(kind: impl Into<String>) -> Self {
        Self::new(501, "no_verifier").with_extra("kind", Value::String(kind.into()))
    }

    pub fn bad_envelope(detail: impl Into<String>) -> Self {
        Self::new(400, "bad_envelope").with_detail(detail)
    }

    pub fn signature_invalid() -> Self {
        Self::new(401, "signature_invalid")
    }

    /// P2.10 — outer `boole.signed.v1` envelope carries a `network_id` that
    /// does not match the network this node is pinned to
    /// (`LocalNodeConfig::network_id`). HTTP 403 because the request is
    /// syntactically valid and cryptographically sound but policy-rejected:
    /// the signer scoped the signature to one network, the verifier runs on
    /// another, and replaying it cross-network would re-bind work to the
    /// wrong reward / reputation ledger. `expected` is the node's pinned
    /// network_id; `got` is the wire envelope's claimed network_id.
    pub fn cross_network_rejected(expected: impl Into<String>, got: impl Into<String>) -> Self {
        Self::new(403, "cross_network_rejected")
            .with_extra("expected", Value::String(expected.into()))
            .with_extra("got", Value::String(got.into()))
    }

    /// P1.6a — signed inner payload's `validBefore` has slipped past the
    /// server's clock. Carries both `validBefore` (what the signer claimed)
    /// and `now` (the server's current Unix-second view) so the caller can
    /// distinguish stale envelope from skewed clock without log-scraping.
    pub fn envelope_expired(valid_before: u64, now: u64) -> Self {
        Self::new(401, "envelope_expired")
            .with_extra("validBefore", json!(valid_before))
            .with_extra("now", json!(now))
    }

    /// P1.6b — a `(signerPk, nonce)` pair the inner payload claims has
    /// already been burned by the per-signer signed-envelope nonce ledger.
    /// Sibling of `nonce_replayed` (which uses `sessionPk` for the
    /// session-bound `/submit` flow) but carries `signerPk` so a single
    /// signer cannot rebind a stolen envelope to a different request.
    /// The 409 mirrors the existing dedup-rejection precedent so clients
    /// can route both kinds through one branch.
    pub fn signed_envelope_nonce_replayed(
        signer_pk: impl Into<String>,
        nonce: impl Into<String>,
    ) -> Self {
        Self::new(409, "nonce_replayed")
            .with_extra("signerPk", Value::String(signer_pk.into()))
            .with_extra("nonce", Value::String(nonce.into()))
    }

    pub fn bad_payload(field: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::new(400, "bad_payload")
            .with_field(field)
            .with_detail(detail)
    }

    pub fn bounty_already_exists(id: impl Into<String>) -> Self {
        Self::new(409, "bounty_already_exists").with_extra("id", Value::String(id.into()))
    }

    pub fn bad_status_value(value: impl Into<String>) -> Self {
        Self::new(400, "bad_status_value").with_extra("newStatus", Value::String(value.into()))
    }

    pub fn bounty_id_mismatch(url_id: impl Into<String>, payload_id: impl Into<String>) -> Self {
        Self::new(400, "bounty_id_mismatch")
            .with_extra("urlId", Value::String(url_id.into()))
            .with_extra("payloadId", Value::String(payload_id.into()))
    }

    pub fn invalid_status_transition(detail: impl Into<String>) -> Self {
        Self::new(400, "invalid_status_transition").with_detail(detail)
    }

    pub fn bounty_terminal(status: impl Into<String>) -> Self {
        Self::new(409, "bounty_terminal").with_extra("status", Value::String(status.into()))
    }

    pub fn verifier_error(detail: impl Into<String>) -> Self {
        Self::new(502, "verifier_error").with_detail(detail)
    }

    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::new(400, "bad_request").with_detail(detail)
    }

    pub fn body_too_large(limit: usize, actual: usize) -> Self {
        Self::new(413, "body_too_large")
            .with_extra("limitBytes", json!(limit))
            .with_extra("actualBytes", json!(actual))
    }

    /// P1.7 — per-source-IP HTTP rate limit breach. Mirrors the wire
    /// shape every other 4xx envelope uses (`reason` is the typed key,
    /// `quota` and `windowMs` describe the policy so callers can back
    /// off without scraping logs).
    pub fn rate_limited(quota: usize, window_ms: u128) -> Self {
        Self::new(429, "rate_limited")
            .with_extra("quota", json!(quota))
            .with_extra("windowMs", json!(window_ms as u64))
    }

    /// P1.7 — the request exceeded its route's processing-time budget
    /// (default 30 s; bounty-proof 90 s). Replaces the bare empty body a
    /// `tower_http::TimeoutLayer` emits with the typed envelope every other
    /// 4xx/5xx response uses, so a timed-out caller can branch on `reason`.
    pub fn request_timeout() -> Self {
        Self::new(408, "request_timeout")
            .with_detail("request exceeded the server processing-time limit")
    }

    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::new(404, "not_found").with_detail(detail)
    }

    pub fn internal(detail: impl Into<String>) -> Self {
        Self::new(500, "internal_error").with_detail(detail)
    }

    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_extra(mut self, key: &str, value: Value) -> Self {
        self.extra.insert(key.to_string(), value);
        self
    }

    pub fn into_json(self) -> Value {
        let mut map = Map::new();
        map.insert("ok".to_string(), Value::Bool(false));
        map.insert("reason".to_string(), Value::String(self.reason.to_string()));
        if let Some(field) = self.field {
            map.insert("field".to_string(), Value::String(field));
        }
        if let Some(detail) = self.detail {
            map.insert("detail".to_string(), Value::String(detail));
        }
        for (k, v) in self.extra {
            map.insert(k, v);
        }
        Value::Object(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unexpected_field_envelope_matches_contract() {
        let err = HttpError::unexpected_field("j");
        assert_eq!(err.status, 400);
        assert_eq!(err.reason, "unexpected_field");
        assert_eq!(
            err.into_json(),
            json!({"ok": false, "reason": "unexpected_field", "field": "j"})
        );
    }

    #[test]
    fn body_too_large_carries_limit_and_actual() {
        let err = HttpError::body_too_large(1024, 4096);
        assert_eq!(err.status, 413);
        assert_eq!(
            err.into_json(),
            json!({
                "ok": false,
                "reason": "body_too_large",
                "limitBytes": 1024,
                "actualBytes": 4096,
            })
        );
    }

    #[test]
    fn request_timeout_envelope_is_typed_408() {
        let err = HttpError::request_timeout();
        assert_eq!(err.status, 408);
        assert_eq!(err.reason, "request_timeout");
        assert_eq!(
            err.into_json(),
            json!({
                "ok": false,
                "reason": "request_timeout",
                "detail": "request exceeded the server processing-time limit",
            })
        );
    }

    #[test]
    fn envelope_expired_carries_valid_before_and_now() {
        let err = HttpError::envelope_expired(100, 1_000_000);
        assert_eq!(err.status, 401);
        assert_eq!(
            err.into_json(),
            json!({
                "ok": false,
                "reason": "envelope_expired",
                "validBefore": 100,
                "now": 1_000_000,
            })
        );
    }

    #[test]
    fn signed_envelope_nonce_replayed_carries_signer_pk_and_nonce() {
        let err = HttpError::signed_envelope_nonce_replayed("aabb", "n-1");
        assert_eq!(err.status, 409);
        assert_eq!(
            err.into_json(),
            json!({
                "ok": false,
                "reason": "nonce_replayed",
                "signerPk": "aabb",
                "nonce": "n-1",
            })
        );
    }

    #[test]
    fn not_found_carries_detail() {
        let err = HttpError::not_found("no route for POST /missing");
        assert_eq!(err.status, 404);
        assert_eq!(
            err.into_json(),
            json!({"ok": false, "reason": "not_found", "detail": "no route for POST /missing"})
        );
    }

    #[test]
    fn cross_network_rejected_carries_expected_and_got() {
        let err = HttpError::cross_network_rejected("boole-testnet", "boole-dev");
        assert_eq!(err.status, 403);
        assert_eq!(
            err.into_json(),
            json!({
                "ok": false,
                "reason": "cross_network_rejected",
                "expected": "boole-testnet",
                "got": "boole-dev",
            })
        );
    }
}
