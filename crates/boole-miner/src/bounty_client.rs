// Bounty mode: submit a bounty proof to the dispatcher.
//
// Wire format mirrors dispatcher/src/httpServer.ts:
//   POST /bounties/:id/proof
//   Body: { proofHash, prover, envelope }  (proofHash + prover are hex32)
//
// Status mapping (typed result kinds, no panics on HTTP errors):
//   200            -> Ok{accepted, duplicate, bounty}
//   400            -> BadRequest{error, detail}
//   404            -> NotFound{id}
//   409            -> Terminal{status}
//   501            -> NoVerifier{verifier_kind}
//   network / else -> NetworkError{cause}
//
// Caller decides retry policy. The miner does NOT auto-retry 409 / 501 —
// those are terminal for the (bounty_id, proof_hash) attempt.
//
// proof_hash is computed locally as SHA-256(envelope_bytes).
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::http_client::{percent_encode_component, HttpClient, HttpError};

const ED25519_PK_BYTES: usize = 32;

#[derive(Debug, Clone)]
pub struct BountyProofInputs<'a> {
    pub bounty_id: &'a str,
    pub prover_pk: &'a [u8; ED25519_PK_BYTES],
    pub envelope: Value,
    pub envelope_bytes: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BountyProofResult {
    Ok {
        accepted: bool,
        duplicate: bool,
        bounty: Value,
    },
    NotFound {
        id: String,
    },
    Terminal {
        status: String,
    },
    NoVerifier {
        verifier_kind: String,
    },
    BadRequest {
        error: String,
        detail: Option<String>,
    },
    NetworkError {
        cause: String,
    },
}

#[derive(Debug, Clone)]
pub struct BountyClient {
    http: HttpClient,
}

impl BountyClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_timeout(base_url, Duration::from_secs(30))
    }

    pub fn with_timeout(base_url: impl Into<String>, timeout: Duration) -> Self {
        Self {
            http: HttpClient::new(base_url, timeout),
        }
    }

    pub fn submit_proof(&self, inputs: BountyProofInputs<'_>) -> BountyProofResult {
        let proof_hash = hex::encode(Sha256::digest(inputs.envelope_bytes));
        let body = serde_json::json!({
            "proofHash": proof_hash,
            "prover": hex::encode(inputs.prover_pk),
            "envelope": inputs.envelope,
        });
        let path = format!(
            "/bounties/{}/proof",
            percent_encode_component(inputs.bounty_id)
        );
        let res = match self.http.post_json(&path, &body) {
            Ok(r) => r,
            Err(err) => {
                return BountyProofResult::NetworkError {
                    cause: render_http_error(&err),
                }
            }
        };
        let payload = parse_json_object(&res.body);
        match res.status {
            200 => BountyProofResult::Ok {
                accepted: matches!(payload.get("accepted"), Some(Value::Bool(true))),
                duplicate: matches!(payload.get("duplicate"), Some(Value::Bool(true))),
                bounty: payload.get("bounty").cloned().unwrap_or(Value::Null),
            },
            404 => BountyProofResult::NotFound {
                id: inputs.bounty_id.to_string(),
            },
            409 => BountyProofResult::Terminal {
                status: take_string(&payload, "status").unwrap_or_else(|| "unknown".to_string()),
            },
            501 => BountyProofResult::NoVerifier {
                verifier_kind: take_string(&payload, "kind")
                    .unwrap_or_else(|| "unknown".to_string()),
            },
            400 => BountyProofResult::BadRequest {
                error: take_string(&payload, "error").unwrap_or_else(|| "unknown".to_string()),
                detail: take_string(&payload, "detail"),
            },
            other => BountyProofResult::NetworkError {
                cause: format!("unexpected status {other}"),
            },
        }
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
