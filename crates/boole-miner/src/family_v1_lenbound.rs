// Family v1 deterministic length-bound instance generator.
//
// v1 is the first public-facing capability calibration family after the
// v031/v0 pipeline bring-up families. It intentionally breaks the pure
// length-preservation helper-chain pattern by requiring at least one
// length-reducing/non-expanding op (`filterByPred` or `dedup`) and renders a
// theorem of the shape:
//
//   ∀ xs : List Int, (<chain xs>).length ≤ xs.length
//
// This file only generates/render targets; it does not claim v1 is a public
// mining backbone. Rollout should stay benchmark/shadow/capped until measured.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pred {
    Even,
    Odd,
    LtK(i64),
    GtK(i64),
    EqK(i64),
    ModK { k: i64, r: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    FilterP(Pred),
    MapAdd(i64),
    MapMul(i64),
    Dedup,
    SortAsc,
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub d: u32,
    pub chain_len: u32,
    pub chain: Vec<Op>,
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn read_nat(&mut self, mod_n: u32) -> u32 {
        if mod_n == 0 || self.buf.is_empty() {
            return 0;
        }
        let byte = self.buf[self.pos % self.buf.len()];
        self.pos += 1;
        (byte as u32) % mod_n
    }

    fn choose_int(&mut self, range_n: u32) -> i64 {
        let n = self.read_nat(range_n * 2 + 1) as i64;
        n - range_n as i64
    }
}

fn gen_pred(c: &mut Cursor<'_>) -> Pred {
    match c.read_nat(6) {
        0 => Pred::Even,
        1 => Pred::Odd,
        2 => Pred::LtK(c.choose_int(9)),
        3 => Pred::GtK(c.choose_int(9)),
        4 => Pred::EqK(c.choose_int(9)),
        _ => {
            let k_raw = c.read_nat(5) as i64;
            let r = c.read_nat(6) as i64;
            Pred::ModK { k: k_raw + 2, r }
        }
    }
}

fn gen_length_reducing_op(c: &mut Cursor<'_>) -> Op {
    match c.read_nat(2) {
        0 => Op::FilterP(gen_pred(c)),
        _ => Op::Dedup,
    }
}

fn gen_context_op(c: &mut Cursor<'_>) -> Op {
    match c.read_nat(3) {
        0 => Op::MapAdd(c.choose_int(9)),
        1 => Op::MapMul(c.read_nat(5) as i64 + 1),
        _ => Op::SortAsc,
    }
}

/// Generate a `v1-lenbound` instance from a 32-byte cursor seed.
///
/// Chain constraints:
/// - total length 2..6;
/// - at least one `filterByPred` or `dedup`;
/// - at least one context op from `mapAdd | mapMul | sortAsc`.
pub fn generate_v1_lenbound(seed: &[u8]) -> Instance {
    let mut c = Cursor::new(seed);
    let d_raw = c.read_nat(6);
    let chain_len = (d_raw % 5) + 2;
    let reducer_index = c.read_nat(chain_len);
    let context_index = if chain_len == 1 {
        0
    } else {
        (reducer_index + 1 + c.read_nat(chain_len - 1)) % chain_len
    };

    let mut chain = Vec::with_capacity(chain_len as usize);
    for i in 0..chain_len {
        if i == reducer_index {
            chain.push(gen_length_reducing_op(&mut c));
        } else if i == context_index {
            chain.push(gen_context_op(&mut c));
        } else {
            match c.read_nat(5) {
                0 => chain.push(gen_length_reducing_op(&mut c)),
                1..=4 => chain.push(gen_context_op(&mut c)),
                _ => unreachable!(),
            }
        }
    }

    Instance {
        d: d_raw,
        chain_len,
        chain,
    }
}

/// Convenience: generate an instance from a hex seed (32 bytes / 64 hex chars).
pub fn generate_from_hex(seed_hex: &str) -> Result<Instance, hex::FromHexError> {
    let bytes = hex::decode(seed_hex)?;
    Ok(generate_v1_lenbound(&bytes))
}

fn render_int(k: i64) -> String {
    format!("({k} : Int)")
}

