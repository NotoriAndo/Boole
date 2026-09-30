//! Boole AIR extractor for SP1 (scratch copy of the SP1 workspace at the census pin; not upstream code).
//!
//! Usage: `boole-air-extract <out-dir>` writes `manifest.jsonl`, `airs/<index>.json` (boole-air-ir/v1) and
//! `rows/<index>.json` (boole-air-rows/v1).
//!
//! * AIRs: every chip of `RiscvAir::machine()` (supervisor and user variants, precompiles), then the recursion
//!   AIRs of `RecursionAir::<_, 3, 2>::machine_wide_with_all_chips()` and `RecursionAir::<_, 3, 1>::wrap_machine()`.
//! * Constraints and interactions come from one `eval` of the chip on [`Extract`], a symbolic builder that records
//!   every `assert_*` polynomial and every `send` / `receive`.  It is needed because p3-uni-stark
//!   0.4.3-succinct's `SymbolicAirBuilder` labels preprocessed cells as main cells; the constraint count is
//!   checked against `Chip::num_constraints` and the interaction lists against `Chip::sends` / `receives`.
//! * Rows: the real executor and trace generation (`generate_records`, `MachineAir::generate_trace`) on
//!   straight-line programs (ALU, memory, control flow, every precompile the machine has a chip for) and linear
//!   recursion programs.
mod boole_air_ir;

use boole_air_ir::*;
use num::{BigUint, One};
use slop_air::{Air, AirBuilder, AirBuilderWithPublicValues, PairBuilder};
use slop_algebra::{AbstractField, Field, PrimeField32};
use slop_matrix::{dense::RowMajorMatrix, Matrix};
use slop_uni_stark::{Entry, SymbolicExpression as SE, SymbolicVariable as SV};
use sp1_core_executor::{ExecutionRecord, Instruction, Opcode, Program, SP1CoreOpts};
use sp1_core_machine::{io::SP1Stdin, riscv::RiscvAir, utils::generate_records};
use sp1_curves::{
    edwards::ed25519::Ed25519,
    weierstrass::{bls12_381::Bls12381, bn254::Bn254, secp256k1::Secp256k1, secp256r1::Secp256r1},
    AffinePoint, EllipticCurve,
};
use sp1_hypercube::{
    air::{AirInteraction, InteractionScope, MachineAir, MessageBuilder},
    Chip, MachineRecord, PROOF_MAX_NUM_PVS,
};
use sp1_primitives::SP1Field;
use sp1_recursion_machine::RecursionAir;
use std::{
    collections::HashMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::Arc,
};

type F = SP1Field;
const P: u64 = 2130706433;

// ------------------------------------------------------------------------------------------ builder

pub struct Extract<Fl: Field> {
    prep: RowMajorMatrix<SV<Fl>>,
    main: RowMajorMatrix<SV<Fl>>,
    pvs: Vec<SV<Fl>>,
    pub constraints: Vec<SE<Fl>>,
    pub sends: Vec<(AirInteraction<SE<Fl>>, InteractionScope)>,
    pub receives: Vec<(AirInteraction<SE<Fl>>, InteractionScope)>,
}

impl<Fl: Field> Extract<Fl> {
    pub fn new(pw: usize, w: usize, npv: usize) -> Self {
        let pw1 = pw.max(1);
        Self {
            prep: RowMajorMatrix::new((0..pw1).map(|i| SV::new(Entry::Preprocessed { offset: 0 }, i)).collect(), pw1),
            // one row: SP1 hypercube AIRs never reference the next row (row_slice(1) would panic)
            main: RowMajorMatrix::new((0..w).map(|i| SV::new(Entry::Main { offset: 0 }, i)).collect(), w.max(1)),
            pvs: (0..npv).map(|i| SV::new(Entry::Public, i)).collect(),
            constraints: vec![],
            sends: vec![],
            receives: vec![],
        }
    }
}

impl<Fl: Field> AirBuilder for Extract<Fl> {
    type F = Fl;
    type Expr = SE<Fl>;
    type Var = SV<Fl>;
    type M = RowMajorMatrix<SV<Fl>>;

    fn main(&self) -> Self::M {
        self.main.clone()
    }
    fn is_first_row(&self) -> Self::Expr {
        panic!("is_first_row is not supported by the hypercube machine")
    }
    fn is_last_row(&self) -> Self::Expr {
        panic!("is_last_row is not supported by the hypercube machine")
    }
    fn is_transition_window(&self, _: usize) -> Self::Expr {
        panic!("is_transition is not supported by the hypercube machine")
    }
    fn assert_zero<I: Into<Self::Expr>>(&mut self, x: I) {
        self.constraints.push(x.into());
    }
}

impl<Fl: Field> PairBuilder for Extract<Fl> {
    fn preprocessed(&self) -> Self::M {
        self.prep.clone()
    }
}

impl<Fl: Field> AirBuilderWithPublicValues for Extract<Fl> {
    type PublicVar = SV<Fl>;
    fn public_values(&self) -> &[SV<Fl>] {
        &self.pvs
    }
}

