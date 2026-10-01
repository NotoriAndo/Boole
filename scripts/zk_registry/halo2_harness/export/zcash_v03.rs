//! Boole H1 export of a `MockProver` run as `boole-halo2-ir/v1` (zcash `halo2_proofs` 0.3 line).
//!
//! The generator copies this file into a scratch copy of the pinned `halo2_proofs` as
//! `src/dev/boole_export.rs` and declares it from `src/dev.rs`; it only reads the prover's state after
//! `MockProver::run` (gates and lookups after selector compression, exactly as keygen builds them, fixed /
//! advice / instance values, the permutation mapping and the regions).  One added call in
//! `MockProver::assign_advice` (`note_advice`) records the region and annotation of every advice assignment,
//! so that a wrapper can name the cells an instruction witnesses from its `Value` parameters.  Nothing here
//! changes what the prover assigns or checks.
//!
//! Field elements are printed as big-endian hex of `PrimeField::to_repr` (reversed little-endian bytes); the
//! document carries the encodings of 1 and 2 so the reader can confirm the byte order.

use std::cell::RefCell;
use std::fmt::Write as _;

use ff::PrimeField;

use super::{CellValue, InstanceValue, MockProver};
use crate::plonk::{Any, Expression};

thread_local! {
    static NOTES: RefCell<Vec<(usize, String, usize, usize, String)>> = RefCell::new(Vec::new());
}

/// Called from `MockProver::assign_advice`: region index, region name, column, row, annotation.
pub(crate) fn note_advice(region: usize, region_name: &str, column: usize, row: usize, annotation: String) {
    NOTES.with(|n| n.borrow_mut().push((region, region_name.to_string(), column, row, annotation)));
}

/// Clears the recorded advice notes (call before `MockProver::run`).
pub fn reset_notes() {
    NOTES.with(|n| n.borrow_mut().clear());
}

fn fe<F: PrimeField>(v: &F) -> String {
    let repr = v.to_repr();
    let mut s = String::with_capacity(70);
    s.push('"');
    for b in repr.as_ref().iter().rev() {
        write!(s, "{:02x}", b).unwrap();
    }
    s.push('"');
    s
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn expr<F: PrimeField>(e: &Expression<F>, out: &mut String) {
    match e {
        Expression::Constant(c) => {
            out.push_str("[\"c\",");
            out.push_str(&fe(c));
            out.push(']');
        }
        Expression::Selector(s) => write!(out, "[\"s\",{}]", s.0).unwrap(),
        Expression::Fixed(q) => write!(out, "[\"f\",{},{}]", q.column_index, q.rotation.0).unwrap(),
        Expression::Advice(q) => write!(out, "[\"a\",{},{}]", q.column_index, q.rotation.0).unwrap(),
        Expression::Instance(q) => write!(out, "[\"i\",{},{}]", q.column_index, q.rotation.0).unwrap(),
        Expression::Negated(a) => {
            out.push_str("[\"neg\",");
            expr(a, out);
            out.push(']');
        }
        Expression::Sum(a, b) => {
            out.push_str("[\"add\",");
            expr(a, out);
            out.push(',');
            expr(b, out);
            out.push(']');
        }
        Expression::Product(a, b) => {
            out.push_str("[\"mul\",");
            expr(a, out);
            out.push(',');
            expr(b, out);
            out.push(']');
        }
        Expression::Scaled(a, c) => {
            out.push_str("[\"scale\",");
            expr(a, out);
            out.push(',');
            out.push_str(&fe(c));
            out.push(']');
        }
    }
}

fn exprs<F: PrimeField>(es: &[Expression<F>], out: &mut String) {
    out.push('[');
    for (i, e) in es.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        expr(e, out);
    }
    out.push(']');
}

fn kind(c: Any) -> &'static str {
    match c {
        Any::Advice => "a",
        Any::Fixed => "f",
        Any::Instance => "i",
    }
}

/// Fields the exporter (and the wrapper driver) accept: those `MockProver::run` / `verify` accept here.
pub trait ExportField: PrimeField + Ord {}

impl<F: PrimeField + Ord> ExportField for F {}

