// Family v031 deterministic instance generator.
//
// Direct Rust port of `scripts/boole-model-benchmark.py`'s v031-lp and
// v031-mixed generators, which themselves mirror pof's Lean reference
// implementation (`Boole.Family.GenTargetCatalogV031`) byte-for-byte.
// The cursor PRNG, op-kind selection, and chooseInt formulae here MUST
// match the Lean / Python reference exactly so the family identity is
// preserved across implementations.
//
// Two profiles:
//   - V031Lp: length-preserved-only ops (mapAdd / mapMul / sortAsc).
//             Theorem shape: `(<chain xs>).length = xs.length`.
//   - V031:   full 5-way invariant family (allSatisfy / sortedAsc /
//             dedupFirst / partitionEq / lengthPreserved). N=1.
//
// Each `Instance` carries enough state to render the Lean module text,
// the theorem RHS, and a canonical proof term. The miner uses the
// module text as the verifier input (substituting an LLM-supplied
// proof for `<YOUR_PROOF>`). The canonical proof is retained for
// self-tests.

use sha2::{Digest, Sha256};

const TRAILING_PRED_KIND_MOD: u32 = 6;
const TRAILING_INPUT_LEN_MOD: u32 = 5;
const TRAILING_NOISE_DEFS_MOD: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    V031Lp,
    V031,
}

