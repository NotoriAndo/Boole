use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone)]
pub struct RingRejectionLogger {
    capacity: usize,
    buf: VecDeque<Value>,
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
        if self.buf.len() == self.capacity {
            self.buf.pop_front();
        }
        let key = reason_key(event.get("reason").expect("event.reason exists"));
        self.buf.push_back(event);
        self.total += 1;
        *self.counts.entry(key).or_insert(0) += 1;
    }

    pub fn events(&self) -> Vec<Value> {
        self.buf.iter().cloned().collect()
    }

    pub fn total_count(&self) -> usize {
        self.total
    }

    pub fn counts_by_reason(&self) -> BTreeMap<String, usize> {
        self.counts.clone()
    }
}

pub fn json_rejection_line(event: &Value) -> String {
    let ts = event.get("ts").expect("ts");
    let ip = event.get("ip").expect("ip");
    let pk = event.get("pk").expect("pk");
    let c = event.get("c").expect("c");
    let reason = event.get("reason").expect("reason");
    format!(
        "{{\"ts\":{},\"ip\":{},\"pk\":{},\"c\":{},\"reason\":{}}}",
        serde_json::to_string(ts).expect("ts json"),
        serde_json::to_string(ip).expect("ip json"),
        serde_json::to_string(pk).expect("pk json"),
        serde_json::to_string(c).expect("c json"),
        reason_json(reason),
    )
}

pub fn reason_key(reason: &Value) -> String {
    let stage = reason
        .get("stage")
        .and_then(Value::as_str)
        .expect("reason.stage");
    match stage {
        "bad_request" => format!("bad_request:{}", string_field(reason, "field")),
        "rate_limit" => format!("rate_limit:{}", string_field(reason, "quota")),
        "decode" => format!("decode:{}", string_field(reason, "field")),
        "validator" => {
            let kind = reason
                .get("reason")
                .and_then(|inner| inner.get("kind"))
                .and_then(Value::as_str)
                .expect("validator reason.kind");
            format!("validator:{kind}")
        }
        "submit_pow" => format!("submit_pow:{}", string_field(reason, "detail")),
        "share_pool" => format!("share_pool:{}", string_field(reason, "detail")),
        "ticket" => format!("ticket:{}", string_field(reason, "detail")),
        other => panic!("unknown rejection stage {other}"),
    }
}

fn reason_json(reason: &Value) -> String {
    let stage = string_field(reason, "stage");
    match stage {
        "bad_request" => format!(
            "{{\"stage\":\"bad_request\",\"field\":{}}}",
            serde_json::to_string(reason.get("field").expect("field")).expect("field json")
        ),
        "rate_limit" => format!(
            "{{\"stage\":\"rate_limit\",\"quota\":{}}}",
            serde_json::to_string(reason.get("quota").expect("quota")).expect("quota json")
        ),
        "decode" => format!(
            "{{\"stage\":\"decode\",\"field\":{},\"detail\":{}}}",
            serde_json::to_string(reason.get("field").expect("field")).expect("field json"),
            serde_json::to_string(reason.get("detail").expect("detail")).expect("detail json")
        ),
        "validator" => format!(
            "{{\"stage\":\"validator\",\"reason\":{}}}",
            validation_reason_json(reason.get("reason").expect("validator reason"))
        ),
        "submit_pow" => format!(
            "{{\"stage\":\"submit_pow\",\"detail\":{}}}",
            serde_json::to_string(reason.get("detail").expect("detail")).expect("detail json")
        ),
        "share_pool" => format!(
            "{{\"stage\":\"share_pool\",\"detail\":{}}}",
            serde_json::to_string(reason.get("detail").expect("detail")).expect("detail json")
        ),
        "ticket" => format!(
            "{{\"stage\":\"ticket\",\"detail\":{}}}",
            serde_json::to_string(reason.get("detail").expect("detail")).expect("detail json")
        ),
        other => panic!("unknown rejection stage {other}"),
    }
}

fn validation_reason_json(reason: &Value) -> String {
    let kind = string_field(reason, "kind");
    match kind {
        "tooLarge" => format!(
            "{{\"kind\":\"tooLarge\",\"size\":{},\"limit\":{}}}",
            serde_json::to_string(reason.get("size").expect("size")).expect("size json"),
            serde_json::to_string(reason.get("limit").expect("limit")).expect("limit json")
        ),
        "tooManyDecls" => format!(
            "{{\"kind\":\"tooManyDecls\",\"declCount\":{},\"limit\":{}}}",
            serde_json::to_string(reason.get("declCount").expect("declCount"))
                .expect("declCount json"),
            serde_json::to_string(reason.get("limit").expect("limit")).expect("limit json")
        ),
        "decode" => format!(
            "{{\"kind\":\"decode\",\"detail\":{}}}",
            decode_detail_json(reason.get("detail").expect("detail"))
        ),
        other => panic!("unknown validator reason {other}"),
    }
}

fn decode_detail_json(detail: &Value) -> String {
    match string_field(detail, "kind") {
        "badMagic" => "{\"kind\":\"badMagic\"}".to_string(),
        "unexpectedEOF" => "{\"kind\":\"unexpectedEOF\"}".to_string(),
        other => serde_json::to_string(detail)
            .unwrap_or_else(|_| panic!("unknown decode detail {other}")),
    }
}

fn string_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(Value::as_str)
        .expect("string field")
}
