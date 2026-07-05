use crate::{
    check_submission_pow_with_policy, find_target_seed_j_index, share_hash, ticket,
    validate_proof_package_with_policy, validation_reason_json, AdmissionDecision, AdmissionError,
    AdmissionStatus, CalibrationPolicy, Hex32, ParsedSubmission, PoolShare, RateLimitResult,
    RateLimiter, RejectionReason, SeedBindingRejectReason, SharePool, SubmissionPowResult,
    SubmitPowRejectReason, TicketAdmissionResult, TicketRejectReason, ValidationResult,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub struct AdmissionDeps<'a> {
    pub policy: &'a CalibrationPolicy,
    pub rate_limiter: &'a mut RateLimiter,
    pub pool: &'a mut SharePool,
    pub now: i64,
    pub ip: &'a str,
    pub body: &'a Map<String, Value>,
}

pub struct AdmissionParsedDeps<'a> {
    pub policy: &'a CalibrationPolicy,
    pub rate_limiter: &'a mut RateLimiter,
    pub pool: &'a mut SharePool,
    pub now: i64,
    pub ip: &'a str,
    pub submission: &'a ParsedSubmission,
}

pub fn check_admission_ticket(ticket_valid: bool, observed: bool) -> TicketAdmissionResult {
    if !ticket_valid {
        return TicketAdmissionResult::Rejected {
            reason: TicketRejectReason::AboveTTicket,
        };
    }
    if !observed {
        return TicketAdmissionResult::Rejected {
            reason: TicketRejectReason::Unobserved,
        };
    }
    TicketAdmissionResult::Allowed
}

pub fn parse_submission_body(
    body: &Map<String, Value>,
) -> Result<ParsedSubmission, AdmissionDecision> {
    for field in ["c", "pk", "n", "j", "nonceS", "bytes"] {
        if !body.contains_key(field) {
            return Err(reject(
                AdmissionStatus::BadRequest,
                AdmissionError::MissingField {
                    field: field.to_string(),
                },
                RejectionReason::BadRequest {
                    field: field.to_string(),
                },
            ));
        }
    }

    let c_hex = string_field(body, "c")?.to_string();
    let pk_hex = string_field(body, "pk")?.to_string();
    let n_hex = string_field(body, "n")?.to_string();
    let j_hex = string_field(body, "j")?.to_string();
    let nonce_s_hex = string_field(body, "nonceS")?.to_string();
    let bytes_hex = string_field(body, "bytes")?;

    let c = hex32(&c_hex).map_err(|detail| decode_reject("c", detail))?;
    let pk = hex32(&pk_hex).map_err(|detail| decode_reject("pk", detail))?;
    let n = hex32(&n_hex).map_err(|detail| decode_reject("n", detail))?;
    let j = hex32(&j_hex).map_err(|detail| decode_reject("j", detail))?;
    let nonce_s = hex32(&nonce_s_hex).map_err(|detail| decode_reject("nonceS", detail))?;
    let package_bytes =
        hex::decode(bytes_hex).map_err(|err| decode_reject("bytes", err.to_string()))?;

    // N0.4b — `seedHex` is OPTIONAL: a body without it still parses (no
    // reject), so pre-N0.4b miners and the submit-lean/bounty flows are
    // unaffected. Empty string means "no persisted seed" downstream.
    let seed_hex = match body.get("seedHex") {
        Some(_) => string_field(body, "seedHex")?.to_string(),
        None => String::new(),
    };

    Ok(ParsedSubmission {
        c_hex,
        pk_hex,
        n_hex,
        j_hex,
        nonce_s_hex,
        c,
        pk,
        n,
        j,
        nonce_s,
        package_bytes,
        seed_hex,
    })
}

pub fn admit_submission_typed(deps: AdmissionDeps<'_>) -> AdmissionDecision {
    let submission = match parse_submission_body(deps.body) {
        Ok(submission) => submission,
        Err(decision) => return decision,
    };
    admit_parsed_submission_typed(AdmissionParsedDeps {
        policy: deps.policy,
        rate_limiter: deps.rate_limiter,
        pool: deps.pool,
        now: deps.now,
        ip: deps.ip,
        submission: &submission,
    })
}

