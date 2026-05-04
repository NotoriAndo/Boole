use crate::{validation_reason_from_json, validation_reason_json, ValidationReason};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectionEvent {
    pub ts: i64,
    pub ip: String,
    pub pk: Option<String>,
    pub c: Option<String>,
    pub reason: LoggedRejectionReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoggedRejectionReason {
    BadRequest { field: String },
    RateLimit { quota: String },
    Decode { field: String, detail: String },
    Validator { reason: ValidationReason },
    SubmitPow { detail: String },
    SharePool { detail: String },
    Ticket { detail: String },
}

#[derive(Debug, Clone)]
pub struct RingRejectionLogger {
    capacity: usize,
    buf: VecDeque<RejectionEvent>,
    total: usize,
    counts: BTreeMap<String, usize>,
}

impl RingRejectionLogger {
    pub fn new(capacity: usize) -> Result<Self, String> {
        if capacity == 0 {
            return Err("capacity must be > 0".to_string());
        }
        Ok(Self {
            capacity,
            buf: VecDeque::with_capacity(capacity),
            total: 0,
            counts: BTreeMap::new(),
        })
    }

    pub fn record(&mut self, event: Value) {
        let event = rejection_event_from_json(&event).expect("valid rejection event json");
        self.record_typed(event);
    }

    pub fn record_typed(&mut self, event: RejectionEvent) {
        if self.buf.len() == self.capacity {
            self.buf.pop_front();
        }
        let key = reason_key_typed(&event.reason);
        self.buf.push_back(event);
        self.total += 1;
        *self.counts.entry(key).or_insert(0) += 1;
    }

    pub fn events(&self) -> Vec<Value> {
        self.buf.iter().map(rejection_event_json).collect()
    }

    pub fn events_typed(&self) -> Vec<RejectionEvent> {
        self.buf.iter().cloned().collect()
    }

    pub fn total_count(&self) -> usize {
        self.total
    }

    pub fn counts_by_reason(&self) -> BTreeMap<String, usize> {
        self.counts.clone()
    }
}

pub fn rejection_event_from_json(event: &Value) -> Result<RejectionEvent, String> {
    Ok(RejectionEvent {
        ts: event
            .get("ts")
            .and_then(Value::as_i64)
            .ok_or_else(|| "event.ts must be integer".to_string())?,
        ip: event
            .get("ip")
            .and_then(Value::as_str)
            .ok_or_else(|| "event.ip must be string".to_string())?
            .to_string(),
        pk: optional_string_field(event, "pk")?,
        c: optional_string_field(event, "c")?,
        reason: rejection_reason_from_json(
            event
                .get("reason")
                .ok_or_else(|| "event.reason must exist".to_string())?,
        )?,
    })
}

pub fn rejection_event_json(event: &RejectionEvent) -> Value {
    json!({
        "ts": event.ts,
        "ip": event.ip,
        "pk": event.pk,
        "c": event.c,
        "reason": rejection_reason_json(&event.reason),
    })
}

pub fn rejection_event_line(event: &RejectionEvent) -> String {
    format!(
        "{{\"ts\":{},\"ip\":{},\"pk\":{},\"c\":{},\"reason\":{}}}",
        event.ts,
        serde_json::to_string(&event.ip).expect("ip json"),
        option_string_json(&event.pk),
        option_string_json(&event.c),
        reason_line_json(&event.reason),
    )
}

pub fn json_rejection_line(event: &Value) -> String {
    let event = rejection_event_from_json(event).expect("valid rejection event json");
    rejection_event_line(&event)
}

pub fn reason_key(reason: &Value) -> String {
    let reason = rejection_reason_from_json(reason).expect("valid rejection reason json");
    reason_key_typed(&reason)
}

pub fn reason_key_typed(reason: &LoggedRejectionReason) -> String {
    match reason {
        LoggedRejectionReason::BadRequest { field } => format!("bad_request:{field}"),
        LoggedRejectionReason::RateLimit { quota } => format!("rate_limit:{quota}"),
        LoggedRejectionReason::Decode { field, .. } => format!("decode:{field}"),
        LoggedRejectionReason::Validator { reason } => {
            let kind = validation_reason_json(reason)
                .get("kind")
                .and_then(Value::as_str)
                .expect("validator reason.kind")
                .to_string();
            format!("validator:{kind}")
        }
        LoggedRejectionReason::SubmitPow { detail } => format!("submit_pow:{detail}"),
        LoggedRejectionReason::SharePool { detail } => format!("share_pool:{detail}"),
        LoggedRejectionReason::Ticket { detail } => format!("ticket:{detail}"),
    }
}

fn rejection_reason_from_json(reason: &Value) -> Result<LoggedRejectionReason, String> {
    let stage = reason
        .get("stage")
        .and_then(Value::as_str)
        .ok_or_else(|| "reason.stage must be string".to_string())?;
    match stage {
        "bad_request" => Ok(LoggedRejectionReason::BadRequest {
            field: required_string(reason, "field")?.to_string(),
        }),
        "rate_limit" => Ok(LoggedRejectionReason::RateLimit {
            quota: required_string(reason, "quota")?.to_string(),
        }),
        "decode" => Ok(LoggedRejectionReason::Decode {
            field: required_string(reason, "field")?.to_string(),
            detail: required_string(reason, "detail")?.to_string(),
        }),
        "validator" => Ok(LoggedRejectionReason::Validator {
            reason: validation_reason_from_json(
                reason
                    .get("reason")
                    .ok_or_else(|| "validator.reason must exist".to_string())?,
            )?,
        }),
        "submit_pow" => Ok(LoggedRejectionReason::SubmitPow {
            detail: required_string(reason, "detail")?.to_string(),
        }),
        "share_pool" => Ok(LoggedRejectionReason::SharePool {
            detail: required_string(reason, "detail")?.to_string(),
        }),
        "ticket" => Ok(LoggedRejectionReason::Ticket {
            detail: required_string(reason, "detail")?.to_string(),
        }),
        other => Err(format!("unknown rejection stage {other}")),
    }
}

fn rejection_reason_json(reason: &LoggedRejectionReason) -> Value {
    match reason {
        LoggedRejectionReason::BadRequest { field } => {
            json!({ "stage": "bad_request", "field": field })
        }
        LoggedRejectionReason::RateLimit { quota } => {
            json!({ "stage": "rate_limit", "quota": quota })
        }
        LoggedRejectionReason::Decode { field, detail } => {
            json!({ "stage": "decode", "field": field, "detail": detail })
        }
        LoggedRejectionReason::Validator { reason } => {
            json!({ "stage": "validator", "reason": validation_reason_json(reason) })
        }
        LoggedRejectionReason::SubmitPow { detail } => {
            json!({ "stage": "submit_pow", "detail": detail })
        }
        LoggedRejectionReason::SharePool { detail } => {
            json!({ "stage": "share_pool", "detail": detail })
        }
        LoggedRejectionReason::Ticket { detail } => {
            json!({ "stage": "ticket", "detail": detail })
        }
    }
}

fn reason_line_json(reason: &LoggedRejectionReason) -> String {
    match reason {
        LoggedRejectionReason::BadRequest { field } => format!(
            "{{\"stage\":\"bad_request\",\"field\":{}}}",
            serde_json::to_string(field).expect("field json")
        ),
        LoggedRejectionReason::RateLimit { quota } => format!(
            "{{\"stage\":\"rate_limit\",\"quota\":{}}}",
            serde_json::to_string(quota).expect("quota json")
        ),
        LoggedRejectionReason::Decode { field, detail } => format!(
            "{{\"stage\":\"decode\",\"field\":{},\"detail\":{}}}",
            serde_json::to_string(field).expect("field json"),
            serde_json::to_string(detail).expect("detail json")
        ),
        LoggedRejectionReason::Validator { reason } => format!(
            "{{\"stage\":\"validator\",\"reason\":{}}}",
            validation_reason_line_json(reason)
        ),
        LoggedRejectionReason::SubmitPow { detail } => format!(
            "{{\"stage\":\"submit_pow\",\"detail\":{}}}",
            serde_json::to_string(detail).expect("detail json")
        ),
        LoggedRejectionReason::SharePool { detail } => format!(
            "{{\"stage\":\"share_pool\",\"detail\":{}}}",
            serde_json::to_string(detail).expect("detail json")
        ),
        LoggedRejectionReason::Ticket { detail } => format!(
            "{{\"stage\":\"ticket\",\"detail\":{}}}",
            serde_json::to_string(detail).expect("detail json")
        ),
    }
}

fn validation_reason_line_json(reason: &ValidationReason) -> String {
    match reason {
        ValidationReason::TooLarge { size, limit } => format!(
            "{{\"kind\":\"tooLarge\",\"size\":{},\"limit\":{}}}",
            size, limit
        ),
        ValidationReason::TooManyDecls { decl_count, limit } => format!(
            "{{\"kind\":\"tooManyDecls\",\"declCount\":{},\"limit\":{}}}",
            decl_count, limit
        ),
        ValidationReason::Decode { detail } => format!(
            "{{\"kind\":\"decode\",\"detail\":{}}}",
            decode_detail_line_json(detail)
        ),
    }
}

fn decode_detail_line_json(detail: &crate::DecodeDetail) -> String {
    match detail {
        crate::DecodeDetail::BadMagic => "{\"kind\":\"badMagic\"}".to_string(),
        crate::DecodeDetail::UnexpectedEof => "{\"kind\":\"unexpectedEOF\"}".to_string(),
        crate::DecodeDetail::UnsupportedVersion { version } => {
            format!("{{\"kind\":\"unsupportedVersion\",\"version\":{version}}}")
        }
        crate::DecodeDetail::TrailingBytes { at, size } => {
            format!("{{\"kind\":\"trailingBytes\",\"at\":{at},\"size\":{size}}}")
        }
        crate::DecodeDetail::RecursionLimit { where_tag, limit } => format!(
            "{{\"kind\":\"recursionLimit\",\"whereTag\":{},\"limit\":{}}}",
            serde_json::to_string(where_tag).expect("whereTag json"),
            limit
        ),
        crate::DecodeDetail::UnknownTag { where_tag, tag } => format!(
            "{{\"kind\":\"unknownTag\",\"whereTag\":{},\"tag\":{}}}",
            serde_json::to_string(where_tag).expect("whereTag json"),
            tag
        ),
    }
}

fn optional_string_field(value: &Value, key: &str) -> Result<Option<String>, String> {
    match value.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("event.{key} must be string or null")),
    }
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{key} must be string"))
}

fn option_string_json(value: &Option<String>) -> String {
    match value {
        Some(value) => serde_json::to_string(value).expect("string json"),
        None => "null".to_string(),
    }
}