impl<Fl: Field> MessageBuilder<AirInteraction<SE<Fl>>> for Extract<Fl> {
    fn send(&mut self, m: AirInteraction<SE<Fl>>, s: InteractionScope) {
        self.sends.push((m, s));
    }
    fn receive(&mut self, m: AirInteraction<SE<Fl>>, s: InteractionScope) {
        self.receives.push((m, s));
    }
}

impl<Fl: Field> sp1_core_machine::air::TrivialOperationBuilder for Extract<Fl> {}

// ------------------------------------------------------------------------------------------ conversion

fn conv(e: &SE<F>, dag: &mut Dag, memo: &mut HashMap<usize, usize>) -> usize {
    let key = e as *const SE<F> as usize;
    if let Some(&i) = memo.get(&key) {
        return i;
    }
    let n = match e {
        SE::Variable(v) => match v.entry {
            Entry::Main { offset } => Node::Main(offset as u8, v.index),
            Entry::Preprocessed { offset } => Node::Prep(offset as u8, v.index),
            Entry::Public => Node::Pub(v.index),
            ref other => panic!("unsupported entry {other:?}"),
        },
        SE::IsFirstRow => Node::First,
        SE::IsLastRow => Node::Last,
        SE::IsTransition => Node::Trans,
        SE::Constant(c) => Node::Const(c.as_canonical_u32() as u64),
        SE::Add { x, y, .. } => {
            let a = conv(x, dag, memo);
            let b = conv(y, dag, memo);
            Node::Add(a, b)
        }
        SE::Sub { x, y, .. } => {
            let a = conv(x, dag, memo);
            let b = conv(y, dag, memo);
            Node::Sub(a, b)
        }
        SE::Mul { x, y, .. } => {
            let a = conv(x, dag, memo);
            let b = conv(y, dag, memo);
            Node::Mul(a, b)
        }
        SE::Neg { x, .. } => {
            let a = conv(x, dag, memo);
            Node::Neg(a)
        }
    };
    let id = dag.intern(n);
    memo.insert(key, id);
    id
}

fn scope_name(s: InteractionScope) -> String {
    match s {
        InteractionScope::Global => "global".into(),
        InteractionScope::Local => "local".into(),
    }
}

fn doc_of(
    b: &Extract<F>,
    name: &str,
    rust_type: &str,
    group: &str,
    index: usize,
    width: usize,
    prep_width: usize,
    meta: Vec<(String, String)>,
) -> AirDoc {
    let mut dag = Dag::default();
    let mut memo = HashMap::new();
    let constraints: Vec<usize> = b.constraints.iter().map(|c| conv(c, &mut dag, &mut memo)).collect();
    let mut interactions = vec![];
    for (dir, list) in [("send", &b.sends), ("receive", &b.receives)] {
        for (m, s) in list.iter() {
            let values = m.values.iter().map(|v| conv(v, &mut dag, &mut memo)).collect();
            let mult = conv(&m.multiplicity, &mut dag, &mut memo);
            interactions.push(Interaction {
                dir,
                kind: Some(m.kind as u32),
                kind_name: format!("{:?}", m.kind),
                bus: None,
                scope: Some(scope_name(*s)),
                values,
                mult,
                count_weight: None,
            });
        }
    }
    AirDoc {
        zkvm: "sp1".into(),
        release: "v6.8.1".into(),
        commit: env_or("BOOLE_COMMIT", "c84ada1ed5911f28c4d3c9d0ed2f9e6cd7edb824"),
        field_name: "KoalaBear".into(),
        p: P,
        name: name.into(),
        rust_type: rust_type.into(),
        group: group.into(),
        index,
        width,
        prep_width,
        num_public_values: PROOF_MAX_NUM_PVS,
        dag,
        constraints,
        interactions,
        meta,
    }
}

fn env_or(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.into())
}

/// Evaluate one chip on [`Extract`]; cross-check the counts against the chip's own records.
fn extract_chip<A>(chip: &Chip<F, A>, group: &str, index: usize, name: &str) -> Result<AirDoc, String>
where
    A: MachineAir<F> + Air<Extract<F>>,
{
    let air = chip.air.as_ref();
    let (w, pw) = (air.width(), air.preprocessed_width());
    let mut b = Extract::<F>::new(pw, w, PROOF_MAX_NUM_PVS);
    air.eval(&mut b);
    let mut meta = vec![
        ("chip_num_constraints".to_string(), chip.num_constraints.to_string()),
        ("chip_sends".to_string(), chip.sends().len().to_string()),
        ("chip_receives".to_string(), chip.receives().len().to_string()),
    ];
    if b.constraints.len() != chip.num_constraints {
        return Err(format!(
            "constraint count {} differs from Chip::num_constraints {}",
            b.constraints.len(),
            chip.num_constraints
        ));
    }
    if b.sends.len() != chip.sends().len() || b.receives.len() != chip.receives().len() {
        return Err("interaction counts differ from Chip::sends / receives".into());
    }
    // the interaction kinds agree with the chip's own InteractionBuilder records
    for (mine, theirs) in b.sends.iter().zip(chip.sends()).chain(b.receives.iter().zip(chip.receives())) {
        if mine.0.kind != theirs.kind || mine.0.values.len() != theirs.values.len() {
            return Err("interaction kinds / lengths differ from the chip's InteractionBuilder".into());
        }
    }
    meta.push(("rust_type_source".into(), "RiscvAir/RecursionAir variant name".into()));
    Ok(doc_of(&b, name, &rust_type_of(name), group, index, w, pw, meta))
}

