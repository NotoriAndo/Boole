//! Boole AIR IR writer (`boole-air-ir/v1`), shared by the per-zkVM extractor adapters.
//!
//! Standard library only, so the same file compiles inside every zkVM workspace.  An adapter walks the
//! zkVM's own symbolic constraint / interaction records and interns every expression node into a
//! hash-consed [`Dag`] (children before parents; node ids are assigned in first-visit order, so the output
//! is deterministic for a deterministic `eval`).  [`AirDoc::to_json`] and [`RowsDoc::to_json`] print the
//! documents read by `scripts/zk_registry/air_ir.py`.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt::Write as _;

/// One DAG node.  Variables carry the row offset (0 = local, 1 = next).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Node {
    Main(u8, usize),
    Prep(u8, usize),
    Pub(usize),
    First,
    Last,
    Trans,
    Const(u64),
    Add(usize, usize),
    Sub(usize, usize),
    Mul(usize, usize),
    Neg(usize),
}

#[derive(Default)]
pub struct Dag {
    pub nodes: Vec<Node>,
    index: HashMap<Node, usize>,
}

impl Dag {
    pub fn intern(&mut self, n: Node) -> usize {
        if let Some(&i) = self.index.get(&n) {
            return i;
        }
        let i = self.nodes.len();
        self.nodes.push(n.clone());
        self.index.insert(n, i);
        i
    }

    pub fn constant(&mut self, c: u64) -> usize {
        self.intern(Node::Const(c))
    }

    /// `Σ weight·var + constant` for a p3 `VirtualPairCol` (terms in the given order; weight 1 unmultiplied).
    pub fn affine(&mut self, terms: &[(Node, u64)], constant: u64) -> usize {
        let mut acc: Option<usize> = None;
        for (var, w) in terms {
            let v = self.intern(var.clone());
            let t = if *w == 1 {
                v
            } else {
                let c = self.constant(*w);
                self.intern(Node::Mul(c, v))
            };
            acc = Some(match acc {
                None => t,
                Some(a) => self.intern(Node::Add(a, t)),
            });
        }
        match acc {
            None => self.constant(constant),
            Some(a) if constant == 0 => a,
            Some(a) => {
                let c = self.constant(constant);
                self.intern(Node::Add(a, c))
            }
        }
    }
}

pub struct Interaction {
    pub dir: &'static str,
    pub kind: Option<u32>,
    pub kind_name: String,
    pub bus: Option<u32>,
    pub scope: Option<String>,
    pub values: Vec<usize>,
    pub mult: usize,
    pub count_weight: Option<u32>,
}

pub struct AirDoc {
    pub zkvm: String,
    pub release: String,
    pub commit: String,
    pub field_name: String,
    pub p: u64,
    pub name: String,
    pub rust_type: String,
    pub group: String,
    pub index: usize,
    pub width: usize,
    pub prep_width: usize,
    pub num_public_values: usize,
    pub dag: Dag,
    pub constraints: Vec<usize>,
    pub interactions: Vec<Interaction>,
    pub meta: Vec<(String, String)>,
}

pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn opt_u32(x: Option<u32>) -> String {
    x.map(|v| v.to_string()).unwrap_or_else(|| "null".into())
}

fn ids(v: &[usize]) -> String {
    v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")
}

