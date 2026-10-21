//! Canonical RLP with original item spans (D10).
use std::ops::Range;

#[derive(Debug, Clone)]
pub struct Item<'a> {
    pub span: Range<usize>,
    pub bytes: &'a [u8],
    pub children: Option<Vec<Item<'a>>>,
}

pub fn decode(raw: &[u8]) -> Result<Item<'_>, &'static str> {
    fn parse(raw: &[u8], start: usize, end: usize, depth: u8) -> Result<Item<'_>, &'static str> {
        if depth > 8 || start >= end {
            return Err("RLP depth or short input");
        }
        let tag = raw[start];
        let mut offset = start.checked_add(1).ok_or("offset overflow")?;
        if tag < 0x80 {
            return Ok(Item {
                span: start..offset,
                bytes: &raw[start..offset],
                children: None,
            });
        }
        let is_list = tag >= 0xc0;
        let mut size = usize::from(tag - if is_list { 0xc0 } else { 0x80 });
        if size > 55 {
            let count = size - 55;
            let stop = offset.checked_add(count).ok_or("length overflow")?;
            if stop > end || raw[offset] == 0 {
                return Err("noncanonical or short length");
            }
            size = 0;
            for b in &raw[offset..stop] {
                size = size
                    .checked_mul(256)
                    .and_then(|n| n.checked_add(usize::from(*b)))
                    .ok_or("length overflow")?;
            }
            if size < 56 {
                return Err("noncanonical long form");
            }
            offset = stop;
        }
        let stop = offset.checked_add(size).ok_or("length overflow")?;
        if stop > end {
            return Err("short payload");
        }
        let payload = &raw[offset..stop];
        let children = if is_list {
            let mut children = Vec::new();
            while offset < stop {
                let child = parse(raw, offset, stop, depth + 1)?;
                offset = child.span.end;
                children.push(child);
            }
            Some(children)
        } else {
            if size == 1 && payload[0] < 0x80 {
                return Err("noncanonical single byte");
            }
            None
        };
        Ok(Item {
            span: start..stop,
            bytes: payload,
            children,
        })
    }
    let item = parse(raw, 0, raw.len(), 0)?;
    if item.span.end != raw.len() {
        return Err("trailing bytes");
    }
    Ok(item)
}

fn wrap(value: &[u8], base: u8) -> Vec<u8> {
    let mut out = Vec::new();
    if value.len() < 56 {
        out.push(base + value.len() as u8);
    } else {
        let size = value.len().to_be_bytes();
        let size = &size[size.iter().position(|b| *b != 0).unwrap_or(size.len())..];
        out.push(base + 55 + size.len() as u8);
        out.extend_from_slice(size);
    }
    out.extend_from_slice(value);
    out
}

pub fn bytes(value: &[u8]) -> Vec<u8> {
    if value.len() == 1 && value[0] < 0x80 {
        value.to_vec()
    } else {
        wrap(value, 0x80)
    }
}

pub fn list(encoded: &[Vec<u8>]) -> Vec<u8> {
    wrap(&encoded.concat(), 0xc0)
}

pub fn uint(value: u64) -> Vec<u8> {
    let raw = value.to_be_bytes();
    bytes(&raw[raw.iter().position(|b| *b != 0).unwrap_or(raw.len())..])
}
