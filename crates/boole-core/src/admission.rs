use crate::{
    check_submission_pow, share_hash, ticket, validate_proof_package, CalibrationReport, Hex32,
    PoolShare, RateLimiter, SharePool,
};
use num_bigint::BigUint;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub struct AdmissionDeps<'a> {
    pub cfg: &'a CalibrationReport,
    pub rate_limiter: &'a mut RateLimiter,
    pub pool: &'a mut SharePool,
    pub now: i64,
    pub ip: &'a str,
    pub body: &'a Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionDecision {
    Accepted {
        share_hash: Hex32,
    },
    Rejected {
        status: AdmissionStatus,
        error: AdmissionError,
        rejection: RejectionReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionStatus {
    BadRequest,
    UnprocessableEntity,
    RateLimited,
}

impl AdmissionStatus {
    fn code(self) -> u16 {
        match self {
            Self::BadRequest => 400,
            Self::UnprocessableEntity => 422,
            Self::RateLimited => 429,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionError {
    MissingField { field: String },
    BadHex { field: String, detail: String },
    Ticket { reason: TicketRejectReason },
    Validator { reason: Value },
    SubmitPow { reason: SubmitPowRejectReason },
    RateLimited { reason: RateLimitRejectReason },
    SharePool { reason: SharePoolRejectReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketRejectReason {
    AboveTTicket,
    Unobserved,
}

impl TicketRejectReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::AboveTTicket => "above_T_ticket",
            Self::Unobserved => "unobserved",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitPowRejectReason {
    AboveTSubmit,
}

impl SubmitPowRejectReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::AboveTSubmit => "above_T_submit",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitRejectReason {
    IpQuota,
    PkQuota,
}

impl RateLimitRejectReason {
    fn from_str(value: &str) -> Self {
        match value {
            "ip_quota" => Self::IpQuota,
            "pk_quota" => Self::PkQuota,
            other => panic!("unknown rate-limit reason {other}"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::IpQuota => "ip_quota",
            Self::PkQuota => "pk_quota",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharePoolRejectReason {
    Duplicate,
    PkCapExceeded,
    StaleC,
}

impl SharePoolRejectReason {
    fn from_str(value: &str) -> Self {
        match value {
            "duplicate" => Self::Duplicate,
            "pk_cap_exceeded" => Self::PkCapExceeded,
            "stale_c" => Self::StaleC,
            other => panic!("unknown share-pool reason {other}"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::PkCapExceeded => "pk_cap_exceeded",
            Self::StaleC => "stale_c",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectionReason {
    BadRequest { field: String },
    Decode { field: String, detail: String },
    Ticket { detail: TicketRejectReason },
    Validator { reason: Value },
    SubmitPow { detail: SubmitPowRejectReason },
    RateLimit { quota: RateLimitRejectReason },
    SharePool { detail: SharePoolRejectReason },
}

pub fn admit_submission_typed(deps: AdmissionDeps<'_>) -> AdmissionDecision {
    for field in ["c", "pk", "n", "j", "nonceS", "bytes"] {
        if !deps.body.contains_key(field) {
            return reject(
                AdmissionStatus::BadRequest,
                AdmissionError::MissingField {
                    field: field.to_string(),
                },
                RejectionReason::BadRequest {
                    field: field.to_string(),
                },
            );
        }
    }

    let c_hex = string_field(deps.body, "c");
    let pk_hex = string_field(deps.body, "pk");
    let n_hex = string_field(deps.body, "n");
    let j_hex = string_field(deps.body, "j");
    let nonce_s_hex = string_field(deps.body, "nonceS");
    let bytes_hex = string_field(deps.body, "bytes");

    let c = match hex32(c_hex) {
        Ok(v) => v,
        Err(detail) => return decode_reject("c", detail),
    };
    let pk = match hex32(pk_hex) {
        Ok(v) => v,
        Err(detail) => return decode_reject("pk", detail),
    };
    let n = match hex32(n_hex) {
        Ok(v) => v,
        Err(detail) => return decode_reject("n", detail),
    };
    let j = match hex32(j_hex) {
        Ok(v) => v,
        Err(detail) => return decode_reject("j", detail),
    };
    let nonce_s = match hex32(nonce_s_hex) {
        Ok(v) => v,
        Err(detail) => return decode_reject("nonceS", detail),
    };
    let package_bytes = match hex::decode(bytes_hex) {
        Ok(v) => v,
        Err(err) => return decode_reject("bytes", err.to_string()),
    };

    let t_ticket = BigUint::parse_bytes(strip_0x(&deps.cfg.T_ticket).as_bytes(), 16)
        .expect("cfg.T_ticket parses");
    let ticket_result = ticket(&c, &pk, &n, &t_ticket);
    if !ticket_result.valid {
        return reject(
            AdmissionStatus::UnprocessableEntity,
            AdmissionError::Ticket {
                reason: TicketRejectReason::AboveTTicket,
            },
            RejectionReason::Ticket {
                detail: TicketRejectReason::AboveTTicket,
            },
        );
    }
    if !deps.rate_limiter.has_observed_ticket(pk_hex, c_hex, n_hex) {
        return reject(
            AdmissionStatus::UnprocessableEntity,
            AdmissionError::Ticket {
                reason: TicketRejectReason::Unobserved,
            },
            RejectionReason::Ticket {
                detail: TicketRejectReason::Unobserved,
            },
        );
    }

    let validator = validate_proof_package(&package_bytes, deps.cfg);
    if !validator
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let reason = validator.get("reason").expect("validator reason").clone();
        return reject(
            AdmissionStatus::UnprocessableEntity,
            AdmissionError::Validator {
                reason: reason.clone(),
            },
            RejectionReason::Validator { reason },
        );
    }

    let canon_hash = Hex32::from_bytes(sha256_32(&package_bytes));
    let pow = check_submission_pow(
        c.as_bytes(),
        pk.as_bytes(),
        nonce_s.as_bytes(),
        canon_hash.as_bytes(),
        deps.cfg,
    );
    if !pow.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return reject(
            AdmissionStatus::UnprocessableEntity,
            AdmissionError::SubmitPow {
                reason: SubmitPowRejectReason::AboveTSubmit,
            },
            RejectionReason::SubmitPow {
                detail: SubmitPowRejectReason::AboveTSubmit,
            },
        );
    }

    let rate = deps.rate_limiter.peek(deps.now, deps.ip, pk_hex, c_hex);
    if !rate
        .get("allowed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let reason = RateLimitRejectReason::from_str(
            rate.get("reason")
                .and_then(Value::as_str)
                .expect("rate reason"),
        );
        return reject(
            AdmissionStatus::RateLimited,
            AdmissionError::RateLimited { reason },
            RejectionReason::RateLimit { quota: reason },
        );
    }

    let share_hash = share_hash(&c, &pk, &n, &j, &canon_hash);
    let pool_result = deps.pool.accept(PoolShare {
        label: "admission".to_string(),
        pk: pk_hex.to_string(),
        n: n_hex.to_string(),
        j: j_hex.to_string(),
        c: c_hex.to_string(),
    });
    if let Some(reason) = pool_result.reason() {
        let reason = SharePoolRejectReason::from_str(reason);
        return reject(
            AdmissionStatus::UnprocessableEntity,
            AdmissionError::SharePool { reason },
            RejectionReason::SharePool { detail: reason },
        );
    }
    deps.rate_limiter.commit(deps.now, deps.ip, pk_hex, c_hex);

    AdmissionDecision::Accepted { share_hash }
}

pub fn admit_submission_json(decision: &AdmissionDecision) -> Value {
    match decision {
        AdmissionDecision::Accepted { share_hash } => json!({
            "accepted": true,
            "status": 200,
            "responseStatus": "accepted",
            "shareHash": share_hash.to_hex()
        }),
        AdmissionDecision::Rejected {
            status,
            error,
            rejection,
        } => rejected_json(*status, error, rejection),
    }
}

pub fn admit_submission(deps: AdmissionDeps<'_>) -> Value {
    let decision = admit_submission_typed(deps);
    admit_submission_json(&decision)
}

fn reject(
    status: AdmissionStatus,
    error: AdmissionError,
    rejection: RejectionReason,
) -> AdmissionDecision {
    AdmissionDecision::Rejected {
        status,
        error,
        rejection,
    }
}

fn decode_reject(field: &str, detail: String) -> AdmissionDecision {
    reject(
        AdmissionStatus::BadRequest,
        AdmissionError::BadHex {
            field: field.to_string(),
            detail: detail.clone(),
        },
        RejectionReason::Decode {
            field: field.to_string(),
            detail,
        },
    )
}

fn rejected_json(
    status: AdmissionStatus,
    error: &AdmissionError,
    rejection: &RejectionReason,
) -> Value {
    match error {
        AdmissionError::MissingField { field } => json!({
            "accepted": false,
            "status": status.code(),
            "error": "missing field",
            "field": field,
            "rejection": rejection_json(rejection)
        }),
        AdmissionError::BadHex { field, .. } => json!({
            "accepted": false,
            "status": status.code(),
            "error": "bad hex",
            "field": field,
            "rejection": rejection_json(rejection)
        }),
        AdmissionError::Ticket { reason } => json!({
            "accepted": false,
            "status": status.code(),
            "error": "ticket",
            "reason": reason.as_str(),
            "rejection": rejection_json(rejection)
        }),
        AdmissionError::Validator { reason } => json!({
            "accepted": false,
            "status": status.code(),
            "error": "validator",
            "reason": reason,
            "rejection": rejection_json(rejection)
        }),
        AdmissionError::SubmitPow { reason } => json!({
            "accepted": false,
            "status": status.code(),
            "error": "submit_pow",
            "reason": reason.as_str(),
            "rejection": rejection_json(rejection)
        }),
        AdmissionError::RateLimited { reason } => json!({
            "accepted": false,
            "status": status.code(),
            "error": "rate_limited",
            "reason": reason.as_str(),
            "rejection": rejection_json(rejection)
        }),
        AdmissionError::SharePool { reason } => json!({
            "accepted": false,
            "status": status.code(),
            "error": "share_pool",
            "reason": reason.as_str(),
            "rejection": rejection_json(rejection)
        }),
    }
}

fn rejection_json(rejection: &RejectionReason) -> Value {
    match rejection {
        RejectionReason::BadRequest { field } => {
            json!({ "stage": "bad_request", "field": field })
        }
        RejectionReason::Decode { field, detail } => {
            json!({ "stage": "decode", "field": field, "detail": detail })
        }
        RejectionReason::Ticket { detail } => {
            json!({ "stage": "ticket", "detail": detail.as_str() })
        }
        RejectionReason::Validator { reason } => {
            json!({ "stage": "validator", "reason": reason })
        }
        RejectionReason::SubmitPow { detail } => {
            json!({ "stage": "submit_pow", "detail": detail.as_str() })
        }
        RejectionReason::RateLimit { quota } => {
            json!({ "stage": "rate_limit", "quota": quota.as_str() })
        }
        RejectionReason::SharePool { detail } => {
            json!({ "stage": "share_pool", "detail": detail.as_str() })
        }
    }
}

fn string_field<'a>(body: &'a Map<String, Value>, field: &str) -> &'a str {
    body.get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{field} must be string"))
}

fn hex32(value: &str) -> Result<Hex32, String> {
    Hex32::from_hex(value).map_err(|err| err.to_string())
}

fn strip_0x(value: &str) -> &str {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value)
}

fn sha256_32(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    digest.into()
}
