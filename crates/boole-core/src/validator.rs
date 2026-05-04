use crate::{calibration_policy, CalibrationPolicy, CalibrationReport};
use serde_json::{json, Value};

const MAGIC: [u8; 4] = [0x42, 0x50, 0x50, 0x4b];
const FORMAT_VERSION: u32 = 1;
const MAX_WALK_DEPTH: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationResult {
    Ok {
        decl_count: u32,
        size: usize,
        universe_arity: u32,
    },
    Err {
        reason: ValidationReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationReason {
    TooLarge { size: usize, limit: i64 },
    TooManyDecls { decl_count: u32, limit: i64 },
    Decode { detail: DecodeDetail },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeDetail {
    BadMagic,
    UnexpectedEof,
    UnsupportedVersion {
        version: u32,
    },
    TrailingBytes {
        at: usize,
        size: usize,
    },
    RecursionLimit {
        where_tag: &'static str,
        limit: usize,
    },
    UnknownTag {
        where_tag: &'static str,
        tag: u8,
    },
}

pub fn validate_proof_package(bytes: &[u8], cfg: &CalibrationReport) -> ValidationResult {
    let policy = calibration_policy(cfg).expect("calibration report is valid");
    validate_proof_package_with_policy(bytes, &policy)
}

pub fn validate_proof_package_with_policy(
    bytes: &[u8],
    policy: &CalibrationPolicy,
) -> ValidationResult {
    if bytes.len() > policy.l {
        return ValidationResult::Err {
            reason: ValidationReason::TooLarge {
                size: bytes.len(),
                limit: policy.l as i64,
            },
        };
    }

    let walked = match walk_package(bytes) {
        Ok(walked) => walked,
        Err(detail) => {
            return ValidationResult::Err {
                reason: ValidationReason::Decode { detail },
            };
        }
    };

    if walked.size > policy.l {
        return ValidationResult::Err {
            reason: ValidationReason::TooLarge {
                size: walked.size,
                limit: policy.l as i64,
            },
        };
    }
    if walked.decl_count > policy.d_max as u32 {
        return ValidationResult::Err {
            reason: ValidationReason::TooManyDecls {
                decl_count: walked.decl_count,
                limit: policy.d_max as i64,
            },
        };
    }

    ValidationResult::Ok {
        decl_count: walked.decl_count,
        size: walked.size,
        universe_arity: walked.universe_arity,
    }
}

pub fn validate_proof_package_json(result: &ValidationResult) -> Value {
    match result {
        ValidationResult::Ok {
            decl_count,
            size,
            universe_arity,
        } => json!({
            "ok": true,
            "declCount": decl_count,
            "size": size,
            "universeArity": universe_arity,
        }),
        ValidationResult::Err { reason } => json!({
            "ok": false,
            "reason": validation_reason_json(reason),
        }),
    }
}

pub fn validation_reason_json(reason: &ValidationReason) -> Value {
    match reason {
        ValidationReason::TooLarge { size, limit } => {
            json!({ "kind": "tooLarge", "size": size, "limit": limit })
        }
        ValidationReason::TooManyDecls { decl_count, limit } => {
            json!({ "kind": "tooManyDecls", "declCount": decl_count, "limit": limit })
        }
        ValidationReason::Decode { detail } => {
            json!({ "kind": "decode", "detail": decode_detail_json(detail) })
        }
    }
}

pub fn decode_detail_json(detail: &DecodeDetail) -> Value {
    match detail {
        DecodeDetail::BadMagic => json!({ "kind": "badMagic" }),
        DecodeDetail::UnexpectedEof => json!({ "kind": "unexpectedEOF" }),
        DecodeDetail::UnsupportedVersion { version } => {
            json!({ "kind": "unsupportedVersion", "version": version })
        }
        DecodeDetail::TrailingBytes { at, size } => {
            json!({ "kind": "trailingBytes", "at": at, "size": size })
        }
        DecodeDetail::RecursionLimit { where_tag, limit } => {
            json!({ "kind": "recursionLimit", "whereTag": where_tag, "limit": limit })
        }
        DecodeDetail::UnknownTag { where_tag, tag } => {
            json!({ "kind": "unknownTag", "whereTag": where_tag, "tag": tag })
        }
    }
}

pub fn validation_reason_from_json(reason: &Value) -> Result<ValidationReason, String> {
    let kind = reason
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "validation reason.kind must be string".to_string())?;
    match kind {
        "tooLarge" => Ok(ValidationReason::TooLarge {
            size: reason
                .get("size")
                .and_then(Value::as_u64)
                .ok_or_else(|| "tooLarge.size must be integer".to_string())?
                as usize,
            limit: reason
                .get("limit")
                .and_then(Value::as_i64)
                .ok_or_else(|| "tooLarge.limit must be integer".to_string())?,
        }),
        "tooManyDecls" => Ok(ValidationReason::TooManyDecls {
            decl_count: reason
                .get("declCount")
                .and_then(Value::as_u64)
                .ok_or_else(|| "tooManyDecls.declCount must be integer".to_string())?
                as u32,
            limit: reason
                .get("limit")
                .and_then(Value::as_i64)
                .ok_or_else(|| "tooManyDecls.limit must be integer".to_string())?,
        }),
        "decode" => Ok(ValidationReason::Decode {
            detail: decode_detail_from_json(
                reason
                    .get("detail")
                    .ok_or_else(|| "decode.detail must exist".to_string())?,
            )?,
        }),
        other => Err(format!("unknown validation reason {other}")),
    }
}

pub fn decode_detail_from_json(detail: &Value) -> Result<DecodeDetail, String> {
    let kind = detail
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "decode detail.kind must be string".to_string())?;
    match kind {
        "badMagic" => Ok(DecodeDetail::BadMagic),
        "unexpectedEOF" => Ok(DecodeDetail::UnexpectedEof),
        "unsupportedVersion" => Ok(DecodeDetail::UnsupportedVersion {
            version: detail
                .get("version")
                .and_then(Value::as_u64)
                .ok_or_else(|| "unsupportedVersion.version must be integer".to_string())?
                as u32,
        }),
        "trailingBytes" => Ok(DecodeDetail::TrailingBytes {
            at: detail
                .get("at")
                .and_then(Value::as_u64)
                .ok_or_else(|| "trailingBytes.at must be integer".to_string())?
                as usize,
            size: detail
                .get("size")
                .and_then(Value::as_u64)
                .ok_or_else(|| "trailingBytes.size must be integer".to_string())?
                as usize,
        }),
        "recursionLimit" => Ok(DecodeDetail::RecursionLimit {
            where_tag: intern_where_tag(required_json_string(detail, "whereTag")?)?,
            limit: detail
                .get("limit")
                .and_then(Value::as_u64)
                .ok_or_else(|| "recursionLimit.limit must be integer".to_string())?
                as usize,
        }),
        "unknownTag" => Ok(DecodeDetail::UnknownTag {
            where_tag: intern_where_tag(required_json_string(detail, "whereTag")?)?,
            tag: detail
                .get("tag")
                .and_then(Value::as_u64)
                .ok_or_else(|| "unknownTag.tag must be integer".to_string())?
                as u8,
        }),
        other => Err(format!("unknown decode detail {other}")),
    }
}

fn intern_where_tag(value: &str) -> Result<&'static str, String> {
    match value {
        "CanonLevel" => Ok("CanonLevel"),
        "CanonLit" => Ok("CanonLit"),
        "CanonExpr" => Ok("CanonExpr"),
        other => Err(format!("unknown whereTag {other}")),
    }
}

