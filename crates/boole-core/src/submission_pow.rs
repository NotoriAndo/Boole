use crate::{hex_to_biguint, submission_pow_ok, CalibrationReport, Hex32};
use num_bigint::BigUint;
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmissionPowResult {
    Ok {
        hash_int: BigUint,
    },
    Err {
        reason: SubmissionPowRejectReason,
        hash_int: BigUint,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmissionPowRejectReason {
    AboveTSubmit,
}

impl SubmissionPowRejectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AboveTSubmit => "above_T_submit",
        }
    }
}

pub fn check_submission_pow(
    c: &[u8],
    pk: &[u8],
    nonce_s: &[u8],
    canon_hash: &[u8],
    cfg: &CalibrationReport,
) -> SubmissionPowResult {
    let c = hex32_from_slice(c, "c");
    let pk = hex32_from_slice(pk, "pk");
    let nonce_s = hex32_from_slice(nonce_s, "nonceS");
    let canon_hash = hex32_from_slice(canon_hash, "canonHash");
    let t_submit =
        hex_to_biguint(&cfg.T_submit).expect("cfg.T_submit parses like TypeScript fixture");
    let (ok, hash_int) = submission_pow_ok(&c, &pk, &nonce_s, &canon_hash, &t_submit);
    if ok {
        SubmissionPowResult::Ok { hash_int }
    } else {
        SubmissionPowResult::Err {
            reason: SubmissionPowRejectReason::AboveTSubmit,
            hash_int,
        }
    }
}

pub fn check_submission_pow_json(result: &SubmissionPowResult) -> Value {
    match result {
        SubmissionPowResult::Ok { hash_int } => {
            json!({ "ok": true, "hashInt": hash_int.to_string() })
        }
        SubmissionPowResult::Err { reason, hash_int } => json!({
            "ok": false,
            "reason": reason.as_str(),
            "hashInt": hash_int.to_string()
        }),
    }
}

fn hex32_from_slice(bytes: &[u8], label: &str) -> Hex32 {
    let array: [u8; 32] = bytes
        .try_into()
        .unwrap_or_else(|_| panic!("{label} must be 32 bytes"));
    Hex32::from_bytes(array)
}
