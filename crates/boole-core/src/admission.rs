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

pub fn admit_submission(deps: AdmissionDeps<'_>) -> Value {
    for field in ["c", "pk", "n", "j", "nonceS", "bytes"] {
        if !deps.body.contains_key(field) {
            return json!({
                "accepted": false,
                "status": 400,
                "error": "missing field",
                "field": field,
                "rejection": { "stage": "bad_request", "field": field }
            });
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
        Err(detail) => return bad_hex("c", detail),
    };
    let pk = match hex32(pk_hex) {
        Ok(v) => v,
        Err(detail) => return bad_hex("pk", detail),
    };
    let n = match hex32(n_hex) {
        Ok(v) => v,
        Err(detail) => return bad_hex("n", detail),
    };
    let j = match hex32(j_hex) {
        Ok(v) => v,
        Err(detail) => return bad_hex("j", detail),
    };
    let nonce_s = match hex32(nonce_s_hex) {
        Ok(v) => v,
        Err(detail) => return bad_hex("nonceS", detail),
    };
    let package_bytes = match hex::decode(bytes_hex) {
        Ok(v) => v,
        Err(err) => return bad_hex("bytes", err.to_string()),
    };

    let t_ticket = BigUint::parse_bytes(strip_0x(&deps.cfg.T_ticket).as_bytes(), 16)
        .expect("cfg.T_ticket parses");
    let ticket_result = ticket(&c, &pk, &n, &t_ticket);
    if !ticket_result.valid {
        return json!({
            "accepted": false,
            "status": 422,
            "error": "ticket",
            "reason": "above_T_ticket",
            "rejection": { "stage": "ticket", "detail": "above_T_ticket" }
        });
    }
    if !deps.rate_limiter.has_observed_ticket(pk_hex, c_hex, n_hex) {
        return json!({
            "accepted": false,
            "status": 422,
            "error": "ticket",
            "reason": "unobserved",
            "rejection": { "stage": "ticket", "detail": "unobserved" }
        });
    }

    let validator = validate_proof_package(&package_bytes, deps.cfg);
    if !validator
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let reason = validator.get("reason").expect("validator reason").clone();
        return json!({
            "accepted": false,
            "status": 422,
            "error": "validator",
            "reason": reason,
            "rejection": { "stage": "validator", "reason": reason }
        });
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
        return json!({
            "accepted": false,
            "status": 422,
            "error": "submit_pow",
            "reason": "above_T_submit",
            "rejection": { "stage": "submit_pow", "detail": "above_T_submit" }
        });
    }

    let rate = deps.rate_limiter.check(deps.now, deps.ip, pk_hex, c_hex);
    if !rate
        .get("allowed")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let reason = rate
            .get("reason")
            .and_then(Value::as_str)
            .expect("rate reason");
        return json!({
            "accepted": false,
            "status": 429,
            "error": "rate_limited",
            "reason": reason,
            "rejection": { "stage": "rate_limit", "quota": reason }
        });
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
        return json!({
            "accepted": false,
            "status": 422,
            "error": "share_pool",
            "reason": reason,
            "rejection": { "stage": "share_pool", "detail": reason }
        });
    }

    json!({
        "accepted": true,
        "status": 200,
        "responseStatus": "accepted",
        "shareHash": share_hash.to_hex()
    })
}

fn string_field<'a>(body: &'a Map<String, Value>, field: &str) -> &'a str {
    body.get(field)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{field} must be string"))
}

fn hex32(value: &str) -> Result<Hex32, String> {
    Hex32::from_hex(value).map_err(|err| err.to_string())
}

fn bad_hex(field: &str, detail: String) -> Value {
    json!({
        "accepted": false,
        "status": 400,
        "error": "bad hex",
        "field": field,
        "rejection": { "stage": "decode", "field": field, "detail": detail }
    })
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