fn rust_type_of(name: &str) -> String {
    name.to_string()
}

// ------------------------------------------------------------------------------------------ programs

const DATA: u64 = 0x0010_0000;

struct Prog {
    ins: Vec<Instruction>,
    mem: HashMap<u64, u64>,
}

impl Prog {
    fn new() -> Self {
        Self { ins: vec![], mem: HashMap::new() }
    }
    fn li(&mut self, rd: u8, v: u64) {
        // ADDI with an immediate below 2^31 (as the in-tree tests do)
        assert!(v < (1 << 31));
        self.ins.push(Instruction::new(Opcode::ADDI, rd, 0, v, false, true));
    }
    /// Load a 64-bit constant through memory: x31 = address, rd = LD.
    fn ld_const(&mut self, rd: u8, v: u64, slot: u64) {
        let addr = DATA + 0x8_0000 + 8 * slot;
        self.mem.insert(addr, v);
        self.li(31, addr);
        self.ins.push(Instruction::new(Opcode::LD, rd, 31, 0, false, true));
    }
    fn words(&mut self, addr: u64, ws: &[u64]) {
        for (i, w) in ws.iter().enumerate() {
            self.mem.insert(addr + 8 * i as u64, *w);
        }
    }
    fn ecall(&mut self, code: u32, a0: u64, a1: u64) {
        self.li(5, code as u64);
        self.li(10, a0);
        self.li(11, a1);
        self.ins.push(Instruction::new(Opcode::ECALL, 5, 10, 11, false, false));
    }
    fn finish(mut self) -> Program {
        sp1_core_executor::add_halt(&mut self.ins);
        let mut p = Program::new(self.ins, 0x1000, 0x1000);
        p.memory_image = Arc::new(self.mem.into_iter().collect());
        p
    }
}

fn edge_values() -> Vec<u64> {
    vec![
        0,
        1,
        2,
        u64::MAX,
        i64::MIN as u64,
        i64::MAX as u64,
        0xFFFF_FFFF,
        0x8000_0000,
        0x7FFF_FFFF,
        0xFFFF_FFFF_0000_0000,
        0x0000_0001_0000_0000,
        0x1234_5678_9abc_def0,
    ]
}

