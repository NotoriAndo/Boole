//! D6/D14: consensus-only L1INFO deposits. VERDICT and bridge DEPOSIT are
//! reserved source-hash domains; neither is implemented in M1.
use crate::{
    attributes::{fixed_bytes, hex_bytes, hex_string},
    config, rlp, InputError, L1Block,
};
use sha2::{Digest, Sha256};

fn source_hash(origin: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(b"BOOLE-S1/L1INFO");
    h.update(origin);
    h.update([0; 32]);
    h.finalize().to_vec()
}

pub fn l1_info(l1: &L1Block) -> Result<String, InputError> {
    let origin = fixed_bytes(&l1.hash, 32, "L1 hash")?;
    let fee = l1
        .base_fee_per_gas
        .strip_prefix("0x")
        .ok_or(InputError::Field("base fee"))?;
    if fee.is_empty() || fee.len() > 64 {
        return Err(InputError::Field("base fee"));
    }
    let padded = format!("0x{fee:0>64}");
    let fee = fixed_bytes(&padded, 32, "base fee")?;
    let mut data = vec![0x3d, 0xb6, 0xbe, 0x2b];
    data.extend([0; 16]);
    data.extend(l1.timestamp.to_be_bytes());
    data.extend(l1.number.to_be_bytes());
    data.extend(fee);
    data.extend([0; 32]);
    data.extend(&origin);
    data.extend([0; 46]);
    let fields = vec![
        rlp::bytes(&source_hash(&origin)),
        rlp::bytes(&fixed_bytes(config::DEPOSITOR, 20, "depositor")?),
        rlp::bytes(&fixed_bytes(config::L1_BLOCK, 20, "L1Block")?),
        rlp::uint(0),
        rlp::uint(0),
        rlp::uint(1_000_000),
        rlp::uint(0),
        rlp::bytes(&data),
    ];
    Ok(hex_string(&[vec![0x7e], rlp::list(&fields)].concat()))
}

pub fn l1_origin(deposit: &str, number: u64) -> Result<String, InputError> {
    let raw = hex_bytes(deposit, "parent L1-info")?;
    if raw.first() != Some(&0x7e) {
        return Err(InputError::L1Link);
    }
    let decoded = rlp::decode(&raw[1..]).map_err(|_| InputError::L1Link)?;
    let f = decoded.children.ok_or(InputError::L1Link)?;
    if f.len() != 8 || f.iter().any(|x| x.children.is_some()) {
        return Err(InputError::L1Link);
    }
    let data = f[7].bytes;
    if data.len() != 178
        || data[..4] != [0x3d, 0xb6, 0xbe, 0x2b]
        || data[4..20] != [0; 16]
        || data[28..36] != number.to_be_bytes()
        || data[68..100] != [0; 32]
        || data[132..] != [0; 46]
        || f[0].bytes != source_hash(&data[100..132])
        || f[1].bytes != fixed_bytes(config::DEPOSITOR, 20, "depositor")?
        || f[2].bytes != fixed_bytes(config::L1_BLOCK, 20, "L1Block")?
        || !f[3].bytes.is_empty()
        || !f[4].bytes.is_empty()
        || f[5].bytes != [0x0f, 0x42, 0x40]
        || !f[6].bytes.is_empty()
    {
        return Err(InputError::L1Link);
    }
    Ok(hex_string(&data[100..132]))
}
