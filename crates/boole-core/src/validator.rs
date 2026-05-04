use crate::CalibrationReport;
use serde_json::{json, Value};

const MAGIC: [u8; 4] = [0x42, 0x50, 0x50, 0x4b];
const FORMAT_VERSION: u32 = 1;
const MAX_WALK_DEPTH: usize = 4096;

pub fn validate_proof_package(bytes: &[u8], cfg: &CalibrationReport) -> Value {
    if bytes.len() > cfg.L as usize {
        return json!({
            "ok": false,
            "reason": { "kind": "tooLarge", "size": bytes.len(), "limit": cfg.L }
        });
    }

    let walked = match walk_package(bytes) {
        Ok(walked) => walked,
        Err(detail) => {
            return json!({
                "ok": false,
                "reason": { "kind": "decode", "detail": detail }
            });
        }
    };

    if walked.size > cfg.L as usize {
        return json!({
            "ok": false,
            "reason": { "kind": "tooLarge", "size": walked.size, "limit": cfg.L }
        });
    }
    if walked.decl_count > cfg.D_max as u32 {
        return json!({
            "ok": false,
            "reason": { "kind": "tooManyDecls", "declCount": walked.decl_count, "limit": cfg.D_max }
        });
    }

    json!({
        "ok": true,
        "declCount": walked.decl_count,
        "size": walked.size,
        "universeArity": walked.universe_arity,
    })
}

#[derive(Debug, Clone, Copy)]
struct WalkResult {
    decl_count: u32,
    size: usize,
    universe_arity: u32,
}

fn walk_package(bytes: &[u8]) -> Result<WalkResult, Value> {
    if bytes.len() < 12 {
        return Err(json!({ "kind": "unexpectedEOF" }));
    }
    if bytes[0..4] != MAGIC {
        return Err(json!({ "kind": "badMagic" }));
    }

    let mut cursor = Cursor::new(bytes);
    cursor.skip(4)?;
    let version = cursor.read_u32_le()?;
    if version != FORMAT_VERSION {
        return Err(json!({ "kind": "unsupportedVersion", "version": version }));
    }
    let universe_arity = cursor.read_u32_le()?;
    walk_name(&mut cursor)?;
    walk_expr(&mut cursor, 0)?;
    walk_expr(&mut cursor, 0)?;
    let decl_count = cursor.read_u32_le()?;
    for _ in 0..decl_count {
        walk_decl(&mut cursor)?;
    }
    if cursor.pos != bytes.len() {
        return Err(json!({ "kind": "trailingBytes", "at": cursor.pos, "size": bytes.len() }));
    }

    Ok(WalkResult {
        decl_count,
        size: cursor.pos,
        universe_arity,
    })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn ensure(&self, n: usize) -> Result<(), Value> {
        if self.pos + n > self.bytes.len() {
            Err(json!({ "kind": "unexpectedEOF" }))
        } else {
            Ok(())
        }
    }

    fn read_byte(&mut self) -> Result<u8, Value> {
        self.ensure(1)?;
        let out = self.bytes[self.pos];
        self.pos += 1;
        Ok(out)
    }

    fn read_u32_le(&mut self) -> Result<u32, Value> {
        self.ensure(4)?;
        let out = u32::from_le_bytes([
            self.bytes[self.pos],
            self.bytes[self.pos + 1],
            self.bytes[self.pos + 2],
            self.bytes[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(out)
    }

    fn skip(&mut self, n: usize) -> Result<(), Value> {
        self.ensure(n)?;
        self.pos += n;
        Ok(())
    }
}

fn walk_level(cursor: &mut Cursor<'_>, depth: usize) -> Result<(), Value> {
    if depth > MAX_WALK_DEPTH {
        return Err(
            json!({ "kind": "recursionLimit", "whereTag": "CanonLevel", "limit": MAX_WALK_DEPTH }),
        );
    }
    let tag = cursor.read_byte()?;
    match tag {
        0x00 => Ok(()),
        0x01 => walk_level(cursor, depth + 1),
        0x02 | 0x03 => {
            walk_level(cursor, depth + 1)?;
            walk_level(cursor, depth + 1)
        }
        0x04 => {
            cursor.read_u32_le()?;
            Ok(())
        }
        _ => Err(json!({ "kind": "unknownTag", "whereTag": "CanonLevel", "tag": tag })),
    }
}

fn walk_lit(cursor: &mut Cursor<'_>) -> Result<(), Value> {
    let tag = cursor.read_byte()?;
    match tag {
        0x00 => {
            cursor.read_u32_le()?;
            Ok(())
        }
        0x01 => {
            let n = cursor.read_u32_le()? as usize;
            cursor.skip(n)
        }
        _ => Err(json!({ "kind": "unknownTag", "whereTag": "CanonLit", "tag": tag })),
    }
}

fn walk_name(cursor: &mut Cursor<'_>) -> Result<(), Value> {
    let parts = cursor.read_u32_le()?;
    for _ in 0..parts {
        let len = cursor.read_u32_le()? as usize;
        cursor.skip(len)?;
    }
    Ok(())
}

fn walk_expr(cursor: &mut Cursor<'_>, depth: usize) -> Result<(), Value> {
    if depth > MAX_WALK_DEPTH {
        return Err(
            json!({ "kind": "recursionLimit", "whereTag": "CanonExpr", "limit": MAX_WALK_DEPTH }),
        );
    }
    let tag = cursor.read_byte()?;
    match tag {
        0x10 => {
            cursor.read_u32_le()?;
            Ok(())
        }
        0x11 => walk_level(cursor, 0),
        0x12 => {
            walk_name(cursor)?;
            let k = cursor.read_u32_le()?;
            for _ in 0..k {
                walk_level(cursor, 0)?;
            }
            Ok(())
        }
        0x13 | 0x14 | 0x15 => {
            walk_expr(cursor, depth + 1)?;
            walk_expr(cursor, depth + 1)
        }
        0x16 => {
            walk_expr(cursor, depth + 1)?;
            walk_expr(cursor, depth + 1)?;
            walk_expr(cursor, depth + 1)
        }
        0x17 => walk_lit(cursor),
        0x18 => {
            walk_name(cursor)?;
            cursor.read_u32_le()?;
            walk_expr(cursor, depth + 1)
        }
        _ => Err(json!({ "kind": "unknownTag", "whereTag": "CanonExpr", "tag": tag })),
    }
}

fn walk_decl(cursor: &mut Cursor<'_>) -> Result<(), Value> {
    walk_name(cursor)?;
    walk_expr(cursor, 0)?;
    walk_expr(cursor, 0)
}