/// ALU (register and immediate forms, rd = x0), loads and stores of every width, branches taken and not
/// taken, JAL / JALR, LUI / AUIPC.
fn program_core(rng: &mut Rng) -> Program {
    let mut p = Prog::new();
    let edge = edge_values();
    let mut vals = edge.clone();
    while vals.len() < 40 {
        vals.push(rng.next());
    }
    let rr = [
        Opcode::ADD,
        Opcode::SUB,
        Opcode::XOR,
        Opcode::OR,
        Opcode::AND,
        Opcode::SLL,
        Opcode::SRL,
        Opcode::SRA,
        Opcode::SLT,
        Opcode::SLTU,
        Opcode::MUL,
        Opcode::MULH,
        Opcode::MULHU,
        Opcode::MULHSU,
        Opcode::DIV,
        Opcode::DIVU,
        Opcode::REM,
        Opcode::REMU,
        Opcode::ADDW,
        Opcode::SUBW,
        Opcode::SLLW,
        Opcode::SRLW,
        Opcode::SRAW,
        Opcode::MULW,
        Opcode::DIVW,
        Opcode::DIVUW,
        Opcode::REMW,
        Opcode::REMUW,
    ];
    let mut slot = 0u64;
    for (k, op) in rr.iter().enumerate() {
        for j in 0..14 {
            let b = vals[(j + k) % vals.len()];
            let c = vals[(3 * j + 2 * k + 1) % vals.len()];
            p.ld_const(10, b, slot);
            p.ld_const(11, c, slot + 1);
            slot += 2;
            let rd = if j == 13 { 0 } else { 12 }; // rd = x0 once per opcode (AluX0)
            p.ins.push(Instruction::new(*op, rd, 10, 11, false, false));
        }
    }
    // immediate forms
    let ri = [Opcode::ADDI, Opcode::XOR, Opcode::OR, Opcode::AND, Opcode::SLL, Opcode::SRL, Opcode::SRA, Opcode::SLT,
        Opcode::SLTU, Opcode::ADDW, Opcode::SLLW, Opcode::SRLW, Opcode::SRAW];
    for (k, op) in ri.iter().enumerate() {
        for j in 0..12 {
            let b = vals[(j * 5 + k) % vals.len()];
            p.ld_const(10, b, slot);
            slot += 1;
            let imm = if matches!(op, Opcode::SLL | Opcode::SRL | Opcode::SRA) {
                (j as u64 * 7) % 64
            } else if matches!(op, Opcode::SLLW | Opcode::SRLW | Opcode::SRAW) {
                (j as u64 * 5) % 32
            } else {
                [0u64, 1, 2047, 0xFFFF_FFFF_FFFF_F800, 5, 1000, 0xFFFF_FFFF_FFFF_FFFF, 3, 42, 100, 7, 2046][j]
            };
            let rd = if j == 11 { 0 } else { 13 };
            p.ins.push(Instruction::new(*op, rd, 10, imm, false, true));
        }
    }
    // loads and stores
    let base = DATA + 0x1_0000;
    let mut words = vec![];
    for _ in 0..16 {
        words.push(rng.next());
    }
    p.words(base, &words);
    p.li(20, base);
    let loads = [Opcode::LB, Opcode::LH, Opcode::LW, Opcode::LD, Opcode::LBU, Opcode::LHU, Opcode::LWU];
    for (k, op) in loads.iter().enumerate() {
        let align = match op {
            Opcode::LB | Opcode::LBU => 1,
            Opcode::LH | Opcode::LHU => 2,
            Opcode::LW | Opcode::LWU => 4,
            _ => 8,
        };
        for j in 0..12u64 {
            let off = ((j * 13 + k as u64 * 3) % 64) / align * align;
            let rd = if j == 11 { 0 } else { 14 };
            p.ins.push(Instruction::new(*op, rd, 20, off, false, true));
        }
    }
    let stores = [Opcode::SB, Opcode::SH, Opcode::SW, Opcode::SD];
    for (k, op) in stores.iter().enumerate() {
        let align = match op {
            Opcode::SB => 1,
            Opcode::SH => 2,
            Opcode::SW => 4,
            _ => 8,
        };
        for j in 0..12u64 {
            p.ld_const(15, vals[(j as usize * 3 + k) % vals.len()], slot);
            slot += 1;
            let off = ((j * 11 + k as u64 * 5) % 64) / align * align;
            p.ins.push(Instruction::new(*op, 15, 20, off, false, true));
        }
    }
    // branches: offset 8 skips one filler instruction when taken
    let br = [Opcode::BEQ, Opcode::BNE, Opcode::BLT, Opcode::BGE, Opcode::BLTU, Opcode::BGEU];
    for (k, op) in br.iter().enumerate() {
        for j in 0..12 {
            let b = vals[(j + 2 * k) % vals.len()];
            let c = if j % 3 == 0 { b } else { vals[(j * 7 + k + 1) % vals.len()] };
            p.ld_const(10, b, slot);
            p.ld_const(11, c, slot + 1);
            slot += 2;
            p.ins.push(Instruction::new(*op, 10, 11, 8, false, true));
            p.ins.push(Instruction::new(Opcode::ADDI, 16, 16, 1, false, true));
        }
    }
    // JAL / JALR / LUI / AUIPC
    for j in 0..12u64 {
        let rd = if j == 11 { 0 } else { 1 };
        p.ins.push(Instruction::new(Opcode::JAL, rd, 8, 0, true, true));
        p.ins.push(Instruction::new(Opcode::ADDI, 16, 16, 1, false, true));
    }
    for j in 0..12u64 {
        // x21 = pc of the JALR + 8 (skips the filler)
        let jalr_pc = 0x1000 + 4 * (p.ins.len() as u64 + 1);
        p.li(21, jalr_pc + 8);
        let rd = if j == 11 { 0 } else { 1 };
        p.ins.push(Instruction::new(Opcode::JALR, rd, 21, 0, false, true));
        p.ins.push(Instruction::new(Opcode::ADDI, 16, 16, 1, false, true));
    }
    for j in 0..12u64 {
        let rd = if j == 11 { 0 } else { 17 };
        let (u1, u2) = ((j * 0x0123_4000) & 0x7FFF_F000, (j * 0x0098_7000) & 0x7FFF_F000);
        p.ins.push(Instruction::new(Opcode::LUI, rd, u1, u1, true, true));
        p.ins.push(Instruction::new(Opcode::AUIPC, rd, u2, u2, true, true));
    }
    p.finish()
}

fn words_of(x: &BigUint, n: usize) -> Vec<u64> {
    let mut d = x.to_u64_digits();
    d.resize(n, 0);
    d
}

fn to_u32s(ws: &[u64]) -> Vec<u32> {
    ws.iter().flat_map(|w| [*w as u32, (*w >> 32) as u32]).collect()
}

fn points<E: EllipticCurve>(n: usize) -> Vec<AffinePoint<E>> {
    let g = E::ec_generator();
    let mut out = vec![g.clone()];
    let mut cur = E::ec_double(&g);
    while out.len() < n {
        out.push(cur.clone());
        cur = E::ec_add(&cur, &g);
    }
    out
}