pub fn admit_parsed_submission_typed(deps: AdmissionParsedDeps<'_>) -> AdmissionDecision {
    let s = deps.submission;
    let ticket_result = ticket(&s.c, &s.pk, &s.n, &deps.policy.thresholds.t_ticket);
    let ticket_check = check_admission_ticket(
        ticket_result.valid,
        deps.rate_limiter
            .has_observed_ticket(&s.pk_hex, &s.c_hex, &s.n_hex),
    );
    if let TicketAdmissionResult::Rejected { reason } = ticket_check {
        return reject(
            AdmissionStatus::UnprocessableEntity,
            AdmissionError::Ticket { reason },
            RejectionReason::Ticket { detail: reason },
        );
    }

    // Seed↔prev-block binding — a CLAIMED `seedHex` must be one the chain
    // posed: `target_seed(c, pk, n, j_index)` for some in-bound `j_index`.
    // An empty `seedHex` stays admissible (pre-N0.4b legacy posture;
    // mandatory seeds are N3.3 scope). Replay enforces the same rule in
    // `verify_selected_share_evidence`.
    if !s.seed_hex.is_empty() && find_target_seed_j_index(&s.c, &s.pk, &s.n, &s.seed_hex).is_none()
    {
        return reject(
            AdmissionStatus::UnprocessableEntity,
            AdmissionError::SeedBinding {
                reason: SeedBindingRejectReason::NotDerivedFromContext,
            },
            RejectionReason::SeedBinding {
                detail: SeedBindingRejectReason::NotDerivedFromContext,
            },
        );
    }

    let validator = validate_proof_package_with_policy(&s.package_bytes, deps.policy);
    if let ValidationResult::Err { reason } = validator {
        return reject(
            AdmissionStatus::UnprocessableEntity,
            AdmissionError::Validator {
                reason: reason.clone(),
            },
            RejectionReason::Validator { reason },
        );
    }

    let canon_hash = Hex32::from_bytes(sha256_32(&s.package_bytes));
    let pow = check_submission_pow_with_policy(
        s.c.as_bytes(),
        s.pk.as_bytes(),
        s.nonce_s.as_bytes(),
        canon_hash.as_bytes(),
        deps.policy,
    );
    if let SubmissionPowResult::Err { .. } = pow {
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

    let rate = deps
        .rate_limiter
        .peek(deps.now, deps.ip, &s.pk_hex, &s.c_hex);
    if let RateLimitResult::Rejected { reason } = rate {
        return reject(
            AdmissionStatus::RateLimited,
            AdmissionError::RateLimited { reason },
            RejectionReason::RateLimit { quota: reason },
        );
    }

    let share_hash = share_hash(&s.c, &s.pk, &s.n, &s.j, &canon_hash);
    let pool_result = deps.pool.accept(PoolShare {
        label: "admission".to_string(),
        pk: s.pk_hex.clone(),
        n: s.n_hex.clone(),
        j: s.j_hex.clone(),
        c: s.c_hex.clone(),
    });
    if let Some(reason) = pool_result.reason_typed() {
        return reject(
            AdmissionStatus::UnprocessableEntity,
            AdmissionError::SharePool { reason },
            RejectionReason::SharePool { detail: reason },
        );
    }
    deps.rate_limiter
        .commit(deps.now, deps.ip, &s.pk_hex, &s.c_hex);

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
        AdmissionError::InvalidFieldType { field, expected } => json!({
            "accepted": false,
            "status": status.code(),
            "error": "invalid field type",
            "field": field,
            "expected": expected,
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
            "reason": validation_reason_json(reason),
            "rejection": rejection_json(rejection)
        }),
        AdmissionError::SubmitPow { reason } => json!({
            "accepted": false,
            "status": status.code(),
            "error": "submit_pow",
            "reason": reason.as_str(),
            "rejection": rejection_json(rejection)
        }),
        AdmissionError::SeedBinding { reason } => json!({
            "accepted": false,
            "status": status.code(),
            "error": "seed_binding",
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
            json!({ "stage": "validator", "reason": validation_reason_json(reason) })
        }
        RejectionReason::SubmitPow { detail } => {
            json!({ "stage": "submit_pow", "detail": detail.as_str() })
        }
        RejectionReason::SeedBinding { detail } => {
            json!({ "stage": "seed_binding", "detail": detail.as_str() })
        }
        RejectionReason::RateLimit { quota } => {
            json!({ "stage": "rate_limit", "quota": quota.as_str() })
        }
        RejectionReason::SharePool { detail } => {
            json!({ "stage": "share_pool", "detail": detail.as_str() })
        }
    }
}

fn string_field<'a>(
    body: &'a Map<String, Value>,
    field: &str,
) -> Result<&'a str, AdmissionDecision> {
    let Some(value) = body.get(field) else {
        return Err(reject(
            AdmissionStatus::BadRequest,
            AdmissionError::MissingField {
                field: field.to_string(),
            },
            RejectionReason::BadRequest {
                field: field.to_string(),
            },
        ));
    };
    value.as_str().ok_or_else(|| {
        reject(
            AdmissionStatus::BadRequest,
            AdmissionError::InvalidFieldType {
                field: field.to_string(),
                expected: "string".to_string(),
            },
            RejectionReason::BadRequest {
                field: field.to_string(),
            },
        )
    })
}

fn hex32(value: &str) -> Result<Hex32, String> {
    Hex32::from_hex(value).map_err(|err| err.to_string())
}

fn sha256_32(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    digest.into()
}
