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
        validate_bounty_ledger_event(&event)?;
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

pub fn validate_bounty_ledger_event(event: &Value) -> Result<(), String> {
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
    if !matches!(kind, "create" | "status_change" | "proof" | "credit") {
        return Err(format!("bountyLedger: unknown event kind: {kind}"));
    }

    // S23c — credit events carry block-level identifiers (height, c) and
    // payment routing (familyId, bountyId, prover, amount). They predate
    // the workId/problemHash/verifierKind/ts shape used by the other
    // three event kinds, so dispatch validation by kind here.
    if kind == "credit" {
        if event.get("height").and_then(Value::as_u64).is_none() {
            return Err(
                "bountyLedger: credit event requires height (unsigned integer)".to_string(),
            );
        }
        if !string_field(event, "c").is_some_and(is_hex32) {
            return Err(
                "bountyLedger: credit event requires c (32-byte lowercase hex)".to_string(),
            );
        }
        if string_field(event, "familyId").is_none_or(str::is_empty) {
            return Err("bountyLedger: credit event requires familyId".to_string());
        }
        if string_field(event, "bountyId").is_none_or(str::is_empty) {
            return Err("bountyLedger: credit event requires bountyId".to_string());
        }
        if !string_field(event, "prover").is_some_and(is_hex32) {
            return Err(
                "bountyLedger: credit event requires prover (32-byte lowercase hex)".to_string(),
            );
        }
        match string_field(event, "amount") {
            Some(s) if s.parse::<u128>().is_ok() => {}
            _ => {
                return Err(
                    "bountyLedger: credit event requires amount (u128 decimal string)".to_string(),
                );
            }
        }
        return Ok(());
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

    if kind == "create" {
        // S13b durable announce events embed the full Bounty under `bounty`
        // so a restart can rebuild a dynamically-announced registry without
        // an external catalog. Legacy pof fixtures predate this and carry
        // only the flat fields, so the sub-object is optional. When it IS
        // present the cross-checks run — a divergence between the flat
        // index fields and the embedded record would let a replay restore
        // a bounty under the wrong id and silently corrupt state.
        if let Some(bounty) = event.get("bounty").and_then(Value::as_object) {
            let bounty_id = bounty
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| "bountyLedger: create event bounty.id missing".to_string())?;
            let work_id = string_field(event, "workId").unwrap_or("");
            if bounty_id != work_id {
                return Err(format!(
                    "bountyLedger: create event workId/bounty.id mismatch ({work_id} vs {bounty_id})"
                ));
            }
            let bounty_problem_hash = bounty
                .get("problemHash")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    "bountyLedger: create event bounty.problemHash missing".to_string()
                })?;
            let problem_hash = string_field(event, "problemHash").unwrap_or("");
            if bounty_problem_hash != problem_hash {
                return Err(
                    "bountyLedger: create event problemHash mismatch with bounty.problemHash"
                        .to_string(),
                );
            }
            let bounty_verifier_kind = bounty
                .get("verifier")
                .and_then(|v| v.get("kind"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    "bountyLedger: create event bounty.verifier.kind missing".to_string()
                })?;
            let verifier_kind = string_field(event, "verifierKind").unwrap_or("");
            if bounty_verifier_kind != verifier_kind {
                return Err(
                    "bountyLedger: create event verifierKind mismatch with bounty.verifier.kind"
                        .to_string(),
                );
            }
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
