//! POFP wire-format mirror of pof TS `lib/proofPackage.ts`. Byte-for-byte
//! identical to pof's encoder/walker so dispatcher `walkPackage` accepts
//! miner-emitted bytes without taking a build-time dependency on the
//! dispatcher package.

use boole_core::Hex32;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const MAGIC: [u8; 4] = [0x50, 0x4f, 0x46, 0x50]; // "POFP"
pub const FORMAT_VERSION: u32 = 1;
pub const MAX_WALK_DEPTH: u32 = 4096;

pub mod level_tag {
    pub const ZERO: u8 = 0x00;
    pub const SUCC: u8 = 0x01;
    pub const MAX: u8 = 0x02;
    pub const IMAX: u8 = 0x03;
    pub const PARAM: u8 = 0x04;
}

pub mod lit_tag {
    pub const NAT_VAL: u8 = 0x00;
    pub const STR_VAL: u8 = 0x01;
}

pub mod expr_tag {
    pub const BVAR: u8 = 0x10;
    pub const SORT: u8 = 0x11;
    pub const CONST: u8 = 0x12;
    pub const APP: u8 = 0x13;
    pub const LAM: u8 = 0x14;
    pub const FORALL_E: u8 = 0x15;
    pub const LET_E: u8 = 0x16;
    pub const LIT: u8 = 0x17;
    pub const PROJ: u8 = 0x18;
}

#[derive(Debug, Default)]
pub struct BppkBuilder {
    bytes: Vec<u8>,
}

