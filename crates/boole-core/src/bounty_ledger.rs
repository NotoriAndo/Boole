use serde_json::Value;
use std::collections::HashMap;

const HEX32_LEN: usize = 64;

#[derive(Debug, Default, Clone)]
pub struct BountyEventLedger {
    events: Vec<Value>,
    by_work_id: HashMap<String, Vec<Value>>,
    by_solver_pk: HashMap<String, Vec<Value>>,
    by_verifier_kind: HashMap<String, Vec<Value>>,
}

impl BountyEventLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, event: Value) -> Result<(), String> {
        validate_event(&event)?;
        let work_id = string_field(&event, "workId").expect("validated workId");
        let verifier_kind = string_field(&event, "verifierKind").expect("validated verifierKind");
        push_or_create(&mut self.by_work_id, work_id, event.clone());
        push_or_create(&mut self.by_verifier_kind, verifier_kind, event.clone());
        if string_field(&event, "kind") == Some("proof") {
            if let Some(solver_pk) = string_field(&event, "solverPk") {
                push_or_create(&mut self.by_solver_pk, solver_pk, event.clone());
            }
        }
        self.events.push(event);
        Ok(())
    }

    pub fn get_all(&self) -> Vec<Value> {
        self.events.clone()
    }

    pub fn get_by_work_id(&self, work_id: &str) -> Vec<Value> {
        self.by_work_id.get(work_id).cloned().unwrap_or_default()
    }

    pub fn get_by_solver_pk(&self, pk: &str) -> Vec<Value> {
        self.by_solver_pk.get(pk).cloned().unwrap_or_default()
    }

    pub fn get_by_verifier_kind(&self, kind: &str) -> Vec<Value> {
        self.by_verifier_kind.get(kind).cloned().unwrap_or_default()
    }

    pub fn size(&self) -> usize {
        self.events.len()
    }
}

fn validate_event(event: &Value) -> Result<(), String> {
    let schema = event.get("schemaVersion").and_then(Value::as_i64);
    if schema != Some(1) {
        let rendered = event
            .get("schemaVersion")
            .map(render_scalar)
            .unwrap_or_else(|| "undefined".to_string());
        return Err(format!(
            "bountyLedger: unsupported schemaVersion {rendered} (only v1 accepted)"
        ));
    }

    let kind = string_field(event, "kind").unwrap_or("");
    if !matches!(kind, "create" | "status_change" | "proof") {
        return Err(format!("bountyLedger: unknown event kind: {kind}"));
    }
    if string_field(event, "workId").is_none_or(str::is_empty) {
        return Err("bountyLedger: workId must be a non-empty string".to_string());
    }
    if !string_field(event, "problemHash").is_some_and(is_hex32) {
        return Err("bountyLedger: problemHash must be 32-byte lowercase hex".to_string());
    }
    if string_field(event, "verifierKind").is_none_or(str::is_empty) {
        return Err("bountyLedger: verifierKind must be a non-empty string".to_string());
    }
    if event
        .get("ts")
        .and_then(Value::as_i64)
        .is_none_or(|ts| ts < 0)
    {
        return Err("bountyLedger: ts must be a non-negative integer (unix ms)".to_string());
    }

    if kind == "proof" {
        if !string_field(event, "proofHash").is_some_and(is_hex32) {
            return Err(
                "bountyLedger: proof event requires proofHash (32-byte lowercase hex)".to_string(),
            );
        }
        if !string_field(event, "solverPk").is_some_and(is_hex32) {
            return Err(
                "bountyLedger: proof event requires solverPk (32-byte lowercase hex)".to_string(),
            );
        }
        if !event.get("accepted").is_some_and(Value::is_boolean) {
            return Err("bountyLedger: proof event requires accepted (boolean)".to_string());
        }
    }

    if kind == "status_change" {
        if !string_field(event, "prevStatus").is_some_and(is_status) {
            return Err("bountyLedger: status_change event requires prevStatus".to_string());
        }
        if !string_field(event, "newStatus").is_some_and(is_status) {
            return Err("bountyLedger: status_change event requires newStatus".to_string());
        }
    }

    Ok(())
}

fn push_or_create(map: &mut HashMap<String, Vec<Value>>, key: &str, event: Value) {
    map.entry(key.to_string()).or_default().push(event);
}

fn string_field<'a>(event: &'a Value, key: &str) -> Option<&'a str> {
    event.get(key).and_then(Value::as_str)
}

fn is_status(value: &str) -> bool {
    matches!(value, "open" | "solved" | "expired" | "withdrawn")
}

fn is_hex32(value: &str) -> bool {
    value.len() == HEX32_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn render_scalar(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
