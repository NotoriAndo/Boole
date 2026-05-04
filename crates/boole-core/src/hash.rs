use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Hex32Error {
    #[error("hex32 must be 64 lowercase hex characters")]
    InvalidShape,
    #[error("hex32 decode failed")]
    DecodeFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Hex32([u8; 32]);

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
