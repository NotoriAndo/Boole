//! F2–F6, in consensus order (D9–D13).
use crate::rlp;
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use sha3::{Digest, Keccak256};

pub fn keccak(raw: &[u8]) -> [u8; 32] {
    Keccak256::digest(raw).into()
}

fn decode(raw: &[u8]) -> Result<(Vec<rlp::Item<'_>>, Signature, u8), &'static str> {
    let fields = rlp::decode(raw)?.children.ok_or("expected list")?;
    if fields.len() != 12 {
        return Err("expected 12 fields");
    }
    for (i, width) in [
        (0, 8),
        (1, 8),
        (2, 16),
        (3, 16),
        (4, 8),
        (6, 32),
        (9, 1),
        (10, 32),
        (11, 32),
    ] {
        let f = &fields[i];
        if f.children.is_some() || f.bytes.first() == Some(&0) || f.bytes.len() > width {
            return Err("integer encoding/width");
        }
    }
    if fields[5].children.is_some()
        || ![0, 20].contains(&fields[5].bytes.len())
        || fields[7].children.is_some()
    {
        return Err("destination/data");
    }
    for entry in fields[8].children.as_ref().ok_or("access list")? {
        let pair = entry.children.as_ref().ok_or("access list entry")?;
        if pair.len() != 2 || pair[0].children.is_some() || pair[0].bytes.len() != 20 {
            return Err("access list address");
        }
        for key in pair[1].children.as_ref().ok_or("storage list")? {
            if key.children.is_some() || key.bytes.len() != 32 {
                return Err("storage key");
            }
        }
    }
    let parity = fields[9].bytes.first().copied().unwrap_or(0);
    if parity > 1 {
        return Err("parity");
    }
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r[32 - fields[10].bytes.len()..].copy_from_slice(fields[10].bytes);
    s[32 - fields[11].bytes.len()..].copy_from_slice(fields[11].bytes);
    let signature = Signature::from_scalars(r, s).map_err(|_| "signature scalar")?;
    if signature.normalize_s().is_some() {
        return Err("high s");
    }
    Ok((fields, signature, parity))
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UserTxDecision {
    pub rule: &'static str,
    pub forward: bool,
    pub sender: Option<String>,
    pub tx_hash: String,
}

pub fn filter_user(raw: &[u8], parent_gas_limit: u64) -> UserTxDecision {
    let mut decision = UserTxDecision {
        rule: "USER-TYPE-NOT-ALLOWED",
        forward: false,
        sender: None,
        tx_hash: format!("0x{}", hex::encode(keccak(raw))),
    };
    if raw.first() != Some(&2) {
        return decision;
    }
    decision.rule = "USER-DECODE-OR-SIGNATURE-INVALID";
    let Ok((fields, signature, parity)) = decode(&raw[1..]) else {
        return decision;
    };
    let unsigned: Vec<_> = fields[..9]
        .iter()
        .map(|f| raw[1 + f.span.start..1 + f.span.end].to_vec())
        .collect();
    let signing = [vec![2], rlp::list(&unsigned)].concat();
    decision.rule = "USER-SENDER-UNRECOVERABLE";
    let Ok(key) = VerifyingKey::recover_from_prehash(
        &keccak(&signing),
        &signature,
        RecoveryId::new(parity == 1, false),
    ) else {
        return decision;
    };
    let encoded = key.to_encoded_point(false);
    let hash = keccak(&encoded.as_bytes()[1..]);
    decision.sender = Some(format!("0x{}", hex::encode(&hash[12..])));
    let gas = fields[4]
        .bytes
        .iter()
        .fold(0u64, |n, b| (n << 8) | u64::from(*b));
    decision.rule = "USER-GAS-EXCEEDS-PARENT-BLOCK-LIMIT";
    if gas > parent_gas_limit {
        return decision;
    }
    decision.rule = "FORWARD-TO-CLIENT-SEQUENTIAL-VALIDATION";
    decision.forward = true;
    decision
}
