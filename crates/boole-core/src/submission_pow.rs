use crate::{hex_to_biguint, submission_pow_ok, CalibrationReport, Hex32};
use serde_json::{json, Value};

pub fn check_submission_pow(
    c: &[u8],
    pk: &[u8],
    nonce_s: &[u8],
    canon_hash: &[u8],
    cfg: &CalibrationReport,
) -> Value {
    let c = hex32_from_slice(c, "c");
    let pk = hex32_from_slice(pk, "pk");
    let nonce_s = hex32_from_slice(nonce_s, "nonceS");
    let canon_hash = hex32_from_slice(canon_hash, "canonHash");
    let t_submit =
        hex_to_biguint(&cfg.T_submit).expect("cfg.T_submit parses like TypeScript fixture");
    let (ok, hash_int) = submission_pow_ok(&c, &pk, &nonce_s, &canon_hash, &t_submit);
    if ok {
        json!({ "ok": true, "hashInt": hash_int.to_string() })
    } else {
        json!({ "ok": false, "reason": "above_T_submit", "hashInt": hash_int.to_string() })
    }
}

fn hex32_from_slice(bytes: &[u8], label: &str) -> Hex32 {
    let array: [u8; 32] = bytes
        .try_into()
        .unwrap_or_else(|_| panic!("{label} must be 32 bytes"));
    Hex32::from_bytes(array)
}
