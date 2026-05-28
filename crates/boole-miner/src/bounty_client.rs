// Bounty mode: submit a bounty proof to the dispatcher.
//
// Wire format (P1.6d): `boole.signed.v1` envelope around a
// `boole.bounty.proof.v1` payload. The signer pk is the prover; the
// node rejects mismatches.
//
//   POST /bounties/:id/proof
//   Body: { schema: "boole.signed.v1", payload: <inner>, pk, signature }
//   inner = { schema: "boole.bounty.proof.v1", bountyId, proofHash,
//             prover, envelope }
//
// Status mapping (typed result kinds, no panics on HTTP errors):
//   200            -> Ok{accepted, duplicate, bounty}
//   400            -> BadRequest{error, detail}
//   401            -> BadRequest{error: "signature_invalid", ...}
//   404            -> NotFound{id}
//   409            -> Terminal{status}
//   501            -> NoVerifier{verifier_kind}
//   network / else -> NetworkError{cause}
//
// Caller decides retry policy. The miner does NOT auto-retry 409 / 501 —
// those are terminal for the (bounty_id, proof_hash) attempt.
//
// proof_hash is computed locally as SHA-256(envelope_bytes).
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use boole_core::SigningKeyV2;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::http_client::{percent_encode_component, HttpClient, HttpError};

pub const BOUNTY_PROOF_PAYLOAD_SCHEMA: &str = "boole.bounty.proof.v1";

/// P1.6a — window between local stamp time and the node's clock-skew
/// leeway. 300s is wide enough to absorb queueing, retries, and modest
/// host clock drift, while staying well under the operator-visible
/// "submit and walk away" expectation.
const BOUNTY_PROOF_VALID_BEFORE_WINDOW_SECS: u64 = 300;

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// P1.6b — opaque per-envelope nonce stamped into every
/// `boole.bounty.proof.v1` payload. The node persists `(signerPk,
/// nonce)` into the per-signer signed-envelope ledger and rejects
/// replays with 409 `nonce_replayed`. Uses 16 cryptographic bytes from
/// the OS RNG so two miners with synchronized clocks cannot collide.
fn fresh_signed_envelope_nonce() -> String {
    use rand_core::{OsRng, RngCore};
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub struct BountyProofInputs<'a> {
    pub bounty_id: &'a str,
    /// Ed25519 signing key. The miner derives the prover pk from this
    /// key; the node requires `envelope.pk == payload.prover`.
    pub signing_key: &'a SigningKeyV2,
    pub envelope: Value,
    pub envelope_bytes: &'a [u8],
    /// P2.10 — network id that scopes the produced `boole.signed.v1`
    /// envelope. Folded into the digest via `sign_for_network` and
    /// stamped on the wire body so the receiving node can verify
    /// `network_id == LocalNodeConfig::network_id` and reject
    /// cross-network replay with 403 `cross_network_rejected`.
    pub network_id: &'a str,
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
        let prover = inputs.signing_key.pk_hex();
        let valid_before = now_unix_secs().saturating_add(BOUNTY_PROOF_VALID_BEFORE_WINDOW_SECS);
        let nonce = fresh_signed_envelope_nonce();
        let payload = serde_json::json!({
            "schema": BOUNTY_PROOF_PAYLOAD_SCHEMA,
            "bountyId": inputs.bounty_id,
            "proofHash": proof_hash,
            "prover": prover,
            "envelope": inputs.envelope,
            "validBefore": valid_before,
            "nonce": nonce,
        });
        let signed = match inputs
            .signing_key
            .sign_for_network(&payload, Some(inputs.network_id))
        {
            Ok(s) => s,
            Err(detail) => {
                return BountyProofResult::NetworkError {
                    cause: format!("sign proof payload: {detail}"),
                }
            }
        };
        let body = serde_json::json!({
            "schema": signed.schema,
            "payload": signed.payload,
            "pk": signed.pk,
            "signature": signed.signature,
            "network_id": inputs.network_id,
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