/// Ten calls of every precompile that has a chip.
fn program_precompiles(rng: &mut Rng, which: &str) -> Program {
    let mut p = Prog::new();
    let mut slot = DATA;
    let mut alloc = |n: u64| {
        let a = slot;
        slot += (8 * n + 0xFF) / 0x100 * 0x100;
        a
    };
    const N: usize = 10;
    match which {
        "sha" => {
            for _ in 0..N {
                let w = alloc(64);
                let ws: Vec<u64> = (0..64).map(|i| if i < 16 { rng.next() & 0xFFFF_FFFF } else { 0 }).collect();
                p.words(w, &ws);
                p.ecall(0x00_30_01_05, w, 0);
                let h = alloc(8);
                let hs: Vec<u64> = (0..8).map(|_| rng.next() & 0xFFFF_FFFF).collect();
                p.words(h, &hs);
                p.ecall(0x00_01_01_06, w, h);
            }
        }
        "keccak" => {
            for _ in 0..N {
                let s = alloc(25);
                let st: Vec<u64> = (0..25).map(|_| rng.next()).collect();
                p.words(s, &st);
                p.ecall(0x00_01_01_09, s, 0);
            }
        }
        "ec" => {
            fn sw<E: EllipticCurve>(p: &mut Prog, alloc: &mut dyn FnMut(u64) -> u64, add: u32, dbl: u32) {
                let pts = points::<E>(N + 1);
                for k in 0..N {
                    let a = pts[k + 1].to_words_le();
                    let b = pts[0].to_words_le();
                    let pa = alloc(a.len() as u64);
                    let pb = alloc(b.len() as u64);
                    p.words(pa, &a);
                    p.words(pb, &b);
                    p.ecall(add, pa, pb);
                    let d = pts[k].to_words_le();
                    let pd = alloc(d.len() as u64);
                    p.words(pd, &d);
                    p.ecall(dbl, pd, 0);
                }
            }
            sw::<Secp256k1>(&mut p, &mut alloc, 0x00_01_01_0A, 0x00_00_01_0B);
            sw::<Secp256r1>(&mut p, &mut alloc, 0x00_01_01_2C, 0x00_00_01_2D);
            sw::<Bn254>(&mut p, &mut alloc, 0x00_01_01_0E, 0x00_00_01_0F);
            sw::<Bls12381>(&mut p, &mut alloc, 0x00_01_01_1E, 0x00_00_01_1F);
        }
        "ed" => {
            let pts = points::<Ed25519>(N + 1);
            for k in 0..N {
                let a = pts[k + 1].to_words_le();
                let b = pts[0].to_words_le();
                let pa = alloc(8);
                let pb = alloc(8);
                p.words(pa, &a);
                p.words(pb, &b);
                p.ecall(0x00_01_01_07, pa, pb);
                // decompress: y at slice + 32, sign bit = x mod 2
                let s = alloc(8);
                let y = pts[k].y.clone();
                p.words(s + 32, &words_of(&y, 4));
                let sign = if (&pts[k].x % BigUint::from(2u32)) == BigUint::one() { 1 } else { 0 };
                p.ecall(0x00_00_01_08, s, sign);
            }
        }
        "uint256" => {
            for k in 0..N {
                // operands reduced modulo the modulus (0 encodes 2^256), as guest code calls the precompile
                let m: Vec<u64> = if k % 3 == 0 { vec![0; 4] } else { (0..4).map(|i| rng.next() | (i == 0) as u64).collect() };
                let mm = if k % 3 == 0 { BigUint::one() << 256 } else { BigUint::from_slice(&to_u32s(&m)) };
                let xr = BigUint::from_slice(&to_u32s(&(0..4).map(|_| rng.next()).collect::<Vec<_>>())) % &mm;
                let yr = BigUint::from_slice(&to_u32s(&(0..4).map(|_| rng.next()).collect::<Vec<_>>())) % &mm;
                let x = words_of(&xr, 4);
                let mut y = words_of(&yr, 4);
                y.extend(m);
                let px = alloc(4);
                let py = alloc(8);
                p.words(px, &x);
                p.words(py, &y);
                p.ecall(0x00_01_01_1D, px, py);
                for code in [0x00_01_01_30u32, 0x00_01_01_31] {
                    let (a, b, c) = (alloc(4), alloc(4), alloc(4));
                    let (d, e) = (alloc(4), alloc(4));
                    for ptr in [a, b, c] {
                        let v: Vec<u64> = (0..4).map(|_| rng.next()).collect();
                        p.words(ptr, &v);
                    }
                    p.li(12, c);
                    p.li(13, d);
                    p.li(14, e);
                    p.ecall(code, a, b);
                }
            }
        }
        "fp" => {
            fn fp<E: EllipticCurve>(p: &mut Prog, alloc: &mut dyn FnMut(u64) -> u64, nw: usize, fp_codes: [u32; 3], fp2_codes: [u32; 3]) {
                let pts = points::<E>(N + 2);
                for k in 0..N {
                    for code in fp_codes {
                        let (x, y) = (alloc(nw as u64), alloc(nw as u64));
                        p.words(x, &words_of(&pts[k].x, nw));
                        p.words(y, &words_of(&pts[k + 1].y, nw));
                        p.ecall(code, x, y);
                    }
                    for code in fp2_codes {
                        let (x, y) = (alloc(2 * nw as u64), alloc(2 * nw as u64));
                        let mut a = words_of(&pts[k].x, nw);
                        a.extend(words_of(&pts[k].y, nw));
                        let mut b = words_of(&pts[k + 1].x, nw);
                        b.extend(words_of(&pts[k + 2].y, nw));
                        p.words(x, &a);
                        p.words(y, &b);
                        p.ecall(code, x, y);
                    }
                }
            }
            fp::<Bls12381>(&mut p, &mut alloc, 6, [0x00_01_01_20, 0x00_01_01_21, 0x00_01_01_22], [0x00_01_01_23, 0x00_01_01_24, 0x00_01_01_25]);
            fp::<Bn254>(&mut p, &mut alloc, 4, [0x00_01_01_26, 0x00_01_01_27, 0x00_01_01_28], [0x00_01_01_29, 0x00_01_01_2A, 0x00_01_01_2B]);
        }
        "poseidon2" => {
            for _ in 0..N {
                let s = alloc(8);
                let ws: Vec<u64> = (0..8).map(|_| (rng.next() % P) | ((rng.next() % P) << 32)).collect();
                p.words(s, &ws);
                p.ecall(0x00_00_01_33, s, 0);
            }
        }
        _ => unreachable!(),
    }
    p.finish()
}

