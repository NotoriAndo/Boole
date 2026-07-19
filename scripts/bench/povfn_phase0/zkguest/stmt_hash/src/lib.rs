//! Structural statement hashing over lean4export NDJSON (format 3.1.0).
//!
//! Shared by the zkVM guest (computes the hash of the target theorem's type
//! inside the proof) and the host/node side (computes the expected hash from
//! a statement-only reference export). The hash is content-addressed and
//! independent of the interning ids a particular export run assigns.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

fn hex(d: impl AsRef<[u8]>) -> String {
    d.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

struct Tables {
    names: HashMap<u64, Value>,
    levels: HashMap<u64, Value>,
    exprs: HashMap<u64, Value>,
    decls: Vec<Value>,
}

fn parse(bytes: &[u8]) -> Result<Tables, String> {
    let mut t = Tables {
        names: HashMap::new(),
        levels: HashMap::new(),
        exprs: HashMap::new(),
        decls: Vec::new(),
    };
    for (lineno, line) in bytes.split(|b| *b == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let v: Value = serde_json::from_slice(line)
            .map_err(|e| format!("line {lineno}: {e}"))?;
        let o = v.as_object().ok_or("non-object line")?;
        if let Some(i) = o.get("in").and_then(Value::as_u64) {
            t.names.insert(i, v.clone());
        } else if let Some(i) = o.get("il").and_then(Value::as_u64) {
            t.levels.insert(i, v.clone());
        } else if let Some(i) = o.get("ie").and_then(Value::as_u64) {
            t.exprs.insert(i, v.clone());
        } else if ["thm", "def", "axiom", "inductive", "opaque", "quot"]
            .iter()
            .any(|k| o.contains_key(*k))
        {
            t.decls.push(v.clone());
        }
    }
    Ok(t)
}

fn name_string(t: &Tables, idx: u64) -> String {
    if idx == 0 {
        return String::new();
    }
    let node = &t.names[&idx];
    let o = node.as_object().unwrap();
    let (pre, part) = if let Some(s) = o.get("str") {
        (
            s["pre"].as_u64().unwrap(),
            s["str"].as_str().unwrap().to_string(),
        )
    } else {
        let n = &o["num"];
        (n["pre"].as_u64().unwrap(), n["i"].to_string())
    };
    let prefix = name_string(t, pre);
    if prefix.is_empty() {
        part
    } else {
        format!("{prefix}.{part}")
    }
}

fn hash_name(t: &Tables, memo: &mut HashMap<u64, String>, idx: u64) -> String {
    if idx == 0 {
        return "anon".into();
    }
    if let Some(h) = memo.get(&idx) {
        return h.clone();
    }
    let o = t.names[&idx].as_object().unwrap();
    let payload = if let Some(s) = o.get("str") {
        format!(
            "str|{}|{}",
            hash_name(t, memo, s["pre"].as_u64().unwrap()),
            s["str"].as_str().unwrap()
        )
    } else {
        let n = &o["num"];
        format!(
            "num|{}|{}",
            hash_name(t, memo, n["pre"].as_u64().unwrap()),
            n["i"]
        )
    };
    let h = hex(Sha256::digest(payload.as_bytes()));
    memo.insert(idx, h.clone());
    h
}

fn hash_level(
    t: &Tables,
    nmemo: &mut HashMap<u64, String>,
    memo: &mut HashMap<u64, String>,
    idx: u64,
) -> String {
    if idx == 0 {
        return "zero".into();
    }
    if let Some(h) = memo.get(&idx) {
        return h.clone();
    }
    let o = t.levels[&idx].as_object().unwrap();
    let payload = if let Some(v) = o.get("param") {
        format!("param|{}", hash_name(t, nmemo, v.as_u64().unwrap()))
    } else if let Some(v) = o.get("succ") {
        format!("succ|{}", hash_level(t, nmemo, memo, v.as_u64().unwrap()))
    } else if let Some(v) = o.get("max") {
        let a = hash_level(t, nmemo, memo, v[0].as_u64().unwrap());
        let b = hash_level(t, nmemo, memo, v[1].as_u64().unwrap());
        format!("max|{a}|{b}")
    } else if let Some(v) = o.get("imax") {
        let a = hash_level(t, nmemo, memo, v[0].as_u64().unwrap());
        let b = hash_level(t, nmemo, memo, v[1].as_u64().unwrap());
        format!("imax|{a}|{b}")
    } else {
        format!("lvl|{}", Value::Object(o.clone()))
    };
    let h = hex(Sha256::digest(payload.as_bytes()));
    memo.insert(idx, h.clone());
    h
}

fn hash_expr(
    t: &Tables,
    nmemo: &mut HashMap<u64, String>,
    lmemo: &mut HashMap<u64, String>,
    memo: &mut HashMap<u64, String>,
    idx: u64,
) -> String {
    if let Some(h) = memo.get(&idx) {
        return h.clone();
    }
    let o = t.exprs[&idx].as_object().unwrap();
    let (kind, val) = o.iter().find(|(k, _)| *k != "ie").unwrap();
    let payload = match kind.as_str() {
        "bvar" => format!("bvar|{val}"),
        "sort" => format!("sort|{}", hash_level(t, nmemo, lmemo, val.as_u64().unwrap())),
        "const" => {
            let name = hash_name(t, nmemo, val["name"].as_u64().unwrap());
            let us: Vec<String> = val["us"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|u| hash_level(t, nmemo, lmemo, u.as_u64().unwrap()))
                        .collect()
                })
                .unwrap_or_default();
            format!("const|{name}|{}", us.join(","))
        }
        "app" => format!(
            "app|{}|{}",
            hash_expr(t, nmemo, lmemo, memo, val["fn"].as_u64().unwrap()),
            hash_expr(t, nmemo, lmemo, memo, val["arg"].as_u64().unwrap())
        ),
        "lam" | "forallE" => format!(
            "{kind}|{}|{}|{}",
            hash_expr(t, nmemo, lmemo, memo, val["type"].as_u64().unwrap()),
            hash_expr(t, nmemo, lmemo, memo, val["body"].as_u64().unwrap()),
            val.get("binderInfo").map(|v| v.to_string()).unwrap_or_default()
        ),
        "letE" => format!(
            "letE|{}|{}|{}",
            hash_expr(t, nmemo, lmemo, memo, val["type"].as_u64().unwrap()),
            hash_expr(t, nmemo, lmemo, memo, val["value"].as_u64().unwrap()),
            hash_expr(t, nmemo, lmemo, memo, val["body"].as_u64().unwrap())
        ),
        "proj" => format!(
            "proj|{}|{}|{}",
            hash_name(t, nmemo, val["struct"].as_u64().unwrap()),
            val["idx"],
            hash_expr(t, nmemo, lmemo, memo, val["expr"].as_u64().unwrap())
        ),
        "natVal" => format!("natVal|{val}"),
        "strVal" => format!("strVal|{val}"),
        "mdata" => format!(
            "mdata|{}",
            hash_expr(t, nmemo, lmemo, memo, val["expr"].as_u64().unwrap())
        ),
        other => format!("{other}|{val}"),
    };
    let h = hex(Sha256::digest(payload.as_bytes()));
    memo.insert(idx, h.clone());
    h
}

/// Find `full_name` among exported declarations and return the structural
/// hash of its TYPE (the statement). Returns None when absent.
pub fn statement_hash(bytes: &[u8], full_name: &str) -> Result<Option<String>, String> {
    let t = parse(bytes)?;
    let mut nmemo = HashMap::new();
    let mut lmemo = HashMap::new();
    let mut ememo = HashMap::new();
    for d in &t.decls {
        let o = d.as_object().unwrap();
        for kind in ["thm", "def", "axiom"] {
            if let Some(body) = o.get(kind) {
                let name_idx = body["name"].as_u64().unwrap();
                if name_string(&t, name_idx) == full_name {
                    let ty = body["type"].as_u64().unwrap();
                    return Ok(Some(hash_expr(&t, &mut nmemo, &mut lmemo, &mut ememo, ty)));
                }
            }
        }
    }
    Ok(None)
}
