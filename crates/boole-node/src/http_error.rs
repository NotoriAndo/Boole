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