// ------------------------------------------------------------------------------------------ rows

struct RowSink {
    docs: HashMap<usize, RowsDoc>,
}

impl RowSink {
    /// A new trace replaces the kept one only if it has more distinct dumped rows.
    fn better(&self, i: usize, rows: &[(usize, Vec<u64>)]) -> bool {
        let distinct = |r: &[(usize, Vec<u64>)]| r.iter().map(|x| &x.1).collect::<std::collections::HashSet<_>>().len();
        match self.docs.get(&i) {
            None => true,
            Some(d) => distinct(rows) > distinct(&d.main_rows),
        }
    }
}

fn matrix_rows(m: &RowMajorMatrix<F>, idx: &[usize]) -> Vec<(usize, Vec<u64>)> {
    idx.iter()
        .map(|&i| (i, m.row_slice(i).iter().map(|x| x.as_canonical_u32() as u64).collect()))
        .collect()
}

fn dump_riscv(machine_chips: &[Chip<F, RiscvAir<F>>], program: Program, label: &str, sink: &mut RowSink, log: &mut String) {
    let prog = Arc::new(program);
    let res = catch_unwind(AssertUnwindSafe(|| {
        generate_records::<F>(prog.clone(), SP1Stdin::new(), SP1CoreOpts::default(), [0; 4])
    }));
    let records = match res {
        Ok(Ok((r, _))) => r,
        Ok(Err(e)) => {
            log.push_str(&format!("{label}: generate_records error: {e}\n"));
            return;
        }
        Err(e) => {
            log.push_str(&format!("{label}: generate_records panic: {}\n", panic_message(e.as_ref())));
            return;
        }
    };
    log.push_str(&format!("{label}: {} records\n", records.len()));
    let preps: Vec<Option<RowMajorMatrix<F>>> =
        machine_chips.iter().map(|c| c.air.as_ref().generate_preprocessed_trace(&prog)).collect();
    for (ri, rec) in records.iter().enumerate() {
        let mut traces: Vec<(usize, RowMajorMatrix<F>)> = vec![];
        for (i, c) in machine_chips.iter().enumerate() {
            if !c.air.as_ref().included(rec) {
                continue;
            }
            let r = catch_unwind(AssertUnwindSafe(|| c.air.as_ref().generate_trace(rec, &mut ExecutionRecord::default())));
            match r {
                Ok(t) if t.height() > 0 => traces.push((i, t)),
                Ok(_) => {}
                Err(e) => log.push_str(&format!("{label}#{ri} {}: generate_trace panic: {}\n", c.name(), panic_message(e.as_ref()))),
            }
        }
        // public values are read after every trace (Global writes the global cumulative sum into the record)
        let pv: Vec<u64> = rec.public_values::<F>().iter().map(|x| x.as_canonical_u32() as u64).collect();
        for (i, t) in traces {
            let c = &machine_chips[i];
            let idx = select_rows(t.height(), 48, 16, 0x5eed ^ i as u64);
            let prep_rows = match &preps[i] {
                Some(pm) => {
                    let pidx: Vec<usize> = idx.iter().copied().filter(|&j| j < pm.height()).collect();
                    matrix_rows(pm, &pidx)
                }
                None => vec![],
            };
            let main_rows = matrix_rows(&t, &idx);
            if !sink.better(i, &main_rows) {
                continue;
            }
            sink.docs.insert(
                i,
                RowsDoc {
                    air_index: i,
                    name: c.name().to_string(),
                    source: format!("{label} record {ri} (generate_records + MachineAir::generate_trace)"),
                    height: t.height(),
                    width: t.width(),
                    prep_width: c.air.as_ref().preprocessed_width(),
                    public_values: pv.clone(),
                    main_rows,
                    prep_rows,
                },
            );
        }
    }
}