fn render_pred_to_bool(pred: &Pred) -> String {
    match pred {
        Pred::Even => "(fun x : Int => x % 2 == 0)".to_string(),
        Pred::Odd => "(fun x : Int => x % 2 != 0)".to_string(),
        Pred::LtK(k) => format!("(fun x : Int => decide (x < {}))", render_int(*k)),
        Pred::GtK(k) => format!("(fun x : Int => decide ({} < x))", render_int(*k)),
        Pred::EqK(k) => format!("(fun x : Int => x == {})", render_int(*k)),
        Pred::ModK { k, r } => format!(
            "(fun x : Int => x % {} == {})",
            render_int(*k),
            render_int(*r)
        ),
    }
}

pub fn helper_manifest() -> &'static str {
    "Available v1 length-bound helpers:\n\
     - length_filterByPred_le (p : Int → Bool) (xs : List Int)\n\
     - length_dedup_le (xs : List Int)\n\
     - length_mapAdd (k : Int) (xs : List Int)\n\
     - length_mapMul (k : Int) (xs : List Int)\n\
     - length_sortAsc (xs : List Int)"
}

fn render_op_apply(op: &Op, inner: &str) -> String {
    match op {
        Op::FilterP(p) => {
            let pred = render_pred_to_bool(p);
            format!("(filterByPred {pred} {inner})")
        }
        Op::MapAdd(k) => format!("(mapAdd {} {inner})", render_int(*k)),
        Op::MapMul(k) => format!("(mapMul {} {inner})", render_int(*k)),
        Op::Dedup => format!("(dedup {inner})"),
        Op::SortAsc => format!("(sortAsc {inner})"),
    }
}

pub fn render_chain_expr(chain: &[Op], var: &str) -> String {
    let mut expr = var.to_string();
    for op in chain {
        expr = render_op_apply(op, &expr);
    }
    expr
}

pub fn theorem_rhs(instance: &Instance) -> String {
    let result_expr = render_chain_expr(&instance.chain, "xs");
    format!("({result_expr}).length ≤ xs.length")
}

fn render_step_lemma(op: &Op, input_expr: &str) -> (String, String) {
    match op {
        Op::FilterP(p) => (
            "≤".to_string(),
            format!(
                "Boole.Family.V0Helpers.length_filterByPred_le {} {input_expr}",
                render_pred_to_bool(p)
            ),
        ),
        Op::MapAdd(k) => (
            "=".to_string(),
            format!(
                "Boole.Family.V0Helpers.length_mapAdd {} {input_expr}",
                render_int(*k)
            ),
        ),
        Op::MapMul(k) => (
            "=".to_string(),
            format!(
                "Boole.Family.V0Helpers.length_mapMul {} {input_expr}",
                render_int(*k)
            ),
        ),
        Op::Dedup => (
            "≤".to_string(),
            format!("Boole.Family.V0Helpers.length_dedup_le {input_expr}"),
        ),
        Op::SortAsc => (
            "=".to_string(),
            format!("Boole.Family.V0Helpers.length_sortAsc {input_expr}"),
        ),
    }
}

pub fn render_canonical_proof(instance: &Instance) -> String {
    let mut exprs = Vec::with_capacity(instance.chain.len() + 1);
    exprs.push("xs".to_string());
    for op in &instance.chain {
        let prev = exprs.last().expect("exprs has xs").clone();
        exprs.push(render_op_apply(op, &prev));
    }

    let mut lines = vec![
        "by".to_string(),
        "  intro xs".to_string(),
        "  calc".to_string(),
    ];
    for idx in (0..instance.chain.len()).rev() {
        let op = &instance.chain[idx];
        let input_expr = &exprs[idx];
        let output_expr = &exprs[idx + 1];
        let (rel, lemma) = render_step_lemma(op, input_expr);
        let prefix = if idx == instance.chain.len() - 1 {
            format!("    ({output_expr}).length")
        } else {
            "    _".to_string()
        };
        lines.push(format!("{prefix} {rel} ({input_expr}).length := by"));
        lines.push(format!("      exact {lemma}"));
    }
    lines.join("\n")
}