impl<F: ExportField> MockProver<F> {
    /// The run as a `boole-halo2-ir/v1` JSON document.  `meta` is a JSON object text supplied by the wrapper
    /// (target, inputs by annotation, output column, sample description).
    pub fn boole_export(&self, meta: &str) -> String {
        let n = self.n as usize;
        let mut o = String::with_capacity(1 << 20);
        o.push_str("{\"format\":\"boole-halo2-ir/v1\",\"halo2_line\":\"zcash-0.3\",");
        write!(
            o,
            "\"k\":{},\"n\":{},\"usable_rows\":{},\"num_fixed\":{},\"num_advice\":{},\"num_instance\":{},",
            self.k,
            n,
            self.usable_rows.end,
            self.fixed.len(),
            self.advice.len(),
            self.instance.len()
        )
        .unwrap();
        write!(o, "\"field\":{{\"modulus\":{},\"one\":{},\"two\":{}}},", json_str(F::MODULUS), fe(&F::ONE),
               fe(&F::from(2u64)))
        .unwrap();
        o.push_str("\"meta\":");
        o.push_str(if meta.is_empty() { "{}" } else { meta });
        o.push_str(",\"gates\":[");
        for (gi, g) in self.cs.gates.iter().enumerate() {
            if gi > 0 {
                o.push(',');
            }
            write!(o, "{{\"name\":{},\"polys\":[", json_str(g.name())).unwrap();
            for (pi, p) in g.polynomials().iter().enumerate() {
                if pi > 0 {
                    o.push(',');
                }
                write!(o, "{{\"name\":{},\"e\":", json_str(g.constraint_name(pi))).unwrap();
                expr(p, &mut o);
                o.push('}');
            }
            o.push_str("]}");
        }
        o.push_str("],\"lookups\":[");
        for (li, l) in self.cs.lookups.iter().enumerate() {
            if li > 0 {
                o.push(',');
            }
            o.push_str("{\"input\":");
            exprs(&l.input_expressions, &mut o);
            o.push_str(",\"table\":");
            exprs(&l.table_expressions, &mut o);
            o.push('}');
        }
        o.push_str("],\"perm_columns\":[");
        let pcols = self.cs.permutation.get_columns();
        for (i, c) in pcols.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            write!(o, "[\"{}\",{}]", kind(*c.column_type()), c.index()).unwrap();
        }
        o.push_str("],\"cycles\":[");
        let mapping = &self.permutation.mapping;
        let mut seen: Vec<Vec<bool>> = mapping.iter().map(|c| vec![false; c.len()]).collect();
        let mut first = true;
        for c in 0..mapping.len() {
            for r in 0..mapping[c].len() {
                if seen[c][r] || mapping[c][r] == (c, r) {
                    continue;
                }
                let mut cyc = vec![(c, r)];
                seen[c][r] = true;
                let mut cur = mapping[c][r];
                while cur != (c, r) {
                    seen[cur.0][cur.1] = true;
                    cyc.push(cur);
                    cur = mapping[cur.0][cur.1];
                }
                if !first {
                    o.push(',');
                }
                first = false;
                o.push('[');
                for (j, (cc, rr)) in cyc.iter().enumerate() {
                    if j > 0 {
                        o.push(',');
                    }
                    write!(o, "[{},{}]", cc, rr).unwrap();
                }
                o.push(']');
            }
        }
        o.push_str("],\"fixed\":[");
        let mut first = true;
        for (c, col) in self.fixed.iter().enumerate() {
            for (r, v) in col.iter().enumerate() {
                let txt = match v {
                    CellValue::Assigned(x) if !bool::from(x.is_zero()) => fe(x),
                    CellValue::Poison(_) => "\"poison\"".to_string(),
                    _ => continue,
                };
                if !first {
                    o.push(',');
                }
                first = false;
                write!(o, "[{},{},{}]", c, r, txt).unwrap();
            }
        }
        o.push_str("],\"advice\":[");
        let mut first = true;
        for (c, col) in self.advice.iter().enumerate() {
            for (r, v) in col.iter().enumerate() {
                let txt = match v {
                    CellValue::Assigned(x) => fe(x),
                    _ => continue,
                };
                if !first {
                    o.push(',');
                }
                first = false;
                write!(o, "[{},{},{}]", c, r, txt).unwrap();
            }
        }
        o.push_str("],\"instance\":[");
        let mut first = true;
        for (c, col) in self.instance.iter().enumerate() {
            for (r, v) in col.iter().enumerate() {
                let txt = match v {
                    InstanceValue::Assigned(x) => fe(x),
                    _ => continue,
                };
                if !first {
                    o.push(',');
                }
                first = false;
                write!(o, "[{},{},{}]", c, r, txt).unwrap();
            }
        }
        o.push_str("],\"regions\":[");
        for (i, r) in self.regions.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            match r.rows {
                Some((s, e)) => write!(o, "[{},{},{}]", json_str(&r.name), s, e).unwrap(),
                None => write!(o, "[{},null,null]", json_str(&r.name)).unwrap(),
            }
        }
        o.push_str("],\"notes\":[");
        NOTES.with(|ns| {
            for (i, (ri, rn, c, r, a)) in ns.borrow().iter().enumerate() {
                if i > 0 {
                    o.push(',');
                }
                write!(o, "[{},{},{},{},{}]", ri, json_str(rn), c, r, json_str(a)).unwrap();
            }
        });
        o.push_str("],\"verify\":");
        match self.verify() {
            Ok(()) => o.push_str("[]"),
            Err(errs) => {
                o.push('[');
                for (i, e) in errs.iter().take(20).enumerate() {
                    if i > 0 {
                        o.push(',');
                    }
                    let mut t = format!("{:?}", e);
                    t.truncate(400);
                    o.push_str(&json_str(&t));
                }
                o.push(']');
            }
        }
        o.push('}');
        o
    }

    /// `verify()` with one advice cell replaced by `v` (restored afterwards, also when `verify` panics);
    /// `None` when `verify` panicked.
    pub fn boole_verify_override(&mut self, column: usize, row: usize, v: F) -> Option<bool> {
        let old = self.advice[column][row];
        self.advice[column][row] = CellValue::Assigned(v);
        let this = &*self;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| this.verify().is_ok()));
        self.advice[column][row] = old;
        r.ok()
    }
}