// ------------------------------------------------------------------------------------------ recursion rows

mod rec {
    use super::*;
    use slop_algebra::extension::BinomialExtensionField;
    use sp1_recursion_executor::{instruction as instr, linear_program, BaseAluOpcode, Executor, ExtAluOpcode, MemAccessKind, D};

    pub fn record() -> Result<(sp1_recursion_executor::RecursionProgram<F>, sp1_recursion_executor::ExecutionRecord<F>), String> {
        let mut ins = vec![];
        let n = 24u32;
        ins.push(instr::mem(MemAccessKind::Write, 1, 0, 0));
        ins.push(instr::mem(MemAccessKind::Write, n + 2, 1, 1));
        for i in 2..=n {
            let op = [BaseAluOpcode::AddF, BaseAluOpcode::SubF, BaseAluOpcode::MulF, BaseAluOpcode::AddF][(i % 4) as usize];
            ins.push(instr::base_alu(op, 2, i, i - 2, i - 1));
        }
        // extension ALU on a few elements
        let mut addr = 1000u32;
        let mut rng = Rng(0x0dec_0ded);
        for _ in 0..12 {
            let a: [F; 4] = core::array::from_fn(|_| F::from_canonical_u32((rng.next() % P) as u32));
            let b: [F; 4] = core::array::from_fn(|_| F::from_canonical_u32((rng.next() % P) as u32));
            use slop_algebra::AbstractExtensionField;
            let ea = BinomialExtensionField::<F, D>::from_base_slice(&a);
            let eb = BinomialExtensionField::<F, D>::from_base_slice(&b);
            ins.push(instr::mem_ext(MemAccessKind::Write, 1, addr, ea));
            ins.push(instr::mem_ext(MemAccessKind::Write, 1, addr + 1, eb));
            ins.push(instr::ext_alu(ExtAluOpcode::MulE, 1, addr + 2, addr, addr + 1));
            addr += 3;
        }
        // Poseidon2 permutations and selects
        let mut base = 5000u32;
        for _ in 0..12 {
            for j in 0..16u32 {
                ins.push(instr::mem(MemAccessKind::Write, 1, base + j, (rng.next() % P) as u32));
            }
            let input: [u32; 16] = core::array::from_fn(|j| base + j as u32);
            let output: [u32; 16] = core::array::from_fn(|j| base + 16 + j as u32);
            // output multiplicity 1: the output writes are active rows (no later read in this sample program)
            ins.push(instr::poseidon2([1; 16], output, input));
            base += 32;
        }
        for k in 0..12u32 {
            ins.push(instr::mem(MemAccessKind::Write, 1, base, k % 2));
            ins.push(instr::mem(MemAccessKind::Write, 1, base + 1, (rng.next() % P) as u32));
            ins.push(instr::mem(MemAccessKind::Write, 1, base + 2, (rng.next() % P) as u32));
            ins.push(instr::select(1, 1, base, base + 3, base + 4, base + 1, base + 2));
            base += 5;
        }
        let program = linear_program(ins).map_err(|e| format!("{e:?}"))?;
        let mut ex = Executor::<F, BinomialExtensionField<F, D>, sp1_primitives::SP1DiffusionMatrix>::new(
            Arc::new(program.clone()),
            sp1_hypercube::inner_perm(),
        );
        ex.run().map_err(|e| format!("{e:?}"))?;
        Ok((program, ex.record))
    }
}

