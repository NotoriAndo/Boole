use num_bigint::BigUint;
use num_traits::{One, Zero};
use thiserror::Error;

const DOMAIN_TICKET: &[u8] = b"ticket";
const DOMAIN_SHARE: &[u8] = b"share";
const DOMAIN_SUBMIT: &[u8] = b"submit";
const DOMAIN_TARGET: &[u8] = b"target";

/// Consensus bound on the target index: a claimed `seedHex` must equal
/// `target_seed(c, pk, n, j_index)` for some `j_index` below this bound
/// (admission and replay both search the same range). A miner profile's
/// per-ticket target count `M` must stay ≤ this bound for its claimed
/// seeds to admit.
pub const TARGET_SEED_J_INDEX_BOUND: u32 = 256;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Hex32Error {
    #[error("hex32 must be 64 lowercase hex characters")]
    InvalidShape,
    #[error("hex32 decode failed")]
    DecodeFailed,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Hex64Error {
    #[error("hex64 must be 128 lowercase hex characters")]
    InvalidShape,
    #[error("hex64 decode failed")]
    DecodeFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hex32([u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hex64([u8; 64]);

impl Hex32 {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_hex(value: &str) -> Result<Self, Hex32Error> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(Hex32Error::InvalidShape);
        }
        let mut out = [0u8; 32];
        hex::decode_to_slice(value, &mut out).map_err(|_| Hex32Error::DecodeFailed)?;
        Ok(Self(out))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl Hex64 {
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    pub fn from_hex(value: &str) -> Result<Self, Hex64Error> {
        if value.len() != 128
            || !value
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(Hex64Error::InvalidShape);
        }
        let mut out = [0u8; 64];
        hex::decode_to_slice(value, &mut out).map_err(|_| Hex64Error::DecodeFailed)?;
        Ok(Self(out))
    }

    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketResult {
    pub valid: bool,
    pub hash_bytes: Hex32,
    pub hash_int: BigUint,
}

pub fn h_protocol(domain: &[u8], parts: &[&[u8]]) -> Hex32 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    for part in parts {
        hasher.update(part);
    }
    Hex32::from_bytes(*hasher.finalize().as_bytes())
}

pub fn block_hash(prev_c: &Hex32, share_hashes: &[Hex32]) -> Hex32 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"block");
    hasher.update(prev_c.as_bytes());
    for share_hash in share_hashes {
        hasher.update(share_hash.as_bytes());
    }
    Hex32::from_bytes(*hasher.finalize().as_bytes())
}

pub fn digest_to_biguint(d: &Hex32) -> BigUint {
    BigUint::from_bytes_be(d.as_bytes())
}

pub fn ticket(c: &Hex32, pk: &Hex32, n: &Hex32, t_ticket: &BigUint) -> TicketResult {
    let hash_bytes = h_protocol(DOMAIN_TICKET, &[c.as_bytes(), pk.as_bytes(), n.as_bytes()]);
    let hash_int = digest_to_biguint(&hash_bytes);
    TicketResult {
        valid: &hash_int < t_ticket,
        hash_bytes,
        hash_int,
    }
}

/// Deterministic problem seed for `(c, pk, n, j_index)` — the protocol's
/// "examiner": `c` is the previous block's hash, so the posed instance is a
/// pure function of the chain head and the miner's admitted ticket. The
/// `j_index` is the integer target counter 0..M, NOT the 32-byte grinded
/// share `j`. Byte-compatible with pof's `targetGen.ts::targetSeed`.
pub fn target_seed(c: &Hex32, pk: &Hex32, n: &Hex32, j_index: u32) -> Hex32 {
    let j_be = j_index.to_be_bytes();
    h_protocol(
        DOMAIN_TARGET,
        &[c.as_bytes(), pk.as_bytes(), n.as_bytes(), &j_be],
    )
}

/// Search `j_index ∈ [0, TARGET_SEED_J_INDEX_BOUND)` for the index whose
/// `target_seed(c, pk, n, j_index)` equals `seed_hex`. `None` means the
/// claimed seed does not derive from this share's `(c, pk, n)` context —
/// including any `seed_hex` that is not 64 lowercase hex characters.
pub fn find_target_seed_j_index(c: &Hex32, pk: &Hex32, n: &Hex32, seed_hex: &str) -> Option<u32> {
    let claimed = Hex32::from_hex(seed_hex).ok()?;
    (0..TARGET_SEED_J_INDEX_BOUND).find(|&j_index| target_seed(c, pk, n, j_index) == claimed)
}

pub fn share_hash(c: &Hex32, pk: &Hex32, n: &Hex32, j: &Hex32, canon_hash: &Hex32) -> Hex32 {
    h_protocol(
        DOMAIN_SHARE,
        &[
            c.as_bytes(),
            pk.as_bytes(),
            n.as_bytes(),
            j.as_bytes(),
            canon_hash.as_bytes(),
        ],
    )
}

pub fn share_score(share_hash_bytes: &Hex32) -> BigUint {
    let two_to_256 = BigUint::one() << 256usize;
    let denominator = digest_to_biguint(share_hash_bytes) + BigUint::one();
    two_to_256 / denominator
}

pub fn difficulty_weight(t: &BigUint) -> anyhow::Result<BigUint> {
    if t.is_zero() {
        anyhow::bail!("T must be > 0");
    }
    Ok((BigUint::one() << 256usize) / t)
}

pub fn min_share_score(t_share: &BigUint, multiplier_nanos: u64) -> anyhow::Result<BigUint> {
    let scale = BigUint::from(1_000_000_000u64);
    Ok((difficulty_weight(t_share)? * BigUint::from(multiplier_nanos)) / scale)
}

pub fn submission_pow_hash(c: &Hex32, pk: &Hex32, nonce_s: &Hex32, canon_hash: &Hex32) -> Hex32 {
    h_protocol(
        DOMAIN_SUBMIT,
        &[
            c.as_bytes(),
            pk.as_bytes(),
            nonce_s.as_bytes(),
            canon_hash.as_bytes(),
        ],
    )
}

pub fn submission_pow_ok(
    c: &Hex32,
    pk: &Hex32,
    nonce_s: &Hex32,
    canon_hash: &Hex32,
    t_submit: &BigUint,
) -> (bool, BigUint) {
    let hash = submission_pow_hash(c, pk, nonce_s, canon_hash);
    let hash_int = digest_to_biguint(&hash);
    (&hash_int < t_submit, hash_int)
}

pub fn parse_biguint_hex(value: &str) -> anyhow::Result<BigUint> {
    let without_prefix = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    BigUint::parse_bytes(without_prefix.as_bytes(), 16)
        .ok_or_else(|| anyhow::anyhow!("invalid hex bigint"))
}