impl BppkBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, b: u8) -> &mut Self {
        self.bytes.push(b);
        self
    }

    pub fn push_u32_le(&mut self, n: u32) -> &mut Self {
        self.bytes.extend_from_slice(&n.to_le_bytes());
        self
    }

    pub fn push_bytes(&mut self, bs: &[u8]) -> &mut Self {
        self.bytes.extend_from_slice(bs);
        self
    }

    pub fn push_string(&mut self, s: &str) -> &mut Self {
        let bs = s.as_bytes();
        self.push_u32_le(bs.len() as u32);
        self.push_bytes(bs);
        self
    }

    pub fn push_name(&mut self, parts: &[&str]) -> &mut Self {
        self.push_u32_le(parts.len() as u32);
        for p in parts {
            self.push_string(p);
        }
        self
    }

    pub fn build(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum BppkDecodeError {
    #[error("bad magic")]
    BadMagic,
    #[error("unsupported version: {0}")]
    UnsupportedVersion(u32),
    #[error("unexpected EOF")]
    UnexpectedEof,
    #[error("unknown tag {tag:#x} at {where_tag}")]
    UnknownTag {
        where_tag: &'static str,
        tag: u8,
    },
    #[error("recursion limit at {where_tag}: {limit}")]
    RecursionLimit {
        where_tag: &'static str,
        limit: u32,
    },
    #[error("trailing bytes at {at} of {size}")]
    TrailingBytes { at: usize, size: usize },
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn ensure(&self, n: usize) -> Result<(), BppkDecodeError> {
        if self.pos + n > self.bytes.len() {
            Err(BppkDecodeError::UnexpectedEof)
        } else {
            Ok(())
        }
    }

    fn read_byte(&mut self) -> Result<u8, BppkDecodeError> {
        self.ensure(1)?;
        let b = self.bytes[self.pos];
        self.pos += 1;
        Ok(b)
    }

    fn read_u32_le(&mut self) -> Result<u32, BppkDecodeError> {
        self.ensure(4)?;
        let arr = [
            self.bytes[self.pos],
            self.bytes[self.pos + 1],
            self.bytes[self.pos + 2],
            self.bytes[self.pos + 3],
        ];
        self.pos += 4;
        Ok(u32::from_le_bytes(arr))
    }

    fn skip(&mut self, n: usize) -> Result<(), BppkDecodeError> {
        self.ensure(n)?;
        self.pos += n;
        Ok(())
    }
}

fn walk_level(c: &mut Cursor, depth: u32) -> Result<(), BppkDecodeError> {
    if depth > MAX_WALK_DEPTH {
        return Err(BppkDecodeError::RecursionLimit {
            where_tag: "CanonLevel",
            limit: MAX_WALK_DEPTH,
        });
    }
    let tag = c.read_byte()?;
    match tag {
        level_tag::ZERO => Ok(()),
        level_tag::SUCC => walk_level(c, depth + 1),
        level_tag::MAX | level_tag::IMAX => {
            walk_level(c, depth + 1)?;
            walk_level(c, depth + 1)
        }
        level_tag::PARAM => {
            c.read_u32_le()?;
            Ok(())
        }
        _ => Err(BppkDecodeError::UnknownTag {
            where_tag: "CanonLevel",
            tag,
        }),
    }
}

fn walk_lit(c: &mut Cursor) -> Result<(), BppkDecodeError> {
    let tag = c.read_byte()?;
    match tag {
        lit_tag::NAT_VAL => {
            c.read_u32_le()?;
            Ok(())
        }
        lit_tag::STR_VAL => {
            let n = c.read_u32_le()? as usize;
            c.skip(n)
        }
        _ => Err(BppkDecodeError::UnknownTag {
            where_tag: "CanonLit",
            tag,
        }),
    }
}

fn walk_name(c: &mut Cursor) -> Result<(), BppkDecodeError> {
    let parts = c.read_u32_le()?;
    for _ in 0..parts {
        let len = c.read_u32_le()? as usize;
        c.skip(len)?;
    }
    Ok(())
}

fn walk_expr(c: &mut Cursor, depth: u32) -> Result<(), BppkDecodeError> {
    if depth > MAX_WALK_DEPTH {
        return Err(BppkDecodeError::RecursionLimit {
            where_tag: "CanonExpr",
            limit: MAX_WALK_DEPTH,
        });
    }
    let tag = c.read_byte()?;
    match tag {
        expr_tag::BVAR => {
            c.read_u32_le()?;
            Ok(())
        }
        expr_tag::SORT => walk_level(c, 0),
        expr_tag::CONST => {
            walk_name(c)?;
            let k = c.read_u32_le()?;
            for _ in 0..k {
                walk_level(c, 0)?;
            }
            Ok(())
        }
        expr_tag::APP | expr_tag::LAM | expr_tag::FORALL_E => {
            walk_expr(c, depth + 1)?;
            walk_expr(c, depth + 1)
        }
        expr_tag::LET_E => {
            walk_expr(c, depth + 1)?;
            walk_expr(c, depth + 1)?;
            walk_expr(c, depth + 1)
        }
        expr_tag::LIT => walk_lit(c),
        expr_tag::PROJ => {
            walk_name(c)?;
            c.read_u32_le()?;
            walk_expr(c, depth + 1)
        }
        _ => Err(BppkDecodeError::UnknownTag {
            where_tag: "CanonExpr",
            tag,
        }),
    }
}

fn walk_decl(c: &mut Cursor) -> Result<(), BppkDecodeError> {
    walk_name(c)?;
    walk_expr(c, 0)?;
    walk_expr(c, 0)
}

#[derive(Debug, Clone, Copy)]
pub struct BppkWalkResult {
    pub decl_count: u32,
    pub size: usize,
    pub universe_arity: u32,
}

pub fn walk_bppk(bytes: &[u8]) -> Result<BppkWalkResult, BppkDecodeError> {
    if bytes.len() < 12 {
        return Err(BppkDecodeError::UnexpectedEof);
    }
    if bytes[..4] != MAGIC {
        return Err(BppkDecodeError::BadMagic);
    }
    let mut c = Cursor::new(bytes);
    c.skip(4)?;
    let ver = c.read_u32_le()?;
    if ver != FORMAT_VERSION {
        return Err(BppkDecodeError::UnsupportedVersion(ver));
    }
    let universe_arity = c.read_u32_le()?;
    walk_name(&mut c)?;
    walk_expr(&mut c, 0)?;
    walk_expr(&mut c, 0)?;
    let decl_count = c.read_u32_le()?;
    for _ in 0..decl_count {
        walk_decl(&mut c)?;
    }
    if c.pos != bytes.len() {
        return Err(BppkDecodeError::TrailingBytes {
            at: c.pos,
            size: bytes.len(),
        });
    }
    Ok(BppkWalkResult {
        decl_count,
        size: c.pos,
        universe_arity,
    })
}

pub fn bppk_canon_hash(bytes: &[u8]) -> Hex32 {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    Hex32::from_bytes(out)
}