impl AirDoc {
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        s.push_str("{\n");
        let _ = writeln!(s, " \"format\": \"boole-air-ir/v1\",");
        let _ = writeln!(s, " \"zkvm\": {}, \"release\": {}, \"commit\": {},", json_str(&self.zkvm), json_str(&self.release), json_str(&self.commit));
        let _ = writeln!(s, " \"field\": {{\"name\": {}, \"p\": {}}},", json_str(&self.field_name), self.p);
        let _ = writeln!(
            s,
            " \"air\": {{\"name\": {}, \"rust_type\": {}, \"group\": {}, \"index\": {}}},",
            json_str(&self.name),
            json_str(&self.rust_type),
            json_str(&self.group),
            self.index
        );
        let _ = writeln!(
            s,
            " \"width\": {}, \"preprocessed_width\": {}, \"num_public_values\": {},",
            self.width, self.prep_width, self.num_public_values
        );
        s.push_str(" \"meta\": {");
        let meta: Vec<String> = self.meta.iter().map(|(k, v)| format!("{}: {}", json_str(k), json_str(v))).collect();
        s.push_str(&meta.join(", "));
        s.push_str("},\n \"nodes\": [\n");
        for (i, n) in self.dag.nodes.iter().enumerate() {
            let t = match n {
                Node::Main(o, c) => format!("[\"main\",{o},{c}]"),
                Node::Prep(o, c) => format!("[\"prep\",{o},{c}]"),
                Node::Pub(i) => format!("[\"pub\",{i}]"),
                Node::First => "[\"first\"]".into(),
                Node::Last => "[\"last\"]".into(),
                Node::Trans => "[\"trans\"]".into(),
                Node::Const(c) => format!("[\"const\",{c}]"),
                Node::Add(a, b) => format!("[\"add\",{a},{b}]"),
                Node::Sub(a, b) => format!("[\"sub\",{a},{b}]"),
                Node::Mul(a, b) => format!("[\"mul\",{a},{b}]"),
                Node::Neg(a) => format!("[\"neg\",{a}]"),
            };
            let _ = writeln!(s, "  {}{}", t, if i + 1 < self.dag.nodes.len() { "," } else { "" });
        }
        let _ = writeln!(s, " ],\n \"constraints\": [{}],", ids(&self.constraints));
        s.push_str(" \"interactions\": [\n");
        for (i, it) in self.interactions.iter().enumerate() {
            let _ = writeln!(
                s,
                "  {{\"dir\": \"{}\", \"kind\": {}, \"kind_name\": {}, \"bus\": {}, \"scope\": {}, \"values\": [{}], \"mult\": {}, \"count_weight\": {}}}{}",
                it.dir,
                opt_u32(it.kind),
                json_str(&it.kind_name),
                opt_u32(it.bus),
                it.scope.as_deref().map(json_str).unwrap_or_else(|| "null".into()),
                ids(&it.values),
                it.mult,
                opt_u32(it.count_weight),
                if i + 1 < self.interactions.len() { "," } else { "" }
            );
        }
        s.push_str(" ]\n}\n");
        s
    }
}

/// Real trace rows of one AIR from the zkVM's own trace generation.
pub struct RowsDoc {
    pub air_index: usize,
    pub name: String,
    pub source: String,
    pub height: usize,
    pub width: usize,
    pub prep_width: usize,
    pub public_values: Vec<u64>,
    pub main_rows: Vec<(usize, Vec<u64>)>,
    pub prep_rows: Vec<(usize, Vec<u64>)>,
}

fn row_json(rows: &[(usize, Vec<u64>)]) -> String {
    let parts: Vec<String> = rows
        .iter()
        .map(|(i, r)| format!("\"{}\": [{}]", i, r.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")))
        .collect();
    format!("{{\n  {}\n }}", parts.join(",\n  "))
}

impl RowsDoc {
    pub fn to_json(&self) -> String {
        format!(
            "{{\n \"format\": \"boole-air-rows/v1\",\n \"air_index\": {}, \"name\": {}, \"source\": {},\n \"height\": {}, \"width\": {}, \"preprocessed_width\": {},\n \"public_values\": [{}],\n \"main\": {},\n \"prep\": {}\n}}\n",
            self.air_index,
            json_str(&self.name),
            json_str(&self.source),
            self.height,
            self.width,
            self.prep_width,
            self.public_values.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","),
            row_json(&self.main_rows),
            row_json(&self.prep_rows)
        )
    }
}

/// Deterministic xorshift64* generator (no external crates).
pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
}

/// Row indices to dump from a trace of `height` rows: the first `first` rows, `random` further rows and the
/// last row, each with its successor (wrapping), sorted and deduplicated.
pub fn select_rows(height: usize, first: usize, random: usize, seed: u64) -> Vec<usize> {
    if height == 0 {
        return vec![];
    }
    let mut v: Vec<usize> = (0..first.min(height)).collect();
    let mut rng = Rng(seed | 1);
    for _ in 0..random {
        v.push((rng.next() % height as u64) as usize);
    }
    v.push(height - 1);
    let succ: Vec<usize> = v.iter().map(|i| (i + 1) % height).collect();
    v.extend(succ);
    v.sort_unstable();
    v.dedup();
    v
}

/// Manifest line for one AIR: extracted, or failed with the reason.
pub fn manifest_line(index: usize, name: &str, rust_type: &str, group: &str, status: &str, detail: &str) -> String {
    format!(
        "{{\"index\": {}, \"name\": {}, \"rust_type\": {}, \"group\": {}, \"status\": {}, \"detail\": {}}}",
        index,
        json_str(name),
        json_str(rust_type),
        json_str(group),
        json_str(status),
        json_str(detail)
    )
}

/// Message of a caught panic payload.
pub fn panic_message(e: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = e.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = e.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic with a non-string payload".into()
    }
}