impl Profile {
    pub fn as_str(self) -> &'static str {
        match self {
            Profile::V031Lp => "v031-lp",
            Profile::V031 => "v031",
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvariantClass {
    AllSatisfy(Pred),
    SortedAsc,
    DedupFirst,
    PartitionEq(Pred),
    LengthPreserved,
}

impl InvariantClass {
    pub fn kind(&self) -> &'static str {
        match self {
            InvariantClass::AllSatisfy(_) => "allSatisfy",
            InvariantClass::SortedAsc => "sortedAsc",
            InvariantClass::DedupFirst => "dedupFirst",
            InvariantClass::PartitionEq(_) => "partitionEq",
            InvariantClass::LengthPreserved => "lengthPreserved",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub profile: Profile,
    pub d: u32,
    pub chain_len: u32,
    pub invariant: InvariantClass,
    pub body_chain: Vec<Op>,
    pub witness_op: Option<Op>,
    pub branch_chain: Vec<Op>,
}

// ---------------------------------------------------------------------------
// Cursor PRNG — mirrors pof's `Cursor.readNat` / `Cursor.chooseInt`.

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

// ---------------------------------------------------------------------------
// Predicate / op / invariant generators (mirror Python reference).

fn gen_pred(c: &mut Cursor) -> Pred {
    let i = c.read_nat(6);
    match i {
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

/// 3-way op pick over the length-preserving subset (mapAdd / mapMul /
/// sortAsc). Used for v031-lp generation and for the body chain when
/// the v031-mixed invariant is `lengthPreserved`.
fn gen_op_length(c: &mut Cursor) -> Op {
    let i = c.read_nat(3);
    match i {
        0 => Op::MapAdd(c.choose_int(9)),
        1 => Op::MapMul(c.read_nat(5) as i64 + 1),
        _ => Op::SortAsc,
    }
}

/// 5-way op pick over the full v031 op set.
fn gen_op_full(c: &mut Cursor) -> Op {
    let i = c.read_nat(5);
    match i {
        0 => Op::FilterP(gen_pred(c)),
        1 => Op::MapAdd(c.choose_int(9)),
        2 => Op::MapMul(c.read_nat(5) as i64 + 1),
        3 => Op::Dedup,
        _ => Op::SortAsc,
    }
}

fn gen_inv_class(c: &mut Cursor) -> InvariantClass {
    let i = c.read_nat(5);
    match i {
        0 => InvariantClass::AllSatisfy(gen_pred(c)),
        1 => InvariantClass::SortedAsc,
        2 => InvariantClass::DedupFirst,
        3 => InvariantClass::PartitionEq(gen_pred(c)),
        _ => InvariantClass::LengthPreserved,
    }
}

fn witness_op_for(inv: &InvariantClass) -> Option<Op> {
    match inv {
        InvariantClass::AllSatisfy(p) => Some(Op::FilterP(p.clone())),
        InvariantClass::SortedAsc => Some(Op::SortAsc),
        InvariantClass::DedupFirst => Some(Op::Dedup),
        InvariantClass::PartitionEq(_) | InvariantClass::LengthPreserved => None,
    }
}

// ---------------------------------------------------------------------------
// Top-level generators.

fn advance_trailing(c: &mut Cursor) {
    // pof's `GenTargetCatalogV031Lp` advances the cursor through trailing
    // legacy-pred / inputLen / noiseDefs positions. We must consume the
    // same bytes so cursor state is byte-compatible with future N>=2
    // extensions.
    let _ = gen_pred(c);
    let _ = c.read_nat(TRAILING_INPUT_LEN_MOD);
    let _ = c.read_nat(TRAILING_NOISE_DEFS_MOD);
}

fn advance_trailing_lp(c: &mut Cursor) {
    let _ = c.read_nat(TRAILING_PRED_KIND_MOD);
    let _ = c.read_nat(TRAILING_INPUT_LEN_MOD);
    let _ = c.read_nat(TRAILING_NOISE_DEFS_MOD);
}

/// Generate a `v031-lp` instance from a 32-byte cursor seed.
pub fn generate_v031_lp(seed: &[u8]) -> Instance {
    let mut c = Cursor::new(seed);
    let d_raw = c.read_nat(6);
    let chain_len = (d_raw % 6) + 1;
    let mut chain = Vec::with_capacity(chain_len as usize);
    for _ in 0..chain_len {
        chain.push(gen_op_length(&mut c));
    }
    advance_trailing_lp(&mut c);
    Instance {
        profile: Profile::V031Lp,
        d: d_raw,
        chain_len,
        invariant: InvariantClass::LengthPreserved,
        body_chain: chain.clone(),
        witness_op: None,
        branch_chain: chain,
    }
}

/// Generate a `v031` (full 5-way mixed) instance from a 32-byte cursor seed.
pub fn generate_v031(seed: &[u8]) -> Instance {
    let mut c = Cursor::new(seed);
    let d_raw = c.read_nat(6);
    let chain_len = (d_raw % 6) + 1;
    let inv = gen_inv_class(&mut c);
    let (body_chain, witness, branch_chain) = match &inv {
        InvariantClass::LengthPreserved => {
            let mut body = Vec::with_capacity(chain_len as usize);
            for _ in 0..chain_len {
                body.push(gen_op_length(&mut c));
            }
            let branch = body.clone();
            (body, None, branch)
        }
        _ => {
            let witness = witness_op_for(&inv);
            let body_len = if witness.is_some() {
                chain_len.saturating_sub(1)
            } else {
                chain_len
            };
            let mut body = Vec::with_capacity(body_len as usize);
            for _ in 0..body_len {
                body.push(gen_op_full(&mut c));
            }
            let mut branch = body.clone();
            if let Some(w) = &witness {
                branch.push(w.clone());
            }
            (body, witness, branch)
        }
    };
    advance_trailing(&mut c);
    Instance {
        profile: Profile::V031,
        d: d_raw,
        chain_len,
        invariant: inv,
        body_chain,
        witness_op: witness,
        branch_chain,
    }
}

/// Convenience: generate an instance from a hex seed (32 bytes / 64 hex
/// chars). The hex seed is what the protocol's `target_seed(...)` returns.
pub fn generate_from_hex(seed_hex: &str, profile: Profile) -> Result<Instance, hex::FromHexError> {
    let bytes = hex::decode(seed_hex)?;
    Ok(match profile {
        Profile::V031Lp => generate_v031_lp(&bytes),
        Profile::V031 => generate_v031(&bytes),
    })
}

/// Derive a 32-byte cursor seed using the same scheme as
/// `scripts/boole-model-benchmark.py::attempt_context`. Used by the
/// golden-fixture regression test.
pub fn benchmark_cursor_seed(
    run_id: &str,
    target: &str,
    attempt_index: u32,
    benchmark_mode: &str,
    target_family: &str,
    domain: &str,
) -> [u8; 32] {
    let inner = format!("{run_id}|{target}|{attempt_index}|{benchmark_mode}|{target_family}");
    let mut h = Sha256::new();
    h.update(inner.as_bytes());
    h.update(b"|");
    h.update(domain.as_bytes());
    h.finalize().into()
}

// ---------------------------------------------------------------------------
// Lean source rendering — must match Python `_v031mixed_render_*` byte-for-byte.

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

fn render_pred_to_bool_not(pred: &Pred) -> String {
    let base = render_pred_to_bool(pred);
    format!("(fun y : Int => !({base} y))")
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

/// Fold a chain of ops onto `var` via successive `Op.applyTo`.
pub fn render_chain_expr(chain: &[Op], var: &str) -> String {
    let mut expr = var.to_string();
    for op in chain {
        expr = render_op_apply(op, &expr);
    }
    expr
}

/// Render the theorem RHS for a given invariant + result expression
/// (full chain applied to xs).
pub fn render_theorem_rhs(inv: &InvariantClass, result_expr: &str) -> String {
    match inv {
        InvariantClass::AllSatisfy(p) => {
            let pbool = render_pred_to_bool(p);
            format!("({result_expr}).all {pbool} = true")
        }
        InvariantClass::SortedAsc => format!("List.Pairwise (· ≤ ·) {result_expr}"),
        InvariantClass::DedupFirst => format!("List.Nodup {result_expr}"),
        InvariantClass::PartitionEq(p) => {
            let pbool = render_pred_to_bool(p);
            let pnot = render_pred_to_bool_not(p);
            format!(
                "({result_expr}).partition {pbool} = \
                 (({result_expr}).filter {pbool}, ({result_expr}).filter {pnot})"
            )
        }
        InvariantClass::LengthPreserved => format!("({result_expr}).length = xs.length"),
    }
}

// ---------------------------------------------------------------------------
// Canonical proof rendering — matches `_v031{lp,mixed}_render_canonical_proof`.

fn length_lemma_call(op: &Op, inner: &str) -> String {
    match op {
        Op::MapAdd(k) => format!("length_mapAdd {} {inner}", render_int(*k)),
        Op::MapMul(k) => format!("length_mapMul {} {inner}", render_int(*k)),
        Op::SortAsc => format!("length_sortAsc {inner}"),
        // FilterP / Dedup are not length-preserving; this branch is unreachable
        // given the v031-lp + lengthPreserved-body invariants.
        Op::FilterP(_) | Op::Dedup => panic!("length_lemma_call: non-length-preserving op"),
    }
}

fn render_canonical_proof_length(chain: &[Op]) -> String {
    if chain.is_empty() {
        return "fun xs => rfl".to_string();
    }
    let innermost = length_lemma_call(&chain[0], "xs");
    if chain.len() == 1 {
        return format!("fun xs => {innermost}");
    }
    let mut body = innermost;
    for op in &chain[1..] {
        let outer = length_lemma_call(op, "_");
        body = format!("({outer}).trans ({body})");
    }
    format!("fun xs => {body}")
}

/// Render the canonical proof term for an instance. For witness-bearing
/// invariants the proof is a single witness-lemma application against
/// `body_expr` (the chain *before* the witness op is appended). For
/// `lengthPreserved` it composes `length_*` lemmas via `Eq.trans`.
pub fn render_canonical_proof(instance: &Instance) -> String {
    let body_expr = render_chain_expr(&instance.body_chain, "xs");
    match &instance.invariant {
        InvariantClass::AllSatisfy(p) => {
            let pbool = render_pred_to_bool(p);
            format!("fun xs => all_filterByPred_self {pbool} {body_expr}")
        }
        InvariantClass::SortedAsc => format!("fun xs => pairwise_sortAsc {body_expr}"),
        InvariantClass::DedupFirst => format!("fun xs => nodup_dedup {body_expr}"),
        InvariantClass::PartitionEq(p) => {
            let pbool = render_pred_to_bool(p);
            format!("fun xs => partition_eq_filter_filter {pbool} {body_expr}")
        }
        InvariantClass::LengthPreserved => render_canonical_proof_length(&instance.body_chain),
    }
}

// ---------------------------------------------------------------------------
// Module wrap — produces a Lean source file in the shape boole-lean-runner
// (== `lake exec boole_check`) expects: `import Boole.Family.V0Helpers`,
// namespaced `BooleVerifyMod`, with a single `instance_thm` theorem.

const VERIFY_NAMESPACE: &str = "BooleVerifyMod";
const VERIFY_THEOREM: &str = "instance_thm";

/// Compute the body of the theorem RHS for an instance.
pub fn theorem_rhs(instance: &Instance) -> String {
    let result_expr = render_chain_expr(&instance.branch_chain, "xs");
    render_theorem_rhs(&instance.invariant, &result_expr)
}

/// Wrap a proof term into the full Lean module the verifier elaborates.
/// The proof term is inserted verbatim after `:=`. It can be either an
/// LLM-supplied candidate or `render_canonical_proof(instance)` for
/// self-tests.
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

/// Render a deterministic theorem-statement description suitable for
/// inclusion in the LLM prompt. We use the Lean source theorem statement
/// directly — unambiguous, byte-stable, and the LLM is expected to
/// produce a Lean proof anyway.
pub fn render_text(instance: &Instance) -> String {
    let rhs = theorem_rhs(instance);
    format!(
        "theorem {thm} : ∀ (xs : List Int), {rhs}",
        thm = VERIFY_THEOREM,
    )
}

// ---------------------------------------------------------------------------
// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::path::Path;

    const FIXTURE_PATH: &str = "../../fixtures/benchmarks/v031-mixed/golden-instances.json";

    fn parse_op(v: &Value) -> Op {
        let kind = v.get("op").and_then(Value::as_str).expect("op kind");
        match kind {
            "filterP" => Op::FilterP(parse_pred(&v["pred"])),
            "mapAdd" => Op::MapAdd(v["k"].as_i64().expect("mapAdd k")),
            "mapMul" => Op::MapMul(v["k"].as_i64().expect("mapMul k")),
            "dedup" => Op::Dedup,
            "sortAsc" => Op::SortAsc,
            other => panic!("unexpected op kind: {other}"),
        }
    }

    fn parse_pred(v: &Value) -> Pred {
        let kind = v.get("kind").and_then(Value::as_str).expect("pred kind");
        match kind {
            "even" => Pred::Even,
            "odd" => Pred::Odd,
            "ltK" => Pred::LtK(v["k"].as_i64().expect("ltK k")),
            "gtK" => Pred::GtK(v["k"].as_i64().expect("gtK k")),
            "eqK" => Pred::EqK(v["k"].as_i64().expect("eqK k")),
            "modK" => Pred::ModK {
                k: v["k"].as_i64().expect("modK k"),
                r: v["r"].as_i64().expect("modK r"),
            },
            other => panic!("unexpected pred kind: {other}"),
        }
    }

    fn parse_chain(v: &Value) -> Vec<Op> {
        v.as_array()
            .expect("chain array")
            .iter()
            .map(parse_op)
            .collect()
    }

    #[test]
    fn cursor_read_nat_matches_python_semantics() {
        // Empty buf returns 0 without advancing.
        let mut c = Cursor::new(&[]);
        assert_eq!(c.read_nat(5), 0);
        assert_eq!(c.pos, 0);

        // mod_n=0 returns 0 without advancing.
        let buf = [42u8];
        let mut c = Cursor::new(&buf);
        assert_eq!(c.read_nat(0), 0);
        assert_eq!(c.pos, 0);

        // wrap-around: pos % len(buf).
        let buf = [10u8, 20, 30];
        let mut c = Cursor::new(&buf);
        assert_eq!(c.read_nat(7), 10 % 7);
        assert_eq!(c.read_nat(7), 20 % 7);
        assert_eq!(c.read_nat(7), 30 % 7);
        assert_eq!(c.read_nat(7), 10 % 7); // wraps back to pos=0
    }

    #[test]
    fn cursor_choose_int_centered_at_zero() {
        let buf = [0u8];
        let mut c = Cursor::new(&buf);
        // read_nat(2*9+1=19) on byte 0 -> 0; choose_int = 0 - 9 = -9.
        assert_eq!(c.choose_int(9), -9);

        let buf = [9u8];
        let mut c = Cursor::new(&buf);
        // read_nat(19) on byte 9 -> 9; choose_int = 9 - 9 = 0.
        assert_eq!(c.choose_int(9), 0);
    }

    /// Golden regression: for each fixture instance, regenerate from the
    /// same recipe and verify branchChain / bodyChain / witnessOp / D /
    /// chainLen / theoremRhs / canonicalProof match byte-for-byte.
    #[test]
    fn regenerates_v031_mixed_golden_fixture() {
        let path = Path::new(FIXTURE_PATH);
        let raw = std::fs::read_to_string(path).expect("read fixture");
        let json: Value = serde_json::from_str(&raw).expect("parse fixture");
        let target_family = json
            .get("fixtureFamily")
            .and_then(Value::as_str)
            .expect("fixtureFamily");
        let instances = json["instances"].as_array().expect("instances");
        assert!(!instances.is_empty(), "fixture has no instances");

        for entry in instances {
            let run_id = entry["runId"].as_str().unwrap();
            let target = entry["target"].as_str().unwrap();
            let attempt_index = entry["attemptIndex"].as_u64().unwrap() as u32;
            let benchmark_mode = entry["benchmarkMode"].as_str().unwrap();
            let cursor_seed = benchmark_cursor_seed(
                run_id,
                target,
                attempt_index,
                benchmark_mode,
                target_family,
                "v031-mixed-cursor",
            );

            let instance = generate_v031(&cursor_seed);

            // chainLen + D
            assert_eq!(
                instance.chain_len as u64,
                entry["chainLen"].as_u64().unwrap(),
                "chainLen mismatch at attempt {attempt_index}"
            );
            assert_eq!(
                instance.d as u64,
                entry["D"].as_u64().unwrap(),
                "D mismatch at attempt {attempt_index}"
            );

            // invariant kind
            let expected_kind = entry["invariantClass"].as_str().unwrap();
            assert_eq!(
                instance.invariant.kind(),
                expected_kind,
                "invariantClass mismatch at attempt {attempt_index}"
            );

            // branch / body / witness chains
            let expected_branch = parse_chain(&entry["branchChain"]);
            assert_eq!(
                instance.branch_chain, expected_branch,
                "branchChain mismatch at attempt {attempt_index}"
            );
            let expected_body = parse_chain(&entry["bodyChain"]);
            assert_eq!(
                instance.body_chain, expected_body,
                "bodyChain mismatch at attempt {attempt_index}"
            );
            let expected_witness = match entry["witnessOp"].clone() {
                Value::Null => None,
                v => Some(parse_op(&v)),
            };
            assert_eq!(
                instance.witness_op, expected_witness,
                "witnessOp mismatch at attempt {attempt_index}"
            );

            // theoremRhs + canonicalProof byte-for-byte
            assert_eq!(
                theorem_rhs(&instance),
                entry["theoremRhs"].as_str().unwrap(),
                "theoremRhs mismatch at attempt {attempt_index}"
            );
            assert_eq!(
                render_canonical_proof(&instance),
                entry["canonicalProof"].as_str().unwrap(),
                "canonicalProof mismatch at attempt {attempt_index}"
            );
        }
    }

    #[test]
    fn lean_module_has_required_shape() {
        // Pick a simple length-preserved instance to ensure module wrap is sane.
        let seed = [0u8; 32];
        let inst = generate_v031_lp(&seed);
        let proof = render_canonical_proof(&inst);
        let module = lean_module(&inst, &proof);
        assert!(module.contains("import Boole.Family.V0Helpers"));
        assert!(module.contains("namespace BooleVerifyMod"));
        assert!(module.contains("open Boole.Family.V0Helpers"));
        assert!(module.contains("theorem instance_thm"));
        assert!(module.ends_with("end BooleVerifyMod\n"));
    }

    #[test]
    fn render_text_round_trips_theorem_statement() {
        let seed = [3u8; 32];
        let inst = generate_v031(&seed);
        let txt = render_text(&inst);
        assert!(txt.starts_with("theorem instance_thm : ∀ (xs : List Int), "));
        assert!(txt.contains(&theorem_rhs(&inst)));
    }
}