const VERIFY_NAMESPACE: &str = "BooleVerifyMod";
const VERIFY_THEOREM: &str = "instance_thm";

/// Wrap a proof term into the full Lean module the verifier elaborates.
///
/// This mirrors the v031 verifier contract but renders the v1 length-bound
/// theorem body. The supplied proof is inserted verbatim after `:=`.
pub fn lean_module(instance: &Instance, proof_term: &str) -> String {
    let rhs = theorem_rhs(instance);
    format!(
        "import Boole.Family.V0Helpers\n\
         \n\
         namespace {ns}\n\
         \n\
         open Boole.Family.V0Helpers\n\
         \n\
         theorem {thm} : ∀ (xs : List Int),\n    \
         {rhs} :=\n\
         {proof_term}\n\
         \n\
         end {ns}\n",
        ns = VERIFY_NAMESPACE,
        thm = VERIFY_THEOREM,
    )
}

/// Render a deterministic theorem-statement description suitable for the LLM prompt.
pub fn render_text(instance: &Instance) -> String {
    let rhs = theorem_rhs(instance);
    format!(
        "theorem {thm} : ∀ (xs : List Int), {rhs}",
        thm = VERIFY_THEOREM,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_reducer(chain: &[Op]) -> bool {
        chain
            .iter()
            .any(|op| matches!(op, Op::FilterP(_) | Op::Dedup))
    }

    fn has_context(chain: &[Op]) -> bool {
        chain
            .iter()
            .any(|op| matches!(op, Op::MapAdd(_) | Op::MapMul(_) | Op::SortAsc))
    }

    #[test]
    fn v1_instances_force_reducer_and_context_ops() {
        for byte in 0u8..=64 {
            let seed = [byte; 32];
            let inst = generate_v1_lenbound(&seed);
            assert!((2..=6).contains(&inst.chain_len));
            assert_eq!(inst.chain_len as usize, inst.chain.len());
            assert!(
                has_reducer(&inst.chain),
                "missing reducer: {:?}",
                inst.chain
            );
            assert!(
                has_context(&inst.chain),
                "missing context op: {:?}",
                inst.chain
            );
        }
    }

    #[test]
    fn render_text_uses_length_bound_theorem_shape() {
        let inst = generate_v1_lenbound(&[0u8; 32]);
        let text = render_text(&inst);
        assert!(text.starts_with("theorem instance_thm : ∀ (xs : List Int), "));
        assert!(text.contains(".length ≤ xs.length"));
        assert!(text.contains("filterByPred") || text.contains("dedup"));
        assert!(text.contains("mapAdd") || text.contains("mapMul") || text.contains("sortAsc"));
    }

    #[test]
    fn lean_module_has_required_shape() {
        let inst = generate_v1_lenbound(&[0u8; 32]);
        let module = lean_module(&inst, "by intro xs; simp");
        assert!(module.contains("import Boole.Family.V0Helpers"));
        assert!(module.contains("namespace BooleVerifyMod"));
        assert!(module.contains("open Boole.Family.V0Helpers"));
        assert!(module.contains("theorem instance_thm"));
        assert!(module.contains(".length ≤ xs.length"));
        assert!(module.ends_with("end BooleVerifyMod\n"));
    }

    #[test]
    fn v1_helper_manifest_exposes_length_bound_surface() {
        let manifest = helper_manifest();
        assert!(manifest.contains("length_filterByPred_le"));
        assert!(manifest.contains("length_dedup_le"));
        assert!(manifest.contains("length_mapAdd"));
        assert!(manifest.contains("length_mapMul"));
        assert!(manifest.contains("length_sortAsc"));
    }

    #[test]
    fn canonical_v1_proof_uses_length_bound_helpers() {
        let inst = Instance {
            d: 0,
            chain_len: 2,
            chain: vec![Op::FilterP(Pred::Even), Op::Dedup],
        };
        let proof = render_canonical_proof(&inst);
        assert!(proof.contains("length_filterByPred_le"));
        assert!(proof.contains("length_dedup_le"));
        assert!(!proof.contains("sorry"));
        assert!(!proof.contains("admit"));
    }
}