fn dump_recursion<const D1: usize, const V: usize>(
    chips: &[Chip<F, RecursionAir<F, D1, V>>],
    offset: usize,
    sink: &mut RowSink,
    log: &mut String,
) {
    let r = catch_unwind(AssertUnwindSafe(rec::record));
    let (program, record) = match r {
        Ok(Ok(x)) => x,
        Ok(Err(e)) => {
            log.push_str(&format!("recursion: {e}\n"));
            return;
        }
        Err(e) => {
            log.push_str(&format!("recursion panic: {}\n", panic_message(e.as_ref())));
            return;
        }
    };
    let pv: Vec<u64> = record.public_values::<F>().iter().map(|x| x.as_canonical_u32() as u64).collect();
    for (k, c) in chips.iter().enumerate() {
        let i = offset + k;
        let t = catch_unwind(AssertUnwindSafe(|| c.air.as_ref().generate_trace(&record, &mut Default::default())));
        let t = match t {
            Ok(t) if t.height() > 0 => t,
            Ok(_) => continue,
            Err(e) => {
                log.push_str(&format!("recursion {}: generate_trace panic: {}\n", c.name(), panic_message(e.as_ref())));
                continue;
            }
        };
        let prep = c.air.as_ref().generate_preprocessed_trace(&program);
        let idx = select_rows(t.height(), 48, 16, 0x5eed ^ i as u64);
        let prep_rows = prep
            .as_ref()
            .map(|pm| matrix_rows(pm, &idx.iter().copied().filter(|&j| j < pm.height()).collect::<Vec<_>>()))
            .unwrap_or_default();
        sink.docs.insert(
            i,
            RowsDoc {
                air_index: i,
                name: c.name().to_string(),
                source: "linear recursion program (base / extension ALU, memory) + MachineAir::generate_trace".into(),
                height: t.height(),
                width: t.width(),
                prep_width: c.air.as_ref().preprocessed_width(),
                public_values: pv.clone(),
                main_rows: matrix_rows(&t, &idx),
                prep_rows,
            },
        );
    }
}

// ------------------------------------------------------------------------------------------ main

fn write(path: &str, text: &str) {
    std::fs::write(path, text).unwrap_or_else(|e| panic!("write {path}: {e}"));
}

fn run(out: String, rows: bool) {
    std::fs::create_dir_all(format!("{out}/airs")).unwrap();
    std::fs::create_dir_all(format!("{out}/rows")).unwrap();
    let mut manifest = String::new();
    let mut log = String::new();

    let machine = RiscvAir::<F>::machine();
    let chips = machine.chips();
    let mut index = 0usize;
    let mut handle = |name: &str, group: &str, r: std::thread::Result<Result<AirDoc, String>>, index: usize, manifest: &mut String| {
        let (status, detail) = match r {
            Ok(Ok(doc)) => {
                write(&format!("{out}/airs/{index}.json"), &doc.to_json());
                ("extracted".to_string(), format!("{} constraints, {} interactions", doc.constraints.len(), doc.interactions.len()))
            }
            Ok(Err(e)) => ("failed".to_string(), e),
            Err(e) => ("failed".to_string(), format!("panic: {}", panic_message(e.as_ref()))),
        };
        manifest.push_str(&manifest_line(index, name, name, group, &status, &detail));
        manifest.push('\n');
    };
    for c in chips.iter() {
        let name = c.name().to_string();
        let r = catch_unwind(AssertUnwindSafe(|| extract_chip(c, "riscv", index, &name)));
        handle(&name, "riscv", r, index, &mut manifest);
        index += 1;
    }
    let riscv_count = index;
    let rmachine = RecursionAir::<F, 3, 2>::machine_wide_with_all_chips();
    let rec_offset = index;
    for c in rmachine.chips().iter() {
        let name = format!("Recursion{}", c.name());
        let r = catch_unwind(AssertUnwindSafe(|| extract_chip(c, "recursion-compress", index, &name)));
        handle(&name, "recursion-compress", r, index, &mut manifest);
        index += 1;
    }
    let wmachine = RecursionAir::<F, 3, 1>::wrap_machine();
    let wrap_offset = index;
    for c in wmachine.chips().iter() {
        let name = format!("RecursionWrap{}", c.name());
        let r = catch_unwind(AssertUnwindSafe(|| extract_chip(c, "recursion-wrap", index, &name)));
        handle(&name, "recursion-wrap", r, index, &mut manifest);
        index += 1;
    }
    write(&format!("{out}/manifest.jsonl"), &manifest);
    eprintln!("extracted {index} AIRs ({riscv_count} RISC-V)");

    if rows {
        let mut sink = RowSink { docs: HashMap::new() };
        let mut rng = Rng(0x5eed_5eed_1234_5678);
        dump_riscv(chips, program_core(&mut rng), "core", &mut sink, &mut log);
        for which in ["sha", "keccak", "ec", "ed", "uint256", "fp", "poseidon2"] {
            dump_riscv(chips, program_precompiles(&mut rng, which), which, &mut sink, &mut log);
        }
        dump_recursion(rmachine.chips(), rec_offset, &mut sink, &mut log);
        dump_recursion(wmachine.chips(), wrap_offset, &mut sink, &mut log);
        for (i, d) in sink.docs.iter() {
            write(&format!("{out}/rows/{i}.json"), &d.to_json());
        }
        log.push_str(&format!("rows for {} of {index} AIRs\n", sink.docs.len()));
    }
    write(&format!("{out}/extract.log"), &log);
    eprint!("{log}");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().expect("usage: boole-air-extract <out-dir> [--no-rows]");
    let rows = !args.iter().any(|a| a == "--no-rows");
    // deep expression trees: run on a thread with a large stack
    std::thread::Builder::new().stack_size(1 << 30).spawn(move || run(out, rows)).unwrap().join().unwrap();
}

#[allow(dead_code)]
fn _unused(_: F) -> F {
    F::zero()
}
