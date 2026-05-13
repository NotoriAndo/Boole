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
        Self::new(400, "malformed-pk")
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

    pub fn work_not_found(id: impl Into<String>) -> Self {
        Self::new(404, "work_not_found").with_extra("id", Value::String(id.into()))
    }

    pub fn bounty_not_found(id: impl Into<String>) -> Self {
        Self::new(404, "bounty_not_found").with_extra("id", Value::String(id.into()))
    }

    pub fn bad_proof_hash() -> Self {
        Self::new(400, "bad_proof_hash")
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
    fn not_found_carries_detail() {
        let err = HttpError::not_found("no route for POST /missing");
        assert_eq!(err.status, 404);
        assert_eq!(
            err.into_json(),
            json!({"ok": false, "reason": "not_found", "detail": "no route for POST /missing"})
        );
    }
}
