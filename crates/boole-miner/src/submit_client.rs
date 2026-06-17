// Submit a fully-formed share to the dispatcher and announce tickets.
//
// Wire format mirrors dispatcher/src/httpServer.ts:
//   POST /ticket   { c, pk, n }
//   POST /submit   { c, pk, n, j, nonceS, bytes }
//
// Status mapping for /submit:
//   200 (accepted=true)  -> Accepted{share_hash_hex}
//   200 (accepted=false) -> Rejected{status:422, ...}
//   400 / 422            -> Rejected{status, error, reason, field, detail}
//   429                  -> RateLimited{reason}
//   anything else        -> NetworkError
use std::time::Duration;

use serde_json::Value;

use crate::http_client::{HttpClient, HttpError};

#[derive(Debug, Clone)]
pub struct AnnounceTicketInputs<'a> {
    pub c_hex: &'a str,
    pub pk_hex: &'a str,
    pub n_hex: &'a str,
}

#[derive(Debug, Clone)]
pub struct SubmitInputs<'a> {
    pub c_hex: &'a str,
    pub pk_hex: &'a str,
    pub n_hex: &'a str,
    pub j_hex: &'a str,
    pub nonce_s_hex: &'a str,
    pub canon_bytes: &'a [u8],
    /// N0.4b (Path 2) — the family seed the canon derives from. Sent as the
    /// optional `seedHex` body field so the node can persist it and later
    /// re-derive the canonical Lean source for deep verification. Empty for
    /// non-family submitters (the node treats absence/empty as "no seed").
    pub seed_hex: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnounceTicketResult {
    Observed {
        hash_hex: String,
    },
    Replay,
    Rejected {
        status: u16,
        error: String,
        reason: Option<String>,
    },
    NetworkError {
        cause: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitResult {
    Accepted {
        share_hash_hex: String,
    },
    Rejected {
        status: u16,
        error: String,
        reason: Option<String>,
        field: Option<String>,
        detail: Option<String>,
    },
    RateLimited {
        reason: String,
    },
    NetworkError {
        cause: String,
    },
}

/// Trait the mining loop consumes. Lets integration tests swap in a
/// recording stub without standing up a TcpListener.
pub trait Submitter: Send + Sync {
    fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult;
    fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult;
}

#[derive(Debug, Clone)]
pub struct SubmitClient {
    http: HttpClient,
}

impl SubmitClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_timeout(base_url, Duration::from_secs(30))
    }

    pub fn with_timeout(base_url: impl Into<String>, timeout: Duration) -> Self {
        Self {
            http: HttpClient::new(base_url, timeout),
        }
    }

    pub fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
        let body = serde_json::json!({
            "c": inputs.c_hex,
            "pk": inputs.pk_hex,
            "n": inputs.n_hex,
        });
        let res = match self.http.post_json("/ticket", &body) {
            Ok(r) => r,
            Err(err) => {
                return AnnounceTicketResult::NetworkError {
                    cause: render_http_error(&err),
                }
            }
        };
        let payload = parse_json_object(&res.body);
        if res.status == 200 {
            return AnnounceTicketResult::Observed {
                hash_hex: take_string(&payload, "hashHex").unwrap_or_default(),
            };
        }
        if res.status == 422 && take_string(&payload, "reason").as_deref() == Some("replay") {
            return AnnounceTicketResult::Replay;
        }
        AnnounceTicketResult::Rejected {
            status: res.status,
            error: take_string(&payload, "error").unwrap_or_else(|| "unknown".to_string()),
            reason: take_string(&payload, "reason"),
        }
    }

    pub fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
        let mut body = serde_json::json!({
            "c": inputs.c_hex,
            "pk": inputs.pk_hex,
            "n": inputs.n_hex,
            "j": inputs.j_hex,
            "nonceS": inputs.nonce_s_hex,
            "bytes": hex::encode(inputs.canon_bytes),
        });
        // N0.4b — only emit `seedHex` when present, so non-family submitters
        // keep the exact pre-N0.4b body shape.
        if !inputs.seed_hex.is_empty() {
            body["seedHex"] = serde_json::Value::String(inputs.seed_hex.to_string());
        }
        let res = match self.http.post_json("/submit", &body) {
            Ok(r) => r,
            Err(err) => {
                return SubmitResult::NetworkError {
                    cause: render_http_error(&err),
                }
            }
        };
        let payload = parse_json_object(&res.body);
        if res.status == 200 {
            let accepted_false = matches!(payload.get("accepted"), Some(Value::Bool(false)))
                || matches!(payload.get("ok"), Some(Value::Bool(false)));
            if accepted_false {
                return SubmitResult::Rejected {
                    status: 422,
                    error: take_string(&payload, "error")
                        .unwrap_or_else(|| "not_accepted".to_string()),
                    reason: take_string(&payload, "decision")
                        .or_else(|| take_string(&payload, "reason")),
                    field: None,
                    detail: None,
                };
            }
            return SubmitResult::Accepted {
                share_hash_hex: take_string(&payload, "shareHash").unwrap_or_default(),
            };
        }
        if res.status == 429 {
            return SubmitResult::RateLimited {
                reason: take_string(&payload, "reason")
                    .unwrap_or_else(|| "rate_limited".to_string()),
            };
        }
        if res.status == 400 || res.status == 422 {
            return SubmitResult::Rejected {
                status: res.status,
                error: take_string(&payload, "error").unwrap_or_else(|| "unknown".to_string()),
                reason: take_string(&payload, "reason"),
                field: take_string(&payload, "field"),
                detail: take_string(&payload, "detail"),
            };
        }
        SubmitResult::NetworkError {
            cause: format!("unexpected status {}", res.status),
        }
    }
}

impl Submitter for SubmitClient {
    fn announce_ticket(&self, inputs: AnnounceTicketInputs<'_>) -> AnnounceTicketResult {
        SubmitClient::announce_ticket(self, inputs)
    }
    fn submit(&self, inputs: SubmitInputs<'_>) -> SubmitResult {
        SubmitClient::submit(self, inputs)
    }
}

fn parse_json_object(body: &[u8]) -> serde_json::Map<String, Value> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    }
}

fn take_string(payload: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

fn render_http_error(err: &HttpError) -> String {
    err.to_string()
}
