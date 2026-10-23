//! Portable raw/RLE Zstandard frame subset, including the standard XXH64 checksum.
//! Compressed blocks still require the externally pinned full decoder.
use anyhow::{ensure, Result};
fn round(a: u64, b: u64) -> u64 {
    a.wrapping_add(b.wrapping_mul(14029467366897019727))
        .rotate_left(31)
        .wrapping_mul(11400714785074694791)
}
fn merge(a: u64, b: u64) -> u64 {
    (a ^ round(0, b))
        .wrapping_mul(11400714785074694791)
        .wrapping_add(9650029242287828579)
}
fn xxh64(b: &[u8]) -> u64 {
    const P1: u64 = 11400714785074694791;
    const P2: u64 = 14029467366897019727;
    const P3: u64 = 1609587929392839161;
    const P4: u64 = 9650029242287828579;
    const P5: u64 = 2870177450012600261;
    let mut at = 0;
    let mut h = if b.len() >= 32 {
        let mut v = [P1.wrapping_add(P2), P2, 0, 0u64.wrapping_sub(P1)];
        while at + 32 <= b.len() {
            for (i, lane) in v.iter_mut().enumerate() {
                *lane = round(
                    *lane,
                    u64::from_le_bytes(b[at + 8 * i..at + 8 * i + 8].try_into().expect("lane")),
                );
            }
            at += 32;
        }
        let mut h = v[0]
            .rotate_left(1)
            .wrapping_add(v[1].rotate_left(7))
            .wrapping_add(v[2].rotate_left(12))
            .wrapping_add(v[3].rotate_left(18));
        for lane in v {
            h = merge(h, lane);
        }
        h
    } else {
        P5
    };
    h = h.wrapping_add(b.len() as u64);
    while at + 8 <= b.len() {
        h ^= round(
            0,
            u64::from_le_bytes(b[at..at + 8].try_into().expect("word")),
        );
        h = h.rotate_left(27).wrapping_mul(P1).wrapping_add(P4);
        at += 8;
    }
    if at + 4 <= b.len() {
        h ^=
            u64::from(u32::from_le_bytes(b[at..at + 4].try_into().expect("word"))).wrapping_mul(P1);
        h = h.rotate_left(23).wrapping_mul(P2).wrapping_add(P3);
        at += 4;
    }
    for byte in &b[at..] {
        h ^= u64::from(*byte).wrapping_mul(P5);
        h = h.rotate_left(11).wrapping_mul(P1);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(P2);
    h ^= h >> 29;
    h = h.wrapping_mul(P3);
    h ^ (h >> 32)
}
pub fn encode(raw: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        !raw.is_empty() && raw.len() as u64 <= boole_derivation::m2::RAW_CAP,
        "raw size"
    );
    let mut out = vec![0x28, 0xb5, 0x2f, 0xfd];
    if raw.len() <= 255 {
        out.extend([0x24, raw.len() as u8]);
    } else {
        out.push(0xa4);
        out.extend((raw.len() as u32).to_le_bytes());
    }
    let count = raw.len().div_ceil(128 * 1024);
    for (i, chunk) in raw.chunks(128 * 1024).enumerate() {
        let header = ((chunk.len() as u32) << 3) | u32::from(i + 1 == count);
        out.extend(&header.to_le_bytes()[..3]);
        out.extend(chunk);
    }
    out.extend((xxh64(raw) as u32).to_le_bytes());
    Ok(out)
}
pub fn decode(frame: &[u8], raw_len: u64) -> Result<Option<Vec<u8>>> {
    crate::codec::validate_frame(frame, raw_len)?;
    let descriptor = frame[4];
    let single = descriptor & 0x20 != 0;
    let size = match descriptor >> 6 {
        0 => usize::from(single),
        1 => 2,
        2 => 4,
        _ => 8,
    };
    let mut at = 5 + usize::from(!single) + size;
    let mut out = Vec::new();
    loop {
        let h = u32::from_le_bytes([frame[at], frame[at + 1], frame[at + 2], 0]);
        at += 3;
        let n = (h >> 3) as usize;
        let kind = (h >> 1) & 3;
        if kind == 2 {
            return Ok(None);
        }
        ensure!(
            (out.len() as u64)
                .checked_add(n as u64)
                .is_some_and(|v| v <= raw_len),
            "decoded size cap"
        );
        match kind {
            0 => {
                out.extend(&frame[at..at + n]);
                at += n;
            }
            1 => {
                out.resize(out.len() + n, frame[at]);
                at += 1;
            }
            _ => anyhow::bail!("reserved block"),
        }
        if h & 1 != 0 {
            break;
        }
    }
    ensure!(
        out.len() as u64 == raw_len && (xxh64(&out) as u32).to_le_bytes() == frame[at..at + 4],
        "decoded length/checksum"
    );
    Ok(Some(out))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_xxh64_values() {
        assert_eq!(xxh64(b""), 0xef46db3751d8e999);
        assert_eq!(xxh64(b"a"), 0xd24ec4f1a98c6e5b);
        assert_eq!(xxh64(b"abc"), 0x44bc2cf5ad770999);
    }
}