fn required_json_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} must be string"))
}

#[derive(Debug, Clone, Copy)]
struct WalkResult {
    decl_count: u32,
    size: usize,
    universe_arity: u32,
}

fn walk_package(bytes: &[u8]) -> Result<WalkResult, DecodeDetail> {
    if bytes.len() < 12 {
        return Err(DecodeDetail::UnexpectedEof);
    }
    if bytes[0..4] != MAGIC {
        return Err(DecodeDetail::BadMagic);
    }

    let mut cursor = Cursor::new(bytes);
    cursor.skip(4)?;
    let version = cursor.read_u32_le()?;
    if version != FORMAT_VERSION {
        return Err(DecodeDetail::UnsupportedVersion { version });
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
        return Err(DecodeDetail::TrailingBytes {
            at: cursor.pos,
            size: bytes.len(),
        });
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

    fn ensure(&self, n: usize) -> Result<(), DecodeDetail> {
        if self.pos + n > self.bytes.len() {
            Err(DecodeDetail::UnexpectedEof)
        } else {
            Ok(())
        }
    }

    fn read_byte(&mut self) -> Result<u8, DecodeDetail> {
        self.ensure(1)?;
        let out = self.bytes[self.pos];
        self.pos += 1;
        Ok(out)
    }

    fn read_u32_le(&mut self) -> Result<u32, DecodeDetail> {
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

    fn skip(&mut self, n: usize) -> Result<(), DecodeDetail> {
        self.ensure(n)?;
        self.pos += n;
        Ok(())
    }
}

fn walk_level(cursor: &mut Cursor<'_>, depth: usize) -> Result<(), DecodeDetail> {
    if depth > MAX_WALK_DEPTH {
        return Err(DecodeDetail::RecursionLimit {
            where_tag: "CanonLevel",
            limit: MAX_WALK_DEPTH,
        });
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
        _ => Err(DecodeDetail::UnknownTag {
            where_tag: "CanonLevel",
            tag,
        }),
    }
}

fn walk_lit(cursor: &mut Cursor<'_>) -> Result<(), DecodeDetail> {
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
        _ => Err(DecodeDetail::UnknownTag {
            where_tag: "CanonLit",
            tag,
        }),
    }
}

fn walk_name(cursor: &mut Cursor<'_>) -> Result<(), DecodeDetail> {
    let parts = cursor.read_u32_le()?;
    for _ in 0..parts {
        let len = cursor.read_u32_le()? as usize;
        cursor.skip(len)?;
    }
    Ok(())
}

fn walk_expr(cursor: &mut Cursor<'_>, depth: usize) -> Result<(), DecodeDetail> {
    if depth > MAX_WALK_DEPTH {
        return Err(DecodeDetail::RecursionLimit {
            where_tag: "CanonExpr",
            limit: MAX_WALK_DEPTH,
        });
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
        _ => Err(DecodeDetail::UnknownTag {
            where_tag: "CanonExpr",
            tag,
        }),
    }
}

fn walk_decl(cursor: &mut Cursor<'_>) -> Result<(), DecodeDetail> {
    walk_name(cursor)?;
    walk_expr(cursor, 0)?;
    walk_expr(cursor, 0)
}
